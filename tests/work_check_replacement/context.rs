#[path = "../support/mod.rs"]
mod support;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: support::TempRoot,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("work-check-replacement");
        support::git_topology::repository(&root);
        let mut init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        init.args(["init", "--mode", "portable", "--root"])
            .arg(&root);
        clear_native_selectors(&mut init);
        let output = init.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        fs::write(
            root.join("checker.sh"),
            "test -f missing-generated-target\n",
        )
        .unwrap();
        Self { root }
    }
    fn call(&self, args: &[&str]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(command_args(args))
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !args.contains(&"--config") {
            command.args(["--config", "exitbind.json"]);
        }
        clear_native_selectors(&mut command);
        command.output().unwrap()
    }
    fn call_with_input(&self, args: &[&str], input: &[u8]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(command_args(args))
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !args.contains(&"--config") {
            command.args(["--config", "exitbind.json"]);
        }
        clear_native_selectors(&mut command);
        let mut child = command.spawn().unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
if let Err(error) = child.stdin.take().unwrap().write_all(input) { assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}"); }
        child.wait_with_output().unwrap()
    }
    fn call_in_session(&self, session: &str, store: &std::path::Path, args: &[&str]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(command_args(args))
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env("EXITBIND_SESSION_GOALS_DIR", store)
            .env("CODEX_THREAD_ID", session)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !args.contains(&"--config") {
            command.args(["--config", "exitbind.json"]);
        }
        clear_native_selectors(&mut command);
        command
            .env("EXITBIND_SESSION_GOALS_DIR", store)
            .env("CODEX_THREAD_ID", session);
        command.output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
    }
    fn ok_in_session(&self, session: &str, store: &std::path::Path, args: &[&str]) -> Value {
        let output = self.call_in_session(session, store, args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
    }
    fn full_next(&self, work: &str) -> Value {
        let compact = self.ok(&["work", "next", work, "--json"]);
        if compact["truncated"] != true {
            return compact;
        }
        let command = compact["fullCommand"]
            .as_array()
            .expect("a truncated current response supplies its fullCommand")
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        self.ok(&refs(&command))
    }
    fn ledger(&self, work: &str) -> PathBuf {
        self.root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))
    }
    fn events(&self, work: &str) -> Vec<Value> {
        fs::read_to_string(self.ledger(work))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn begin_failed_check(&self) -> (String, Value) {
        self.begin_failed_check_with("exec sh checker.sh", "repair the checker")
    }
    fn begin_failed_check_with(&self, check_command: &str, goal: &str) -> (String, Value) {
        let work = self.begin_completed_worker(check_command, goal);
        self.finish_failed_check(work)
    }
    fn begin_failed_check_with_boundary(
        &self,
        check_command: &str,
        goal: &str,
        boundary: &str,
    ) -> (String, Value) {
        let work = self.begin_completed_worker_with_boundary(check_command, goal, Some(boundary));
        self.finish_failed_check(work)
    }
    fn finish_failed_check(&self, work: String) -> (String, Value) {
        let failed = self.call(&["work", "check", &work, "--timeout-ms", "2000"]);
        assert!(
            !failed.status.success(),
            "failed checker should remain a failed observation: {failed:?}"
        );
        let event = fs::read_to_string(self.ledger(&work))
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| event["action"] == "check")
            .unwrap_or_else(|| {
                panic!(
                    "durable failed check event missing; work check stdout={} stderr={} ledger={}",
                    String::from_utf8_lossy(&failed.stdout),
                    String::from_utf8_lossy(&failed.stderr),
                    fs::read_to_string(self.ledger(&work)).unwrap_or_default()
                )
            });
        assert!(
            event["result"]["code"]
                .as_i64()
                .is_some_and(|code| code != 0)
                || event["result"]["signal"].is_number()
        );
        (work, event)
    }
    fn begin_completed_worker(&self, check_command: &str, goal: &str) -> String {
        self.begin_completed_worker_with_boundary(check_command, goal, None)
    }
    fn begin_completed_worker_with_boundary(
        &self,
        check_command: &str,
        goal: &str,
        boundary: Option<&str>,
    ) -> String {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            check_command,
            "--review-policy",
            "required",
        ];
        if let Some(boundary) = boundary {
            args.extend(["--boundary", boundary]);
        }
        let begin = self.ok(&args);
        let work = begin["work"].as_str().unwrap().to_string();
        let lead = begin["next"]["assignment"].as_str().unwrap();
        self.ok(&["work", "return", &work, lead, "--outcome", "scoped"]);
        let worker = self.ok(&["work", "next", &work, "--json"]);
        let worker_assignment = worker["next"]["assignment"].as_str().unwrap();
        self.ok(&[
            "work",
            "return",
            &work,
            worker_assignment,
            "--outcome",
            "completed",
        ]);
        work
    }
}
fn command_args<'a>(args: &'a [&str]) -> &'a [&'a str] {
    if args
        .first()
        .is_some_and(|arg| *arg == env!("CARGO_BIN_EXE_exitbind"))
    {
        &args[1..]
    } else {
        args
    }
}
fn clear_native_selectors(command: &mut Command) {
    for name in [
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "EXITBIND_NATIVE_SESSION_ID",
        "EXITBIND_SESSION_GOALS_DIR",
        "CLAUDE_ENV_FILE",
        "XDG_STATE_HOME",
    ] {
        command.env_remove(name);
    }
}

fn replacement_args(work: &str, binding: &str) -> Vec<String> {
    vec![
        "work".into(),
        "replace-check".into(),
        work.into(),
        "--check-command".into(),
        "rm -f preserved-evidence; exit 9".into(),
        "--reason".into(),
        "the frozen command targeted a missing generated file".into(),
        "--classification".into(),
        "defective-contract".into(),
        "--current-binding".into(),
        binding.into(),
    ]
}
fn refs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn install_narrow_boundary(f: &Fixture) {
    fs::create_dir_all(f.root.join("src")).unwrap();
    fs::write(f.root.join("src/a.txt"), "bounded file\n").unwrap();
    let config_path = f.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["observe"] = json!(["src/**"]);
    config["agents"]["worker"]["write"] = json!(["src/**"]);
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let path = f.root.join(".agents/boundaries");
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("task.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "agents": {"worker": {"observe": ["src/a.txt"], "write": ["src/a.txt"]}}
        }))
        .unwrap(),
    )
    .unwrap();
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn digest_value(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn native_session_root(store: &std::path::Path, session: &str) -> PathBuf {
    let key = digest_value(&json!({"host":"codex","identity":session}));
    store.join(format!("session-{key}"))
}

fn start_intent_sha256(start: &Value) -> String {
    const FIELDS: &[&str] = &[
        "version",
        "kind",
        "producer",
        "action",
        "workflow",
        "goal",
        "configSha256",
        "plan",
        "harnessReceipt",
        "checkPolicy",
        "preservation",
        "basisProtocol",
        "recoveryProtocol",
        "observationNoveltyProtocol",
        "checkObservationProtocol",
        "basis",
        "reviewPolicy",
        "governor",
    ];
    let mut projection = Map::new();
    for field in FIELDS {
        if let Some(value) = start.get(*field) {
            projection.insert((*field).to_owned(), value.clone());
        }
    }
    digest_value(&Value::Object(projection))
}

fn append_goal_admission(session_root: &std::path::Path, admission: Value) {
    let path = session_root.join(".exitbind/session-goal.jsonl");
    let mut records = support::goal_journal::records(&path);
    let previous = records.last().unwrap().clone();
    let mut next = previous.clone();
    next["revision"] = json!(previous["revision"].as_u64().unwrap() + 1);
    next["predecessor"] = json!({"goalId":previous["goalId"],"revision":previous["revision"]});
    next["successorOf"] = Value::Null;
    next["previousEventSha256"] = previous["eventSha256"].clone();
    next["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
    next["journeyAdmissions"]
        .as_array_mut()
        .unwrap()
        .push(admission);
    next.as_object_mut().unwrap().remove("eventSha256");
    next["eventSha256"] = json!(digest_value(&next));
    records.push(next);
    support::goal_journal::write(&path, &records);
}

fn admission_for_start(member: &Value, work: &str, start: &Value, parent_goal: &Value) -> Value {
    json!({
        "work": work,
        "startIntentSha256": start_intent_sha256(start),
        "parentGoalId": parent_goal["goalId"],
        "parentObservedGoalEventSha256": parent_goal["eventSha256"],
        "workGoalSha256": digest_text(start["goal"].as_str().unwrap()),
        "host": member["host"],
        "sessionKey": member["sessionKey"],
        "ownerIdentitySha256": member["ownerIdentitySha256"],
        "ownerProjectId": member["ownerProjectId"],
        "projectIdentitySha256": member["projectIdentitySha256"],
        "localIdentitySha256": member["localIdentitySha256"],
        "configPath": member["configPath"],
        "configSha256": member["configSha256"],
        "productRoot": member["productRoot"],
        "stateRoot": member["stateRoot"],
    })
}

fn native_failed_work(f: &Fixture, session: &str, store: &std::path::Path) -> (String, String) {
    let goal = f.call_in_session(
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "r46-replacement-goal",
            "--goal",
            "Preserve the original goal and its side obligation",
            "--obligation",
            "Keep the original side obligation intact",
        ],
    );
    assert!(goal.status.success(), "{goal:?}");
    let begin = f.call_in_session(
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A session-bound implementation task",
            "--check-command",
            "false",
        ],
    );
    assert!(begin.status.success(), "{begin:?}");
    let begin: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = begin["work"].as_str().unwrap().to_owned();
    let lead_assignment = begin["next"]["assignment"].as_str().unwrap();
    let scoped = f.call_in_session(
        session,
        store,
        &[
            "work",
            "return",
            &work,
            lead_assignment,
            "--outcome",
            "scoped",
        ],
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = f.ok_in_session(session, store, &["work", "next", &work, "--json"]);
    let worker_assignment = worker["next"]["assignment"].as_str().unwrap();
    let returned = f.call_in_session(
        session,
        store,
        &[
            "work",
            "return",
            &work,
            worker_assignment,
            "--outcome",
            "completed",
        ],
    );
    assert!(returned.status.success(), "{returned:?}");
    let failed = f.call_in_session(
        session,
        store,
        &["work", "check", &work, "--timeout-ms", "2000"],
    );
    assert!(
        !failed.status.success(),
        "expected fixture checker failure: {failed:?}"
    );
    let current = f.ok_in_session(session, store, &["work", "next", &work, "--json"]);
    let binding = current["current"]["binding"].as_str().unwrap().to_owned();
    (work, binding)
}

fn run_entries(f: &Fixture) -> Vec<String> {
    let mut names = fs::read_dir(f.root.join(".exitbind/runs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn full_work_detail_in_session(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    work: &str,
) -> Value {
    let output = f.call_in_session(session, store, &["work", "detail", work, "--json"]);
    assert!(
        output.status.success(),
        "full work detail output={output:?}"
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: full work detail output={output:?}"))
}

fn emitted_choice(detail: &Value, label: &str) -> Value {
    detail["actionForms"]["choices"]
        .as_array()
        .unwrap_or_else(|| panic!("detail lacks action choices: {detail:#}"))
        .iter()
        .find(|choice| choice["label"] == label)
        .cloned()
        .unwrap_or_else(|| panic!("detail lacks `{label}` action choice: {detail:#}"))
}

fn assert_emitted_choice_context(detail: &Value, choice: &Value, work: &str) {
    let assignment = detail["recipient"]["assignment"]
        .as_str()
        .unwrap_or_else(|| panic!("detail has no current recipient assignment: {detail:#}"));
    let binding = detail["binding"]
        .as_str()
        .or(detail["current"]["binding"].as_str())
        .unwrap_or_else(|| panic!("detail has no current binding: {detail:#}"));
    let argv = choice["command"]["argv"]
        .as_array()
        .unwrap_or_else(|| panic!("choice has no argv: {choice:#}"));
    let argv = argv
        .iter()
        .map(|argument| argument.as_str().expect("argv values are strings"))
        .collect::<Vec<_>>();
    assert!(argv.contains(&work), "work missing from choice: {detail:#}");
    assert!(
        argv.contains(&assignment),
        "current recipient assignment missing from choice: {detail:#} choice={choice:#}"
    );
    assert!(
        argv.contains(&binding),
        "current binding missing from choice: {detail:#} choice={choice:#}"
    );
}

fn emitted_choice_args(choice: &Value, reason: &str) -> Vec<String> {
    let mut replaced_reason = false;
    let args = choice["command"]["argv"]
        .as_array()
        .unwrap_or_else(|| panic!("choice has no argv: {choice:#}"))
        .iter()
        .map(|argument| {
            let argument = argument.as_str().expect("argv values are strings");
            if argument == "<REASON>" {
                replaced_reason = true;
                reason.to_owned()
            } else {
                argument.to_owned()
            }
        })
        .collect::<Vec<_>>();
    assert!(
        replaced_reason,
        "choice did not declare a reason placeholder: {choice:#}"
    );
    args
}

fn call_in_session_with_input(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    args: &[String],
    input: &[u8],
) -> Output {
    let args = refs(args);
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&f.root)
        .args(command_args(&args))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("CODEX_THREAD_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !args.contains(&"--config") {
        command.args(["--config", "exitbind.json"]);
    }
    clear_native_selectors(&mut command);
    command
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("CODEX_THREAD_ID", session);
    let mut child = command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

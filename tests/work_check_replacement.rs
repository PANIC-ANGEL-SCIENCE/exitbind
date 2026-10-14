//! An explicit Lead decision can preserve a failed Work while binding a new
//! frozen checker to a claimed successor.
#![cfg(unix)]
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
    root: PathBuf,
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
        child.stdin.take().unwrap().write_all(input).unwrap();
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
    fn ledger(&self, work: &str) -> PathBuf {
        self.root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))
    }
    fn begin_failed_check(&self) -> (String, Value) {
        self.begin_failed_check_with("exec sh checker.sh", "repair the checker")
    }
    fn begin_failed_check_with(&self, check_command: &str, goal: &str) -> (String, Value) {
        let work = self.begin_completed_worker(check_command, goal);
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
        let begin = self.ok(&[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            check_command,
            "--review-policy",
            "required",
        ]);
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
    ] {
        command.env_remove(name);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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
    let raw = fs::read_to_string(&path).unwrap();
    let mut records = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
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
    fs::write(
        path,
        format!(
            "{}\n",
            records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
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

#[test]
fn preview_and_apply_create_one_immutable_successor_without_running_new_check() {
    let f = Fixture::new();
    fs::write(f.root.join("preserved-evidence"), b"retain").unwrap();
    let (work, source) = f.begin_failed_check();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap().to_owned();
    let source_sha = source["eventSha256"].as_str().unwrap();
    let before = fs::read(f.ledger(&work)).unwrap();
    let predecessor_start: Value =
        serde_json::from_slice(before.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    let predecessor_events: Vec<Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let predecessor_state = f.ok(&[
        "run",
        "status",
        &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
        "--json",
    ]);

    let preview_args = replacement_args(&work, &binding);
    let preview = f.ok(&refs(&preview_args));
    assert_eq!(preview["state"], "preview");
    assert_eq!(preview["decision"]["classification"], "defective_contract");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    assert_eq!(
        fs::read(f.root.join("preserved-evidence")).unwrap(),
        b"retain"
    );

    let apply_args = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok(&refs(&apply_args));
    assert_eq!(applied["state"], "successor_created");
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "predecessor JSONL stays byte-identical"
    );
    assert_eq!(
        fs::read(f.root.join("preserved-evidence")).unwrap(),
        b"retain",
        "replacement check must never be run by continuation"
    );
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();
    let successor_events: Vec<Value> = fs::read_to_string(f.ledger(&successor))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let start = &successor_events[0];
    assert_eq!(
        successor_events.len(),
        1,
        "continuation writes only a new start, never a copied pass or review"
    );
    assert_eq!(
        start["checkReplacement"]["source"]["eventSha256"],
        source_sha
    );
    assert_eq!(
        start["checkReplacement"]["classification"],
        "defective_contract"
    );
    assert_eq!(
        start["checkPolicy"]["commandSha256"], preview["decision"]["newPolicy"]["commandSha256"],
        "the committed start policy matches the previewed replacement decision"
    );
    assert_eq!(
        start["checkReplacement"]["oldPolicy"]["commandSha256"],
        predecessor_start["checkPolicy"]["commandSha256"]
    );
    assert_eq!(
        start["checkReplacement"]["oldPolicy"]["origin"],
        predecessor_start["checkPolicy"]["origin"]
    );
    assert_eq!(
        start["checkReplacement"]["newPolicy"],
        serde_json::json!({"commandSha256":start["checkPolicy"]["commandSha256"],"origin":start["checkPolicy"]["origin"]}),
        "the replacement marker and actual frozen successor policy are identical"
    );
    assert_eq!(
        start["checkReplacement"]["predecessorHeadEventSha256"],
        predecessor_events.last().unwrap()["eventSha256"],
        "the replacement binds the exact immutable predecessor head"
    );
    assert_eq!(
        start["supersedes"]["ledgerPath"],
        format!(".exitbind/runs/work-{}.jsonl", &work[4..])
    );
    let lead_owner = &start["checkReplacement"]["leadOwner"];
    assert!(
        predecessor_start["plan"]["stages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stage| stage["agents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|agent| agent["role"] == "lead"
                    && agent["name"] == lead_owner["agent"]
                    && agent["nativeTaskName"] == lead_owner["nativeTaskName"])),
        "the marker owner is exactly one configured Lead"
    );
    assert_eq!(
        start["reviewPolicy"], predecessor_start["reviewPolicy"],
        "the predecessor's full review decision is preserved without reconstruction"
    );
    assert_eq!(
        start.get("basis"),
        predecessor_start.get("basis"),
        "replacement preserves the exact optional basis"
    );
    assert_eq!(
        start.get("preservation"),
        predecessor_start.get("preservation"),
        "replacement preserves the exact optional preservation contract"
    );
    assert_eq!(
        start["budget"],
        fs::read_to_string(f.ledger(&work))
            .unwrap()
            .lines()
            .next()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["budget"].clone())
            .unwrap()
    );
    let carried = &start["governorCarry"]["accounting"];
    let spent_before = predecessor_state["governor"]["spent"].as_u64().unwrap();
    assert!(
        spent_before > 0,
        "the fixture must spend some of the same-Goal budget"
    );
    assert_eq!(
        carried["spent"].as_u64(),
        Some(spent_before),
        "successor accounting includes the predecessor's already-spent work"
    );
    assert_eq!(
        carried["budget"], predecessor_state["governor"]["budget"],
        "continuation cannot reset or widen the total budget"
    );
    assert_eq!(
        carried["noInformationStreak"],
        predecessor_state["governor"]["noInformationStreak"]
    );
    assert_eq!(
        carried["postReplanSpent"],
        predecessor_state["governor"]["postReplanSpent"]
    );

    let claim_path = format!("{}.supersede", f.ledger(&work).display());
    let claim: Value = serde_json::from_slice(&fs::read(claim_path).unwrap()).unwrap();
    assert_eq!(claim["checkReplacement"], start["checkReplacement"]);
    assert_eq!(
        claim["oldHeadEventSha256"],
        start["supersedes"]["headEventSha256"]
    );
    assert_eq!(
        claim["checkReplacement"]["newPolicy"]["origin"],
        start["checkPolicy"]["origin"]
    );

    let relation = f.ok(&["work", "detail", &successor, "--json"]);
    assert_eq!(relation["checkReplacement"]["predecessor"]["work"], work);
    assert_eq!(
        relation["checkReplacement"]["source"]["eventSha256"],
        source_sha
    );
    let retry = f.ok(&refs(&apply_args));
    assert_eq!(retry["state"], "existing_successor_recovered");
    let mut changed = apply_args.clone();
    let reason_index = changed.iter().position(|arg| arg == "--reason").unwrap() + 1;
    changed[reason_index] = "different reason".into();
    assert!(
        !f.call(&refs(&changed)).status.success(),
        "different relation must not create another successor"
    );
}

#[test]
fn nonexistent_cargo_test_target_is_recorded_and_continued_as_a_new_contract() {
    let f = Fixture::new();
    let package = f.root.join(".exitbind/r46-missing-target-package");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname = \"r46-missing-target-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(package.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();

    let (work, failed) = f.begin_failed_check_with(
        "cargo test --offline --manifest-path .exitbind/r46-missing-target-package/Cargo.toml --test absent_r46_target",
        "the configured test target is absent",
    );
    assert_eq!(
        failed["result"]["code"], 101,
        "an actual Cargo missing-target diagnostic is distinct from command-not-found (127)"
    );
    let before = fs::read(f.ledger(&work)).unwrap();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let preview = f.ok(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "touch replacement-check-must-not-run",
        "--reason",
        "The frozen command names a test target absent from this package.",
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
    ]);
    assert_eq!(
        preview["decision"]["source"]["eventSha256"],
        failed["eventSha256"]
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect::<Vec<_>>();
    let committed = f.ok(&apply);
    assert_eq!(committed["state"], "successor_created");
    assert!(
        !f.root.join("replacement-check-must-not-run").exists(),
        "continuation records the new test contract without running it"
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "the failed Cargo event and frozen policy remain byte-identical"
    );
}

#[test]
fn destructive_count_check_is_exercised_once_and_its_source_stays_immutable() {
    let f = Fixture::new();
    let evidence_dir = f.root.join(".exitbind/r46-destructive-count");
    fs::create_dir_all(&evidence_dir).unwrap();
    fs::write(evidence_dir.join("count-evidence"), b"1\n").unwrap();
    fs::write(
        evidence_dir.join("destroy-count.sh"),
        "rm -f .exitbind/r46-destructive-count/count-evidence\ntest \"$(cat .exitbind/r46-destructive-count/count-evidence)\" = 1\n",
    )
    .unwrap();
    let (work, failed) = f.begin_failed_check_with(
        "exec sh .exitbind/r46-destructive-count/destroy-count.sh",
        "the count checker destroys the evidence before reading it",
    );
    assert_eq!(failed["result"]["code"], 1);
    assert!(
        !evidence_dir.join("count-evidence").exists(),
        "the controlled reproduction records the check's known destructive side effect"
    );
    let before = fs::read(f.ledger(&work)).unwrap();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let preview = f.ok(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "touch replacement-check-must-not-run",
        "--reason",
        "The frozen count command deletes its own required input before counting.",
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
    ]);
    assert_eq!(
        preview["decision"]["source"]["eventSha256"],
        failed["eventSha256"]
    );
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect::<Vec<_>>();
    let committed = f.ok(&apply);
    assert_eq!(committed["state"], "successor_created");
    assert!(
        !f.root.join("replacement-check-must-not-run").exists(),
        "the successor checker is not run during replacement"
    );
    assert!(
        !evidence_dir.join("count-evidence").exists(),
        "replacement never replays the destructive predecessor check"
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "the predecessor keeps the single original failed check and its policy"
    );
}

#[test]
fn destructive_bound_input_observation_never_becomes_replacement_source_credit() {
    let f = Fixture::new();
    let evidence_dir = f.root.join(".exitbind/r46-destructive-bound-input");
    fs::create_dir_all(&evidence_dir).unwrap();
    fs::write(f.root.join("count-evidence"), b"1\n").unwrap();
    fs::write(
        evidence_dir.join("destroy-count.sh"),
        "printf x >> .exitbind/r46-destructive-bound-input/attempts\nrm -f count-evidence\ntest \"$(cat count-evidence)\" = 1\n",
    )
    .unwrap();

    let work = f.begin_completed_worker(
        "exec sh .exitbind/r46-destructive-bound-input/destroy-count.sh",
        "the checker mutates its bound product input before counting",
    );
    let failed = f.call(&["work", "check", &work, "--timeout-ms", "2000"]);
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    let failure: Value = serde_json::from_slice(&failed.stdout)
        .unwrap_or_else(|error| panic!("{error}: {failed:?}"));
    assert_eq!(
        failure["observationFailure"]["facts"]["diagnostic"],
        "observation binding changed before commit; no check was recorded"
    );
    assert_eq!(failure["observationFailure"]["facts"]["process"]["code"], 1);
    assert_eq!(
        failure["observationFailure"]["facts"]["termination"],
        "ended"
    );
    assert_eq!(
        failure["observationFailure"]["facts"]["storage"],
        "not_committed"
    );
    assert_eq!(
        String::from_utf8_lossy(&failed.stderr),
        "Session Goal: not requested | Result readiness: BLOCKED | Next: lead_decision\n"
    );
    assert_eq!(fs::read(evidence_dir.join("attempts")).unwrap(), b"x");
    assert!(!f.root.join("count-evidence").exists());

    let ledger_after_observation = fs::read(f.ledger(&work)).unwrap();
    let events: Vec<Value> = std::str::from_utf8(&ledger_after_observation)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "check_observation"),
        "the execution admission remains durable: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "check_observation_failed"),
        "the ended execution failure is durably classified: {events:?}"
    );
    assert!(
        !events.iter().any(|event| event["action"] == "check"),
        "input-mutating execution has no failed-check source credit: {events:?}"
    );

    let entries_before = run_entries(&f);
    let current = f.ok(&["work", "next", &work, "--json"]);
    let binding = current["current"]["binding"].as_str().unwrap();
    for apply in [false, true] {
        let mut args = replacement_args(&work, binding);
        if apply {
            args.push("--apply".into());
        }
        let refused = f.call(&refs(&args));
        assert_eq!(refused.status.code(), Some(1), "{refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains(
                "predecessor contains no exact failed-check or blocked-worker source evidence"
            ),
            "{stderr}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), ledger_after_observation);
        assert_eq!(
            run_entries(&f),
            entries_before,
            "no successor or claim is created"
        );
        assert_eq!(
            fs::read(evidence_dir.join("attempts")).unwrap(),
            b"x",
            "replacement must not replay the destructive check"
        );
    }
}

#[test]
fn stale_binding_and_non_defective_or_missing_sources_refuse_without_mutation() {
    let f = Fixture::new();
    fs::write(f.root.join("preserved-evidence"), b"retain").unwrap();
    let (work, source) = f.begin_failed_check();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap().to_owned();
    let before = fs::read(f.ledger(&work)).unwrap();

    for (args, description) in [(
        {
            let mut value = replacement_args(&work, &"0".repeat(64));
            value.push("--apply".into());
            value
        },
        "stale binding",
    )] {
        let output = f.call(&refs(&args));
        assert!(!output.status.success(), "{description}: {output:?}");
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    }
    let preview = f.ok(&refs(&replacement_args(&work, &binding)));
    let mut wrong_class = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let class_index = wrong_class
        .iter()
        .position(|arg| arg == "--classification")
        .unwrap()
        + 1;
    wrong_class[class_index] = "ordinary-failure".into();
    assert!(
        !f.call(&refs(&wrong_class)).status.success(),
        "ordinary product failure is not defective-contract authorization"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    assert_eq!(
        fs::read(f.root.join("preserved-evidence")).unwrap(),
        b"retain"
    );
    assert!(source["eventSha256"].is_string());
}

#[test]
fn blocked_worker_has_a_bound_sibling_lead_replacement_without_fake_submission() {
    let f = Fixture::new();
    let begin = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "continue blocked work",
        "--check-command",
        "test -f missing-generated-target",
        "--review-policy",
        "required",
    ]);
    let work = begin["work"].as_str().unwrap().to_owned();
    f.ok(&[
        "work",
        "return",
        &work,
        begin["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = f.ok(&["work", "next", &work, "--json"]);
    let blocked = f.ok(&[
        "work",
        "return",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "blocked",
    ]);
    let source_sha = blocked["event"]["eventSha256"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            fs::read_to_string(f.ledger(&work))
                .unwrap()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find(|event| {
                    event["action"] == "submit"
                        && event["role"] == "worker"
                        && event["outcome"] == "blocked"
                })
                .unwrap()["eventSha256"]
                .as_str()
                .unwrap()
                .to_owned()
        });
    let detail = f.ok(&["work", "detail", &work, "--json"]);
    let binding = detail["binding"]
        .as_str()
        .or(detail["current"]["binding"].as_str())
        .unwrap()
        .to_owned();
    let action_form = &detail["actionForms"];
    assert_eq!(action_form["state"], "blocked_lead_check_replacement");
    assert_eq!(action_form["leadChoiceRequired"], true);
    assert_eq!(action_form["choices"].as_array().unwrap().len(), 1);
    let before = fs::read(f.ledger(&work)).unwrap();

    let preview_args = [
        "work",
        "replace-check",
        &work,
        "--check-command",
        "rm -f preserved-evidence; exit 9",
        "--reason",
        "blocked worker evidence shows the check contract cannot proceed",
        "--classification",
        "defective-contract",
        "--current-binding",
        &binding,
    ];
    let preview = f.ok(&preview_args);
    assert_eq!(preview["decision"]["source"]["kind"], "blocked_worker");
    assert_eq!(preview["decision"]["source"]["eventSha256"], source_sha);
    assert!(preview["decision"]["leadOwner"]["agent"].is_string());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);

    let apply_args = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    let applied = f.ok(&apply_args);
    assert_eq!(applied["state"], "successor_created");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    let events: Vec<Value> = fs::read_to_string(f.ledger(&work))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let previous_events: Vec<Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let lead_submissions = |events: &[Value]| {
        events
            .iter()
            .filter(|event| event["action"] == "submit" && event["role"] == "lead")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(
        lead_submissions(&events) == lead_submissions(&previous_events),
        "replacement adds no Lead submission; the original Lead scope decision remains"
    );
}

#[test]
fn accepted_predecessor_and_unclassified_failure_are_refused() {
    let f = Fixture::new();
    let begin = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "accepted stays closed",
        "--check-command",
        "true",
        "--review-policy",
        "required",
    ]);
    let work = begin["work"].as_str().unwrap().to_owned();
    f.ok(&[
        "work",
        "return",
        &work,
        begin["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = f.ok(&["work", "next", &work, "--json"]);
    f.ok(&[
        "work",
        "return",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"]);
    let reviewer = f.ok(&["work", "next", &work, "--json"]);
    assert_eq!(reviewer["next"]["role"], "reviewer");
    f.ok(&[
        "work",
        "return",
        &work,
        reviewer["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "approved",
    ]);
    let lead = f.ok(&["work", "next", &work, "--json"]);
    assert_eq!(lead["next"]["role"], "lead");
    f.ok(&[
        "work",
        "return",
        &work,
        lead["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "accepted",
    ]);
    let terminal = f.ok(&["work", "next", &work, "--json"]);
    let binding = terminal["current"]["binding"].as_str().unwrap().to_owned();
    let before = fs::read(f.ledger(&work)).unwrap();
    let rejected = f.call(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "false",
        "--reason",
        "test",
        "--classification",
        "defective-contract",
        "--current-binding",
        &binding,
        "--apply",
    ]);
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("accepted predecessor"),
        "{rejected:?}"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);

    let (failed_work, _) = f.begin_failed_check();
    let failed_lead = f.ok(&["work", "next", &failed_work, "--json"]);
    let failed_binding = failed_lead["current"]["binding"].as_str().unwrap();
    let unclassified = f.call(&[
        "work",
        "replace-check",
        &failed_work,
        "--check-command",
        "true",
        "--reason",
        "ordinary assertion failure",
        "--current-binding",
        failed_binding,
        "--apply",
    ]);
    assert!(
        !unclassified.status.success(),
        "a failed assertion cannot be classified automatically"
    );
    assert!(
        String::from_utf8_lossy(&unclassified.stderr).contains("--classification"),
        "{unclassified:?}"
    );
}

#[test]
fn missing_environment_tool_is_not_silently_classified_as_a_defective_contract() {
    let f = Fixture::new();
    let (work, failure) = f.begin_failed_check_with(
        "r46_tool_that_is_deliberately_not_installed --version",
        "the check environment lacks a tool",
    );
    assert_eq!(
        failure["result"]["code"], 127,
        "missing executable remains distinct from the Cargo missing-target result"
    );
    let before = fs::read(f.ledger(&work)).unwrap();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let refused = f.call(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "true",
        "--reason",
        "the worker's environment does not contain the required tool",
        "--current-binding",
        binding,
        "--apply",
    ]);
    assert!(
        !refused.status.success(),
        "environment failure cannot be silently promoted to a Lead defective-contract decision"
    );
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--classification"));
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "no successor is created without an explicit Lead classification"
    );
}

#[test]
fn native_goal_work_cannot_be_replaced_from_a_foreign_session() {
    let f = Fixture::new();
    let store = f.root.join(".exitbind/native-test-session-goals");
    let owner_session = "r46-replacement-owner-session";
    let incorporate = f.call_in_session(
        owner_session,
        &store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "r46-session-bound-goal",
            "--goal",
            "The original conversation commitment remains in this session",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    assert!(incorporate.status.success(), "{incorporate:?}");
    let begin = f.call_in_session(
        owner_session,
        &store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A session-bound implementation task",
            "--check-command",
            "test -f absent-r46-generated-input",
        ],
    );
    assert!(begin.status.success(), "{begin:?}");
    let begin: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = begin["work"].as_str().unwrap().to_owned();
    let scoped = f.call_in_session(
        owner_session,
        &store,
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = f.call_in_session(owner_session, &store, &["work", "next", &work, "--json"]);
    assert!(worker.status.success(), "{worker:?}");
    let worker: Value = serde_json::from_slice(&worker.stdout).unwrap();
    let returned = f.call_in_session(
        owner_session,
        &store,
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
    );
    assert!(returned.status.success(), "{returned:?}");
    let failed = f.call_in_session(
        owner_session,
        &store,
        &["work", "check", &work, "--timeout-ms", "2000"],
    );
    assert!(
        !failed.status.success(),
        "the deliberately absent input must fail once: {failed:?}"
    );
    let events_before = fs::read(f.ledger(&work)).unwrap();
    let binding = f.ok_in_session(
        "r46-unrelated-session",
        &store,
        &["work", "next", &work, "--json"],
    )["current"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let foreign = f.call_in_session(
        "r46-unrelated-session",
        &store,
        &[
            "work",
            "replace-check",
            &work,
            "--check-command",
            "true",
            "--reason",
            "must stay bound to the original session",
            "--classification",
            "defective-contract",
            "--current-binding",
            &binding,
            "--apply",
        ],
    );
    assert!(
        !foreign.status.success(),
        "a fresh but foreign native session cannot continue this member: {foreign:?}"
    );
    let diagnostic = String::from_utf8_lossy(&foreign.stderr);
    assert!(
        diagnostic.contains("native Goal/session/project association"),
        "{diagnostic}"
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        events_before,
        "session mismatch must refuse before any successor or predecessor write"
    );
}

#[test]
fn native_goal_replacement_keeps_original_member_and_resolves_successor_lineage_read_only() {
    let f = Fixture::new();
    let session = "r46-native-lineage-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let (work, binding) = native_failed_work(&f, session, &store);
    let session_root = native_session_root(&store, session);
    let journal_path = session_root.join(".exitbind/session-goal.jsonl");
    let journal_before = fs::read(&journal_path).unwrap();
    let records_before = journal_before
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let member_before = records_before
        .iter()
        .rev()
        .find_map(|record| {
            record["journeyMembers"]
                .as_array()?
                .iter()
                .find(|member| member["work"] == work)
        })
        .expect("original native Goal member")
        .clone();
    assert_eq!(
        records_before.last().unwrap()["journeyMembers"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "this fixture begins with one original conversation member"
    );
    assert!(records_before.last().unwrap()["obligations"]
        .to_string()
        .contains("Keep the original side obligation intact"));
    let predecessor_before = fs::read(f.ledger(&work)).unwrap();

    let preview = f.ok_in_session(session, &store, &refs(&replacement_args(&work, &binding)));
    let argv = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok_in_session(session, &store, &refs(&argv));
    assert_eq!(applied["state"], "successor_created");
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();

    assert_eq!(fs::read(f.ledger(&work)).unwrap(), predecessor_before);
    assert_eq!(
        fs::read(&journal_path).unwrap(),
        journal_before,
        "replacement does not append a successor member or rewrite admissions/obligations"
    );
    let records_after = fs::read_to_string(&journal_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        records_after.last().unwrap()["journeyMembers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        records_after.last().unwrap()["journeyMembers"][0],
        member_before
    );
    assert!(records_after.last().unwrap()["obligations"]
        .to_string()
        .contains("Keep the original side obligation intact"));

    let detail = f.ok_in_session(session, &store, &["work", "detail", &successor, "--json"]);
    assert_eq!(
        detail["goalAssociation"]["kind"],
        "verified_successor_lineage"
    );
    assert_eq!(detail["goalAssociation"]["work"], successor);
    assert_eq!(detail["goalAssociation"]["originalMemberWork"], work);
    assert_eq!(detail["checkReplacement"]["predecessor"]["work"], work);
    assert_eq!(detail["checkReplacement"]["successor"]["work"], successor);

    let next = f.ok_in_session(session, &store, &["work", "next", &successor, "--json"]);
    assert_eq!(
        next["goalProgress"]["workAssociation"]["kind"],
        "verified_successor_lineage"
    );
    assert_eq!(
        next["goalProgress"]["workAssociation"]["originalMemberWork"],
        work
    );
}

#[test]
fn pending_native_admission_blocks_check_replacement_preview_and_apply() {
    let f = Fixture::new();
    let session = "r46-pending-admission-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let (work, _) = native_failed_work(&f, session, &store);
    let session_root = native_session_root(&store, session);

    // A separate unlinked Work start creates a valid admission against this
    // session's current member. Its pending ownership blocks replacement of
    // the already-associated failed Work as well.
    let admitted = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "An interrupted side request",
        "--check-command",
        "true",
    ]);
    let admitted_work = admitted["work"].as_str().unwrap();
    let admitted_ledger = fs::read(f.ledger(admitted_work)).unwrap();
    let start: Value =
        serde_json::from_slice(admitted_ledger.split(|byte| *byte == b'\n').next().unwrap())
            .unwrap();
    let journal_path = session_root.join(".exitbind/session-goal.jsonl");
    let history = fs::read_to_string(&journal_path).unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let parent = records.last().unwrap();
    let member = records
        .iter()
        .rev()
        .find_map(|record| {
            record["journeyMembers"]
                .as_array()?
                .iter()
                .find(|member| member["work"] == work)
        })
        .expect("native Work membership")
        .clone();
    let admission = admission_for_start(&member, admitted_work, &start, parent);
    append_goal_admission(&session_root, admission);

    let journal_with_admission = fs::read(&journal_path).unwrap();
    let latest: Value = serde_json::from_slice(
        journal_with_admission
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(latest["journeyAdmissions"].as_array().unwrap().len(), 1);
    assert!(latest["obligations"]
        .to_string()
        .contains("Keep the original side obligation intact"));

    let binding = f.ok_in_session(session, &store, &["work", "next", &work, "--json"])["current"]
        ["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let ledger_before = fs::read(f.ledger(&work)).unwrap();
    let entries_before = run_entries(&f);
    for apply in [false, true] {
        let mut args = replacement_args(&work, &binding);
        if apply {
            args.push("--apply".into());
        }
        let output = f.call_in_session(session, &store, &refs(&args));
        assert!(
            !output.status.success(),
            "pending admission must refuse: {output:?}"
        );
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic
                .contains("check replacement is blocked by unresolved native Goal admissions"),
            "{diagnostic}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), ledger_before);
        assert_eq!(
            run_entries(&f),
            entries_before,
            "refusal creates no claim or successor"
        );
        assert_eq!(fs::read(&journal_path).unwrap(), journal_with_admission);
        let latest: Value = serde_json::from_str(
            fs::read_to_string(&journal_path)
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert!(latest["obligations"]
            .to_string()
            .contains("Keep the original side obligation intact"));
    }
    assert_eq!(fs::read(f.ledger(admitted_work)).unwrap(), admitted_ledger);
}

#[test]
fn same_session_corrupt_native_companion_blocks_check_replacement() {
    let f = Fixture::new();
    let session = "r46-corrupt-companion-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let (work, _) = native_failed_work(&f, session, &store);
    let ledger_before = fs::read(f.ledger(&work)).unwrap();
    let entries_before = run_entries(&f);
    let companion = f.root.join(format!(
        ".exitbind/runs/work-{}.session-goal.json",
        &work[4..]
    ));
    assert!(
        companion.is_file(),
        "fixture has the native association companion"
    );
    fs::write(&companion, b"corrupted fixture companion\n").unwrap();
    let corrupted = fs::read(&companion).unwrap();

    let binding = f.ok_in_session(session, &store, &["work", "next", &work, "--json"])["current"]
        ["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let mut args = replacement_args(&work, &binding);
    args.push("--apply".into());
    let output = f.call_in_session(session, &store, &refs(&args));
    assert!(
        !output.status.success(),
        "corrupt companion must refuse: {output:?}"
    );
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("native Goal/session/project association is malformed or foreign; check replacement is blocked"),
        "{diagnostic}"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), ledger_before);
    assert_eq!(
        run_entries(&f),
        entries_before,
        "refusal creates no claim or successor"
    );
    assert_eq!(fs::read(&companion).unwrap(), corrupted);
}

#[test]
fn prepared_native_child_blocks_replacement_until_its_ownership_resolves() {
    let f = Fixture::new();
    let session = "r46-prepared-child-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    f.ok_in_session(
        session,
        &store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "r46-prepared-child-goal",
            "--goal",
            "Keep this native Goal bound while child work is prepared",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let begin = f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "inspect unresolved child ownership",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "child-review:Inspect unresolved child ownership.",
            "--preservation-check-command",
            "true",
        ],
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let initialization = serde_json::json!({
        "action":"init",
        "sourceRef":"owner:r46-child-review",
        "sourceText":"Inspect unresolved child ownership.",
        "requirements":[{"id":"child-review","text":"Inspect unresolved child ownership."}]
    });
    let initialized = f.call_with_input(
        &["work", "record", &work],
        initialization.to_string().as_bytes(),
    );
    assert!(initialized.status.success(), "{initialized:?}");
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
    );
    let worker = f.ok_in_session(session, &store, &["work", "next", &work, "--json"]);
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
    );
    let failed = f.call_in_session(
        session,
        &store,
        &["work", "check", &work, "--timeout-ms", "2000"],
    );
    assert!(!failed.status.success(), "{failed:?}");
    assert!(
        fs::read_to_string(f.ledger(&work))
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|event| event["action"] == "check" && event["result"]["code"] != 0),
        "fixture must retain an actual failed check before preparing a child"
    );
    let continuation = f.ok_in_session(session, &store, &["work", "continuation", &work]);
    let token = continuation["mutationContext"]["token"].as_str().unwrap();
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "bind",
            &work,
            "--context",
            token,
            "--host",
            "codex",
            "--session",
            session,
            "--host-version",
            "0.1-test",
        ],
    );
    let continuation = f.ok_in_session(session, &store, &["work", "continuation", &work]);
    let token = continuation["mutationContext"]["token"].as_str().unwrap();
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "child",
            "prepare",
            &work,
            "inspect retained state",
            "--context",
            token,
        ],
    );
    let children = f.ok_in_session(session, &store, &["work", "continuation", &work]);
    assert_eq!(children["preparedChildren"][0]["state"], "prepared");
    let current = f.ok_in_session(session, &store, &["work", "next", &work, "--json"]);
    let binding = current["current"]["binding"].as_str().unwrap();
    let before = fs::read(f.ledger(&work)).unwrap();
    let refused = f.call_in_session(
        session,
        &store,
        &[
            "work",
            "replace-check",
            &work,
            "--check-command",
            "true",
            "--reason",
            "a child remains prepared against the predecessor",
            "--classification",
            "defective-contract",
            "--current-binding",
            binding,
            "--apply",
        ],
    );
    assert!(
        !refused.status.success(),
        "a prepared child is an unresolved live continuation: {refused:?}"
    );
    let diagnostic = String::from_utf8_lossy(&refused.stderr);
    assert!(
        diagnostic
            .contains("check replacement is blocked by a live, prepared, or retained child result"),
        "unexpected earlier replacement guard; stderr={diagnostic}; stdout={}",
        String::from_utf8_lossy(&refused.stdout)
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "child ownership blocks successor creation before predecessor or successor mutation"
    );
}

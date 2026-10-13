//! Packaged CLI scenario with real checks. Role returns below are deterministic
//! fixtures, not claims of independent semantic review, model use or adoption.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const REQUIREMENTS: [(&str, &str); 3] = [
    ("R1", "Reject invalid input before persistence."),
    ("R2", "Persist each valid name only once."),
    ("R3", "Show the number of distinct imported names."),
];
const TERMINAL: &str = "+----------------------------+\n| This room remains nothing. |\n+----------------------------+\nEXIT READY";

struct Project {
    root: PathBuf,
    fixture_store: PathBuf,
    native_session: Option<(String, PathBuf)>,
}

type NativeStoreSnapshot = (bool, Vec<(String, Option<Vec<u8>>)>);
impl Project {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let mut init = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        init.args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .arg("--skip-skills");
        for name in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
            "CLAUDE_ENV_FILE",
            "XDG_STATE_HOME",
        ] {
            init.env_remove(name);
        }
        let output = support::run(&mut init);
        assert!(output.status.success(), "{output:?}");
        let name = root
            .file_name()
            .expect("fixture project directory name")
            .to_string_lossy();
        Self {
            fixture_store: root.with_file_name(format!("{name}-native-session-goals")),
            root,
            native_session: None,
        }
    }
    fn with_native_session(mut self, identity: &str) -> Self {
        self.set_native_session(identity);
        self
    }
    fn set_native_session(&mut self, identity: &str) {
        let name = self
            .root
            .file_name()
            .expect("fixture project directory name")
            .to_string_lossy();
        self.native_session = Some((
            identity.to_owned(),
            self.root
                .with_file_name(format!("{name}-native-session-goals")),
        ));
    }
    fn call(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .env_remove("EXITBIND_SESSION_GOALS_DIR")
            .env_remove("CLAUDE_ENV_FILE")
            .env_remove("XDG_STATE_HOME")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some((identity, store)) = &self.native_session {
            command
                .env("CODEX_THREAD_ID", identity)
                .env("EXITBIND_SESSION_GOALS_DIR", store);
        } else {
            command.env("EXITBIND_SESSION_GOALS_DIR", &self.fixture_store);
        }
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.stdin(Stdio::null());
            support::run(&mut command)
        }
    }
    fn value(&self, args: &[&str], input: Option<&str>) -> Value {
        let mut json_args = args.to_vec();
        if args.get(..2) == Some(&["goal", "close"][..]) && !json_args.contains(&"--json") {
            json_args.push("--json");
        }
        let output = self.call(&json_args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn reject(&self, args: &[&str], reason: &str) {
        let output = self.call(args, None);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason)
                || String::from_utf8_lossy(&output.stdout).contains(reason),
            "{output:?}"
        );
    }
    fn file(&self, name: &str, contents: &str) {
        fs::write(self.root.join(name), contents).unwrap();
    }
    fn run_entries(&self) -> Vec<String> {
        let mut entries = fs::read_dir(self.root.join(".exitbind/runs"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }
    fn goal_journal(&self) -> Option<Vec<u8>> {
        fs::read(self.root.join(".exitbind/session-goal.jsonl")).ok()
    }
    fn native_store_snapshot(&self) -> NativeStoreSnapshot {
        fn visit(
            root: &Path,
            path: &Path,
            output: &mut Vec<(String, Option<Vec<u8>>)>,
        ) -> std::io::Result<()> {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    output.push((relative, None));
                    visit(root, &path, output)?;
                } else {
                    output.push((relative, Some(fs::read(&path).unwrap())));
                }
            }
            Ok(())
        }
        let root = self
            .native_session
            .as_ref()
            .map(|(_, store)| store)
            .unwrap_or(&self.fixture_store);
        let mut output = Vec::new();
        let root_exists = root.exists();
        if root_exists {
            visit(root, root, &mut output).unwrap();
        }
        output.sort_by(|left, right| left.0.cmp(&right.0));
        (root_exists, output)
    }
    fn start_basis(&self, work: &str) -> String {
        let token = work.strip_prefix("smw_").expect("Work handle prefix");
        let ledger =
            fs::read_to_string(self.root.join(format!(".exitbind/runs/work-{token}.jsonl")))
                .expect("read actual Work ledger");
        let start: Value = serde_json::from_str(ledger.lines().next().expect("Work start line"))
            .expect("parse immutable Work start");
        serde_json::to_string(&start["basis"]).expect("serialize exact recorded basis")
    }
    fn assert_marker_refusal_without_effects(&self, args: &[&str]) {
        let runs = self.run_entries();
        let project_goal = self.goal_journal();
        let native_store = self.native_store_snapshot();
        let output = self.call(args, None);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(
                "journey integration scope must be selected through --journey-integration"
            ) || String::from_utf8_lossy(&output.stdout).contains(
                "journey integration scope must be selected through --journey-integration"
            ),
            "{output:?}"
        );
        assert_eq!(self.run_entries(), runs, "refusal must precede Work start");
        assert_eq!(
            self.goal_journal(),
            project_goal,
            "refusal must not mutate project Goal"
        );
        assert_eq!(
            self.native_store_snapshot(),
            native_store,
            "refusal must leave the entire native store unchanged, even without a synthetic identity"
        );
    }
    fn setup(&self) {
        self.file(
            "request.txt",
            &REQUIREMENTS
                .iter()
                .map(|(_, text)| *text)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.file("format.txt", ",");
        self.file("data.txt", "alpha,x\nalpha,y\nbeta,x\n");
        self.file(
            "validate.sh",
            "set -eu\nawk -F \"$(cat format.txt)\" '{if ($1 == \"bad\" || $1 == \"\") exit 2; print $1}'\n",
        );
        self.file(
            "persist.sh",
            "set -eu\ncat \"$1\" - | sort -u > \"$1.next\"\nmv \"$1.next\" \"$1\"\ncat \"$1\"\n",
        );
        self.file(
            "import.sh",
            "set -eu\nnames=$(sh validate.sh < \"$2\")\nif [ -n \"$names\" ]; then printf '%s\\n' \"$names\" | sh persist.sh \"$1\"; else cat \"$1\"; fi\n",
        );
        self.file("render.sh", "set -eu\nawk 'END {print NR \" items\"}'\n");
        self.file(
            "check.sh",
            r#"set -eu
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
db="$scratch/names"
: > "$db"
case "$1" in
R1) printf 'kept\n' > "$db"
    printf 'alpha\nbad\n' > "$scratch/input"
    if sh import.sh "$db" "$scratch/input" > /dev/null; then exit 3; fi
    test "$(cat "$db")" = kept
    printf 'alpha\n' > "$scratch/input"
    sh import.sh "$db" "$scratch/input" > /dev/null
    test "$(cat "$db")" = "$(printf 'alpha\nkept')" ;;
R2) printf 'alpha\nalpha\n' > "$scratch/input"
    sh import.sh "$db" "$scratch/input" > /dev/null
    sh import.sh "$db" "$scratch/input" > /dev/null
    test "$(cat "$db")" = alpha ;;
R3) test "$(printf 'alpha\nbeta\n' | sh render.sh)" = '2 items' ;;
all) sh check.sh R1; sh check.sh R2; sh check.sh R3
     sh import.sh "$db" data.txt > "$scratch/output"
     test "$(sh render.sh < "$scratch/output")" = '2 items' ;;
*) exit 4 ;;
esac
"#,
        );
        self.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "import",
                "--goal",
                "Import names safely and show a count",
                "--none-applicable",
                "findings,blockers,decisions,externalActions",
            ],
            None,
        );
        for (id, text) in REQUIREMENTS {
            self.value(
                &[
                    "goal",
                    "require",
                    "--goal-id",
                    "import",
                    "--requirement",
                    id,
                    "--obligation",
                    text,
                    "--artifact",
                    "request.txt",
                ],
                None,
            );
        }
        self.value(&["goal", "cover", "--goal-id", "import"], None);
    }
    fn status(&self) -> Value {
        self.value(&["goal", "status", "--json"], None)
    }
    fn begin(&self, ids: &str, command: &str) -> String {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            "Check the declared import requirements",
            "--goal-id",
            "import",
            "--requirement",
            ids,
            "--artifact",
            "check.sh",
            "--check-command",
            command,
            "--review-policy",
            "required",
        ];
        if command == "sh check.sh all" {
            args.extend(["--scope", "integration"]);
        }
        let started = self.value(&args, None);
        let work = started["work"].as_str().unwrap().to_owned();
        assert_eq!(started["next"]["goalProgress"]["decomposition"]["total"], 3);
        let current = self.value(&["work", "next", &work, "--json", "--full"], None);
        assert_eq!(
            started["next"]["current"]["binding"],
            current["next"]["current"]["binding"]
        );
        work
    }
    fn begin_ordinary(&self, goal: &str, command: &str, extra: &[&str]) -> String {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            command,
            "--review-policy",
            "required",
        ];
        args.extend_from_slice(extra);
        self.value(&args, None)["work"].as_str().unwrap().to_owned()
    }
    fn return_stage(&self, work: &str, next: &Value, outcome: &str) {
        self.value(
            &[
                "work",
                "return",
                work,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            Some("Deterministic process fixture decision; not a semantic review.\n"),
        );
    }
    fn drive(&self, work: &str, passing: bool) {
        for _ in 0..12 {
            let detail = self.value(&["work", "next", work, "--json", "--full"], None);
            let next = &detail["next"];
            match next["action"].as_str().unwrap() {
                "lead_decision" => {
                    let scoped = next["outcomes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item == "scoped");
                    self.return_stage(work, next, if scoped { "scoped" } else { "accepted" });
                }
                "spawn" if next["role"] == "worker" => {
                    self.value(
                        &[
                            "work",
                            "permit",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--operation",
                            "Return fixture evidence",
                        ],
                        None,
                    );
                    self.return_stage(work, next, "completed");
                }
                "spawn" => self.return_stage(work, next, "approved"),
                "check" => {
                    let output = self.call(&["work", "check", work], None);
                    assert_eq!(output.status.success(), passing, "{output:?}");
                    let checked: Value = serde_json::from_slice(&output.stdout).unwrap();
                    assert_eq!(checked["effect"], "recorded");
                    if !passing {
                        return;
                    }
                }
                "done" => return,
                action => panic!("unexpected {action}: {detail}"),
            }
        }
        panic!("fixture did not finish");
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
        let store = self
            .native_session
            .as_ref()
            .map(|(_, store)| store)
            .unwrap_or(&self.fixture_store);
        if store.exists() {
            fs::remove_dir_all(store).unwrap();
        }
    }
}
fn row(status: &Value, id: &str) -> Value {
    status["requirements"]["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap()
        .clone()
}

fn incorporate_native_generic_goal(project: &Project, goal_id: &str, goal: &str) -> Value {
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            goal_id,
            "--goal",
            goal,
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    )
}

#[test]
fn native_generic_owner_close_and_status_render_only_verified_main_and_side_work() {
    let mut p = Project::new("native-generic-owner-terminal");

    // This accepted Work predates the Goal and remains an explicit closure
    // result only; it is never retroactively inserted into journey membership.
    p.file("history.txt", "original evidence\n");
    let historical = p.begin_ordinary(
        "Validate the existing main-task evidence",
        "test \"$(cat history.txt)\" = 'original evidence'",
        &[],
    );
    p.drive(&historical, true);
    p.set_native_session("native-generic-owner-terminal-session");
    let empty = p.status();
    assert!(empty["presentation"]["terminal"].is_null());

    let incorporated = incorporate_native_generic_goal(
        &p,
        "native-main",
        "Complete the main task and all side tasks in this conversation",
    );
    assert_eq!(incorporated["journeyMembershipProtocol"], 2);
    assert!(incorporated["journeyMembers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(incorporated.get("requirements").is_none());

    let side = p.begin_ordinary("Complete a side task in this conversation", "true", &[]);
    p.drive(&side, true);
    let current = p.status();
    let journey = &current["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "current");
    assert_eq!(journey["members"]["total"], 1);
    assert_eq!(journey["members"]["current"], 1);
    let members = current["journeyMembers"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["work"], side);
    assert!(!members.iter().any(|member| member["work"] == historical));
    assert!(current.get("requirements").is_none());

    let close = p.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        None,
    );
    assert!(close.status.success(), "{close:?}");
    assert_eq!(
        String::from_utf8(close.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );
    let closed = p.status();
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["currentReadiness"]["state"], "current");
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    assert!(closed.get("requirements").is_none());
    let status = p.call(&["goal", "status"], None);
    assert!(status.status.success(), "{status:?}");
    assert_eq!(
        String::from_utf8(status.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );

    // The explicit legacy selector keeps project-local status and closure
    // separate from this native session journal.
    let local = p.value(&["goal", "status", "--project", "--json"], None);
    assert!(local["presentation"]["terminal"].is_null());
    p.reject(
        &[
            "goal",
            "close",
            "--project",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        "no canonical session goal is open",
    );

    // A different configured project can read only the context summary and
    // cannot close the owner journal.
    let (identity, store) = p.native_session.as_ref().unwrap();
    let mut child = Project::new("native-generic-nonowner");
    child.native_session = Some((identity.clone(), store.clone()));
    let child_status = child.value(&["goal", "status", "--json"], None);
    assert_eq!(child_status["contextOnly"], true);
    assert_eq!(child_status["authority"], "none");
    assert!(child_status["presentation"]["terminal"].is_null());
    child.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        "owned by another configured project",
    );
    child.native_session = None;

    // Starting a later failed side task reopens whole-journey readiness;
    // the prior closure remains historical and cannot emit the terminal.
    let pending = p.begin_ordinary("A later side task remains unfinished", "false", &[]);
    p.drive(&pending, false);
    let blocked = p.status();
    assert_eq!(blocked["closure"]["closed"], false);
    assert_eq!(
        blocked["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert!(blocked["presentation"]["terminal"].is_null());
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        "journey",
    );
}

#[test]
fn native_generic_owner_terminal_suppresses_stale_unadmitted_result_proof() {
    let mut p = Project::new("native-generic-stale-proof");
    p.file("proof.txt", "original\n");
    let historical = p.begin_ordinary(
        "Validate the original proof source",
        "test \"$(cat proof.txt)\" = original",
        &[],
    );
    p.drive(&historical, true);
    p.set_native_session("native-generic-stale-proof-session");

    let incorporated = incorporate_native_generic_goal(
        &p,
        "proof-goal",
        "Finish the main task using current accepted evidence",
    );
    assert!(incorporated["journeyMembers"]
        .as_array()
        .unwrap()
        .is_empty());
    let closed = p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "proof-goal",
            "--result-ref",
            &historical,
        ],
        None,
    );
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    assert_eq!(
        p.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    p.file("proof.txt", "changed after acceptance\n");
    let stale = p.status();
    assert_eq!(stale["closure"]["closed"], true);
    assert_eq!(stale["currentReadiness"]["state"], "stale");
    assert_eq!(
        stale["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert!(stale["presentation"]["terminal"].is_null());
}

#[test]
fn native_direct_closure_does_not_expose_the_governed_terminal() {
    let p = Project::new("native-direct-terminal-boundary")
        .with_native_session("native-direct-terminal-boundary-session");
    p.value(
        &[
            "goal",
            "incorporate",
            "--direct",
            "--goal-id",
            "direct-goal",
            "--goal",
            "Complete this direct task",
            "--finding",
            "The lead completed the direct task",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "direct-goal",
            "--goal",
            "Complete this direct task",
            "--none-applicable",
            "obligations,blockers,decisions,externalActions",
        ],
        None,
    );
    let closed = p.value(
        &["goal", "close", "--direct", "--goal-id", "direct-goal"],
        None,
    );
    assert_eq!(closed["closure"]["kind"], "direct");
    assert!(closed["presentation"]["terminal"].is_null());
    let status = p.status();
    assert_eq!(status["closure"]["kind"], "direct");
    assert!(status["presentation"]["terminal"].is_null());
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("EXIT READY"));
}

#[test]
fn three_requirements_multiple_works_interruption_drift_composition_and_closure() {
    let p = Project::new("goal-import-scenario");
    p.setup();
    assert_eq!(row(&p.status(), "R3")["state"], "unmapped");
    let a = p.begin("R1,R2", "sh check.sh R1 && sh check.sh R2");
    p.drive(&a, true);
    let partial = p.status();
    assert_eq!(row(&partial, "R1")["state"], "current");
    assert_eq!(row(&partial, "R2")["state"], "current");
    assert_eq!(row(&partial, "R3")["state"], "unmapped");
    assert_eq!(partial["requirements"]["coverageCurrent"], false);
    assert!(partial["presentation"]["terminal"].is_null());
    p.reject(
        &["goal", "close", "--goal-id", "import", "--result-ref", &a],
        "coverage",
    );
    // Every call is a new CLI process. Recovery uses only persisted supported views.
    let recovered = p.status();
    assert_eq!(recovered["revision"], partial["revision"]);
    let detail = p.value(&["work", "next", &a, "--json", "--full"], None);
    assert_eq!(detail["next"]["goalProgress"]["decomposition"]["total"], 3);
    assert!(detail["presentation"]["terminal"].is_null());
    let compact = p.value(&["work", "next", &a, "--json"], None);
    assert!(compact["presentation"]["terminal"].is_null());
    let b = p.begin("R3", "sh check.sh R3");
    p.drive(&b, true);
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    assert!(p.status()["presentation"]["terminal"].is_null());
    p.reject(
        &["goal", "close", "--goal-id", "import", "--result-ref", &b],
        "integration Work",
    );
    // The producer now uses another format. Unit checks still pass, composition fails.
    p.file("format.txt", "|");
    assert_eq!(row(&p.status(), "R1")["state"], "stale");
    let c = p.begin("R1,R2", "sh check.sh R1 && sh check.sh R2");
    p.drive(&c, true);
    let d = p.begin("R3", "sh check.sh R3");
    p.drive(&d, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2",
            "--result-ref",
            &c,
            "--disposition",
            "replace",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R3",
            "--result-ref",
            &d,
            "--disposition",
            "replace",
        ],
        None,
    );
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    let failed = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&failed, false);
    assert_eq!(row(&p.status(), "R1")["state"], "failed");
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &failed,
        ],
        "coverage",
    );
    // Repair shared data, create a legitimate validation Work, keep old Works immutable.
    p.file("data.txt", "alpha|x\nalpha|y\nbeta|x\n");
    let integrated = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&integrated, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integrated,
            "--disposition",
            "replace",
        ],
        None,
    );
    let before_repeat = p.status()["revision"].clone();
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integrated,
            "--disposition",
            "replace",
        ],
        None,
    );
    assert_eq!(p.status()["revision"], before_repeat);
    let close = p.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &integrated,
        ],
        None,
    );
    assert!(close.status.success(), "{close:?}");
    assert_eq!(
        String::from_utf8(close.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );
    let closed = p.status();
    assert_eq!(closed["currentReadiness"]["state"], "current");
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    let work = p.value(&["work", "next", &integrated, "--json"], None);
    let full = p.value(&["work", "next", &integrated, "--json", "--full"], None);
    let after_read = p.status();
    assert_eq!(
        work["presentation"]["terminal"],
        TERMINAL,
        "{}",
        json!({"compactPresentation":work["presentation"],
            "fullPresentation":full["presentation"], "currentReadiness":after_read["currentReadiness"],
            "goalTerminal":after_read["presentation"]["terminal"], "reason":work["reason"],
            "work":integrated, "mappedWorks":after_read["continuation"]["mappings"]
                .as_array().map(|items|items.iter().map(|item|item["work"].clone()).collect::<Vec<_>>())})
    );
    assert_eq!(full["presentation"]["terminal"], TERMINAL);
    let repeat = p.call(&["goal", "status"], None);
    assert!(repeat.status.success(), "{repeat:?}");
    assert_eq!(
        String::from_utf8(repeat.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );
    assert_eq!(p.status()["revision"], closed["revision"]);
    for (id, _) in REQUIREMENTS {
        let evidence = &row(&closed, id)["support"][0]["evidence"];
        assert_eq!(evidence["check"]["acquisition"], "observed");
        assert_eq!(evidence["accepted"], true);
    }
    println!("Process-only scenario: 3 requirements; multi-Work; partial; process recovery; shared-input drift; failed composition; new checked integration; fixture review/Lead; explicit current closure.");
    p.file("format.txt", ",");
    let stale = p.status();
    assert_eq!(stale["closure"], closed["closure"]);
    assert_eq!(stale["currentReadiness"]["state"], "stale");
    assert!(stale["presentation"]["terminal"].is_null());
    let work = p.value(&["work", "next", &integrated, "--json"], None);
    assert!(work["presentation"]["terminal"].is_null());
}

#[test]
fn correction_retains_meaning_history_and_refuses_unrelated_or_old_work() {
    let p = Project::new("goal-correction");
    p.setup();
    let a = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&a, true);
    let before = p.status()["revision"].clone();
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            REQUIREMENTS[0].1,
            "--artifact",
            "request.txt",
        ],
        None,
    );
    assert_eq!(p.status()["revision"], before);
    let corrected = "Reject malformed input before persistence.";
    p.file(
        "request.txt",
        &format!(
            "{corrected}\n{}\n{}\n",
            REQUIREMENTS[1].1, REQUIREMENTS[2].1
        ),
    );
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            corrected,
            "--artifact",
            "request.txt",
        ],
        None,
    );
    let changed = p.status();
    assert_eq!(row(&changed, "R1")["revision"], 2);
    assert_eq!(row(&changed, "R1")["history"][0]["text"], REQUIREMENTS[0].1);
    assert_eq!(changed["goalProgress"]["decomposition"]["total"], 3);
    assert_eq!(changed["requirements"]["coverageConfirmed"], false);
    p.reject(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--result-ref",
            &a,
        ],
        "exact requirement revision",
    );
    let unrelated = p.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "unrelated pass",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
        None,
    )["work"]
        .as_str()
        .unwrap()
        .to_owned();
    p.drive(&unrelated, true);
    p.reject(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R2",
            "--result-ref",
            &unrelated,
        ],
        "frozen named requirement",
    );
    p.reject(
        &["goal", "close", "--goal-id", "import", "--direct"],
        "named requirements",
    );
}

#[test]
fn journey_integration_refreshes_only_the_frozen_accepted_member_set() {
    let p = Project::new("journey-integration-freeze")
        .with_native_session("journey-integration-session");
    p.setup();

    let first = p.begin("R1", "sh check.sh R1");
    p.drive(&first, true);
    assert_eq!(
        p.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    // A later source change makes the accepted first Work historically valid
    // but no longer current against this project's checked inputs.
    p.file("format.txt", "|");
    p.file("data.txt", "alpha|x\nalpha|y\nbeta|x\n");
    let integration = p.begin_ordinary(
        "Revalidate the accepted conversation work on current inputs",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--scope",
            "integration",
            "--journey-integration",
        ],
    );
    p.drive(&integration, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integration,
            "--disposition",
            "replace",
        ],
        None,
    );
    let supported = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(
        supported["state"], "current",
        "journey after integration: {supported}"
    );
    assert_eq!(supported["members"]["total"], 1);
    assert_eq!(supported["members"]["current"], 0);
    assert_eq!(supported["members"]["integrationSupported"], 1);
    assert_eq!(supported["integrationWork"], integration);

    // A later ordinary side Work is new membership, so the earlier frozen
    // integration no longer covers the complete journey.
    let second = p.begin_ordinary(
        "Complete a separate side task in the same conversation",
        "true",
        &[],
    );
    p.drive(&second, true);
    let expanded = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(expanded["state"], "blocked");
    assert_eq!(expanded["members"]["total"], 2);
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &integration,
        ],
        "journey",
    );

    let updated = p.begin_ordinary(
        "Revalidate every accepted member after the side task",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--scope",
            "integration",
            "--journey-integration",
        ],
    );
    p.drive(&updated, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &updated,
            "--disposition",
            "replace",
        ],
        None,
    );
    let current = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(current["state"], "current");
    assert_eq!(current["members"]["total"], 2);
    assert_eq!(current["members"]["integrationSupported"], 1);
    assert_eq!(current["integrationWork"], updated);
    let closed = p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &updated,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);

    // A new unaccepted member cannot be included by starting integration.
    let pending = p.begin_ordinary("A final side task still needs acceptance", "false", &[]);
    p.drive(&pending, false);
    let blocked = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(blocked["state"], "blocked");
    let after_pending = p.status();
    assert_eq!(after_pending["closure"]["closed"], false);
    assert!(after_pending["presentation"]["terminal"].is_null());
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "This must not invent acceptance for the pending task",
            "--check-command",
            "sh check.sh all",
            "--artifact",
            "check.sh",
            "--journey-integration",
            "--review-policy",
            "required",
        ],
        "no terminal accepted result",
    );
}

#[test]
fn native_store_snapshot_distinguishes_missing_and_empty_roots() {
    let project = Project::new("native-store-empty-root-snapshot");
    let missing = project.native_store_snapshot();
    assert_eq!(missing, (false, Vec::new()));

    fs::create_dir_all(&project.fixture_store).unwrap();
    let empty = project.native_store_snapshot();
    assert_eq!(empty, (true, Vec::new()));
    assert_ne!(missing, empty);
}

#[test]
fn copied_journey_basis_markers_are_rejected_before_any_work_or_goal_mutation() {
    let p = Project::new("journey-marker-origin-boundary")
        .with_native_session("journey-marker-origin-session");
    p.file(
        "request.txt",
        "R1: reject invalid input before persistence.\n",
    );
    p.file("check.sh", "set -eu\nexit 0\n");
    p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "import",
            "--goal",
            "Validate input safely",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            "R1: reject invalid input before persistence.",
            "--artifact",
            "request.txt",
        ],
        None,
    );
    p.value(&["goal", "cover", "--goal-id", "import"], None);

    let first = p.begin_ordinary("Accepted main conversation task", "sh check.sh", &[]);
    p.drive(&first, true);
    let integration = p.begin_ordinary(
        "Create a legitimate frozen journey integration basis",
        "sh check.sh",
        &["--artifact", "check.sh", "--journey-integration"],
    );
    let basis = p.start_basis(&integration);
    let parsed_basis: Value = serde_json::from_str(&basis).unwrap();
    let constraints = parsed_basis["constraints"].as_array().unwrap();
    assert!(constraints.iter().any(|item| item
        .as_str()
        .is_some_and(|value| { value.starts_with("exitbind.session-journey-integration.") })));
    assert!(constraints.iter().all(|item| !item
        .as_str()
        .is_some_and(|value| value.starts_with("exitbind.goal-support.v1:"))));

    p.value(
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "legacy",
            "--goal",
            "Project-local named contract",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "require",
            "--project",
            "--goal-id",
            "legacy",
            "--requirement",
            "P1",
            "--obligation",
            "R1: reject invalid input before persistence.",
            "--artifact",
            "request.txt",
        ],
        None,
    );
    p.value(&["goal", "cover", "--project", "--goal-id", "legacy"], None);

    let ordinary = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&ordinary);

    let standalone = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--standalone",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&standalone);

    let native_named = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--goal-id",
        "import",
        "--requirement",
        "R1",
        "--artifact",
        "check.sh",
        "--check-command",
        "sh check.sh R1",
        "--review-policy",
        "required",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&native_named);

    let project_named = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--goal-id",
        "legacy",
        "--requirement",
        "P1",
        "--artifact",
        "check.sh",
        "--check-command",
        "sh check.sh R1",
        "--review-policy",
        "required",
        "--project-goal",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&project_named);

    let no_identity = Project::new("journey-marker-no-native-identity");
    no_identity.assert_marker_refusal_without_effects(&ordinary);
    let native_without_goal = Project::new("journey-marker-native-without-goal")
        .with_native_session("journey-marker-native-without-goal-session");
    native_without_goal.assert_marker_refusal_without_effects(&ordinary);
}

#[test]
fn a_closed_predecessor_cannot_emit_terminal_after_a_new_session_goal_is_incorporated() {
    let p = Project::new("journey-successor-suppresses-old-room")
        .with_native_session("journey-successor-session");
    p.setup();
    let work = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&work, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &work,
            "--disposition",
            "replace",
        ],
        None,
    );
    let closed = p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    assert_eq!(p.status()["presentation"]["terminal"], TERMINAL);
    let prior_next = p.value(&["work", "next", &work, "--json", "--full"], None);
    assert_eq!(prior_next["presentation"]["terminal"], TERMINAL);

    let successor = p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "successor",
            "--goal",
            "A later unresolved side request in the same conversation",
            "--obligation",
            "The later request still needs work",
        ],
        None,
    );
    assert_eq!(successor["journeyMembers"].as_array().unwrap().len(), 1);
    assert_eq!(successor["journeyMembers"][0]["work"], work);
    assert_eq!(successor["journeyMembers"][0]["parentGoalId"], "import");

    let old_next = p.value(&["work", "next", &work, "--json", "--full"], None);
    let old_detail = p.value(&["work", "detail", &work, "--json"], None);
    assert!(old_next["presentation"]["terminal"].is_null());
    assert_eq!(
        old_next["presentation"]["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(old_detail["goalAssociation"]["kind"], "context_only");
    assert_eq!(
        old_detail["sections"]["tasks"]["goal"],
        "Import names safely and show a count"
    );
    assert_eq!(old_detail["sections"]["tasks"]["overall"], "in_progress");
    assert_eq!(p.status()["goalId"], "successor");
    assert!(p.status()["presentation"]["terminal"].is_null());
}

#[test]
fn checker_coverage_and_intake_are_guarded_before_start() {
    let p = Project::new("goal-checker-coverage");
    p.reject(
        &[
            "goal",
            "require",
            "--goal-id",
            "missing",
            "--requirement",
            "R1",
            "--obligation",
            "x",
            "--artifact",
            "missing.txt",
        ],
        "source",
    );
    p.setup();
    p.reject(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R4",
            "--obligation",
            "invented requirement",
            "--artifact",
            "request.txt",
        ],
        "exact excerpt",
    );
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "wrong checker",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
        "name its declared",
    );
    let mut git = Command::new("git");
    git.current_dir(&p.root).args(["init", "-q"]);
    assert!(support::run(&mut git).status.success());
    p.file(".gitignore", "check.sh\n");
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "ignored checker",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh R1",
            "--review-policy",
            "required",
        ],
        "outside tested",
    );
    assert_eq!(row(&p.status(), "R1")["state"], "unmapped");
    for child in ["require", "cover", "assign", "status"] {
        let help = p.call(&["goal", child, "--help"], None);
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains(child));
    }
}

#[test]
fn relocated_project_cannot_reuse_a_named_goal_identity() {
    let mut p = Project::new("goal-project-identity");
    p.setup();
    let relocated = support::temp("goal-other-root");
    fs::remove_dir(&relocated).unwrap();
    fs::rename(&p.root, &relocated).unwrap();
    p.root = relocated;
    p.reject(&["goal", "status", "--json"], "another project");
}

#[test]
fn full_mapping_of_individual_passes_is_not_an_integration_declaration() {
    let p = Project::new("goal-integration-declaration");
    p.setup();
    let work = p.begin(
        "R1,R2,R3",
        "sh check.sh R1 && sh check.sh R2 && sh check.sh R3",
    );
    p.drive(&work, true);
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        "integration Work",
    );
    let next = p.status()["requirements"]["nextAction"]["command"].clone();
    assert!(next
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "integration"));
}

#[test]
fn ordinary_work_without_basis_has_no_named_projection_diagnostic() {
    let p = Project::new("ordinary-work-without-basis");
    let work = p.begin_ordinary("Make an ordinary change", "true", &[]);
    let detail = p.value(&["work", "detail", &work, "--json"], None);
    assert_eq!(detail["valid"], true);
    assert_eq!(detail["complete"], true);
    assert!(
        detail.get("projectGoalProgress").is_none(),
        "absent optional named-contract metadata is not malformed: {}",
        detail["projectGoalProgress"]
    );
}

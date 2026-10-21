//! Packaged CLI scenario with real checks. Role returns below are deterministic
//! fixtures, not claims of independent semantic review, model use or adoption.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
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
        let mut init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
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
        self.call_with_config(args, input, &self.root.join("exitbind.json"))
    }
    fn call_with_config(&self, args: &[&str], input: Option<&str>, config: &Path) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(config)
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
    fn call_exact_argv(&self, argv: &[String]) -> Output {
        let (executable, args) = argv
            .split_first()
            .expect("an emitted exact command has an executable");
        let mut command = support::git_topology::command(executable);
        command
            .current_dir(&self.root)
            .args(args)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .env_remove("EXITBIND_SESSION_GOALS_DIR")
            .env_remove("CLAUDE_ENV_FILE")
            .env_remove("XDG_STATE_HOME")
            .stdin(Stdio::null());
        if let Some((identity, store)) = &self.native_session {
            command
                .env("CODEX_THREAD_ID", identity)
                .env("EXITBIND_SESSION_GOALS_DIR", store);
        } else {
            command.env("EXITBIND_SESSION_GOALS_DIR", &self.fixture_store);
        }
        support::run(&mut command)
    }
    fn call_with_selectors(
        &self,
        args: &[&str],
        config: &Path,
        selectors: &[(&str, &str)],
    ) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(config)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .env_remove("EXITBIND_SESSION_GOALS_DIR")
            .env_remove("CLAUDE_ENV_FILE")
            .env_remove("XDG_STATE_HOME")
            .env(
                "EXITBIND_SESSION_GOALS_DIR",
                self.native_session
                    .as_ref()
                    .map(|(_, store)| store)
                    .unwrap_or(&self.fixture_store),
            )
            .stdin(Stdio::null());
        for (name, value) in selectors {
            command.env(name, value);
        }
        support::run(&mut command)
    }
    fn value(&self, args: &[&str], input: Option<&str>) -> Value {
        self.value_with_config(args, input, &self.root.join("exitbind.json"))
    }
    fn value_with_config(&self, args: &[&str], input: Option<&str>, config: &Path) -> Value {
        let mut json_args = args.to_vec();
        if args.get(..2) == Some(&["goal", "close"][..]) && !json_args.contains(&"--json") {
            json_args.push("--json");
        }
        let output = self.call_with_config(&json_args, input, config);
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
                // The human display projection is not the Goal journal.
                if path.parent() == Some(root) && entry.file_name() == "display" {
                    continue;
                }
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
    fn return_stage_with_config(&self, work: &str, next: &Value, outcome: &str, config: &Path) {
        self.value_with_config(
            &[
                "work",
                "return",
                work,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            Some("Deterministic process fixture decision; not a semantic review.\n"),
            config,
        );
    }
    fn drive(&self, work: &str, passing: bool) {
        self.drive_with_config(work, passing, &self.root.join("exitbind.json"));
    }
    fn drive_with_config(&self, work: &str, passing: bool, config: &Path) {
        for _ in 0..12 {
            let detail =
                self.value_with_config(&["work", "next", work, "--json", "--full"], None, config);
            let next = &detail["next"];
            match next["action"].as_str().unwrap() {
                "lead_decision" => {
                    let scoped = next["outcomes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item == "scoped");
                    self.return_stage_with_config(
                        work,
                        next,
                        if scoped { "scoped" } else { "accepted" },
                        config,
                    );
                }
                "spawn" if next["role"] == "worker" => {
                    let current =
                        self.value_with_config(&["work", "detail", work, "--json"], None, config);
                    if current["actionForms"]["recovery"]["state"] == "replan_required" {
                        let form = &current["actionForms"]["recovery"];
                        assert_eq!(form["command"]["sameConfigRequired"], true);
                        let argv = form["command"]["argv"].as_array().unwrap();
                        let mut args = Vec::new();
                        for arg in argv.iter().skip(1) {
                            let arg = arg.as_str().unwrap();
                            if arg == "--config" {
                                break;
                            }
                            args.push(match arg {
                                "<HYPOTHESIS>" => {
                                    "The selected successor needs its own current check.".to_owned()
                                }
                                "<SCOPE_DECISION>" => {
                                    "Keep the same task Goal and selected project roots.".to_owned()
                                }
                                "<EVIDENCE_REQUEST>" => {
                                    "Run its declared check and inspect accepted evidence."
                                        .to_owned()
                                }
                                other => other.to_owned(),
                            });
                        }
                        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
                        self.value_with_config(&refs, None, config);
                        continue;
                    }
                    self.value_with_config(
                        &[
                            "work",
                            "permit",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--operation",
                            "Return fixture evidence",
                        ],
                        None,
                        config,
                    );
                    self.return_stage_with_config(work, next, "completed", config);
                }
                "spawn" => self.return_stage_with_config(work, next, "approved", config),
                "check" => {
                    let output = self.call_with_config(&["work", "check", work], None, config);
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

#[path = "goal_requirements/checker_derivation.rs"]
mod checker_derivation;
#[path = "goal_requirements/closure_flows.rs"]
mod closure_flows;
#[path = "goal_requirements/correction_and_journey.rs"]
mod correction_and_journey;
#[path = "goal_requirements/emitted_routes.rs"]
mod emitted_routes;
#[path = "goal_requirements/intake_and_identity.rs"]
mod intake_and_identity;
#[path = "goal_requirements/native_recovery.rs"]
mod native_recovery;
#[path = "goal_requirements/refuse_before_write.rs"]
mod refuse_before_write;
#[path = "goal_requirements/setup_first_work.rs"]
mod setup_first_work;
#[path = "goal_requirements/setup_start_path.rs"]
mod setup_start_path;
#[path = "goal_requirements/successor_timing.rs"]
mod successor_timing;

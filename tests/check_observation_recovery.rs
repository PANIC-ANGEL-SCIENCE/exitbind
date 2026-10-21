//! Real candidate Work transitions; deterministic role returns prove mechanics.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    native_session: Option<String>,
}
impl Fixture {
    fn new(script: &str) -> Self {
        let root = support::temp("check-observation");
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        fs::write(root.join("checker.sh"), script).unwrap();
        Self {
            root,
            native_session: None,
        }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .env("EXITBIND_NO_UPDATE_CHECK", "1");
        self.native_environment(&mut command);
        command
    }
    fn native_environment(&self, command: &mut Command) {
        if let Some(session) = &self.native_session {
            command.env("CODEX_THREAD_ID", session).env(
                "EXITBIND_SESSION_GOALS_DIR",
                self.root.join(".exitbind/native-goals"),
            );
        }
    }
    fn enable_owned_outputs(&self) {
        let path = self.root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["project"]["verificationPreparation"] = json!({
            "version": 1,
            "pythonBytecode": "disabled",
            "scratch": "taskOwned"
        });
        fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    }
    fn call(&self, args: &[&str], body: &[u8]) -> Output {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(body) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }
    fn emitted(&self, command: &Value, body: &[u8]) -> Output {
        let argv = command["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(argv.len() > 1, "{command}");
        let mut command = support::git_topology::command(argv[0]);
        command.current_dir(&self.root).args(&argv[1..]);
        self.native_environment(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(body) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], body: &[u8]) -> Value {
        let output = self.call(args, body);
        assert!(output.status.success(), "{args:?}: {output:?}");
        value(&output)
    }
    fn next(&self, work: &str) -> Value {
        self.ok(&["work", "next", work, "--full"], b"")["next"].clone()
    }
    fn give(&self, work: &str, action: &Value, outcome: &str) -> Value {
        self.ok(
            &[
                "work",
                "return",
                work,
                action["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            outcome.as_bytes(),
        )
    }
    fn begin(&self) -> String {
        self.begin_with_optional_basis(None)
    }
    fn begin_with_optional_basis(&self, basis: Option<&str>) -> String {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            "repair the checker",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
        ];
        if let Some(basis) = basis {
            args.extend(["--basis", basis]);
        }
        let start = self.ok(&args, b"");
        let work = start["work"].as_str().unwrap().to_string();
        let worker = self.give(&work, &start["next"], "scoped")["next"].clone();
        self.give(&work, &worker, "completed");
        work
    }
    fn events(&self, work: &str) -> Vec<Value> {
        fs::read_to_string(self.ledger(work))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn ledger(&self, work: &str) -> PathBuf {
        self.root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))
    }
    fn repair(&self, work: &str) -> Value {
        let lead = self.next(work);
        self.ok(
            &[
                "work",
                "disposition",
                work,
                lead["assignment"].as_str().unwrap(),
                "--decision",
                "repair",
                "--reason",
                "checker defect inside accepted task",
                "--repair-boundary",
                "checker.sh only",
                "--regression",
                "short observed deadline and successful repaired checker",
            ],
            b"",
        )["next"]
            .clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained failed fixture: {}", self.root.display());
            return;
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}

fn find_named_file(root: &std::path::Path, name: &str) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).ok()? {
            let entry = entry.ok()?;
            if entry.file_name() == name {
                return Some(entry.path());
            }
            if entry.file_type().ok()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    None
}

fn rehash(value: &mut Value) {
    use sha2::{Digest, Sha256};
    value.as_object_mut().unwrap().remove("eventSha256");
    value["eventSha256"] = json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).unwrap())
    ));
}

fn storage_interruption() -> (Fixture, String, Value, Vec<u8>) {
    storage_interruption_with_preparation(false)
}

fn storage_interruption_with_preparation(selected: bool) -> (Fixture, String, Value, Vec<u8>) {
    // A real post-admission storage failure on every supported Unix host,
    // including privileged test runners: make the ledger directory unavailable
    // while the checker runs, then restore the exact original bytes after its
    // observer has returned. No accepted event or process fact is constructed.
    let script = if selected {
        "printf ran >> .exitbind/attempt-count; printf owned > \"$EXITBIND_VERIFICATION_SCRATCH/prepared-effect\"; mv .exitbind/runs .exitbind/runs-retained; printf unavailable > .exitbind/runs; printf retained-output; exit 0\n"
    } else {
        "printf ran >> .exitbind/attempt-count; mv .exitbind/runs .exitbind/runs-retained; printf unavailable > .exitbind/runs; printf retained-output; exit 0\n"
    };
    let f = Fixture::new(script);
    if selected {
        f.enable_owned_outputs();
    }
    let work = f.begin();
    let failed = f.call(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert!(!failed.status.success(), "{failed:?}");
    let failure = value(&failed);
    assert_eq!(failure["durableFailureRecorded"], false);
    assert_eq!(
        failure["reason"]["code"],
        "observation_failure_storage_unavailable"
    );
    assert_eq!(failure["observation"]["termination"], "ended");
    fs::remove_file(f.root.join(".exitbind/runs")).unwrap();
    fs::rename(
        f.root.join(".exitbind/runs-retained"),
        f.root.join(".exitbind/runs"),
    )
    .unwrap();
    let prefix = fs::read(f.ledger(&work)).unwrap();
    let mut d = f.ok(&["work", "recover-check", &work, "--json"], b"")["ownerDecision"].clone();
    d["approved"] = json!(true);
    d["reason"] = json!(
        "Observed checker and capture readers ended; restore storage and record failure only"
    );
    d["response"] = json!(String::from_utf8(failed.stdout).unwrap());
    response_hash(&mut d);
    (f, work, d, prefix)
}

fn response_hash(d: &mut Value) {
    use sha2::{Digest, Sha256};
    d["responseSha256"] = json!(format!(
        "{:x}",
        Sha256::digest(d["response"].as_str().unwrap().as_bytes())
    ));
}

fn recover(f: &Fixture, work: &str, d: &Value) -> Output {
    let preview = f.ok(&["work", "recover-check", work, "--json"], b"");
    recover_with_preview(f, &preview, d)
}

fn recover_with_preview(f: &Fixture, preview: &Value, d: &Value) -> Output {
    assert_eq!(preview["effect"], "no-change");
    assert_eq!(preview["ownerDecision"]["approved"], false);
    let apply = &preview["apply"];
    assert_eq!(apply["stdin"]["required"], true);
    assert!(apply["command"]["sameConfigRequired"].is_boolean());
    assert!(apply["command"]["sameExecutableRequired"].is_boolean());
    let argv = apply["command"]["argv"].as_array().unwrap();
    assert!(argv.iter().any(|arg| arg == "--apply"));
    let binding_index = argv
        .iter()
        .position(|arg| arg == "--current-binding")
        .unwrap();
    assert_eq!(
        argv[binding_index + 1],
        preview["ownerDecision"]["currentBinding"]
    );
    f.emitted(&apply["command"], &serde_json::to_vec(d).unwrap())
}

fn select_fixture_contract(f: &Fixture) {
    // The combined candidate must retain this blocker with a selected contract.
    fs::write(f.root.join("architecture.json"), json!({"version": 1, "revision": "live-effect-v1",
        "responsibilities": [{"id": "checker", "summary": "owned checker", "paths": ["checker.sh"]}],
        "dependencies": [], "interfaces": [], "checks": []}).to_string()).unwrap();
    let preview = f.ok(
        &[
            "project",
            "architecture",
            "select",
            "architecture.json",
            "--decision",
            "reviewed",
            "--reason",
            "fixture project review",
            "--json",
        ],
        b"",
    );
    let args = preview["apply"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    let applied = support::git_topology::command(args[0])
        .args(&args[1..])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(applied.status.success(), "{applied:?}");
}

#[path = "check_observation_recovery/admission_and_recovery.rs"]
mod admission_and_recovery;
#[path = "check_observation_recovery/drift_and_retry_limits.rs"]
mod drift_and_retry_limits;
#[path = "check_observation_recovery/process_containment.rs"]
mod process_containment;
#[path = "check_observation_recovery/quarantine.rs"]
mod quarantine;
#[path = "check_observation_recovery/replay_and_claims.rs"]
mod replay_and_claims;

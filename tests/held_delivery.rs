#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: support::TempRoot,
    config: PathBuf,
    native_session: Option<(String, PathBuf)>,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root);
        for name in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
        ] {
            command.env_remove(name);
        }
        permissive_umask(&mut command);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let config = root.join("exitbind.json");
        Self {
            root,
            config,
            native_session: None,
        }
    }

    fn with_native_session(mut self, identity: &str) -> Self {
        self.native_session = Some((
            identity.to_owned(),
            self.root.join("test-native-session-goals"),
        ));
        self
    }

    fn call(&self, args: &[&str]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config);
        command
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .env_remove("EXITBIND_SESSION_GOALS_DIR");
        if let Some((identity, store)) = &self.native_session {
            command
                .env("CODEX_THREAD_ID", identity)
                .env("EXITBIND_SESSION_GOALS_DIR", store);
        }
        permissive_umask(&mut command);
        command.output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn return_body(&self, work: &str, assignment: &str, body: &[u8]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                work,
                assignment,
                "--outcome",
                "completed",
                "--config",
            ])
            .arg(&self.config)
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .env_remove("EXITBIND_SESSION_GOALS_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        permissive_umask(&mut command);
        let mut child = command.spawn().unwrap();
        use std::io::Write;
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(body) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }

    fn refusal(&self) -> (String, String, Vec<u8>, Value) {
        let started = self.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            "hold a refused worker delivery",
            "--check-command",
            "true",
        ]);
        let work = started["work"].as_str().unwrap().to_owned();
        let scope = self.call(&[
            "work",
            "return",
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ]);
        assert!(scope.status.success(), "{scope:?}");
        for _ in 0..2 {
            let next = self.json(&["work", "next", &work]);
            let permit = self.call(&[
                "work",
                "permit",
                &work,
                next["next"]["assignment"].as_str().unwrap(),
                "--operation",
                "edit",
            ]);
            assert!(permit.status.success(), "{permit:?}");
        }
        let next = self.json(&["work", "next", &work]);
        let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
        let replanned = self.call(&[
            "work",
            "replan",
            &work,
            &assignment,
            "--hypothesis",
            "collect exact evidence",
        ]);
        assert!(replanned.status.success(), "{replanned:?}");
        let next = self.json(&["work", "next", &work]);
        let permit = self.call(&[
            "work",
            "permit",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "edit",
        ]);
        assert!(permit.status.success(), "{permit:?}");
        let next = self.json(&["work", "next", &work]);
        let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
        let before = fs::read(self.ledger(&work)).unwrap();
        let body = b"held worker bytes\n".to_vec();
        let output = self.return_body(&work, &assignment, &body);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(before, fs::read(self.ledger(&work)).unwrap());
        (work, assignment, body, value)
    }

    fn ledger(&self, work: &str) -> PathBuf {
        self.root
            .join(".exitbind/runs")
            .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()))
    }
}

fn permissive_umask(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o002);
            Ok(())
        });
    }
}

#[test]
fn refused_completion_is_private_idempotent_and_resubmittable() {
    let fixture = Fixture::new("held-delivery");
    let (work, assignment, body, first) = fixture.refusal();
    assert_eq!(first["effect"], "held");
    let held = first["held"].clone();
    assert_eq!(held["bytes"], body.len());
    assert_eq!(held["action"], "resubmit_after_replan_or_evidence");

    let duplicate = fixture.return_body(&work, &assignment, &body);
    assert!(duplicate.status.success(), "{duplicate:?}");
    let duplicate: Value = serde_json::from_slice(&duplicate.stdout).unwrap();
    assert_eq!(duplicate["held"]["reference"], held["reference"]);

    let held_dir = fixture.root.join(".exitbind/held");
    assert_eq!(
        fs::metadata(&held_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let entries = fs::read_dir(&held_dir)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(fs::read(entries[0].path()).unwrap(), body);
    assert_eq!(
        fs::metadata(entries[0].path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fixture.json(&["work", "next", &work])["next"]["held"]["reference"],
        held["reference"]
    );

    fs::write(fixture.root.join(".exitbind/evidence.txt"), b"new evidence").unwrap();
    let before_evidence = fs::read(fixture.ledger(&work)).unwrap();
    let unknown = fixture.call(&[
        "work",
        "evidence",
        &work,
        &assignment,
        "--artifact",
        ".exitbind/evidence.txt",
        "--artifact-root",
        "state",
        "--unrecognized",
    ]);
    assert!(!unknown.status.success(), "{unknown:?}");
    assert_eq!(before_evidence, fs::read(fixture.ledger(&work)).unwrap());

    let evidence = fixture.call(&[
        "work",
        "evidence",
        &work,
        &assignment,
        "--artifact",
        ".exitbind/evidence.txt",
        "--artifact-root",
        "state",
        "--json",
    ]);
    assert!(evidence.status.success(), "{evidence:?}");
    let evidence: Value = serde_json::from_slice(&evidence.stdout).unwrap();
    assert_eq!(evidence["work"], work);
    assert!(evidence["event"].is_object());
    let after_evidence = fs::read(fixture.ledger(&work)).unwrap();
    assert_eq!(
        after_evidence.iter().filter(|byte| **byte == b'\n').count(),
        before_evidence
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1
    );
    let output = fixture.call(&[
        "work",
        "return",
        &work,
        &assignment,
        "--outcome",
        "completed",
        "--result-ref",
        held["reference"].as_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let events = fs::read_to_string(fixture.ledger(&work)).unwrap();
    assert_eq!(events.matches("\"outcome\":\"completed\"").count(), 1);
    let artifact = events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["outcome"] == "completed")
        .and_then(|event| event["artifact"]["path"].as_str().map(str::to_owned))
        .unwrap();
    assert_eq!(
        fs::metadata(fixture.root.join(artifact))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(fs::read_dir(&held_dir).unwrap().next().is_none());
}

#[test]
fn changed_held_bytes_are_refused_before_submission() {
    let fixture = Fixture::new("held-integrity");
    let (work, assignment, _body, first) = fixture.refusal();
    let reference = first["held"]["reference"].as_str().unwrap();
    let held_path = fixture
        .root
        .join(".exitbind/held")
        .join(format!("{reference}.bin"));
    fs::write(held_path, b"tampered\n").unwrap();
    let before = fs::read(fixture.ledger(&work)).unwrap();
    let output = fixture.call(&[
        "work",
        "return",
        &work,
        &assignment,
        "--outcome",
        "completed",
        "--result-ref",
        reference,
    ]);
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("changed or has an invalid hash"));
    assert_eq!(before, fs::read(fixture.ledger(&work)).unwrap());
}

#[test]
fn every_current_assignment_held_result_is_discoverable() {
    let fixture = Fixture::new("held-multiple");
    let (work, assignment, _body, first) = fixture.refusal();
    let second = fixture.return_body(&work, &assignment, b"another worker result\n");
    assert!(second.status.success(), "{second:?}");
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    let next = fixture.json(&["work", "next", &work]);
    let held = next["next"]["heldResults"].as_array().unwrap();
    assert_eq!(held.len(), 2);
    let references = held
        .iter()
        .map(|item| item["reference"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(references.contains(&first["held"]["reference"].as_str().unwrap()));
    assert!(references.contains(&second["held"]["reference"].as_str().unwrap()));
    assert!(next["next"]["held"].is_null());
    assert_eq!(
        fixture.json(&["work", "resume"])["next"]["heldResults"],
        next["next"]["heldResults"]
    );
    assert_eq!(
        fs::read_dir(fixture.root.join(".exitbind/held"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn corrupt_optional_session_context_does_not_hide_held_delivery_or_change_its_ledger() {
    let fixture = Fixture::new("held-context-isolation")
        .with_native_session("held-context-isolation-session");
    fixture.json(&[
        "goal",
        "incorporate",
        "--goal-id",
        "held-context",
        "--goal",
        "Optional context cannot own held results",
        "--none-applicable",
        "obligations,findings,blockers,decisions,externalActions",
    ]);
    let (work, _assignment, body, first) = fixture.refusal();
    let reference = first["held"]["reference"].as_str().unwrap();
    let held_path = fixture
        .root
        .join(".exitbind/held")
        .join(format!("{reference}.bin"));
    let held_before = fs::read(&held_path).unwrap();
    let ledger_before = fs::read(fixture.ledger(&work)).unwrap();

    let token = work.strip_prefix("smw_").unwrap();
    let companion = fixture
        .root
        .join(".exitbind/runs")
        .join(format!("work-{token}.session-goal.json"));
    let mut value: Value = serde_json::from_slice(&fs::read(&companion).unwrap()).unwrap();
    value["protocol"] = serde_json::json!(99);
    fs::write(&companion, serde_json::to_vec(&value).unwrap()).unwrap();

    let detail = fixture.call(&["work", "detail", &work, "--json"]);
    assert!(detail.status.success(), "{detail:?}");
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["complete"], true);
    assert!(detail["goalAssociation"].is_null());

    let next = fixture.json(&["work", "next", &work]);
    assert_eq!(next["next"]["held"]["reference"], reference);
    assert_eq!(fs::read(&held_path).unwrap(), held_before);
    assert_eq!(held_before, body);
    assert_eq!(fs::read(fixture.ledger(&work)).unwrap(), ledger_before);
}

#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
    work: String,
    assignment: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let config = root.join("exitbind.json");
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let mut fixture = Self {
            root,
            config,
            work: String::new(),
            assignment: String::new(),
        };
        let begun = fixture.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            "re-plan file input",
            "--check-command",
            "true",
        ]);
        fixture.work = begun["work"].as_str().unwrap().to_owned();
        let scope = fixture.call(&[
            "work",
            "return",
            &fixture.work,
            begun["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ]);
        assert!(scope.status.success(), "{scope:?}");
        for _ in 0..2 {
            let next = fixture.json(&["work", "next", &fixture.work]);
            let permit = fixture.call(&[
                "work",
                "permit",
                &fixture.work,
                next["next"]["assignment"].as_str().unwrap(),
                "--operation",
                "edit",
            ]);
            assert!(permit.status.success(), "{permit:?}");
        }
        let next = fixture.json(&["work", "next", &fixture.work]);
        let compact_action = next["next"]["action"]
            .as_str()
            .expect("compact next action is a string");
        let resolved = match compact_action {
            "spawn" => next["next"].clone(),
            "inspect" => {
                assert_eq!(next["truncated"], true);
                assert!(next["fullCommand"].is_array());
                support::resolved_work_next(
                    &fixture.root,
                    &fixture.work,
                    &next,
                    env!("CARGO_BIN_EXE_exitbind"),
                )
            }
            action => panic!("unexpected compact next action: {action}"),
        };
        assert_eq!(resolved["action"], "spawn");
        fixture.assignment = resolved["assignment"].as_str().unwrap().to_owned();
        fixture
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config);
        command
    }

    fn call(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ledger(&self) -> PathBuf {
        self.root.join(".exitbind/runs").join(format!(
            "work-{}.jsonl",
            self.work.strip_prefix("smw_").unwrap()
        ))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn file_input_preserves_quoted_replan_fields_and_rejects_unsafe_inputs() {
    let fixture = Fixture::new("replan-file");
    let path = fixture.root.join("replan.json");
    let hypothesis = "the check said \"retry\"\nand needs evidence";
    let request = "show the log's first line\nthen inspect the cause";
    fs::write(
        &path,
        serde_json::to_vec(&json!({"hypothesis": hypothesis, "evidenceRequest": request})).unwrap(),
    )
    .unwrap();
    let file = path.to_str().unwrap();
    let before = fs::read(fixture.ledger()).unwrap();
    let mixed = fixture.call(&[
        "work",
        "replan",
        &fixture.work,
        &fixture.assignment,
        "--replan-file",
        file,
        "--hypothesis",
        "conflict",
    ]);
    assert!(!mixed.status.success());
    assert_eq!(fs::read(fixture.ledger()).unwrap(), before);
    let link = fixture.root.join("replan-link.json");
    symlink(&path, &link).unwrap();
    let linked = fixture.call(&[
        "work",
        "replan",
        &fixture.work,
        &fixture.assignment,
        "--replan-file",
        link.to_str().unwrap(),
    ]);
    assert!(!linked.status.success());
    assert_eq!(fs::read(fixture.ledger()).unwrap(), before);
    let unknown = fixture.call(&[
        "work",
        "replan",
        &fixture.work,
        &fixture.assignment,
        "--replan-file",
        file,
        "--unrecognized",
    ]);
    assert!(!unknown.status.success(), "{unknown:?}");
    assert_eq!(fs::read(fixture.ledger()).unwrap(), before);

    let output = fixture.call(&[
        "work",
        "replan",
        &fixture.work,
        &fixture.assignment,
        "--replan-file",
        file,
        "--json",
    ]);
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["event"]["governorEvent"]["hypothesis"], hypothesis);
    assert_eq!(result["event"]["governorEvent"]["evidenceRequest"], request);
    let after = fs::read(fixture.ledger()).unwrap();
    assert_eq!(
        after.iter().filter(|byte| **byte == b'\n').count(),
        before.iter().filter(|byte| **byte == b'\n').count() + 1
    );
}

#[test]
fn stdin_input_is_bounded_and_replans_once() {
    let fixture = Fixture::new("replan-stdin");
    let before = fs::read(fixture.ledger()).unwrap();
    let oversized = format!("{{\"hypothesis\":\"{}\"}}", "x".repeat(17 * 1024));
    let rejected = pipe(&fixture, oversized.as_bytes());
    assert!(!rejected.status.success());
    assert_eq!(fs::read(fixture.ledger()).unwrap(), before);
    let output = pipe(
        &fixture,
        br#"{"scopeDecision":"use the safe path; ask for evidence"}"#,
    );
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["event"]["governorEvent"]["scopeDecision"],
        "use the safe path; ask for evidence"
    );
}

fn pipe(fixture: &Fixture, input: &[u8]) -> Output {
    let mut child = fixture
        .command(&[
            "work",
            "replan",
            &fixture.work,
            &fixture.assignment,
            "--replan-file",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

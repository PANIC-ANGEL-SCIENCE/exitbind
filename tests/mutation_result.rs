//! Exact, read-only result lookup for bounded work mutations.

#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("mutation-result");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }
    fn call(&self, args: &[&str]) -> (Value, Output) {
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        let value = serde_json::from_slice(&output.stdout).unwrap();
        (value, output)
    }
    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.with_extension("pass"));
        let _ = fs::remove_file(self.root.with_extension("count"));
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn recorded_result_survives_later_head_and_inspection_never_rechecks() {
    let fixture = Fixture::new();
    let marker = fixture.root.with_extension("pass");
    let count = fixture.root.with_extension("count");
    let checker = format!(
        "printf x >> '{}'; if test -f '{}'; then exit 0; else exit 1; fi",
        count.display(),
        marker.display()
    );
    let (started, _) = fixture.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "exact result lookup",
        "--check-command",
        &checker,
    ]);
    let work = started["work"].as_str().unwrap();
    let (scope, scope_output) = fixture.call(&[
        "work",
        "return",
        work,
        started["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
        "--json",
    ]);
    assert!(scope_output.stdout.len() <= 8192);
    eprintln!("scope_return_bytes={}", scope_output.stdout.len());
    assert_eq!(scope["effect"], "recorded");
    assert_eq!(scope["outcome"], "scoped");
    assert!(scope["eventSha256"].as_str().is_some());
    let (worker, _) = fixture.call(&["work", "next", work]);
    let (returned, return_output) = fixture.call(&[
        "work",
        "return",
        work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    assert!(return_output.stdout.len() <= 8192);
    eprintln!("worker_return_bytes={}", return_output.stdout.len());
    assert_eq!(returned["outcome"], "completed");
    assert_eq!(returned["assignment"], worker["next"]["assignment"]);
    let (failed, failed_output) = fixture.call(&["work", "check", work]);
    assert!(failed_output.stdout.len() <= 8192);
    eprintln!("failed_check_bytes={}", failed_output.stdout.len());
    assert_eq!(failed["effect"], "recorded");
    assert_eq!(failed["result"]["code"], 1);
    let first_sha = failed["eventSha256"].as_str().unwrap();
    let ledger = fixture.ledger(work);
    fs::write(&marker, b"pass").unwrap();
    let (passed, passed_output) = fixture.call(&["work", "check", work, "--json"]);
    assert!(passed_output.stdout.len() <= 8192);
    eprintln!("passed_check_bytes={}", passed_output.stdout.len());
    assert_eq!(passed["result"]["code"], 0);
    assert_ne!(passed["eventSha256"], failed["eventSha256"]);
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let (historical, _) =
        fixture.call(&["run", "inspect", &ledger, "--event", first_sha, "--json"]);
    let (again, _) = fixture.call(&["run", "inspect", &ledger, "--event", first_sha, "--json"]);
    assert_eq!(historical, again);
    assert_eq!(historical["eventSha256"], first_sha);
    assert_eq!(historical["event"]["result"]["code"], 1);
    assert_eq!(historical["historical"], true);
    assert_eq!(historical["authorizesMutation"], false);
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
    assert_eq!(fs::read(&count).unwrap(), b"xx");
}

#[test]
fn return_help_advertises_json_and_unknown_options_fail_directly() {
    let fixture = Fixture::new();
    let help = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["work", "return", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{help:?}");
    assert!(String::from_utf8_lossy(&help.stdout).contains("[--json]"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["work", "return", "--unrecognized", "--config"])
        .arg(&fixture.config)
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("--unrecognized"));
}

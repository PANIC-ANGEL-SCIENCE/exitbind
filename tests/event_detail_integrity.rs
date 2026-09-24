//! Historical event identity remains readable when a referenced log disappears,
//! and changed log bytes are reported as an integrity failure.

#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("event-detail-integrity");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }

    fn call(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn output(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn selected_check_reports_missing_log_and_refuses_changed_bytes() {
    let fixture = Fixture::new();
    let begin = fixture.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "inspect event evidence",
        "--check-command",
        "printf evidence",
    ]);
    let work = begin["work"].as_str().unwrap();
    fixture.call(&[
        "work",
        "return",
        work,
        begin["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = fixture.call(&["work", "next", work]);
    fixture.call(&[
        "work",
        "return",
        work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    let check = fixture.call(&["work", "check", work]);
    let sha = check["eventSha256"].as_str().unwrap();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let detail = fixture.call(&["run", "inspect", &ledger, "--event", sha]);
    assert_eq!(detail["eventSha256"], sha);
    assert_eq!(detail["evidence"][0]["status"], "verified");
    let path = detail["event"]["stdout"]["path"].as_str().unwrap();
    let log = fixture.root.join(path);
    fs::remove_file(&log).unwrap();
    let missing = fixture.call(&["run", "inspect", &ledger, "--event", sha]);
    assert_eq!(missing["eventSha256"], sha);
    assert_eq!(missing["evidence"][0]["status"], "missing");
    assert_eq!(missing["authorizesMutation"], false);
    fs::write(&log, b"changed").unwrap();
    let corrupt = fixture.output(&["run", "inspect", &ledger, "--event", sha, "--json"]);
    assert!(!corrupt.status.success(), "{corrupt:?}");
    assert!(String::from_utf8_lossy(&corrupt.stdout).contains("evidence is invalid"));
    fs::remove_file(&log).unwrap();
    std::os::unix::fs::symlink(fixture.root.join("absent-target"), &log).unwrap();
    let unsafe_link = fixture.output(&["run", "inspect", &ledger, "--event", sha, "--json"]);
    assert!(!unsafe_link.status.success(), "{unsafe_link:?}");
    assert!(String::from_utf8_lossy(&unsafe_link.stdout).contains("symlink"));
}

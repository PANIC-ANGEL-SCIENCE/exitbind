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
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("r13-check");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        Self { root }
    }

    fn marker(&self) -> PathBuf {
        self.root.with_extension("check-pass")
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .output()
            .unwrap()
    }

    fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.marker());
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn failed_work_check_is_nonzero_and_routes_lead_before_review() {
    let fixture = Fixture::new();
    let marker = fixture.marker();
    let check = format!(
        "if test -f '{}'; then exit 0; else exit 7; fi",
        marker.display()
    );
    let started = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "r13 failed check",
        "--check-command",
        &check,
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    fixture.value(&[
        "work",
        "return",
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = fixture.value(&["work", "next", &work]);
    fixture.value(&[
        "work",
        "return",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);

    let failed = fixture.call(&["work", "check", &work]);
    assert!(
        !failed.status.success(),
        "failed check unexpectedly succeeded"
    );
    let response: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(response["effect"], "recorded");
    assert_eq!(response["result"]["code"], 7);
    assert_eq!(response["next"]["action"], "lead_decision");
    assert_eq!(response["next"]["role"], "lead");

    let next = fixture.value(&["work", "next", &work]);
    assert_eq!(next["next"]["action"], "lead_decision");
    assert_eq!(next["next"]["role"], "lead");

    fs::write(marker, b"pass").unwrap();
    let retry = fixture.call(&["work", "check", &work, "--json"]);
    assert!(retry.status.success(), "explicit retry failed: {retry:?}");
    let response: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(response["result"]["code"], 0);
    assert_eq!(response["next"]["action"], "spawn");
    assert_eq!(response["next"]["role"], "reviewer");
}

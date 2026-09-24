//! The same canonical preservation assignment reaches each stage's human help.

#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = support::temp("preservation-stages");
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self(root)
    }
    fn call(&self, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.0)
            .args(args)
            .arg("--config")
            .arg(self.0.join("exitbind.json"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preservation_assignment_is_stable_before_worker_return_and_later_stages() {
    let fixture = Fixture::new();
    let begin = fixture.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preservation assignment stages",
        "--check-command",
        "true",
        "--preserve-requirement",
        "identity:Keep exact event identity",
        "--preservation-check-command",
        "true",
        "--review-policy",
        "required",
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
    let worker = fixture.call(&["work", "next", work, "--full"]);
    let canonical = worker["next"]["packet"]["preservationAssignment"].clone();
    assert_eq!(canonical["route"], "FORMAL");
    assert_eq!(canonical["quality"], "FULL");
    assert_eq!(
        worker["residual"]["humanHelp"]["preservationAssignment"],
        canonical
    );
    fixture.call(&[
        "work",
        "return",
        work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    let check = fixture.call(&["work", "next", work, "--full"]);
    assert_eq!(check["next"]["action"], "check");
    assert_eq!(
        check["residual"]["humanHelp"]["preservationAssignment"],
        canonical
    );
    fixture.call(&["work", "check", work]);
    fixture.call(&["work", "check", work]);
    let reviewer = fixture.call(&["work", "next", work, "--full"]);
    assert_eq!(reviewer["next"]["role"], "reviewer");
    assert_eq!(
        reviewer["residual"]["humanHelp"]["preservationAssignment"],
        canonical
    );
}

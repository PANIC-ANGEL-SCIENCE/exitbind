#![cfg(unix)]

mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("check-retry");
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self { root }
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
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ledger(&self, work: &str) -> PathBuf {
        self.root
            .join(".exitbind/runs")
            .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for suffix in ["check-pass", "preservation-check-pass"] {
            let _ = fs::remove_file(self.root.with_extension(suffix));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn events(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn observed_checks(path: &Path) -> Vec<Value> {
    events(path)
        .into_iter()
        .filter(|event| event["action"] == "check" && event["acquisition"] == "observed")
        .collect()
}

fn artifact_bytes(fixture: &Fixture, event: &Value, stream: &str) -> Vec<u8> {
    let path = event[stream]["path"].as_str().unwrap();
    let bytes = fs::read(fixture.root.join(path)).unwrap();
    assert_eq!(event[stream]["bytes"], bytes.len());
    assert_eq!(
        event[stream]["sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    bytes
}

#[test]
fn failed_check_retry_selects_same_target_and_rotates_capture_paths() {
    let fixture = Fixture::new();
    let marker = fixture.root.with_extension("check-pass");
    let check = format!(
        "if test -f '{}'; then printf retry-ok; printf retry-ok-err >&2; else printf retry-fail; printf retry-fail-err >&2; exit 1; fi",
        marker.display()
    );
    let started = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "retry failed check",
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

    let ledger = fixture.ledger(&work);
    let first = fixture.value(&["work", "check", &work]);
    assert_eq!(first["next"]["progress"]["reason"]["code"], "check_failed");
    let first_event = observed_checks(&ledger).pop().unwrap();
    assert_eq!(first_event["result"]["code"], 1);
    let first_stdout = artifact_bytes(&fixture, &first_event, "stdout");
    let first_stderr = artifact_bytes(&fixture, &first_event, "stderr");

    fs::write(&marker, b"pass").unwrap();
    let second = fixture.value(&["work", "check", &work]);
    assert_eq!(second["next"]["action"], "spawn");
    let checks = observed_checks(&ledger);
    assert_eq!(checks.len(), 2);
    let second_event = &checks[1];
    assert_eq!(second_event["result"]["code"], 0);
    assert_eq!(
        first_event["targetEventSha256"],
        second_event["targetEventSha256"]
    );
    assert_eq!(first_event["subjectSha256"], second_event["subjectSha256"]);
    assert_eq!(first_event["inputsSha256"], second_event["inputsSha256"]);
    assert_ne!(
        first_event["stdout"]["path"],
        second_event["stdout"]["path"]
    );
    assert_ne!(
        first_event["stderr"]["path"],
        second_event["stderr"]["path"]
    );
    assert_eq!(
        artifact_bytes(&fixture, &first_event, "stdout"),
        first_stdout
    );
    assert_eq!(
        artifact_bytes(&fixture, &first_event, "stderr"),
        first_stderr
    );
    assert_eq!(first_stdout, b"retry-fail");
    assert_eq!(first_stderr, b"retry-fail-err");
    assert_eq!(
        artifact_bytes(&fixture, second_event, "stdout"),
        b"retry-ok"
    );
    assert_eq!(
        artifact_bytes(&fixture, second_event, "stderr"),
        b"retry-ok-err"
    );
}

#[test]
fn failed_functional_check_waits_for_missing_preservation_before_retry() {
    let fixture = Fixture::new();
    let marker = fixture.root.with_extension("preservation-check-pass");
    let functional = format!(
        "if test -f '{}'; then printf functional-ok; else printf functional-fail; exit 1; fi",
        marker.display()
    );
    let started = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "retry functional after preservation",
        "--check-command",
        &functional,
        "--preserve-requirement",
        "precedence:accepted precedence remains binding",
        "--preservation-check-command",
        "printf preservation-ok",
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

    let ledger = fixture.ledger(&work);
    let first_next = fixture.value(&["work", "next", &work]);
    assert_eq!(first_next["next"]["action"], "check");
    assert_eq!(first_next["next"]["check"]["kind"], "check");
    let failed = fixture.value(&["work", "check", &work]);
    assert_eq!(failed["next"]["action"], "check");
    assert_eq!(failed["next"]["check"]["kind"], "preservation");
    let checks = observed_checks(&ledger);
    assert_eq!(checks.len(), 1);
    let failed_event = checks[0].clone();
    assert_eq!(failed_event["result"]["code"], 1);
    let failed_stdout = artifact_bytes(&fixture, &failed_event, "stdout");
    assert_eq!(failed_stdout, b"functional-fail");

    let preservation = fixture.value(&["work", "check", &work]);
    assert_eq!(preservation["next"]["action"], "spawn");
    let checks = observed_checks(&ledger);
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[1]["requirementId"], "precedence");
    let after_preservation = fixture.value(&["work", "next", &work]);
    assert_ne!(after_preservation["next"]["check"]["kind"], "preservation");

    fs::write(&marker, b"pass").unwrap();
    let retried = fixture.value(&["work", "check", &work]);
    let checks = observed_checks(&ledger);
    assert_eq!(checks.len(), 3);
    let retry_event = &checks[2];
    assert_eq!(retry_event["result"]["code"], 0);
    assert_eq!(
        retry_event["targetEventSha256"],
        failed_event["targetEventSha256"]
    );
    assert_eq!(retry_event["subjectSha256"], failed_event["subjectSha256"]);
    assert_eq!(retry_event["inputsSha256"], failed_event["inputsSha256"]);
    assert_ne!(
        retry_event["stdout"]["path"],
        failed_event["stdout"]["path"]
    );
    assert_ne!(
        retry_event["stderr"]["path"],
        failed_event["stderr"]["path"]
    );
    assert_eq!(
        artifact_bytes(&fixture, &failed_event, "stdout"),
        failed_stdout
    );
    assert_eq!(
        artifact_bytes(&fixture, retry_event, "stdout"),
        b"functional-ok"
    );
    assert_eq!(retried["next"]["action"], "spawn");
}

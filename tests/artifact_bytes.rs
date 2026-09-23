mod support;

use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

fn call(root: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .arg("--config")
        .arg(root.join("exitbind.json"))
        .output()
        .unwrap()
}

#[test]
fn current_v8_submission_records_artifact_bytes_and_replays_them() {
    let root = support::temp("artifact-bytes");
    let initialized = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");

    let started = call(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "artifact bytes",
            "--ledger",
            ".exitbind/runs/bytes.jsonl",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
    );
    assert!(started.status.success(), "{started:?}");

    fs::create_dir_all(root.join(".exitbind/artifacts")).unwrap();
    fs::write(root.join(".exitbind/artifacts/lead.md"), b"scope\n").unwrap();
    let submitted = call(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ".exitbind/runs/bytes.jsonl",
            "--outcome",
            "scoped",
            "--artifact",
            ".exitbind/artifacts/lead.md",
            "--artifact-root",
            "state",
        ],
    );
    assert!(submitted.status.success(), "{submitted:?}");
    let response: Value = serde_json::from_slice(&submitted.stdout).unwrap();
    assert_eq!(response["event"]["artifact"]["bytes"], 6);

    let ledger = fs::read_to_string(root.join(".exitbind/runs/bytes.jsonl")).unwrap();
    let event: Value = serde_json::from_str(ledger.lines().last().unwrap()).unwrap();
    assert_eq!(event["artifact"]["bytes"], 6);
}

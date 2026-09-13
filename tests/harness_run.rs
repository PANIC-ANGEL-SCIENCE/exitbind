use serde_json::{json, Value};
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn project() -> (PathBuf, String) {
    let root = support::temp("harness-run");
    let initialized = invoke(&["init", "--root", root.to_str().unwrap()]);
    assert!(initialized.status.success(), "{}", text(&initialized));
    let config = root.join("soulmate.json").to_string_lossy().into_owned();
    (root, config)
}

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn manifest() -> Value {
    json!({
        "version": 1,
        "project": {"id": "harness-run", "session": "codex-session"},
        "harness": {"name": "codex", "version": "2026.08.30"},
        "activations": [
            {"kind": "skill", "name": "soulmate", "evidence": "configured"},
            {"kind": "perspective", "name": "qa-engineer", "evidence": "presented"},
            {"kind": "ponytail", "name": "ponytail:ponytail", "evidence": "hook_observed"}
        ]
    })
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

fn create_receipt(root: &Path, config: &str) -> PathBuf {
    write_json(&root.join("harness-manifest.json"), &manifest());
    let path = root.join(".soulmate/harness-receipt.json");
    let output = invoke(&[
        "plan",
        "change",
        "--goal",
        "receipt setup",
        "--receipt",
        path.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        config,
    ]);
    assert!(output.status.success(), "{}", text(&output));
    path
}

#[test]
fn bound_run_persists_reference_and_keeps_v2_chain() {
    let (root, config) = project();
    let receipt = create_receipt(&root, &config);
    let ledger = root.join(".soulmate/run.jsonl");
    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "synthetic-v0.7-fixture",
        "--ledger",
        ".soulmate/run.jsonl",
        "--harness-receipt",
        receipt.to_str().unwrap(),
        "--config",
        &config,
    ]);
    assert!(started.status.success(), "{}", text(&started));
    let first: Value =
        serde_json::from_str(fs::read_to_string(&ledger).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(first["version"], 2);
    assert_eq!(first["harnessReceipt"]["version"], 2);
    assert_eq!(
        first["harnessReceipt"]["path"],
        ".soulmate/harness-receipt.json"
    );
    let reference = first["harnessReceipt"].clone();

    let next = invoke(&[
        "run",
        "next",
        ".soulmate/run.jsonl",
        "--json",
        "--config",
        &config,
    ]);
    assert!(next.status.success(), "{}", text(&next));
    let packet: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(packet["assignments"][0]["harnessReceipt"], reference);

    fs::write(root.join("artifact.md"), "scoped\n").unwrap();
    let submitted = invoke(&[
        "run",
        "submit",
        "lead",
        ".soulmate/run.jsonl",
        "--outcome",
        "scoped",
        "--artifact",
        "artifact.md",
        "--config",
        &config,
    ]);
    assert!(submitted.status.success(), "{}", text(&submitted));
    let lines = fs::read_to_string(&ledger).unwrap();
    let second: Value = serde_json::from_str(lines.lines().nth(1).unwrap()).unwrap();
    assert_eq!(second["version"], 2);
    assert!(second.get("harnessReceipt").is_none());

    let inspected = invoke(&["run", "inspect", ".soulmate/run.jsonl", "--config", &config]);
    assert!(inspected.status.success(), "{}", text(&inspected));
    assert_eq!(
        serde_json::from_slice::<Value>(&inspected.stdout).unwrap()["valid"],
        true
    );

    let mut mixed_lines = lines.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut mixed: Value = serde_json::from_str(&mixed_lines[1]).unwrap();
    mixed["version"] = json!(1);
    mixed_lines[1] = serde_json::to_string(&mixed).unwrap();
    fs::write(
        root.join(".soulmate/mixed.jsonl"),
        format!("{}\n", mixed_lines.join("\n")),
    )
    .unwrap();
    let rejected = invoke(&[
        "run",
        "inspect",
        ".soulmate/mixed.jsonl",
        "--config",
        &config,
    ]);
    assert!(!rejected.status.success());
    assert!(text(&rejected).contains("mixed event versions"));

    let mut missing_producer: Value = serde_json::from_str(&mixed_lines[0]).unwrap();
    missing_producer.as_object_mut().unwrap().remove("producer");
    missing_producer
        .as_object_mut()
        .unwrap()
        .remove("eventSha256");
    missing_producer["eventSha256"] = json!(sha256_without_event_hash(&missing_producer));
    fs::write(
        root.join(".soulmate/missing-producer.jsonl"),
        format!("{}\n", serde_json::to_string(&missing_producer).unwrap()),
    )
    .unwrap();
    let rejected = invoke(&[
        "run",
        "inspect",
        ".soulmate/missing-producer.jsonl",
        "--config",
        &config,
    ]);
    assert!(!rejected.status.success());
    assert!(text(&rejected).contains("invalid producer"));
    fs::remove_dir_all(root).unwrap();
}

fn sha256_without_event_hash(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn receipt_drift_and_symlink_substitution_block_before_next_mutation() {
    let (root, config) = project();
    let receipt = create_receipt(&root, &config);
    let ledger = root.join(".soulmate/run.jsonl");
    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--harness-receipt",
        receipt.to_str().unwrap(),
        "--config",
        &config,
    ]);
    assert!(started.status.success(), "{}", text(&started));
    let before = fs::read(&ledger).unwrap();
    let external = root.join("receipt-copy.json");
    fs::copy(&receipt, &external).unwrap();
    fs::remove_file(&receipt).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external, &receipt).unwrap();
    #[cfg(not(unix))]
    fs::copy(&external, &receipt).unwrap();

    let drift = invoke(&[
        "run",
        "next",
        ".soulmate/run.jsonl",
        "--json",
        "--config",
        &config,
    ]);
    assert!(!drift.status.success(), "{}", text(&drift));
    let body: Value = serde_json::from_slice(&drift.stdout).unwrap();
    assert_eq!(body["classification"], "harness_receipt_drift");
    assert_eq!(fs::read(&ledger).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wrong_receipt_is_rejected_before_start_ledger_creation() {
    let (root, config) = project();
    write_json(&root.join("harness-manifest.json"), &manifest());
    let receipt = root.join(".soulmate/worker-receipt.json");
    let brief = invoke(&[
        "brief",
        "worker",
        "--task",
        "one agent",
        "--receipt",
        receipt.to_str().unwrap(),
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(brief.status.success(), "{}", text(&brief));
    let ledger = root.join(".soulmate/run.jsonl");
    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--harness-receipt",
        receipt.to_str().unwrap(),
        "--config",
        &config,
    ]);
    assert!(!started.status.success(), "{}", text(&started));
    assert!(!ledger.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unbound_run_remains_v1_and_supersession_can_opt_into_v2() {
    let (root, config) = project();
    let old = root.join(".soulmate/old.jsonl");
    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "old bounded",
        "--ledger",
        ".soulmate/old.jsonl",
        "--config",
        &config,
    ]);
    assert!(started.status.success(), "{}", text(&started));
    let old_bytes = fs::read(&old).unwrap();
    let old_start: Value =
        serde_json::from_slice(old_bytes.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(old_start["version"], 1);
    assert!(old_start.get("harnessReceipt").is_none());

    let receipt = create_receipt(&root, &config);
    let successor = invoke(&[
        "run",
        "supersede",
        ".soulmate/old.jsonl",
        "--workflow",
        "change",
        "--goal",
        "new bounded",
        "--ledger",
        ".soulmate/new.jsonl",
        "--harness-receipt",
        receipt.to_str().unwrap(),
        "--config",
        &config,
    ]);
    assert!(successor.status.success(), "{}", text(&successor));
    assert_eq!(fs::read(&old).unwrap(), old_bytes);
    let next: Value = serde_json::from_str(
        fs::read_to_string(root.join(".soulmate/new.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(next["version"], 2);
    assert_eq!(next["harnessReceipt"]["version"], 2);
    assert!(next.get("supersedes").is_some());
    fs::remove_dir_all(root).unwrap();
}

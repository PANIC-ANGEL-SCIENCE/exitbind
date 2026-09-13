use serde_json::{json, Value};
use sha2::{Digest, Sha256};
mod support;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .unwrap()
}

fn project(label: &str) -> (PathBuf, String) {
    let root = support::temp(&format!("path-compat-{label}"));
    let output = invoke(&["init", "--root", root.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output));
    let config = root.join("soulmate.json").to_string_lossy().into_owned();
    (root, config)
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
        "project": {"id": "path-compat", "session": "fixture"},
        "harness": {"name": "codex", "version": "2026.08.30"},
        "activations": [
            {"kind": "skill", "name": "soulmate", "evidence": "configured"}
        ]
    })
}

fn write_manifest(path: &Path) -> Vec<u8> {
    let bytes = format!("{}\n", serde_json::to_string_pretty(&manifest()).unwrap()).into_bytes();
    fs::write(path, &bytes).unwrap();
    bytes
}

#[test]
fn canonical_manifest_receipt_run_and_lock_paths_work_together() {
    let (root, config) = project("canonical");
    for path in [
        "soulmate/boundaries",
        "soulmate/policies",
        "soulmate/harness",
        ".soulmate/runs",
        ".soulmate/memory",
        ".soulmate/artifacts",
        ".soulmate/receipts",
        ".soulmate/away",
        ".soulmate/locks",
    ] {
        assert!(root.join(path).is_dir(), "{path}");
    }
    write_manifest(&root.join("soulmate/harness/harness-manifest.json"));
    let receipt = invoke(&[
        "plan",
        "change",
        "--goal",
        "canonical paths",
        "--receipt",
        ".soulmate/receipts/plan.json",
        "--harness-manifest",
        "soulmate/harness/harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(receipt.status.success(), "{}", text(&receipt));
    let receipt_value: Value = serde_json::from_str(
        &fs::read_to_string(root.join(".soulmate/receipts/plan.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt_value["version"], 2);
    assert_eq!(
        receipt_value["harness"]["path"],
        "soulmate/harness/harness-manifest.json"
    );

    let ledger = ".soulmate/runs/run.jsonl";
    let digest = format!("{:x}", Sha256::digest(ledger.as_bytes()));
    let lock = root
        .join(".soulmate/locks")
        .join(format!("run-v1-{digest}.lock"));
    fs::write(
        &lock,
        format!(
            "{{\"pid\":{},\"createdAt\":\"2026-08-30T00:00:00.000Z\"}}",
            std::process::id()
        ),
    )
    .unwrap();
    let busy = invoke(&[
        "run", "start", "change", "--goal", "busy", "--ledger", ledger, "--config", &config,
    ]);
    assert!(!busy.status.success());
    assert!(text(&busy).contains("run ledger is busy"));
    assert!(!root.join(ledger).exists());
    assert!(!root.join(format!("{ledger}.lock")).exists());
    fs::remove_file(lock).unwrap();

    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "canonical",
        "--ledger",
        ledger,
        "--harness-receipt",
        ".soulmate/receipts/plan.json",
        "--config",
        &config,
    ]);
    assert!(started.status.success(), "{}", text(&started));
    assert!(root.join(ledger).is_file());
    assert!(!root.join(format!("{ledger}.lock")).exists());
    assert!(invoke(&["run", "inspect", ledger, "--config", &config])
        .status
        .success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_manifest_receipt_and_run_paths_remain_valid() {
    let (root, config) = project("legacy");
    write_manifest(&root.join("harness-manifest.json"));
    let receipt = invoke(&[
        "plan",
        "change",
        "--goal",
        "legacy paths",
        "--receipt",
        ".soulmate/harness-receipt.json",
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        &config,
    ]);
    assert!(receipt.status.success(), "{}", text(&receipt));
    let value: Value = serde_json::from_str(
        &fs::read_to_string(root.join(".soulmate/harness-receipt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(value["harness"]["path"], "harness-manifest.json");
    assert!(invoke(&[
        "verify",
        ".soulmate/harness-receipt.json",
        "--config",
        &config,
    ])
    .status
    .success());
    assert!(invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "legacy",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &config,
    ])
    .status
    .success());
    assert!(root.join(".soulmate/run.jsonl").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn path_migration_is_explicit_and_retains_legacy_manifest_bytes() {
    let (root, config) = project("migration");
    for path in [
        "soulmate/boundaries",
        "soulmate/policies",
        "soulmate/harness",
        ".soulmate/runs",
        ".soulmate/memory",
        ".soulmate/artifacts",
        ".soulmate/receipts",
        ".soulmate/away",
        ".soulmate/locks",
    ] {
        fs::remove_dir(root.join(path)).unwrap();
    }
    let legacy = root.join("harness-manifest.json");
    let legacy_bytes = write_manifest(&legacy);

    let first = invoke(&["migrate", "paths", "--config", &config]);
    let second = invoke(&["migrate", "paths", "--config", &config]);
    assert!(first.status.success(), "{}", text(&first));
    assert_eq!(first.stdout, second.stdout);
    let plan: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(plan["mode"], "dry-run");
    assert_eq!(plan["status"], "ready");
    assert_eq!(plan["directories"].as_array().unwrap().len(), 9);
    assert_eq!(plan["manifest"]["action"], "copy-retain-legacy");
    assert_eq!(plan["historicalEvidenceRewritten"], false);
    assert!(!root.join("soulmate/harness").exists());
    assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);

    let applied = invoke(&["migrate", "paths", "--apply", "--config", &config]);
    assert!(applied.status.success(), "{}", text(&applied));
    assert_eq!(
        serde_json::from_slice::<Value>(&applied.stdout).unwrap()["status"],
        "applied"
    );
    assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);
    assert_eq!(
        fs::read(root.join("soulmate/harness/harness-manifest.json")).unwrap(),
        legacy_bytes
    );
    for path in [
        "soulmate/agents",
        "soulmate/boundaries",
        "soulmate/policies",
        "soulmate/harness",
        ".soulmate/runs",
        ".soulmate/memory",
        ".soulmate/artifacts",
        ".soulmate/receipts",
        ".soulmate/away",
        ".soulmate/locks",
    ] {
        assert!(root.join(path).is_dir(), "{path}");
    }
    let unchanged = invoke(&["migrate", "paths", "--apply", "--config", &config]);
    assert!(unchanged.status.success(), "{}", text(&unchanged));
    assert_eq!(
        serde_json::from_slice::<Value>(&unchanged.stdout).unwrap()["status"],
        "unchanged"
    );
    fs::remove_dir_all(root).unwrap();
}

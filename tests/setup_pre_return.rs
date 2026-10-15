mod support;

use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn fixture(label: &str) -> PathBuf {
    let root = support::temp(label);
    support::git_topology::repository(&root);
    root
}

fn setup(root: &Path, extra: &[&str], apply: bool) -> Output {
    let mut args = vec![
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root.to_str().unwrap(),
        "--scope",
        "worker",
        "--observe",
        "README.md",
        "--write",
        "src",
        "--commands",
        "cargo fmt --check",
        "--check-command",
        "true",
        "--goal",
        "setup pre-return",
        "--review-policy",
        "required",
        "--skip-skills",
    ];
    args.extend_from_slice(extra);
    if apply {
        args.push("--apply");
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    command
        .env("PATH", support::git_topology::git_only_path(root))
        .args(args)
        .output()
        .unwrap()
}

fn invoke(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    command.args(args).output().unwrap()
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn value(output: &Output) -> Value {
    assert!(output.status.success(), "{}", output_text(output));
    serde_json::from_slice(&output.stdout).expect("valid JSON response")
}

fn emitted_begin(report: &Value) -> &str {
    let command = report["next"]["commands"][0]
        .as_str()
        .expect("emitted Work begin command");
    assert!(command.contains("'work' 'begin' 'change'"), "{command}");
    command
}

fn run_emitted(command: &str) -> Value {
    let mut process = Command::new("sh");
    support::git_topology::isolate(&mut process);
    let output = process.args(["-c", command]).output().unwrap();
    value(&output)
}

fn detail(work: &str, root: &Path) -> Value {
    let config = root.join("exitbind.json");
    let output = invoke(&[
        "work",
        "detail",
        work,
        "--json",
        "--config",
        config.to_str().unwrap(),
    ]);
    value(&output)
}

#[test]
fn setup_begin_command_records_pre_return_policy_and_readback() {
    let root = fixture("setup-pre-return");
    let command = "printf '%s' \"owner's value; $HOME\" > 'pre return marker'";
    let preview = value(&setup(
        &root,
        &[
            "--pre-return-command",
            command,
            "--pre-return-timeout-ms",
            "12345",
            "--pre-return-capture-bytes",
            "9876",
        ],
        false,
    ));
    assert_eq!(preview["approvedFacts"]["preReturn"]["command"], command);
    assert_eq!(preview["approvedFacts"]["preReturn"]["timeoutMs"], "12345");
    assert_eq!(
        preview["approvedFacts"]["preReturn"]["captureBytes"],
        "9876"
    );
    let emitted = preview["next"]["commands"][1].as_str().unwrap();
    assert!(emitted.contains("owner'\\''s value; $HOME"), "{emitted}");
    assert!(!root.join("pre return marker").exists());

    let applied = value(&setup(
        &root,
        &[
            "--pre-return-command",
            command,
            "--pre-return-timeout-ms",
            "12345",
            "--pre-return-capture-bytes",
            "9876",
        ],
        true,
    ));
    assert_eq!(applied["status"], "applied");
    let begin_command = emitted_begin(&applied);
    let begun = run_emitted(begin_command);
    assert!(!root.join("pre return marker").exists());
    let work = begun["work"]
        .as_str()
        .expect("Work identity from emitted begin");
    let detail = detail(work, &root);
    let policy = &detail["sections"]["evidence"]["preReturn"]["policy"];
    assert_eq!(detail["sections"]["evidence"]["preReturn"]["enabled"], true);
    assert_eq!(policy["command"], command);
    assert_eq!(policy["timeoutMs"], 12345);
    assert_eq!(policy["captureLimitBytes"], 9876);
    assert!(policy["checkerSources"].is_array());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_omission_keeps_begin_without_pre_return_policy() {
    let root = fixture("setup-pre-return-omitted");
    let preview = value(&setup(&root, &[], false));
    assert!(preview["approvedFacts"].get("preReturn").is_none());
    let applied = value(&setup(&root, &[], true));
    let begin_command = emitted_begin(&applied);
    let begun = run_emitted(begin_command);
    let work = begun["work"].as_str().unwrap();
    let detail = detail(work, &root);
    let pre_return = &detail["sections"]["evidence"]["preReturn"];
    assert_eq!(pre_return["enabled"], false);
    assert!(pre_return["policy"].is_null());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn setup_rejects_orphan_and_invalid_pre_return_limits_before_writing() {
    for (label, options) in [
        ("orphan-timeout", vec!["--pre-return-timeout-ms", "1"]),
        ("orphan-capture", vec!["--pre-return-capture-bytes", "1"]),
        (
            "zero-timeout",
            vec![
                "--pre-return-command",
                "true",
                "--pre-return-timeout-ms",
                "0",
            ],
        ),
        (
            "large-capture",
            vec![
                "--pre-return-command",
                "true",
                "--pre-return-capture-bytes",
                "1048577",
            ],
        ),
    ] {
        let root = fixture(&format!("setup-pre-return-{label}"));
        let output = setup(&root, &options, true);
        assert!(!output.status.success(), "{}", output_text(&output));
        assert!(
            !root.join("exitbind.json").exists()
                && !root.join(".exitbind").exists()
                && !root.join("exitbind").exists(),
            "invalid setup wrote files: {}",
            root.display()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn optional_diagnostic_never_supplies_missing_owner_decisions_or_starts_work() {
    let root = fixture("setup-pre-return-missing-decisions");
    let config = root.join("exitbind.json");
    let output = invoke(&[
        "setup",
        "--root",
        root.to_str().unwrap(),
        "--json",
        "--skip-skills",
        "--apply",
        "--pre-return-command",
        "printf executed > should-not-exist",
    ]);
    let report = value(&output);
    assert!(config.exists());
    let missing = report["missingOwnerDecisions"].as_array().unwrap();
    for name in ["goal", "check-command", "review-policy"] {
        assert!(missing.iter().any(|item| item == name));
    }
    assert!(report["next"]["argv"].as_array().unwrap().is_empty());
    assert!(!root.join("should-not-exist").exists());
    let runs = root.join(".exitbind/runs");
    assert!(!runs.exists() || std::fs::read_dir(runs).unwrap().next().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

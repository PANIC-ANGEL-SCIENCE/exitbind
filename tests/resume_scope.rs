//! Work resume discovery stays inside the configured project and has a fixed
//! candidate bound before it reads any ledger contents.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn init(root: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn invoke(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(std::env::temp_dir())
        .env("PATH", "/nonexistent")
        .args(args)
        .output()
        .unwrap()
}

fn begin(root: &Path) -> String {
    let config = root.join("exitbind.json");
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "bounded resume project",
            "--check-command",
            "true",
            "--config",
        ])
        .arg(config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["work"].as_str().unwrap().to_owned()
}

#[test]
fn explicit_config_resume_uses_the_selected_project_from_a_hostile_cwd() {
    let root = support::temp("resume-scope-explicit-config");
    init(&root);
    let work = begin(&root);
    let config = root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let output = invoke(&["work", "resume", "--json", "--config", config]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "resumed");
    assert_eq!(value["work"], work);
    assert_eq!(value["discovery"]["scope"], "configured_state_root");
    assert_eq!(value["discovery"]["directory"], "work_ledgers");
    assert_eq!(value["discovery"]["recursive"], false);
    assert_eq!(value["discovery"]["automaticWidening"], false);
    assert_eq!(value["discovery"]["complete"], true);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resume_refuses_more_than_bound_before_reading_malformed_ledgers() {
    let root = support::temp("resume-scope-bound");
    init(&root);
    let runs = root.join(".exitbind/runs");
    for index in 0..129u16 {
        let token = format!("{index:04x}").to_string() + &"a".repeat(60);
        fs::write(runs.join(format!("work-{token}.jsonl")), b"not-json\n").unwrap();
    }
    let config = root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let output = invoke(&["work", "resume", "--json", "--config", config]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "unresolved");
    assert_eq!(value["reason"]["code"], "discovery_limited");
    assert_eq!(value["discovery"]["complete"], false);
    assert_eq!(value["discovery"]["observed"], 129);
    assert_eq!(value["discovery"]["maxWorkLedgers"], 128);
    assert_eq!(value["nextAction"]["type"], "provide_explicit_work");
    assert_eq!(value["nextAction"]["safe"], true);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounded_resume_preserves_saved_focus_as_an_exact_read_route() {
    let root = support::temp("resume-scope-focused-bound");
    init(&root);
    let work = begin(&root);
    let runs = root.join(".exitbind/runs");
    for index in 0..129u16 {
        let token = format!("{index:04x}").to_string() + &"b".repeat(60);
        fs::write(runs.join(format!("work-{token}.jsonl")), b"not-json\n").unwrap();
    }
    let config = root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let output = invoke(&["work", "resume", "--json", "--config", config]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "unresolved");
    assert_eq!(value["focus"]["work"], work);
    assert_eq!(value["focus"]["valid"], true);
    assert_eq!(value["nextAction"]["type"], "read_current_work");
    let command = value["nextAction"]["command"].as_array().unwrap();
    assert!(command
        .iter()
        .any(|item| item.as_str() == Some(work.as_str())));
    assert!(command
        .iter()
        .any(|item| item == &Value::String("--json".into())));
    assert!(command
        .iter()
        .any(|item| item == &Value::String(config.into())));
    fs::remove_dir_all(root).unwrap();
}

//! Integration coverage for the default bounded `work next` response.

use serde_json::Value;
use std::process::Command;

#[test]
fn default_work_next_is_bounded_json_with_recovery_identity() {
    let root = std::env::temp_dir().join(format!(
        "exitbind-compact-work-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        "path".repeat(24)
    ));
    std::fs::create_dir(&root).unwrap();
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = root.join("exitbind.json");
    let begin = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "compact response test",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--config",
        ])
        .arg(&config)
        .output()
        .unwrap();
    assert!(begin.status.success(), "{begin:?}");
    let started: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = started["work"].as_str().unwrap();
    let next = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "next", work, "--json", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(next.status.success(), "{next:?}");
    let value: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert!(next.stdout.len() <= 8 * 1024);
    assert_eq!(value["compact"], true);
    assert!(value["next"]["action"].is_string());
    assert!(value["next"]["assignment"].is_string());
    for key in ["recorder", "currentSubject"] {
        if !value[key].is_object() {
            assert!(value[key].is_null());
            assert_eq!(value["truncated"], true);
        }
    }
    if let Some(references) = value.get("references") {
        assert!(references.is_array());
    } else {
        assert_eq!(value["truncated"], true);
    }
    assert!(value["omitted"].is_array());
    let command = value["fullCommand"].as_array().unwrap();
    assert!(command.iter().any(|arg| arg == "--full"));
    assert_eq!(command[command.len() - 2], "--config");
    assert_eq!(command.last().unwrap(), config.to_str().unwrap());
    let recovered = Command::new(command[0].as_str().unwrap())
        .current_dir(std::env::temp_dir())
        .args(command[1..].iter().map(|arg| arg.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    let full = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "next", work, "--full", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(full.status.success(), "{full:?}");
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["next"]["assignment"], value["next"]["assignment"]);
    assert_eq!(recovered["next"]["assignment"], full["next"]["assignment"]);
    assert!(full["recorder"].is_object());
    assert!(full["residual"]["currentSubject"].is_object());
    assert_eq!(
        value["current"]["result"]["subject"]["sha256"],
        full["residual"]["currentSubject"]["sha256"]
    );
    for (key, complete) in [
        ("recorder", &full["recorder"]),
        ("currentSubject", &full["residual"]["currentSubject"]),
    ] {
        if let Some(inline) = value.get(key) {
            assert_eq!(inline, complete);
        }
    }
    assert!(full["residual"]["context"].is_object());
    let history = value
        .get("references")
        .unwrap_or(&full["residual"]["context"]["expansions"])
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["kind"] == "ledger_history")
        .unwrap();
    let reference = history["id"].as_str().unwrap();
    let expanded = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "expand", work, reference, "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["valid"], true);
    assert_eq!(expanded["reference"]["id"], reference);
    let scope = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "return",
            work,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
            "--config",
        ])
        .arg(&config)
        .output()
        .unwrap();
    assert!(scope.status.success(), "{scope:?}");
    let stale = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "expand", work, reference, "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(!stale.status.success(), "{stale:?}");
    let _ = std::fs::remove_dir_all(root);
}

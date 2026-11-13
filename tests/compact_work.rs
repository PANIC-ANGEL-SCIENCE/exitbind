//! Integration coverage for the default bounded `work next` response.
#[allow(dead_code)]
mod support;

use serde_json::Value;

#[test]
fn default_work_next_is_bounded_json_with_recovery_identity() {
    let root = support::temp(&format!("compact-work-{}", "path".repeat(24)));
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = root.join("exitbind.json");
    let begin = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
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
    let next = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
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
    assert_eq!(value["currentDelivery"]["kind"], "work_context_reference");
    assert_eq!(value["currentDelivery"]["complete"], false);
    assert_eq!(
        value["currentDelivery"]["binding"],
        value["current"]["binding"]
    );
    assert!(value["currentDelivery"]["contextIdentity"]["recipientSha256"].is_string());
    assert!(!value["effectiveAction"]["choices"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(value["full"]["readOnly"], true);
    assert_eq!(value["full"]["binding"], value["current"]["binding"]);
    let command = value["full"]["command"].as_array().unwrap();
    assert_eq!(command[1], "work");
    assert_eq!(command[2], "detail");
    assert_eq!(command[command.len() - 2], "--config");
    assert_eq!(command.last().unwrap(), config.to_str().unwrap());
    let recovered = support::git_topology::command(command[0].as_str().unwrap())
        .current_dir(support::temp("compact-outside").path())
        .args(command[1..].iter().map(|arg| arg.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    let full = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "next", work, "--full", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(full.status.success(), "{full:?}");
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["next"]["assignment"], value["next"]["assignment"]);
    assert_eq!(recovered["kind"], "work_detail");
    assert_eq!(recovered["complete"], true);
    assert_eq!(recovered["binding"], value["current"]["binding"]);
    assert_eq!(
        recovered["actionForms"]["assignment"],
        full["next"]["assignment"]
    );
    assert!(full["recorder"].is_object());
    assert!(full["residual"]["currentSubject"].is_object());
    assert_eq!(
        value["current"]["result"]["subject"]["sha256"],
        full["residual"]["currentSubject"]["sha256"]
    );
    assert_eq!(value["recorder"], full["recorder"]);
    assert_eq!(
        value["currentSubject"]["sha256"],
        full["residual"]["currentSubject"]["sha256"]
    );
    assert!(full["residual"]["context"].is_object());
    let history = full["residual"]["context"]["expansions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["kind"] == "ledger_history")
        .unwrap();
    let reference = history["id"].as_str().unwrap();
    let expanded = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "expand", work, reference, "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["valid"], true);
    assert_eq!(expanded["reference"]["id"], reference);
    let scope = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
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
    let stale = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["work", "expand", work, reference, "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(!stale.status.success(), "{stale:?}");
    let _ = std::fs::remove_dir_all(root);
}

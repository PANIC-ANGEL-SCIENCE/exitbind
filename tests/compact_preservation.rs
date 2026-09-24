//! A large accepted preservation command remains available through the full
//! packet while the default response keeps its output budget.

#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn long_preservation_command_is_bounded_and_recoverable() {
    let root = support::temp("compact-preservation");
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = root.join("exitbind.json");
    let command = format!("true # {}", "x".repeat(9_000));
    let begin = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "large preservation",
            "--check-command",
            "true",
            "--preserve-requirement",
            "identity:Keep the current event",
            "--preservation-check-command",
            &command,
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
        .args(["work", "next", work, "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(next.status.success(), "{next:?}");
    assert!(next.stdout.len() <= 8 * 1024, "{} bytes", next.stdout.len());
    let compact: Value = serde_json::from_slice(&next.stdout).unwrap();
    eprintln!("long_preservation_default_bytes={}", next.stdout.len());
    assert_eq!(
        compact["humanHelp"]["preservationAssignment"]["requiresExpansion"],
        true
    );
    assert_eq!(
        compact["humanHelp"]["preservationAssignment"]["route"],
        "FORMAL"
    );
    assert!(compact["next"]["packet"].is_null());
    assert_eq!(compact["next"]["requiresExpansion"], true);
    assert_eq!(compact["next"]["constraintsOmitted"], true);
    let argv = compact["fullCommand"].as_array().unwrap();
    let full = Command::new(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|arg| arg.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(full.status.success(), "{full:?}");
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["next"]["assignment"], compact["next"]["assignment"]);
    assert!(full["residual"]["humanHelp"]["preservationAssignment"]
        .to_string()
        .contains(&command));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_uses_the_exact_long_executable_path() {
    let root = support::temp("long-executable-recovery");
    let original = env!("CARGO_BIN_EXE_exitbind");
    let init = Command::new(original)
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = root.join("exitbind.json");
    let begin = Command::new(original)
        .current_dir(&root)
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "long executable",
            "--check-command",
            "true",
            "--config",
        ])
        .arg(&config)
        .output()
        .unwrap();
    assert!(begin.status.success(), "{begin:?}");
    let started: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = started["work"].as_str().unwrap();
    let binary_root = support::temp("long-executable-binary");
    let mut nested = binary_root.clone();
    for _ in 0..7 {
        nested.push("nested".repeat(16));
    }
    fs::create_dir_all(&nested).unwrap();
    let candidate = nested.join("exitbind");
    fs::copy(original, &candidate).unwrap();
    assert!(candidate.to_str().unwrap().len() > 512);
    let next = Command::new(&candidate)
        .current_dir(&root)
        .args(["work", "next", work, "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(next.status.success(), "{next:?}");
    assert!(next.stdout.len() <= 8 * 1024);
    let compact: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(compact["fullCommand"][0], candidate.to_str().unwrap());
    assert!(compact["fullCommandSameExecutableRequired"].is_null());
    let argv = compact["fullCommand"].as_array().unwrap();
    let expanded = Command::new(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|arg| arg.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(
        expanded["next"]["assignment"],
        compact["next"]["assignment"]
    );
    let returned = Command::new(&candidate)
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
    assert!(returned.status.success(), "{returned:?}");
    assert!(returned.stdout.len() <= 8 * 1024);
    let returned: Value = serde_json::from_slice(&returned.stdout).unwrap();
    assert_eq!(
        returned["nextAction"]["command"][0],
        candidate.to_str().unwrap()
    );
    let detail_argv = returned["nextAction"]["command"].as_array().unwrap();
    let detail = Command::new(detail_argv[0].as_str().unwrap())
        .args(detail_argv[1..].iter().map(|arg| arg.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(detail.status.success(), "{detail:?}");
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["eventSha256"], returned["eventSha256"]);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(binary_root);
}

//! A large accepted preservation command remains available through the full
//! packet while the default response keeps its output budget.

#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{fs, path::Path};

fn route_descriptor(
    value: &Value,
    argv_key: &str,
    executable_key: &str,
    config_key: &str,
) -> Value {
    json!({
        "argv": value[argv_key],
        "sameExecutableRequired": value
            .get(executable_key)
            .cloned()
            .unwrap_or(Value::Bool(false)),
        "sameConfigRequired": value
            .get(config_key)
            .cloned()
            .unwrap_or(Value::Bool(false)),
    })
}

fn reconstruct_inspect_command(
    route: &Value,
    expected_executable: &Path,
    expected_config: &Path,
    invocation_cwd: &Path,
    work: &str,
    event_sha: &str,
) -> Vec<String> {
    let descriptor = route_descriptor(
        route,
        "command",
        "sameExecutableRequired",
        "sameConfigRequired",
    );
    let same_executable = descriptor["sameExecutableRequired"]
        .as_bool()
        .expect("inspect route executable flag is a boolean");
    let same_config = descriptor["sameConfigRequired"]
        .as_bool()
        .expect("inspect route config flag is a boolean");
    let mut argv = descriptor["argv"]
        .as_array()
        .expect("inspect route argv is an array")
        .iter()
        .map(|argument| {
            argument
                .as_str()
                .expect("inspect argument is text")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(!argv.is_empty(), "inspect route argv is not empty");
    let executable = fs::canonicalize(expected_executable).unwrap();
    if same_executable {
        assert_eq!(argv[0], "run");
        argv.insert(0, executable.to_str().unwrap().to_owned());
    } else {
        assert!(
            Path::new(&argv[0]).is_absolute(),
            "inspect executable is explicit"
        );
        assert_eq!(fs::canonicalize(&argv[0]).unwrap(), executable);
    }

    let config = fs::canonicalize(expected_config).unwrap();
    if same_config {
        assert_eq!(argv.last().map(String::as_str), Some("--config"));
        argv.push(config.to_str().unwrap().to_owned());
    }

    assert!(
        work.starts_with("smw_") && work.len() > 4,
        "fixture Work handle is valid"
    );
    let expected_ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let expected_ledger_path = fs::canonicalize(invocation_cwd.join(&expected_ledger)).unwrap();
    let ledger_argument = argv.get(3).expect("inspect route ledger argument");
    let ledger_path = Path::new(ledger_argument);
    let ledger_path = if ledger_path.is_absolute() {
        ledger_path.to_path_buf()
    } else {
        invocation_cwd.join(ledger_path)
    };
    assert_eq!(fs::canonicalize(ledger_path).unwrap(), expected_ledger_path);
    assert_eq!(
        argv.len(),
        9,
        "inspect route has only the exact expected arguments"
    );
    assert_eq!(argv[1], "run");
    assert_eq!(argv[2], "inspect");
    assert_eq!(argv[3], expected_ledger);
    assert_eq!(argv[4], "--event");
    assert_eq!(argv[5], event_sha);
    assert_eq!(argv[6], "--json");
    assert_eq!(argv[7], "--config");
    assert_eq!(argv[8], config.to_str().unwrap());
    argv
}

#[test]
fn long_preservation_command_is_bounded_and_recoverable() {
    let root = support::temp("compact-preservation");
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = root.join("exitbind.json");
    let command = format!("true # {}", "x".repeat(9_000));
    let begin = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
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
    let next = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
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
    let descriptor = route_descriptor(
        &compact,
        "fullCommand",
        "fullCommandSameExecutableRequired",
        "fullCommandSameConfigRequired",
    );
    let argv = support::reconstruct_work_command(
        &descriptor,
        Path::new(env!("CARGO_BIN_EXE_exitbind")),
        &config,
        &root,
    );
    let full = support::git_topology::command(&argv[0])
        .current_dir(&root)
        .args(&argv[1..])
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
    let init = support::git_topology::command(original)
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = root.join("exitbind.json");
    let begin = support::git_topology::command(original)
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
    let mut nested = binary_root.to_path_buf();
    for _ in 0..7 {
        nested.push("nested".repeat(16));
    }
    fs::create_dir_all(&nested).unwrap();
    let candidate = nested.join("exitbind");
    support::place_executable(std::path::Path::new(original), &candidate);
    assert!(candidate.to_str().unwrap().len() > 512);
    let mut next = support::git_topology::command(&candidate);
    next.current_dir(&root)
        .args(["work", "next", work, "--config"])
        .arg(&config);
    let next = support::run(&mut next);
    assert!(next.status.success(), "{next:?}");
    assert!(next.stdout.len() <= 8 * 1024);
    let compact: Value = serde_json::from_slice(&next.stdout).unwrap();
    let descriptor = route_descriptor(
        &compact,
        "fullCommand",
        "fullCommandSameExecutableRequired",
        "fullCommandSameConfigRequired",
    );
    let argv = support::reconstruct_work_command(&descriptor, &candidate, &config, &root);
    let candidate = fs::canonicalize(&candidate).unwrap();
    assert_eq!(Path::new(&argv[0]), candidate);
    let mut expanded = support::git_topology::command(&argv[0]);
    expanded.current_dir(&root).args(&argv[1..]);
    let expanded = support::run(&mut expanded);
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(
        expanded["next"]["assignment"],
        compact["next"]["assignment"]
    );
    let mut returned = support::git_topology::command(&candidate);
    returned
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
        .arg(&config);
    let returned = support::run(&mut returned);
    assert!(returned.status.success(), "{returned:?}");
    assert!(returned.stdout.len() <= 8 * 1024);
    let returned: Value = serde_json::from_slice(&returned.stdout).unwrap();
    let event_sha = returned["eventSha256"].as_str().unwrap();
    let detail_argv = reconstruct_inspect_command(
        &returned["nextAction"],
        &candidate,
        &config,
        &root,
        work,
        event_sha,
    );
    assert_eq!(Path::new(&detail_argv[0]), candidate);
    let mut detail = support::git_topology::command(&detail_argv[0]);
    detail.current_dir(&root).args(&detail_argv[1..]);
    let detail = support::run(&mut detail);
    assert!(detail.status.success(), "{detail:?}");
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["eventSha256"], returned["eventSha256"]);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(binary_root);
}

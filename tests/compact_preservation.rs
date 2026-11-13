//! A large accepted preservation command remains available through the full
//! packet while the default response keeps its output budget.

#![cfg(unix)]

mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn assert_complete_route(route: &Value, defaults: &Value) {
    for flag in ["sameExecutableRequired", "sameConfigRequired"] {
        assert_eq!(
            route
                .get(flag)
                .or_else(|| defaults.get(flag))
                .and_then(Value::as_bool),
            Some(false),
            "route flag {flag} must explicitly be false: {route}"
        );
    }
}

fn exact_inspect_command(
    response: &Value,
    expected_executable: &Path,
    expected_config: &Path,
    invocation_cwd: &Path,
    work: &str,
    event_sha: &str,
) -> Vec<String> {
    let route = &response["nextAction"];
    assert_eq!(route["type"], "inspect");
    assert_eq!(route["safe"], true);
    assert_complete_route(route, &response["routeDefaults"]);
    let argv = route["command"]
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
    assert!(
        Path::new(&argv[0]).is_absolute(),
        "inspect executable is explicit"
    );
    assert_eq!(fs::canonicalize(&argv[0]).unwrap(), executable);
    let config = fs::canonicalize(expected_config).unwrap();

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
    let recovery = &compact["recovery"];
    assert_eq!(recovery["readOnly"], true);
    assert_eq!(recovery["sameExecutableRequired"], false);
    assert_eq!(recovery["sameConfigRequired"], false);
    let argv = recovery["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(argv.len(), 7);
    assert_eq!(
        fs::canonicalize(argv[0]).unwrap(),
        fs::canonicalize(env!("CARGO_BIN_EXE_exitbind")).unwrap()
    );
    assert_eq!(&argv[1..6], &["work", "detail", work, "--json", "--config"]);
    assert_eq!(
        fs::canonicalize(argv[6]).unwrap(),
        fs::canonicalize(&config).unwrap()
    );
    let full = support::git_topology::command(argv[0])
        .current_dir(&root)
        .args(&argv[1..])
        .output()
        .unwrap();
    assert!(full.status.success(), "{full:?}");
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["kind"], "work_detail");
    assert_eq!(full["work"], work);
    assert_eq!(full["binding"], compact["current"]["binding"]);
    assert_eq!(
        full["actionForms"]["assignment"],
        compact["next"]["assignment"]
    );
    assert!(
        full["sections"]["assignment"]["residual"]["humanHelp"]["preservationAssignment"]
            .to_string()
            .contains(&command)
    );
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
    let candidate = fs::canonicalize(&candidate).unwrap();
    let recovery = compact
        .get("full")
        .filter(|route| route["command"].is_array())
        .or_else(|| compact.pointer("/transportCompletion/recovery"))
        .or_else(|| compact.get("recovery"))
        .expect("complete current detail recovery");
    assert_complete_route(recovery, &compact["routeDefaults"]);
    assert_eq!(recovery["readOnly"], true);
    let argv = recovery["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(argv.len(), 7);
    assert_eq!(fs::canonicalize(argv[0]).unwrap(), candidate);
    assert_eq!(&argv[1..6], &["work", "detail", work, "--json", "--config"]);
    assert_eq!(
        fs::canonicalize(argv[6]).unwrap(),
        fs::canonicalize(&config).unwrap()
    );
    let mut expanded = support::git_topology::command(argv[0]);
    expanded.current_dir(&root).args(&argv[1..]);
    let expanded = support::run(&mut expanded);
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["kind"], "work_detail");
    assert_eq!(expanded["work"], work);
    assert_eq!(expanded["binding"], compact["current"]["binding"]);
    assert_eq!(
        expanded["recipient"]["assignment"],
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
    assert!(returned.stdout.len() <= 256 * 1024);
    let returned: Value = serde_json::from_slice(&returned.stdout).unwrap();
    let delivery = &returned["currentDelivery"];
    assert_eq!(delivery["kind"], "work_actor_context");
    assert_eq!(delivery["complete"], false);
    assert_eq!(delivery["requiredContentComplete"], true);
    assert_eq!(delivery["work"], work);
    assert_eq!(delivery["binding"], returned["current"]["binding"]);
    assert_eq!(delivery["recipient"]["actor"], "worker");
    assert_eq!(
        delivery["recipient"]["assignment"],
        returned["effectiveAction"]["assignment"]
    );
    let full_route = &returned["full"];
    assert_eq!(full_route["readOnly"], true);
    assert_eq!(full_route["binding"], delivery["binding"]);
    assert_complete_route(full_route, &returned["routeDefaults"]);
    let full_argv = full_route["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(full_argv.len(), 7);
    assert_eq!(fs::canonicalize(full_argv[0]).unwrap(), candidate);
    assert_eq!(
        &full_argv[1..6],
        &["work", "detail", work, "--json", "--config"]
    );
    assert_eq!(
        fs::canonicalize(full_argv[6]).unwrap(),
        fs::canonicalize(&config).unwrap()
    );
    let full = support::git_topology::command(full_argv[0])
        .current_dir(&root)
        .args(&full_argv[1..])
        .output()
        .unwrap();
    assert!(full.status.success(), "{full:?}");
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["binding"], delivery["binding"]);
    assert_eq!(full["recipient"], delivery["recipient"]);
    assert_eq!(full["contextIdentity"], delivery["contextIdentity"]);
    let profile = &delivery["instructions"]["profile"];
    let full_profile = &full["recipientContext"]["profile"];
    for field in ["content", "sha256", "bytes", "complete"] {
        assert_eq!(profile[field], full_profile[field]);
    }
    let content = profile["content"].as_str().unwrap().as_bytes();
    assert_eq!(profile["bytes"], content.len());
    assert_eq!(profile["sha256"], format!("{:x}", Sha256::digest(content)));
    let event_sha = returned["eventSha256"].as_str().unwrap();
    let detail_argv = exact_inspect_command(&returned, &candidate, &config, &root, work, event_sha);
    assert_eq!(Path::new(&detail_argv[0]), candidate);
    let mut detail = support::git_topology::command(&detail_argv[0]);
    detail.current_dir(&root).args(&detail_argv[1..]);
    let detail = support::run(&mut detail);
    assert!(detail.status.success(), "{detail:?}");
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(detail["eventSha256"], returned["eventSha256"]);

    // The next actor can use the complete delivery without rewriting the
    // executable or configuration path; only the operation is actor-supplied.
    let permit = &returned["effectiveAction"]["beforeEditing"]["command"];
    assert_complete_route(permit, &returned["routeDefaults"]);
    let mut permit_argv = permit["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(Path::new(&permit_argv[0]), candidate);
    assert_eq!(permit_argv[permit_argv.len() - 2], "--config");
    assert_eq!(Path::new(permit_argv.last().unwrap()), config);
    let operation_positions = permit_argv
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == "<OPERATION>").then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(operation_positions.len(), 1);
    permit_argv[operation_positions[0]] = "Inspect the bounded fixture".to_owned();
    let mut permit_command = support::git_topology::command(&permit_argv[0]);
    permit_command.current_dir(&root).args(&permit_argv[1..]);
    let permit_result = support::run(&mut permit_command);
    assert!(permit_result.status.success(), "{permit_result:?}");
    let permit_result: Value = serde_json::from_slice(&permit_result.stdout).unwrap();
    assert_eq!(permit_result["allowed"], true);

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(binary_root);
}

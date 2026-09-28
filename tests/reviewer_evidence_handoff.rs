//! A native reviewer's launch context names the current evidence and the exact
//! commands that resolve it, so the Lead never repackages check records. Only
//! current, same-work references resolve, and a passing check does not decide
//! the review.

#![cfg(unix)]
mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const MARKER: &str = "reviewer-evidence-marker";

fn run(root: &Path, args: &[&str], input: &[u8]) -> Output {
    run_config(root, &root.join("exitbind.json"), None, args, input)
}

fn run_config(
    root: &Path,
    config: &Path,
    bindings: Option<&Path>,
    args: &[&str],
    input: &[u8],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(root)
        .args(args)
        .arg("--config")
        .arg(config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(bindings) = bindings {
        command.env("EXITBIND_BINDINGS_DIR", bindings);
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn ok(root: &Path, args: &[&str], input: &[u8]) -> Value {
    let output = run(root, args, input);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn ok_config(
    root: &Path,
    config: &Path,
    bindings: Option<&Path>,
    args: &[&str],
    input: &[u8],
) -> Value {
    let output = run_config(root, config, bindings, args, input);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    root
}

fn local_project(label: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let base = support::temp(label);
    let root = base.join("product");
    let control = base.join("control");
    let state = base.join("state");
    let bindings = base.join("bindings");
    for path in [&root, &control, &state, &bindings] {
        fs::create_dir_all(path).unwrap();
    }
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .env("EXITBIND_BINDINGS_DIR", &bindings)
        .args([
            "init",
            "--mode",
            "local",
            "--project-id",
            "reviewer_local",
            "--root",
        ])
        .arg(&root)
        .arg("--control-root")
        .arg(&control)
        .arg("--state-root")
        .arg(&state)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config = control.join("exitbind.json");
    assert!(config.is_file());
    (base, root, config, bindings)
}

fn begin(root: &Path, goal: &str) -> String {
    begin_with_config(root, &root.join("exitbind.json"), None, goal, false)
}

fn begin_preserving(root: &Path, goal: &str) -> String {
    begin_with_config(root, &root.join("exitbind.json"), None, goal, true)
}

fn begin_with_config(
    root: &Path,
    config: &Path,
    bindings: Option<&Path>,
    goal: &str,
    preserving: bool,
) -> String {
    let check_command = format!("echo {MARKER}");
    let mut args: Vec<&str> = vec![
        "work",
        "begin",
        "change",
        "--goal",
        goal,
        "--check-command",
        &check_command,
        "--proof-origin",
        "synthetic",
        "--review-policy",
        "required",
    ];
    if preserving {
        args.extend([
            "--preserve-requirement",
            "first:First requirement.",
            "--preservation-check-command",
            "echo preservation-marker",
        ]);
    }
    ok_config(root, config, bindings, &args, b"")["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn give(root: &Path, work: &str, outcome: &str, body: &str) -> Value {
    give_with_config(root, &root.join("exitbind.json"), None, work, outcome, body)
}

fn give_with_config(
    root: &Path,
    config: &Path,
    bindings: Option<&Path>,
    work: &str,
    outcome: &str,
    body: &str,
) -> Value {
    let next = ok_config(
        root,
        config,
        bindings,
        &["work", "next", work, "--full"],
        b"",
    );
    let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
    ok_config(
        root,
        config,
        bindings,
        &["work", "return", work, &assignment, "--outcome", outcome],
        body.as_bytes(),
    )
}

fn reviewer_context(root: &Path, bindings: Option<&Path>) -> String {
    let payload = json!({"hook_event_name":"SubagentStart", "cwd":root, "agent_type":"reviewer"});
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(bindings) = bindings {
        command.env("EXITBIND_BINDINGS_DIR", bindings);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Exact argv and declared working directory for each emitted read route.
fn routes(context: &str) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    for line in context.lines() {
        if let Some((label, route)) = line.trim().split_once(" route: ") {
            if let Ok(route) = serde_json::from_str(route) {
                found.push((label.to_owned(), route));
            }
        }
    }
    found
}

fn execute_route(route: &Value, path: Option<&Path>) -> Output {
    let argv = route["argv"].as_array().unwrap();
    let program = argv[0].as_str().unwrap();
    let mut command = Command::new(program);
    for argument in argv.iter().skip(1) {
        command.arg(argument.as_str().unwrap());
    }
    command.current_dir(route["cwd"].as_str().unwrap());
    if let Some(environment) = route["env"].as_object() {
        for (name, value) in environment {
            command.env(name, value.as_str().unwrap());
        }
    }
    if let Some(path) = path {
        let current_path = std::env::var_os("PATH").unwrap_or_default();
        let paths = std::iter::once(path.to_path_buf()).chain(std::env::split_paths(&current_path));
        command.env("PATH", std::env::join_paths(paths).unwrap());
    }
    command.output().unwrap()
}

fn marker_hex_for(marker: &str) -> String {
    use std::fmt::Write as _;
    format!("{marker}\n")
        .bytes()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

fn marker_hex() -> String {
    marker_hex_for(MARKER)
}

#[test]
fn reviewer_context_resolves_current_check_record_and_log() {
    let root = project("reviewer evidence ñ current");
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let work = begin(&root, "reviewer evidence");
    give(&root, &work, "scoped", "scope");
    give(&root, &work, "completed", "worker result");
    // Before the check runs, no evidence is offered as reviewable.
    assert!(!reviewer_context(&root, None).contains("Evidence for this assignment"));

    ok(&root, &["work", "check", &work], b"");
    let context = reviewer_context(&root, None);
    assert!(context.contains("check passed (current)"), "{context}");
    assert!(context.contains("not an approval"));
    let packet = ok(&root, &["work", "next", &work, "--full"], b"");
    let subject = packet["next"]["packet"]["context"]["subject"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(context.contains(&format!("current subject SHA-256 {subject}")));

    let found = routes(&context);
    let record = found
        .iter()
        .find(|item| item.0 == "check record")
        .unwrap()
        .1
        .clone();
    let log = found
        .iter()
        .find(|item| item.0 == "check log")
        .unwrap()
        .1
        .clone();
    let full = found
        .iter()
        .find(|item| item.0 == "Full assignment packet")
        .unwrap()
        .1
        .clone();
    let config = root.join("exitbind.json");
    let argv = record["argv"].as_array().unwrap();
    assert!(Path::new(argv[0].as_str().unwrap()).is_absolute());
    assert!(argv
        .windows(2)
        .any(|pair| { pair[0] == "--config" && pair[1] == config.to_str().unwrap() }));
    assert_eq!(record["cwd"], root.to_str().unwrap());

    let stale_bin = root.with_extension("stale-bin");
    fs::create_dir(&stale_bin).unwrap();
    let stale_executable = stale_bin.join("exitbind");
    fs::write(&stale_executable, "#!/bin/sh\nexit 97\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&stale_executable, fs::Permissions::from_mode(0o755)).unwrap();

    // Execute each emitted argv in its declared directory without adding a
    // binary, config flag, or working-directory correction.
    let event_output = execute_route(&record, Some(&stale_bin));
    assert!(
        event_output.status.success(),
        "route={record:?}; {event_output:?}"
    );
    let event: Value = serde_json::from_slice(&event_output.stdout).unwrap();
    let hex = event["contentHex"].as_str().unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let event: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(event["action"], "check");
    assert_eq!(event["subjectSha256"], subject);
    assert!(context.contains(event["eventSha256"].as_str().unwrap()));
    let log_output = execute_route(&log, Some(&stale_bin));
    assert!(log_output.status.success(), "{log_output:?}");
    let log: Value = serde_json::from_slice(&log_output.stdout).unwrap();
    assert_eq!(log["stdout"]["contentHex"], marker_hex());
    assert_eq!(log["stdout"]["truncated"], false);
    let full_output = execute_route(&full, Some(&stale_bin));
    assert!(full_output.status.success(), "{full_output:?}");
    let full: Value = serde_json::from_slice(&full_output.stdout).unwrap();
    assert_eq!(full["work"], work);

    // The absolute config route also works from a supported nested directory.
    let mut nested_route = record.clone();
    nested_route["cwd"] = json!(nested);
    assert!(execute_route(&nested_route, Some(&stale_bin))
        .status
        .success());

    // Another work cannot read these references.
    let other = begin(&root, "other work");
    let mut foreign_route = record.clone();
    foreign_route["argv"][3] = json!(other);
    let foreign = execute_route(&foreign_route, None);
    assert!(!foreign.status.success());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(stale_bin).unwrap();
}

#[test]
fn reviewer_context_exposes_preservation_check_record_and_log_routes() {
    let root = project("reviewer evidence preservation");
    let work = begin_preserving(&root, "reviewer evidence with preservation");
    give(&root, &work, "scoped", "scope");
    give(&root, &work, "completed", "worker result");
    ok(&root, &["work", "check", &work], b"");
    ok(&root, &["work", "check", &work], b"");

    let context = reviewer_context(&root, None);
    assert!(
        context.contains("preservation check passed (current, requirement first)"),
        "{context}"
    );
    let found = routes(&context);
    let record = found
        .iter()
        .find(|item| item.0 == "preservation check record")
        .unwrap();
    let log = found
        .iter()
        .find(|item| item.0 == "preservation check log")
        .unwrap();
    let record_output = execute_route(&record.1, None);
    assert!(record_output.status.success(), "{record_output:?}");
    let expanded: Value = serde_json::from_slice(&record_output.stdout).unwrap();
    let hex = expanded["contentHex"].as_str().unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let event: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(event["requirementId"], "first");
    assert_eq!(event["action"], "check");

    let log_output = execute_route(&log.1, None);
    assert!(log_output.status.success(), "{log_output:?}");
    let log: Value = serde_json::from_slice(&log_output.stdout).unwrap();
    assert_eq!(
        log["stdout"]["contentHex"],
        marker_hex_for("preservation-marker")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reviewer_context_routes_external_local_configuration_exactly() {
    let (base, root, config, bindings) = local_project("reviewer evidence ñ external config");
    let work = begin_with_config(
        &root,
        &config,
        Some(&bindings),
        "reviewer evidence external config",
        false,
    );
    give_with_config(&root, &config, Some(&bindings), &work, "scoped", "scope");
    give_with_config(
        &root,
        &config,
        Some(&bindings),
        &work,
        "completed",
        "worker result",
    );
    ok_config(
        &root,
        &config,
        Some(&bindings),
        &["work", "check", &work],
        b"",
    );

    let context = reviewer_context(&root, Some(&bindings));
    assert!(context.contains("check passed (current)"), "{context}");
    let record = routes(&context)
        .into_iter()
        .find(|item| item.0 == "check record")
        .unwrap()
        .1;
    let argv = record["argv"].as_array().unwrap();
    assert!(argv
        .windows(2)
        .any(|pair| { pair[0] == "--config" && pair[1] == config.to_str().unwrap() }));
    assert_eq!(record["cwd"], root.to_str().unwrap());
    assert_eq!(
        record["env"]["EXITBIND_BINDINGS_DIR"],
        bindings.to_str().unwrap()
    );
    let output = execute_route(&record, None);
    assert!(output.status.success(), "{output:?}");

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_passing_check_does_not_decide_review_and_old_references_go_stale() {
    let root = project("reviewer-evidence-stale");
    let work = begin(&root, "reviewer evidence rework");
    give(&root, &work, "scoped", "scope");
    give(&root, &work, "completed", "worker result");
    ok(&root, &["work", "check", &work], b"");
    let context = reviewer_context(&root, None);
    let old = routes(&context)
        .into_iter()
        .filter(|(_, route)| route["argv"][2] == "expand")
        .collect::<Vec<_>>();
    assert!(!old.is_empty());

    // The reviewer still decides: an adverse verdict is recorded despite the
    // passing check, and acceptance stays unavailable.
    let after = give(&root, &work, "rework", "finding against the result");
    assert_eq!(after["next"]["role"], "lead");
    for (_, route) in &old {
        let stale = execute_route(route, None);
        assert!(!stale.status.success(), "stale reference resolved");
    }
    fs::remove_dir_all(root).unwrap();
}

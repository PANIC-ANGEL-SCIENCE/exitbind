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
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn ok(root: &Path, args: &[&str], input: &[u8]) -> Value {
    let output = run(root, args, input);
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

fn begin(root: &Path, goal: &str) -> String {
    ok(
        root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            &format!("echo {MARKER}"),
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    )["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn give(root: &Path, work: &str, outcome: &str, body: &str) -> Value {
    let next = ok(root, &["work", "next", work, "--full"], b"");
    let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
    ok(
        root,
        &["work", "return", work, &assignment, "--outcome", outcome],
        body.as_bytes(),
    )
}

fn reviewer_context(root: &Path) -> String {
    let payload = json!({"hook_event_name":"SubagentStart", "cwd":root, "agent_type":"reviewer"});
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
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

/// `(label, work, reference)` for each `work expand` command in the context.
fn commands(context: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for line in context.lines() {
        for (index, _) in line.match_indices("work expand ") {
            let label = if line[..index].ends_with("log `exitbind ") {
                "log"
            } else if line[..index].ends_with("record `exitbind ") {
                "record"
            } else {
                "event"
            };
            let rest = &line[index + "work expand ".len()..];
            let mut parts = rest.split(|c| c == ' ' || c == '`');
            let work = parts.next().unwrap().to_owned();
            let reference = parts.next().unwrap().to_owned();
            found.push((label.to_owned(), work, reference));
        }
    }
    found
}

fn marker_hex() -> String {
    use std::fmt::Write as _;
    format!("{MARKER}\n")
        .bytes()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

#[test]
fn reviewer_context_resolves_current_check_record_and_log() {
    let root = project("reviewer-evidence-current");
    let work = begin(&root, "reviewer evidence");
    give(&root, &work, "scoped", "scope");
    give(&root, &work, "completed", "worker result");
    // Before the check runs, no evidence is offered as reviewable.
    assert!(!reviewer_context(&root).contains("Evidence for this assignment"));

    ok(&root, &["work", "check", &work], b"");
    let context = reviewer_context(&root);
    assert!(context.contains("check passed (current)"), "{context}");
    assert!(context.contains("not an approval"));
    let packet = ok(&root, &["work", "next", &work, "--full"], b"");
    let subject = packet["next"]["packet"]["context"]["subject"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(context.contains(&format!("current subject SHA-256 {subject}")));

    let found = commands(&context);
    let record = found.iter().find(|item| item.0 == "record").unwrap();
    let log = found.iter().find(|item| item.0 == "log").unwrap();
    assert_eq!(record.1, work);
    let event = ok(&root, &["work", "expand", &work, &record.2], b"");
    let hex = event["contentHex"].as_str().unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let event: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(event["action"], "check");
    assert_eq!(event["subjectSha256"], subject);
    assert!(context.contains(event["eventSha256"].as_str().unwrap()));
    let log = ok(&root, &["work", "expand", &work, &log.2], b"");
    assert_eq!(log["stdout"]["contentHex"], marker_hex());
    assert_eq!(log["stdout"]["truncated"], false);

    // Another work cannot read these references.
    let other = begin(&root, "other work");
    let foreign = run(&root, &["work", "expand", &other, &record.2], b"");
    assert!(!foreign.status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_passing_check_does_not_decide_review_and_old_references_go_stale() {
    let root = project("reviewer-evidence-stale");
    let work = begin(&root, "reviewer evidence rework");
    give(&root, &work, "scoped", "scope");
    give(&root, &work, "completed", "worker result");
    ok(&root, &["work", "check", &work], b"");
    let context = reviewer_context(&root);
    let old = commands(&context);
    assert!(!old.is_empty());

    // The reviewer still decides: an adverse verdict is recorded despite the
    // passing check, and acceptance stays unavailable.
    let after = give(&root, &work, "rework", "finding against the result");
    assert_eq!(after["next"]["role"], "lead");
    for (_, work, reference) in &old {
        let stale = run(&root, &["work", "expand", work, reference], b"");
        assert!(!stale.status.success(), "stale reference resolved");
    }
    fs::remove_dir_all(root).unwrap();
}

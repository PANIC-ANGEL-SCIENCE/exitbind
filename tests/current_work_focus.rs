//! The current-work focus is navigation only: it selects which running work
//! `work resume` returns, and never changes a work ledger.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod support;

fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .output()
        .unwrap()
}

fn ok(root: &Path, args: &[&str]) -> Value {
    let output = call(root, args);
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

fn begin(root: &Path, goal: &str) -> Value {
    ok(
        root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "true",
        ],
    )
}

fn ledgers(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = fs::read_dir(root.join(".exitbind/runs"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn focus_file(root: &Path) -> PathBuf {
    root.join(".exitbind/current-work.json")
}

#[test]
fn begin_selects_new_work_and_older_running_work_stays_history() {
    let root = project("focus-select");
    let old = begin(&root, "older task")["work"]
        .as_str()
        .unwrap()
        .to_owned();
    let started = begin(&root, "current task");
    assert_eq!(started["focus"]["updated"], true);
    assert_eq!(started["focus"]["authority"], "none");
    let current = started["work"].as_str().unwrap().to_owned();
    let before = ledgers(&root);

    let resumed = ok(&root, &["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], current.as_str());
    assert_eq!(resumed["selection"]["basis"], "current_work_focus");
    assert_eq!(resumed["history"]["running"], 1);
    assert_eq!(resumed["history"]["command"][3], "--history");

    let history = ok(&root, &["work", "resume", "--history"]);
    assert_eq!(history["status"], "history");
    let mut listed = history["works"]
        .as_array()
        .unwrap()
        .iter()
        .map(|work| work["work"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    listed.sort();
    let mut expected = vec![old.clone(), current];
    expected.sort();
    assert_eq!(listed, expected);

    // An explicit locator still reaches older work, and refocusing changes
    // navigation only.
    assert!(call(&root, &["work", "next", &old]).status.success());
    let refocused = ok(&root, &["work", "focus", &old]);
    assert_eq!(refocused["effect"], "focus-only");
    assert_eq!(ok(&root, &["work", "resume"])["work"], old.as_str());
    assert_eq!(ledgers(&root), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn focus_on_non_running_work_reports_no_current_work() {
    let root = project("focus-stale");
    begin(&root, "abandoned one");
    begin(&root, "abandoned two");
    let gone = format!("smw_{}", "a".repeat(64));
    fs::write(
        focus_file(&root),
        format!(r#"{{"version":1,"work":"{gone}","authority":"none"}}"#),
    )
    .unwrap();
    let before = ledgers(&root);
    let entry = ok(&root, &["work", "resume"]);
    assert_eq!(entry["status"], "none");
    assert_eq!(entry["reason"]["code"], "no_current_work");
    assert_eq!(entry["focus"]["work"], gone.as_str());
    assert_eq!(entry["history"]["running"], 2);
    assert!(entry.get("work").is_none());
    assert!(!call(&root, &["work", "focus", &gone]).status.success());
    assert_eq!(ledgers(&root), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn legacy_project_without_focus_keeps_count_based_resume() {
    let root = project("focus-legacy");
    begin(&root, "one");
    begin(&root, "two");
    fs::remove_file(focus_file(&root)).unwrap();
    let entry = ok(&root, &["work", "resume"]);
    assert_eq!(entry["status"], "ambiguous");
    assert_eq!(entry["reason"]["code"], "ambiguous_candidates");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn unusable_focus_is_refused_with_a_recovery_route() {
    let root = project("focus-invalid");
    let work = begin(&root, "task")["work"].as_str().unwrap().to_owned();
    fs::write(focus_file(&root), "not json").unwrap();
    let refused = call(&root, &["work", "resume"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("work focus WORK"));
    ok(&root, &["work", "focus", &work]);
    assert_eq!(ok(&root, &["work", "resume"])["work"], work.as_str());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn failed_focus_update_reports_the_committed_work_and_recovery() {
    let root = project("focus-write-failure");
    fs::create_dir(focus_file(&root)).unwrap();
    let started = begin(&root, "task");
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["focus"]["updated"], false);
    assert!(started["focus"]["error"].is_string());
    assert_eq!(started["focus"]["command"][2], "focus");
    assert_eq!(started["focus"]["command"][3], work);
    assert!(call(&root, &["work", "next", work]).status.success());
    assert_eq!(ledgers(&root).len(), 1);
    fs::remove_dir_all(&root).unwrap();
}

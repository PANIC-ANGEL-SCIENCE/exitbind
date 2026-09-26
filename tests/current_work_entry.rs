use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod support;

fn exitbind(root: &Path, args: &[&str], input: Option<&[u8]>) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.unwrap_or_default())
        .unwrap();
    let output = child.wait_with_output().unwrap();
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
    let started = exitbind(
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
        None,
    );
    started["work"].as_str().unwrap().to_owned()
}

fn runs(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
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

#[test]
fn open_session_goal_selects_current_work_and_keeps_other_runs_as_history() {
    let root = project("current-work-open-goal");
    let old = begin(&root, "old unfinished task");
    let current = begin(&root, "current task");
    let unselected = exitbind(&root, &["work", "resume"], None);
    assert_eq!(unselected["status"], "ambiguous");
    assert_eq!(unselected["reason"]["code"], "ambiguous_candidates");

    let init = json!({"action":"init","sourceRef":"owner:task","sourceText":"Do the current task.",
        "requirements":[{"id":"task","text":"Do the current task."}]});
    exitbind(
        &root,
        &["work", "record", &current],
        Some(init.to_string().as_bytes()),
    );
    let before = runs(&root);
    let resumed = exitbind(&root, &["work", "resume", "--full"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], current.as_str());
    assert_eq!(resumed["selection"]["basis"], "open_session_goal");
    assert_eq!(resumed["history"]["running"], 1);
    assert_eq!(resumed["history"]["command"][3], "--history");

    let history = exitbind(&root, &["work", "resume", "--history"], None);
    assert_eq!(history["status"], "history");
    assert_eq!(history["nextAction"]["type"], "choose_explicit_handle");
    let mut listed = history["works"]
        .as_array()
        .unwrap()
        .iter()
        .map(|work| work["work"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    listed.sort();
    let mut expected = vec![old, current];
    expected.sort();
    assert_eq!(listed, expected);
    assert_eq!(runs(&root), before);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn closed_session_goal_reports_no_current_work_instead_of_resuming_history() {
    let root = project("current-work-closed-goal");
    let old = begin(&root, "abandoned task");
    exitbind(
        &root,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "direct-1",
            "--goal",
            "small direct task",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    exitbind(
        &root,
        &["goal", "close", "--goal-id", "direct-1", "--direct"],
        None,
    );
    let before = runs(&root);

    let entry = exitbind(&root, &["work", "resume"], None);
    assert_eq!(entry["status"], "none");
    assert_eq!(entry["reason"]["code"], "no_current_work");
    assert_eq!(entry["next"]["action"], "none");
    assert_eq!(entry["history"]["running"], 1);
    assert!(entry.get("work").is_none());

    let history = exitbind(&root, &["work", "resume", "--history"], None);
    assert_eq!(history["status"], "history");
    assert_eq!(history["works"][0]["work"], old.as_str());
    assert_eq!(runs(&root), before);
    fs::remove_dir_all(&root).unwrap();
}

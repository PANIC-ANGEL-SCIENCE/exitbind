//! Direct-only session-goal closure stays separate from governed run evidence.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    process::{Command, Output, Stdio},
};

const CARD: &str =
    "+------------------------------+\n| Nothing remains here.        |\n+------------------------------+\n\nEXIT READY";

struct Project {
    root: std::path::PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn establish_direct_goal(project: &Project) {
    fs::write(project.root.join("source.txt"), b"direct-input\n").unwrap();
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--none-applicable",
        "obligations,findings,blockers,decisions,externalActions",
    ]);
}

fn pty_status(project: &Project) -> String {
    let command = format!(
        "{} goal status --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).replace('\r', "")
}

fn assert_no_run_files(project: &Project) {
    let runs = project.root.join(".exitbind/runs");
    let count = fs::read_dir(runs).unwrap().count();
    assert_eq!(count, 0, "direct goal created governed run files");
}

#[test]
fn direct_goal_closes_and_emits_exact_card_without_a_run() {
    let project = Project::new("direct-goal-card");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_no_run_files(&project);

    let first = pty_status(&project);
    assert_eq!(first.matches(CARD).count(), 1, "{first}");
    assert_eq!(first.lines().last(), Some("EXIT READY"));
    let second = pty_status(&project);
    assert_eq!(second.matches(CARD).count(), 0, "{second}");
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn direct_item_completion_is_typed_and_unknown_or_open_facts_refuse_close() {
    let project = Project::new("direct-goal-item");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "direct finding",
    ]);
    let closed = project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_eq!(closed["closure"]["kind"], "direct");
    assert_eq!(closed["closure"]["resultRefs"].as_array().unwrap().len(), 0);
    fs::remove_dir_all(project.root).unwrap();

    let open = Project::new("direct-goal-open");
    open.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "open",
        "--goal",
        "open goal",
        "--finding",
        "open finding",
    ]);
    let refused = open.call(&["goal", "close", "--direct", "--goal-id", "open"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unresolved"));
    fs::remove_dir_all(open.root).unwrap();

    let unknown = Project::new("direct-goal-unknown");
    unknown.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "unknown",
        "--goal",
        "unknown goal",
    ]);
    let refused = unknown.call(&["goal", "close", "--direct", "--goal-id", "unknown"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unresolved"));
    fs::remove_dir_all(unknown.root).unwrap();
}

#[test]
fn mixed_direct_closure_cannot_bypass_missing_governed_evidence() {
    let project = Project::new("direct-goal-mixed");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "governed finding",
        "--disposition",
        "accepted",
        "--result-ref",
        "smw_missing",
    ]);
    let refused = project.call(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("current governed result"));
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn direct_card_rejects_input_drift_with_corrupt_or_missing_memo() {
    let project = Project::new("direct-goal-drift");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    let cache = project
        .root
        .join(".exitbind/presentation/session-goal.json");

    fs::write(project.root.join("source.txt"), b"drifted-input\n").unwrap();
    fs::write(&cache, b"corrupt cache").unwrap();
    let corrupt = pty_status(&project);
    assert_eq!(corrupt.matches(CARD).count(), 0, "{corrupt}");
    fs::remove_file(&cache).unwrap();
    let missing = pty_status(&project);
    assert_eq!(missing.matches(CARD).count(), 0, "{missing}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn new_goal_request_invalidates_direct_closure_and_machine_surfaces_stay_silent() {
    let project = Project::new("direct-goal-invalidate");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    let machine = project.value(&["goal", "status", "--json"]);
    assert_eq!(machine["closure"]["kind"], "direct");
    assert_eq!(machine["completionMode"], Value::Null);
    assert!(!serde_json::to_string(&machine).unwrap().contains(CARD));
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);

    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "new finding",
    ]);
    let after = pty_status(&project);
    assert_eq!(after.matches(CARD).count(), 0, "{after}");

    let headless = project.call(&["goal", "status"]);
    assert!(!String::from_utf8_lossy(&headless.stdout).contains(CARD));
    let closed_stdin = project.call(&["goal", "status"]);
    assert!(!String::from_utf8_lossy(&closed_stdin.stdout).contains(CARD));
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

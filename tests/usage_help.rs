//! Existing usage routes are discoverable without configuration or source reads.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    process::{Output, Stdio},
};

fn help(root: &std::path::Path, args: &[&str]) -> String {
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn parent_usage_discovery_works_in_first_and_subsequent_fresh_processes() {
    let root = support::temp("usage-help-no-setup");
    for _ in 0..2 {
        let goal = help(&root, &["goal", "--help"]);
        assert!(goal.contains("goal usage --goal-id ID"), "{goal}");
        assert!(goal.contains("goal <child> --help"), "{goal}");
        let work = help(&root, &["work", "--help"]);
        assert!(work.contains("work usage WORK"), "{work}");
        assert!(work.contains("work <child> --help"), "{work}");
        for args in [["goal", "usage", "--help"], ["work", "usage", "--help"]] {
            let child = help(&root, &args);
            assert!(child.contains("8192 bytes"), "{child}");
            assert!(child.contains("REFERENCE.md"), "{child}");
            assert!(child.contains("Missing counters stay unknown"), "{child}");
            let example: Value =
                serde_json::from_str(child.lines().find(|line| line.starts_with('{')).unwrap())
                    .unwrap();
            assert_eq!(example["status"], "missing");
            assert!(example.get("values").is_none());
        }
    }
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        0,
        "help must not initialize a project"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn usage_help_example_is_admitted_replayed_and_keeps_missing_distinct_from_zero() {
    let root = support::temp("usage-help-contract");
    support::git_topology::repository(&root);
    let run = |args: &[&str], input: Option<&Value>| -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&root).args(args);
        if let Some(input) = input {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.to_string().as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    };
    let value = |args: &[&str], input: Option<&Value>| -> Value {
        let output = run(args, input);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let init = run(
        &[
            "init",
            "--mode",
            "portable",
            "--root",
            root.to_str().unwrap(),
        ],
        None,
    );
    assert!(init.status.success(), "{init:?}");
    value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "help-goal",
            "--goal",
            "fixture usage",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let before = value(&["goal", "status", "--json"], None);
    let child = help(&root, &["goal", "usage", "--help"]);
    let mut event: Value =
        serde_json::from_str(child.lines().find(|line| line.starts_with('{')).unwrap()).unwrap();
    let record = [
        "goal",
        "usage",
        "--goal-id",
        "help-goal",
        "--apply",
        "--json",
    ];
    assert_eq!(value(&record, Some(&event))["status"], "recorded");
    assert_eq!(value(&record, Some(&event))["status"], "replayed");
    let inspect = ["goal", "usage", "--goal-id", "help-goal", "--json"];
    let missing = value(&inspect, None);
    assert!(missing["totals"].is_null(), "{missing}");
    assert_eq!(missing["recordCount"], 1);
    event["id"] = json!("root-zero-1");
    event["status"] = json!("observed");
    event["values"] = json!({"inputTokens":0,"cachedInputTokens":0,"outputTokens":0});
    value(&record, Some(&event));
    let zero = value(&inspect, None);
    assert_eq!(zero["totals"]["totalTokens"], 0);
    assert_eq!(zero["recordCount"], 2);
    event["id"] = json!("rejected-private-content");
    event["transcript"] = json!("fixture-only content");
    let rejected = run(&record, Some(&event));
    assert!(!rejected.status.success());
    let error: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(
        error["error"],
        "usage observation contains an unsupported field"
    );
    assert_eq!(value(&inspect, None)["recordCount"], 2);
    assert_eq!(
        value(&["goal", "status", "--json"], None)["currentReadiness"],
        before["currentReadiness"]
    );
    fs::remove_dir_all(root).unwrap();
}

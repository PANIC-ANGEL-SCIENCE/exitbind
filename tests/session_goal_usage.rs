//! Goal-owned numeric observations have a bounded CLI round trip and do not
//! change semantic goal currentness when telemetry is recorded.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Output, Stdio},
};

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

    fn call(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.stdin(Stdio::null()).output().unwrap()
        }
    }

    fn value(&self, args: &[&str], input: Option<&str>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn goal_usage_records_replay_and_preserves_semantic_currentness() {
    let project = Project::new("goal-usage-round-trip");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "usage-goal",
            "--goal",
            "measure the bounded outcome",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let before = project.value(&["goal", "status", "--json"], None);
    let event = json!({
        "id": "setup-1",
        "source": "host_reported",
        "scope": "setup",
        "phase": "setup",
        "status": "observed",
        "semantics": "delta",
        "lifetime": "invocation",
        "values": {"inputTokens": 4, "cachedInputTokens": 1, "outputTokens": 3}
    });
    let input = format!("{}\n", serde_json::to_string(&event).unwrap());
    let recorded = project.value(
        &[
            "goal",
            "usage",
            "--goal-id",
            "usage-goal",
            "--apply",
            "--json",
        ],
        Some(&input),
    );
    assert_eq!(recorded["status"], "recorded");
    let replayed = project.value(
        &[
            "goal",
            "usage",
            "--goal-id",
            "usage-goal",
            "--apply",
            "--json",
        ],
        Some(&input),
    );
    assert_eq!(replayed["status"], "replayed");
    let inspected = project.value(&["goal", "usage", "--goal-id", "usage-goal"], None);
    assert_eq!(inspected["totals"]["inputTokens"], 4);
    assert_eq!(inspected["totals"]["cachedInputTokens"], 1);
    assert_eq!(inspected["totals"]["outputTokens"], 3);
    assert_eq!(inspected["totals"]["totalTokens"], 7);
    assert_eq!(inspected["recordCount"], 1);
    let after = project.value(&["goal", "status", "--json"], None);
    assert_eq!(before["currentReadiness"], after["currentReadiness"]);
    assert_eq!(
        before["goalProgress"]["overall"],
        after["goalProgress"]["overall"]
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn work_usage_records_a_direct_observation_without_relaunching_work() {
    let project = Project::new("work-usage-round-trip");
    let started = project.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "record one bounded work observation",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = started["work"].as_str().unwrap().to_owned();
    let event = json!({
        "id": "work-1",
        "source": "activity",
        "scope": "direct",
        "phase": "implementation",
        "status": "observed",
        "semantics": "delta",
        "lifetime": "invocation",
        "values": {"inputTokens": 2, "cachedInputTokens": 0, "outputTokens": 1}
    });
    let input = format!("{}\n", serde_json::to_string(&event).unwrap());
    let apply = vec!["work", "usage", work.as_str(), "--apply", "--json"];
    let recorded = project.value(&apply, Some(&input));
    assert_eq!(recorded["status"], "recorded");
    let replayed = project.value(&apply, Some(&input));
    assert_eq!(replayed["status"], "replayed");
    let inspect = vec!["work", "usage", work.as_str(), "--json"];
    let details = project.value(&inspect, None);
    assert_eq!(details["totals"]["totalTokens"], 3);
    assert_eq!(details["recordCount"], 1);
    fs::remove_dir_all(project.root).unwrap();
}

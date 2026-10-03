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
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "work-goal",
            "--goal",
            "record one bounded work observation",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
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
        "goalId": "work-goal",
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
    let goal_details = project.value(&["goal", "usage", "--goal-id", "work-goal"], None);
    assert_eq!(goal_details["totals"]["totalTokens"], 3);
    assert_eq!(goal_details["recordCount"], 1);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn goal_usage_rejects_mixed_counter_representations_and_keeps_valid_delta() {
    let project = Project::new("goal-usage-counter-contract");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "counter-goal",
            "--goal",
            "measure one counter stream",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let record = |event: Value| {
        let input = format!("{}\n", serde_json::to_string(&event).unwrap());
        project.value(
            &[
                "goal",
                "usage",
                "--goal-id",
                "counter-goal",
                "--apply",
                "--json",
            ],
            Some(&input),
        )
    };
    record(json!({
        "id":"counter-100", "source":"host_reported", "scope":"direct",
        "phase":"implementation", "status":"observed", "semantics":"cumulative",
        "lifetime":"session", "sessionId":"session-1", "counterId":"counter-1",
        "adapterVersion":"codex-1",
        "values":{"inputTokens":100,"cachedInputTokens":20,"outputTokens":10}
    }));
    record(json!({
        "id":"counter-110", "source":"host_reported", "scope":"direct",
        "phase":"implementation", "status":"observed", "semantics":"cumulative",
        "lifetime":"session", "sessionId":"session-1", "counterId":"counter-1",
        "adapterVersion":"codex-1",
        "values":{"inputTokens":110,"cachedInputTokens":25,"outputTokens":20}
    }));
    let details = project.value(&["goal", "usage", "--goal-id", "counter-goal"], None);
    assert_eq!(details["totals"]["inputTokens"], 10);
    assert_eq!(details["totals"]["outputTokens"], 10);
    assert_eq!(details["totals"]["totalTokens"], 20);

    let mixed = json!({
        "id":"per-turn-10", "source":"host_reported", "scope":"direct",
        "phase":"implementation", "status":"observed", "semantics":"per_turn",
        "lifetime":"session", "sessionId":"session-1", "counterId":"counter-1",
        "adapterVersion":"codex-1", "turn":"turn-1",
        "values":{"inputTokens":10,"cachedInputTokens":2,"outputTokens":1}
    });
    let input = format!("{}\n", serde_json::to_string(&mixed).unwrap());
    let rejected = project.call(
        &[
            "goal",
            "usage",
            "--goal-id",
            "counter-goal",
            "--apply",
            "--json",
        ],
        Some(&input),
    );
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("mixes delta, cumulative, and per_turn")
    );
    let mut mixed_delta = mixed;
    mixed_delta["id"] = json!("delta-10");
    mixed_delta["semantics"] = json!("delta");
    let input = format!("{}\n", serde_json::to_string(&mixed_delta).unwrap());
    let rejected = project.call(
        &[
            "goal",
            "usage",
            "--goal-id",
            "counter-goal",
            "--apply",
            "--json",
        ],
        Some(&input),
    );
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("mixes delta, cumulative, and per_turn")
    );
    fs::remove_dir_all(project.root).unwrap();
}

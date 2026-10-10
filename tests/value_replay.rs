// Current Exitbind and preserved historical-reader contracts run in the default suite.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CHECK: &str = "exitbind check --config verification.json";

fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let output = invoke(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(output.status.success(), "{}", text(&output));
    root
}

fn invoke(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("soulmate binary should start")
}

fn invoke_exitbind(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("exitbind binary should start")
}

fn invoke_owned(root: &Path, arguments: Vec<String>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("soulmate binary should start")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; output: {}", text(output)))
}

fn state_artifact(root: &Path, name: &str, content: &str) -> String {
    let path = root.join(".exitbind/artifacts").join(name);
    fs::write(path, content).expect("artifact should be written");
    format!(".exitbind/artifacts/{name}")
}

fn checked_start(root: &Path, ledger: &str) {
    let output = invoke(
        root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "replay test",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
}

fn submit(root: &Path, agent: &str, ledger: &str, outcome: &str, artifact: &str) {
    let output = invoke(
        root,
        &[
            "run",
            "submit",
            agent,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
}

fn record_check(root: &Path, ledger: &str, target: &str, exit_code: &str) {
    let output = invoke_owned(
        root,
        vec![
            "run".into(),
            "record-check".into(),
            ledger.into(),
            "--target".into(),
            target.into(),
            "--check-command".into(),
            CHECK.into(),
            "--exit-code".into(),
            exit_code.into(),
            "--json".into(),
            "--config".into(),
            "exitbind.json".into(),
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let value = json_output(&output);
    assert_eq!(value["event"]["action"], "check");
}

fn prepare_checked_project(label: &str) -> (PathBuf, String, Vec<Value>, String) {
    let root = project(label);
    let ledger = ".exitbind/runs/replay.jsonl".to_owned();
    checked_start(&root, &ledger);
    let lead = state_artifact(&root, "replay-lead.md", "lead scope\n");
    submit(&root, "lead", &ledger, "scoped", &lead);
    let worker = state_artifact(&root, "replay-worker.md", "worker completion\n");
    submit(&root, "worker", &ledger, "completed", &worker);
    let events = read_events(&root, &ledger);
    let worker_target = events
        .iter()
        .find(|event| event["role"] == "worker")
        .and_then(|event| event["eventSha256"].as_str())
        .expect("worker event hash")
        .to_owned();
    let reviewer = state_artifact(&root, "replay-reviewer.md", "reviewer approval\n");
    submit(&root, "reviewer", &ledger, "approved", &reviewer);
    let events = read_events(&root, &ledger);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "submit")
            .count(),
        3
    );
    (root, ledger, events, worker_target)
}

#[test]
fn replayed_forged_acceptance_requires_passing_check() {
    let (root, ledger, without_check, worker_target) =
        prepare_checked_project("value-replay-acceptance");

    let mut missing = without_check.clone();
    append_forged_acceptance(&root, &mut missing, "replay-forged-missing.md");
    let missing_ledger = ".exitbind/runs/replay-missing.jsonl";
    write_events(&root, missing_ledger, &missing);
    let rejected = inspect(&root, missing_ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(
        &rejected,
        &["canonical acceptance requires reviewer approval"],
    );
    let missing_status = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    ));
    assert_eq!(missing_status["checks"]["status"], "not_observed");

    record_check(&root, &ledger, &worker_target, "1");
    let with_failed_check = read_events(&root, &ledger);
    let mut failed = with_failed_check;
    append_forged_acceptance(&root, &mut failed, "replay-forged-failed.md");
    let failed_ledger = ".exitbind/runs/replay-failed.jsonl";
    write_events(&root, failed_ledger, &failed);
    let rejected = inspect(&root, failed_ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(
        &rejected,
        &["canonical acceptance requires reviewer approval"],
    );
    let failed_status = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    ));
    assert_eq!(failed_status["checks"]["status"], "blocked");
    assert_eq!(failed_status["checks"]["failedCount"], 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn omitted_review_still_requires_a_passing_check_before_acceptance() {
    let root = project("value-replay-omitted-review-check-gate");
    let ledger = ".exitbind/runs/omitted-review-check-gate.jsonl";
    let started = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "exercise the check gate with recorded review omission",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "local_report",
            "--review-policy",
            "omitted",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(started.status.success(), "{}", text(&started));
    let lead = state_artifact(&root, "omitted-check-lead.md", "scope\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "omitted-check-worker.md", "worker result\n");
    submit(&root, "worker", ledger, "completed", &worker);
    let worker_target = read_events(&root, ledger)
        .iter()
        .find(|event| event["role"] == "worker")
        .and_then(|event| event["eventSha256"].as_str())
        .expect("completed worker event is present in the canonical ledger")
        .to_owned();
    let before_refusal = fs::read(root.join(ledger)).unwrap();
    let accepted_before_check = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!accepted_before_check.status.success());
    assert_contains_all(
        &accepted_before_check,
        &[
            "acceptance refused",
            "configured check evidence is check_missing",
        ],
    );
    let after_refusal = fs::read(root.join(ledger)).unwrap();
    assert!(after_refusal.starts_with(&before_refusal));
    let refused_events = read_events(&root, ledger);
    assert_eq!(refused_events.last().unwrap()["action"], "protect");
    assert_eq!(refused_events.last().unwrap()["reason"], "check_missing");
    assert_eq!(refused_events.len(), 4);

    record_check(&root, ledger, &worker_target, "1");
    let before_failed_refusal = fs::read(root.join(ledger)).unwrap();
    let accepted_after_failed_check = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!accepted_after_failed_check.status.success());
    assert_contains_all(
        &accepted_after_failed_check,
        &[
            "acceptance refused",
            "configured check evidence is check_failed",
        ],
    );
    let after_failed_refusal = fs::read(root.join(ledger)).unwrap();
    assert!(after_failed_refusal.starts_with(&before_failed_refusal));
    let failed_events = read_events(&root, ledger);
    assert_eq!(failed_events.len(), 6);
    assert_eq!(failed_events.last().unwrap()["action"], "protect");
    assert_eq!(failed_events.last().unwrap()["reason"], "check_failed");
    assert!(!failed_events
        .iter()
        .any(|event| event["outcome"] == "accepted"));

    record_check(&root, ledger, &worker_target, "0");
    let accepted_after_check = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(
        accepted_after_check.status.success(),
        "{}",
        text(&accepted_after_check)
    );
    let final_status = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    ));
    assert_eq!(final_status["status"], "accepted");
    assert_eq!(final_status["checks"]["status"], "passed");
    assert_eq!(
        read_events(&root, ledger)[0]["reviewPolicy"]["decision"],
        "omitted"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn historical_reviewer_after_failed_check_is_read_only_compatibility() {
    let (root, ledger, _without_check, worker_target) =
        prepare_checked_project("value-replay-historical-review");
    record_check(&root, &ledger, &worker_target, "1");

    let mut events = read_events(&root, &ledger);
    let reviewer_index = events
        .iter()
        .position(|event| event["role"] == "reviewer")
        .expect("reviewer event");
    let reviewer = events.remove(reviewer_index);
    let check_index = action_index(&events, "check");
    let mut reviewer = reviewer;
    let producer_name = reviewer["producer"]["name"].clone();
    let old_versions = [
        "0.20.0",
        "0.21.0",
        "0.23.1-rc.2",
        "0.24.0-rc.1",
        "0.24.0-rc.7",
        "0.25.0-rc.1",
    ];
    reviewer["timestamp"] = events[check_index]["timestamp"].clone();
    events.insert(check_index + 1, reviewer);
    for event in &mut events {
        event["producer"] = json!({
            "name": producer_name.clone(),
            "version": old_versions[0],
            "commit": null,
        });
    }
    rehash_chain_with_current_worker_target(&mut events);
    let historical_ledger = ".exitbind/runs/replay-historical-review.jsonl";
    write_events(&root, historical_ledger, &events);

    let inspected = inspect(&root, historical_ledger);
    assert!(inspected.status.success(), "{}", text(&inspected));
    let value = json_output(&inspected);
    assert_eq!(
        value["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["role"] == "reviewer")
            .count(),
        1
    );

    let status = invoke(
        &root,
        &[
            "run",
            "status",
            historical_ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(status.status.success(), "{}", text(&status));
    let status = json_output(&status);
    assert_eq!(status["checks"]["status"], "blocked");
    assert_eq!(status["review"]["status"], "stale");
    assert_eq!(status["acceptance"]["status"], "absent");

    for producer_version in old_versions.iter().skip(1) {
        let mut historical = events.clone();
        for event in &mut historical {
            event["producer"]["version"] = json!(producer_version);
        }
        rehash_chain_with_current_worker_target(&mut historical);
        let historical_ledger =
            format!(".exitbind/runs/replay-historical-review-{producer_version}.jsonl");
        write_events(&root, &historical_ledger, &historical);
        let inspected = inspect(&root, &historical_ledger);
        assert!(inspected.status.success(), "{}", text(&inspected));
        let historical_status = invoke(
            &root,
            &[
                "run",
                "status",
                &historical_ledger,
                "--json",
                "--config",
                "exitbind.json",
            ],
        );
        assert!(
            historical_status.status.success(),
            "{}",
            text(&historical_status)
        );
        let historical_status = json_output(&historical_status);
        assert_eq!(historical_status["checks"]["status"], "blocked");
        assert_eq!(historical_status["review"]["status"], "stale");
        assert_eq!(historical_status["acceptance"]["status"], "absent");
    }

    for producer_version in ["0.25.1", "0.26.0", "0.24.1", "not-a-release"] {
        let mut rejected = events.clone();
        for event in &mut rejected {
            event["producer"]["version"] = json!(producer_version);
        }
        rehash_chain_with_current_worker_target(&mut rejected);
        let rejected_ledger = format!(".exitbind/runs/replay-reviewer-{producer_version}.jsonl");
        write_events(&root, &rejected_ledger, &rejected);
        let rejected = inspect(&root, &rejected_ledger);
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert_contains_all(&rejected, &["not currently pending"]);
    }

    for forged_field in ["agent", "stage", "attempt"] {
        let mut forged = events.clone();
        let reviewer = forged
            .iter_mut()
            .find(|event| event["role"] == "reviewer")
            .expect("reviewer event");
        match forged_field {
            "agent" => reviewer["agent"] = json!("unplanned_reviewer"),
            "stage" => reviewer["stage"] = json!(reviewer["stage"].as_u64().unwrap() + 1),
            "attempt" => reviewer["attempt"] = json!(reviewer["attempt"].as_u64().unwrap() + 1),
            _ => unreachable!(),
        }
        rehash_chain_with_current_worker_target(&mut forged);
        let forged_ledger = format!(".exitbind/runs/replay-reviewer-forged-{forged_field}.jsonl");
        write_events(&root, &forged_ledger, &forged);
        let rejected = inspect(&root, &forged_ledger);
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert_contains_all(&rejected, &["not currently pending"]);
    }

    let mut corrupt_hash = events.clone();
    let reviewer = corrupt_hash
        .iter_mut()
        .find(|event| event["role"] == "reviewer")
        .expect("reviewer event");
    reviewer["agent"] = json!("unplanned_reviewer");
    let corrupt_ledger = ".exitbind/runs/replay-reviewer-corrupt-hash.jsonl";
    write_events(&root, corrupt_ledger, &corrupt_hash);
    let rejected = inspect(&root, corrupt_ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(&rejected, &["hash mismatch"]);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replayed_check_after_terminal_state_is_rejected() {
    let (root, ledger, without_check, worker_target) =
        prepare_checked_project("value-replay-terminal-check");
    record_check(&root, &ledger, &worker_target, "0");
    let current_check = read_events(&root, &ledger)
        .into_iter()
        .find(|event| event["action"] == "check")
        .expect("fixture command emits a current check template");

    let mut forged = without_check;
    append_forged_acceptance(&root, &mut forged, "replay-terminal-check.md");
    forged.last_mut().unwrap()["outcome"] = json!("rejected");
    forged
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("inputsSha256");
    forged.push(current_check);
    rehash_chain(&mut forged);
    let terminal_ledger = ".exitbind/runs/replay-terminal-check.jsonl";
    write_events(&root, terminal_ledger, &forged);
    let rejected = inspect(&root, terminal_ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(&rejected, &["run has already reached a terminal state"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replayed_v3_shape_and_policy_mutations_are_rejected_after_rehash() {
    let (root, ledger, without_check, worker_target) =
        prepare_checked_project("value-replay-shapes");
    record_check(&root, &ledger, &worker_target, "0");
    let with_passing_check = read_events(&root, &ledger);
    let check_index = action_index(&with_passing_check, "check");

    let mut unknown_check = with_passing_check.clone();
    unknown_check[check_index]["unexpected"] = json!(true);
    reject_replayed(
        &root,
        ".exitbind/runs/replay-unknown-check.jsonl",
        unknown_check,
        &["malformed check event"],
    );

    let mut malformed_check = with_passing_check.clone();
    malformed_check[check_index]["exitCode"] = json!(-1);
    reject_replayed(
        &root,
        ".exitbind/runs/replay-malformed-check.jsonl",
        malformed_check,
        &["malformed check event"],
    );

    let mut wrong_command = with_passing_check.clone();
    wrong_command[check_index]["checkCommand"] = json!("different check");
    wrong_command[check_index]["checkCommandSha256"] = json!(sha256(b"different check"));
    reject_replayed(
        &root,
        ".exitbind/runs/replay-wrong-command.jsonl",
        wrong_command,
        &["check report does not match configured policy"],
    );

    let mut wrong_origin = with_passing_check.clone();
    wrong_origin[check_index]["origin"] = json!("local_report");
    reject_replayed(
        &root,
        ".exitbind/runs/replay-wrong-origin.jsonl",
        wrong_origin,
        &["check report does not match configured policy"],
    );

    let mut wrong_target = with_passing_check.clone();
    wrong_target[check_index]["targetEventSha256"] = json!("0".repeat(64));
    reject_replayed(
        &root,
        ".exitbind/runs/replay-wrong-target.jsonl",
        wrong_target,
        &["check target is not a current worker completion"],
    );

    let mut mixed_versions = without_check.clone();
    mixed_versions[1]["version"] = json!(2);
    reject_replayed(
        &root,
        ".exitbind/runs/replay-mixed-versions.jsonl",
        mixed_versions,
        &["mixed event versions"],
    );

    record_check(&root, &ledger, &worker_target, "1");
    let lead_accept = state_artifact(&root, "replay-protection-lead.md", "lead acceptance\n");
    let refused = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            &ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead_accept,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    let with_protection = read_events(&root, &ledger);
    assert_eq!(with_protection.last().unwrap()["action"], "protect");

    let mut unknown_protection = with_protection.clone();
    let protection_index = action_index(&unknown_protection, "protect");
    unknown_protection[protection_index]["unexpected"] = json!(true);
    reject_replayed(
        &root,
        ".exitbind/runs/replay-unknown-protection.jsonl",
        unknown_protection,
        &["malformed protection event"],
    );

    let mut malformed_protection = with_protection;
    let protection_index = action_index(&malformed_protection, "protect");
    malformed_protection[protection_index]["checkEvidence"] = json!([]);
    reject_replayed(
        &root,
        ".exitbind/runs/replay-malformed-protection.jsonl",
        malformed_protection,
        &["malformed protection event"],
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_v3_checked_submit_and_record_check_preserve_historical_version() {
    let root = project("value-replay-live-v3");
    let ledger = ".exitbind/runs/live-v3.jsonl";
    checked_start(&root, ledger);
    let lead = state_artifact(&root, "live-v3-lead.md", "lead scope\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let mut prefix = read_events(&root, ledger);
    for event in &mut prefix {
        event["version"] = json!(3);
        event["producer"] = json!({
            "name": "soulmate",
            "version": "0.25.0",
            "commit": null
        });
        let allowed: &[&str] = match event["action"].as_str().unwrap() {
            "start" => &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "workflow",
                "goal",
                "configSha256",
                "plan",
                "checkPolicy",
                "harnessReceipt",
                "supersedes",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            "submit" => &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "outcome",
                "artifact",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            other => panic!("unexpected current prefix action {other}"),
        };
        event
            .as_object_mut()
            .unwrap()
            .retain(|key, _| allowed.contains(&key.as_str()));
    }
    rehash_chain(&mut prefix);
    write_events(&root, ledger, &prefix);

    let worker = state_artifact(&root, "live-v3-worker.md", "worker completion\n");
    submit(&root, "worker", ledger, "completed", &worker);
    let events = read_events(&root, ledger);
    let worker_event = events
        .iter()
        .find(|event| event["role"] == "worker")
        .expect("worker submission");
    assert_eq!(worker_event["version"], 3);
    let target = worker_event["eventSha256"].as_str().unwrap();
    record_check(&root, ledger, target, "0");
    let checked = read_events(&root, ledger);
    let check = checked
        .iter()
        .find(|event| event["action"] == "check")
        .expect("check event");
    assert_eq!(check["version"], 3);
    assert_eq!(check["exitCode"], 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn frozen_v3_fixture_inspects_successfully() {
    let root = project("value-replay-frozen");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-v3.jsonl");
    let ledger = ".exitbind/runs/frozen-v3.jsonl";
    fs::copy(fixture, root.join(ledger)).expect("frozen v3 fixture should copy");
    let inspected = inspect(&root, ledger);
    assert!(inspected.status.success(), "{}", text(&inspected));
    let value = json_output(&inspected);
    assert_eq!(value["valid"], true);
    assert_eq!(value["status"], "running");
    assert_eq!(value["events"].as_array().unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn frozen_v1_history_remains_readable_without_an_old_writer() {
    let root = project("value-replay-v1-history");
    let ledger = ".exitbind/runs/frozen-v1.jsonl";
    let fixture = include_bytes!("fixtures/v0.0.8-run.jsonl");
    fs::write(root.join(ledger), fixture).expect("frozen v1 fixture should copy");
    let accepted = inspect(&root, ledger);
    assert!(accepted.status.success(), "{}", text(&accepted));
    assert_eq!(json_output(&accepted)["valid"], true);
    assert_eq!(fs::read(root.join(ledger)).unwrap(), fixture);

    // The frozen producer-less v1 above and an explicitly named historical
    // producer both remain readable. Rehash mutations so identity refusal is
    // exercised on an otherwise intact chain.
    let mut historical = read_events(&root, ledger);
    assert!(historical[0].get("producer").is_none());
    historical[0]["producer"] = json!({
        "name": "soulmate",
        "version": "0.9.0",
        "commit": null
    });
    rehash_chain(&mut historical);
    let historical_ledger = ".exitbind/runs/named-historical-v1.jsonl";
    write_events(&root, historical_ledger, &historical);
    let historical_read = inspect(&root, historical_ledger);
    assert!(
        historical_read.status.success(),
        "{}",
        text(&historical_read)
    );
    assert_eq!(json_output(&historical_read)["valid"], true);

    let mut relabeled = historical;
    relabeled[0]["producer"] = json!({
        "name": "exitbind",
        "version": env!("CARGO_PKG_VERSION"),
        "commit": null
    });
    let relabeled_ledger = ".exitbind/runs/relabelled-v1.jsonl";
    reject_replayed(&root, relabeled_ledger, relabeled, &["invalid producer"]);
    assert_eq!(fs::read(root.join(ledger)).unwrap(), fixture);

    let v5_root = project("value-replay-v5-producer");
    let v5_ledger = ".exitbind/runs/v5.jsonl";
    let v5_started = invoke_exitbind(
        &v5_root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "current producer",
            "--ledger",
            v5_ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(v5_started.status.success(), "{}", text(&v5_started));
    let v5_events = read_events(&v5_root, v5_ledger);
    assert_eq!(v5_events[0]["version"], 8);
    assert_eq!(v5_events[0]["producer"]["name"], "exitbind");
    let v5_inspected = invoke_exitbind(
        &v5_root,
        &["run", "inspect", v5_ledger, "--config", "exitbind.json"],
    );
    assert!(v5_inspected.status.success(), "{}", text(&v5_inspected));

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(v5_root).unwrap();
}

fn append_forged_acceptance(root: &Path, events: &mut Vec<Value>, name: &str) {
    let artifact = state_artifact(root, name, "forged acceptance artifact\n");
    let bytes = fs::read(root.join(&artifact)).expect("forged artifact should be readable");
    let head_timestamp = events
        .last()
        .and_then(|event| event.get("timestamp"))
        .cloned()
        .expect("ledger head timestamp");
    let reviewer = events
        .iter()
        .rev()
        .find(|event| event["role"] == "reviewer")
        .expect("reviewer event")
        .clone();
    let mut forged = reviewer;
    forged["stage"] = json!(4);
    forged["agent"] = json!("lead");
    forged["role"] = json!("lead");
    forged["outcome"] = json!("accepted");
    forged["timestamp"] = head_timestamp;
    forged["artifact"] = json!({
        "root": "state",
        "path": artifact,
        "sha256": sha256(&bytes),
    });
    events.push(forged);
    rehash_chain(events);
    assert_chain(events);
}

fn reject_replayed(root: &Path, ledger: &str, mut events: Vec<Value>, expected: &[&str]) {
    rehash_chain(&mut events);
    assert_chain(&events);
    write_events(root, ledger, &events);
    let rejected = inspect(root, ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(&rejected, expected);
}

fn inspect(root: &Path, ledger: &str) -> Output {
    invoke(
        root,
        &["run", "inspect", ledger, "--config", "exitbind.json"],
    )
}

fn assert_contains_all(output: &Output, expected: &[&str]) {
    let actual = text(output);
    for needle in expected {
        assert!(actual.contains(needle), "missing {needle} in {actual}");
    }
}

fn action_index(events: &[Value], action: &str) -> usize {
    events
        .iter()
        .position(|event| event["action"] == action)
        .unwrap_or_else(|| panic!("missing {action} event"))
}

fn read_events(root: &Path, ledger: &str) -> Vec<Value> {
    fs::read_to_string(root.join(ledger))
        .expect("ledger should be readable")
        .lines()
        .map(|line| serde_json::from_str(line).expect("ledger line should be JSON"))
        .collect()
}

fn write_events(root: &Path, ledger: &str, events: &[Value]) {
    let source = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("event should serialize"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.join(ledger), format!("{source}\n")).expect("replayed ledger should be written");
}

fn rehash_chain(events: &mut [Value]) {
    let mut previous = Value::Null;
    for event in events {
        event["previousEventSha256"] = previous.clone();
        let mut without_hash = event.clone();
        without_hash
            .as_object_mut()
            .expect("event should be an object")
            .remove("eventSha256");
        let event_hash = sha256(canonical(&without_hash).as_bytes());
        event["eventSha256"] = json!(event_hash.clone());
        previous = json!(event_hash);
    }
}

fn rehash_chain_with_current_worker_target(events: &mut [Value]) {
    rehash_chain(events);
    let worker_target = events
        .iter()
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .and_then(|event| event["eventSha256"].as_str())
        .expect("worker event hash")
        .to_owned();
    for event in events
        .iter_mut()
        .filter(|event| event["action"] == "check" && event.get("requirementId").is_none())
    {
        event["targetEventSha256"] = json!(worker_target);
    }
    rehash_chain(events);
}

fn assert_chain(events: &[Value]) {
    let mut previous = Value::Null;
    for event in events {
        assert_eq!(event["previousEventSha256"], previous);
        let event_hash = event["eventSha256"].as_str().expect("event hash");
        let mut without_hash = event.clone();
        without_hash
            .as_object_mut()
            .expect("event should be an object")
            .remove("eventSha256");
        assert_eq!(event_hash, sha256(canonical(&without_hash).as_bytes()));
        previous = json!(event_hash);
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON key should serialize"),
                        canonical(&object[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).expect("JSON value should serialize"),
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CHECK: &str = "soulmate check --config verification.json";

fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let output = invoke(&root, &["init", "--root", "."]);
    assert!(output.status.success(), "{}", text(&output));
    root
}

fn invoke(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
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
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
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
    let path = root.join(".soulmate/artifacts").join(name);
    fs::write(path, content).expect("artifact should be written");
    format!(".soulmate/artifacts/{name}")
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
            "soulmate.json",
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
            "soulmate.json",
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
            "soulmate.json".into(),
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let value = json_output(&output);
    assert_eq!(value["event"]["action"], "check");
}

fn prepare_checked_project(label: &str) -> (PathBuf, String, Vec<Value>, String) {
    let root = project(label);
    let ledger = ".soulmate/runs/replay.jsonl".to_owned();
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
    let missing_ledger = ".soulmate/runs/replay-missing.jsonl";
    write_events(&root, missing_ledger, &missing);
    let rejected = inspect(&root, missing_ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(
        &rejected,
        &[
            "canonical acceptance requires passing checks",
            "check_missing",
        ],
    );

    record_check(&root, &ledger, &worker_target, "1");
    let with_failed_check = read_events(&root, &ledger);
    let mut failed = with_failed_check;
    append_forged_acceptance(&root, &mut failed, "replay-forged-failed.md");
    let failed_ledger = ".soulmate/runs/replay-failed.jsonl";
    write_events(&root, failed_ledger, &failed);
    let rejected = inspect(&root, failed_ledger);
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert_contains_all(
        &rejected,
        &[
            "canonical acceptance requires passing checks",
            "check_failed",
        ],
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replayed_check_after_terminal_state_is_rejected() {
    let (root, _ledger, without_check, worker_target) =
        prepare_checked_project("value-replay-terminal-check");
    let mut forged = without_check;
    append_forged_acceptance(&root, &mut forged, "replay-terminal-check.md");
    forged[4]["outcome"] = json!("rejected");
    let head = forged.last().unwrap().clone();
    forged.push(json!({
        "version": 4,
        "kind": "run",
        "producer": {"name": "soulmate", "version": env!("CARGO_PKG_VERSION"), "commit": null},
        "action": "check",
        "runId": forged[0]["runId"],
        "targetEventSha256": worker_target,
        "checkCommand": CHECK,
        "checkCommandSha256": sha256(CHECK.as_bytes()),
        "origin": "synthetic",
        "acquisition": "reported",
        "result": {"kind": "exit", "code": 0},
        "durationMs": null,
        "previousEventSha256": head["eventSha256"],
        "timestamp": head["timestamp"],
    }));
    rehash_chain(&mut forged);
    let terminal_ledger = ".soulmate/runs/replay-terminal-check.jsonl";
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
        ".soulmate/runs/replay-unknown-check.jsonl",
        unknown_check,
        &["malformed check event"],
    );

    let mut malformed_check = with_passing_check.clone();
    malformed_check[check_index]["exitCode"] = json!(-1);
    reject_replayed(
        &root,
        ".soulmate/runs/replay-malformed-check.jsonl",
        malformed_check,
        &["malformed check event"],
    );

    let mut wrong_command = with_passing_check.clone();
    wrong_command[check_index]["checkCommand"] = json!("different check");
    wrong_command[check_index]["checkCommandSha256"] = json!(sha256(b"different check"));
    reject_replayed(
        &root,
        ".soulmate/runs/replay-wrong-command.jsonl",
        wrong_command,
        &["check report does not match configured policy"],
    );

    let mut wrong_origin = with_passing_check.clone();
    wrong_origin[check_index]["origin"] = json!("local_report");
    reject_replayed(
        &root,
        ".soulmate/runs/replay-wrong-origin.jsonl",
        wrong_origin,
        &["check report does not match configured policy"],
    );

    let mut wrong_target = with_passing_check.clone();
    wrong_target[check_index]["targetEventSha256"] = json!("0".repeat(64));
    reject_replayed(
        &root,
        ".soulmate/runs/replay-wrong-target.jsonl",
        wrong_target,
        &["check target is not a current worker completion"],
    );

    let mut mixed_versions = without_check.clone();
    mixed_versions[1]["version"] = json!(2);
    reject_replayed(
        &root,
        ".soulmate/runs/replay-mixed-versions.jsonl",
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
            "soulmate.json",
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
        ".soulmate/runs/replay-unknown-protection.jsonl",
        unknown_protection,
        &["malformed protection event"],
    );

    let mut malformed_protection = with_protection;
    let protection_index = action_index(&malformed_protection, "protect");
    malformed_protection[protection_index]["checkEvidence"] = json!([]);
    reject_replayed(
        &root,
        ".soulmate/runs/replay-malformed-protection.jsonl",
        malformed_protection,
        &["malformed protection event"],
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_v3_checked_submit_and_record_check_preserve_historical_version() {
    let root = project("value-replay-live-v3");
    let ledger = ".soulmate/runs/live-v3.jsonl";
    checked_start(&root, ledger);
    let lead = state_artifact(&root, "live-v3-lead.md", "lead scope\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let mut prefix = read_events(&root, ledger);
    for event in &mut prefix {
        event["version"] = json!(3);
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
    let ledger = ".soulmate/runs/frozen-v3.jsonl";
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
fn producer_identity_preserves_v1_history_and_accepts_current_exitbind() {
    let root = project("value-replay-v1-producer");
    let ledger = ".soulmate/runs/v1.jsonl";
    let started = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "historical producer",
            "--ledger",
            ledger,
            "--config",
            "soulmate.json",
        ],
    );
    assert!(started.status.success(), "{}", text(&started));
    let historical = read_events(&root, ledger);
    assert_eq!(historical[0]["version"], 1);
    assert_eq!(historical[0]["producer"]["name"], "soulmate");

    let soulmate_ledger = ".soulmate/runs/v1-soulmate.jsonl";
    write_events(&root, soulmate_ledger, &historical);
    let accepted = inspect(&root, soulmate_ledger);
    assert!(accepted.status.success(), "{}", text(&accepted));

    let mut producerless = historical.clone();
    producerless[0]
        .as_object_mut()
        .expect("historical start should be an object")
        .remove("producer");
    rehash_chain(&mut producerless);
    let producerless_ledger = ".soulmate/runs/v1-producerless.jsonl";
    write_events(&root, producerless_ledger, &producerless);
    let accepted = inspect(&root, producerless_ledger);
    assert!(accepted.status.success(), "{}", text(&accepted));

    let mut relabeled = historical;
    relabeled[0]["producer"]["name"] = json!("exitbind");
    reject_replayed(
        &root,
        ".soulmate/runs/v1-exitbind.jsonl",
        relabeled,
        &["invalid producer"],
    );

    let v5_root = project("value-replay-v5-producer");
    let v5_ledger = ".soulmate/runs/v5.jsonl";
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
            "soulmate.json",
        ],
    );
    assert!(v5_started.status.success(), "{}", text(&v5_started));
    let v5_events = read_events(&v5_root, v5_ledger);
    assert_eq!(v5_events[0]["version"], 6);
    assert_eq!(v5_events[0]["producer"]["name"], "exitbind");
    let v5_inspected = invoke_exitbind(
        &v5_root,
        &["run", "inspect", v5_ledger, "--config", "soulmate.json"],
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
        &["run", "inspect", ledger, "--config", "soulmate.json"],
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

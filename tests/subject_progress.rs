#![cfg(unix)]

#[path = "support/opaque_work_surface.rs"]
mod opaque_work_surface;
mod support;

use opaque_work_surface::{assert_opaque_envelope, assert_opaque_reference};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::new_with_workers(true)
    }

    fn new_single() -> Self {
        Self::new_with_workers(false)
    }

    fn new_with_workers(two_workers: bool) -> Self {
        let root = support::temp("subject-progress");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config_path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        if two_workers {
            let base = config["agents"]["worker"].clone();
            let mut second = base;
            second["profile"] = serde_json::json!("exitbind/agents/worker_two.md");
            second["purpose"] = serde_json::json!("Complete bounded work for worker_two.");
            config["agents"]["worker_two"] = second;
            fs::write(
                root.join("exitbind/agents/worker_two.md"),
                b"# worker_two\n\nComplete bounded work.\n",
            )
            .unwrap();
            config["workflows"]["change"]["workers"] = serde_json::json!(["worker", "worker_two"]);
        }
        fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        Self { root }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.root).args(args);
        if args.first() == Some(&"work") && matches!(args.get(1), Some(&"next" | &"resume")) {
            command.arg("--full");
        }
        command
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            return child.wait_with_output().unwrap();
        }
        command.output().unwrap()
    }

    fn value(&self, args: &[&str], input: Option<&[u8]>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn recorded_check_failure(&self, work: &str) -> Value {
        let output = self.call(&["work", "check", work], None);
        assert!(
            !output.status.success(),
            "failed check unexpectedly succeeded"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["effect"], "recorded", "{value}");
        assert!(
            value["result"]["signal"].is_number()
                || value["result"]["code"]
                    .as_i64()
                    .is_some_and(|code| code != 0),
            "recorded check did not report failure: {value}"
        );
        assert_eq!(value["next"]["action"], "lead_decision", "{value}");
        assert_eq!(value["next"]["role"], "lead", "{value}");
        value
    }

    /// Check-execution counters live outside the product root so recording a
    /// count never changes the tested inputs being counted.
    fn counter(&self, name: &str) -> std::path::PathBuf {
        let directory = self.root.with_extension("counters");
        fs::create_dir_all(&directory).unwrap();
        directory.join(name)
    }

    fn counting_command(&self, name: &str) -> String {
        let path = self.counter(name);
        let path = path.to_str().unwrap();
        format!(
            "n=$(cat '{path}' 2>/dev/null || echo 0); n=$((n+1)); printf '%s\\n' \"$n\" > '{path}'"
        )
    }

    fn count(&self, name: &str) -> String {
        fs::read_to_string(self.counter(name)).unwrap()
    }

    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }

    fn pin_work(&self, actual: &str, fixed: &str) -> String {
        fs::rename(
            self.root.join(self.ledger(actual)),
            self.root.join(self.ledger(fixed)),
        )
        .unwrap();
        fixed.to_owned()
    }

    fn submit_low_level(&self, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Output {
        self.call(
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
                "--json",
            ],
            None,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
        let _ = fs::remove_dir_all(self.root.with_extension("counters"));
    }
}

fn complete_current_worker(fixture: &Fixture, work: &str) -> String {
    let current = fixture.value(&["work", "next", work], None);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            work,
            current["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"same-goal carry scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"same-goal carry worker result"),
    );
    let ledger = fixture.ledger(work);
    fs::read_to_string(fixture.root.join(ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_progress(action: &Value) {
    if action["requiresExpansion"] == true {
        assert!(action["progress"]["state"].is_string());
        assert!(action["progress"]["reason"]["code"].is_string());
        return;
    }
    assert_eq!(action["progress"]["applicable"], true);
    assert!(
        action["progress"]["percent"].is_number(),
        "missing progress: {action}"
    );
    assert!(action["progress"]["weights"]["worker"].is_number());
}

#[test]
fn successful_work_envelopes_are_opaque_and_expandable() {
    let fixture = Fixture::new_single();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "opaque successful envelope",
            "--check-command",
            "printf out; printf err >&2",
        ],
        None,
    );
    assert_opaque_envelope(&started);
    let work = started["work"].as_str().unwrap().to_owned();
    let history_id = started["next"]["packet"]["context"]["expansions"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let expanded_history = fixture.call(&["work", "expand", &work, &history_id], None);
    assert!(expanded_history.status.success(), "{expanded_history:?}");

    let scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    assert_opaque_envelope(&scoped);
    let worker = fixture.value(&["work", "next", &work], None);
    assert_opaque_envelope(&worker);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_opaque_envelope(&completed);
    let checked = fixture.value(&["work", "check", &work], None);
    assert_opaque_envelope(&checked);
    let current = fixture.value(&["work", "next", &work], None);
    let log_id = current["next"]["packet"]["context"]["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item.get("checkLogRef"))
        .and_then(|reference| reference["id"].as_str())
        .unwrap()
        .to_owned();
    let expanded_log = fixture.call(&["work", "expand", &work, &log_id], None);
    assert!(expanded_log.status.success(), "{expanded_log:?}");
    let expanded_log: Value = serde_json::from_slice(&expanded_log.stdout).unwrap();
    assert_eq!(expanded_log["stdout"]["contentHex"], "6f7574");
    assert_eq!(expanded_log["stderr"]["contentHex"], "657272");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_opaque_envelope(&resumed);
    let packet_path = fixture.root.join(".exitbind/residual.json");
    fs::write(
        &packet_path,
        serde_json::to_vec(&resumed["residual"]).unwrap(),
    )
    .unwrap();
    let packet_path = packet_path.to_str().unwrap();
    let validated = fixture.value(&["work", "validate", &work, "--packet", packet_path], None);
    assert_opaque_envelope(&validated);
    assert_eq!(validated["result"], "usable");
}

#[test]
fn work_discovery_diagnostics_are_table_driven_and_fail_closed() {
    for case in [
        "drift",
        "artifact_drift",
        "memory_drift",
        "profile_drift",
        "ambiguity",
        "corruption",
        "explicit",
        "stale",
    ] {
        match case {
            "drift" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                let ledger = fixture.ledger(work);
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                let config = fixture.root.join("exitbind.json");
                let mut bytes = fs::read(&config).unwrap();
                bytes.push(b'\n');
                fs::write(&config, bytes).unwrap();
                let output = fixture.call(&["work", "next", work, "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "config_drift"
                );
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value);
            }
            "artifact_drift" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic artifact drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap().to_owned();
                fixture.value(
                    &[
                        "work",
                        "return",
                        &work,
                        begin["next"]["assignment"].as_str().unwrap(),
                        "--outcome",
                        "scoped",
                    ],
                    Some(b"scope"),
                );
                let worker = fixture.value(&["work", "next", &work], None);
                fixture.value(
                    &[
                        "work",
                        "return",
                        &work,
                        worker["next"]["assignment"].as_str().unwrap(),
                        "--outcome",
                        "completed",
                    ],
                    Some(b"worker result"),
                );
                let ledger = fixture.ledger(&work);
                let artifact = fs::read_to_string(fixture.root.join(&ledger))
                    .unwrap()
                    .lines()
                    .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                    .find(|event| event["action"] == "submit" && event["role"] == "worker")
                    .and_then(|event| event["artifact"]["path"].as_str().map(str::to_owned))
                    .unwrap();
                fs::write(fixture.root.join(&artifact), b"changed artifact").unwrap();
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                let output = fixture.call(&["work", "next", &work, "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "artifact_drift"
                );
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value);
            }
            "memory_drift" => {
                let fixture = Fixture::new_single();
                let config_path = fixture.root.join("exitbind.json");
                let mut config: Value =
                    serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
                config["memory"] = serde_json::json!({
                    "root": ".exitbind/memory",
                    "maxItems": 8,
                    "maxBytes": 32768,
                    "protocolScopes": ["invariants"],
                    "syntheticScopes": ["synthetic"]
                });
                config["agents"]["lead"]["memoryRead"] = serde_json::json!(["invariants"]);
                config["agents"]["lead"]["crossContext"] = serde_json::json!("protocol-only");
                config["agents"]["worker"]["memoryWrite"] = serde_json::json!(["invariants"]);
                config["agents"]["worker"]["memoryReview"] = serde_json::json!(["invariants"]);
                config["agents"]["lead"]["memoryPromote"] = serde_json::json!(["invariants"]);
                fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
                fs::write(fixture.root.join("memory.md"), b"accepted invariant\n").unwrap();
                fs::create_dir_all(fixture.root.join(".exitbind/memory")).unwrap();
                for args in [
                    vec![
                        "memory",
                        "propose",
                        "worker",
                        "memory.md",
                        "--scope",
                        "invariants",
                        "--ledger",
                        ".exitbind/memory/invariant.jsonl",
                    ],
                    vec![
                        "memory",
                        "review",
                        "worker",
                        ".exitbind/memory/invariant.jsonl",
                    ],
                    vec![
                        "memory",
                        "promote",
                        "lead",
                        ".exitbind/memory/invariant.jsonl",
                    ],
                ] {
                    let output = fixture.call(&args, None);
                    assert!(output.status.success(), "{args:?}: {output:?}");
                }
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic memory drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap().to_owned();
                let ledger = fixture.ledger(&work);
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                fs::write(fixture.root.join("memory.md"), b"changed memory\n").unwrap();
                let output = fixture.call(&["work", "next", &work, "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "memory_drift"
                );
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value);
            }
            "profile_drift" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic profile drift",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                let config: Value =
                    serde_json::from_slice(&fs::read(fixture.root.join("exitbind.json")).unwrap())
                        .unwrap();
                let profile = config["agents"]["lead"]["profile"].as_str().unwrap();
                fs::remove_file(fixture.root.join(profile)).unwrap();
                let ledger = fixture.ledger(work);
                let before = fs::read(fixture.root.join(&ledger)).unwrap();
                let output = fixture.call(&["work", "resume", "--json"], None);
                assert!(
                    output.status.success(),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["status"], "resumed");
                assert_eq!(
                    value["next"]["warnings"][0]["classification"],
                    "profile_drift"
                );
                assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
                assert_opaque_envelope(&value);
            }
            "ambiguity" => {
                let fixture = Fixture::new_single();
                let first = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic one",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let second = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic two",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let first = fixture.pin_work(
                    first["work"].as_str().unwrap(),
                    "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                );
                let second = fixture.pin_work(
                    second["work"].as_str().unwrap(),
                    "smw_0000000000000000000000000000000000000000000000000000000000000001",
                );
                let ledgers = [fixture.ledger(&first), fixture.ledger(&second)];
                // Diagnose the legacy discovery path: no current-work focus.
                fs::remove_file(fixture.root.join(".exitbind/current-work.json")).unwrap();
                let before = ledgers
                    .iter()
                    .map(|ledger| fs::read(fixture.root.join(ledger)).unwrap())
                    .collect::<Vec<_>>();
                let value = fixture.value(&["work", "resume", "--json"], None);
                let repeat = fixture.value(&["work", "resume", "--json"], None);
                assert_eq!(value.to_string(), repeat.to_string());
                assert_eq!(value["status"], "ambiguous");
                assert_eq!(value["reason"]["code"], "ambiguous_candidates");
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "choose_explicit_handle");
                assert_eq!(value["nextAction"]["safe"], true);
                let mut expected = vec![serde_json::json!(first), serde_json::json!(second)];
                expected.sort_by_key(Value::to_string);
                assert_eq!(
                    value["works"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|candidate| candidate["work"].clone())
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_opaque_reference(
                    &value["reference"],
                    serde_json::json!({"works": expected}),
                );
                assert_opaque_envelope(&value);
                for (ledger, expected) in ledgers.iter().zip(before) {
                    assert_eq!(fs::read(fixture.root.join(ledger)).unwrap(), expected);
                }
            }
            "corruption" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic corruption",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                let ledger = fixture.ledger(work);
                fs::write(fixture.root.join(&ledger), b"not-json\n").unwrap();
                let output = fixture.call(&["work", "next", work, "--json"], None);
                assert_eq!(output.status.code(), Some(1));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "corrupt_ledger");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "inspect");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_opaque_envelope(&value);
            }
            "explicit" => {
                let fixture = Fixture::new_single();
                let begin = fixture.value(
                    &[
                        "work",
                        "begin",
                        "change",
                        "--goal",
                        "diagnostic explicit",
                        "--check-command",
                        "true",
                    ],
                    None,
                );
                let work = begin["work"].as_str().unwrap();
                let value = fixture.value(&["work", "next", work, "--json"], None);
                assert_eq!(value["reason"]["code"], "explicit_handle");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "continue");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_opaque_envelope(&value);
            }
            "stale" => {
                let fixture = Fixture::new_single();
                let work = "smw_0000000000000000000000000000000000000000000000000000000000000000";
                let output = fixture.call(&["work", "next", work, "--json"], None);
                assert_eq!(output.status.code(), Some(1));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "stale_handle");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "inspect");
                assert_eq!(value["nextAction"]["safe"], true);
                assert_opaque_envelope(&value);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn resume_lists_healthy_and_corrupt_candidates_without_selecting_one() {
    let fixture = Fixture::new_single();
    let healthy = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "healthy candidate",
            "--check-command",
            "true",
        ],
        None,
    );
    let corrupt = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "corrupt candidate",
            "--check-command",
            "true",
        ],
        None,
    );
    let corrupt_work = corrupt["work"].as_str().unwrap();
    let corrupt_ledger = fixture.ledger(corrupt_work);
    fs::write(fixture.root.join(corrupt_ledger), b"not-json\n").unwrap();

    let output = fixture.call(&["work", "resume", "--json"], None);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ambiguous");
    assert_eq!(value["reason"]["code"], "unreadable_candidate");
    assert_eq!(value["effect"], "no-change");
    assert_eq!(value["works"].as_array().unwrap().len(), 1);
    assert_eq!(value["works"][0]["work"], healthy["work"]);
    assert_eq!(value["works"][0]["command"][1], "work");
    assert_eq!(value["works"][0]["command"][2], "next");
    let command = value["works"][0]["command"].as_array().unwrap();
    assert_eq!(command[command.len() - 2], "--config");
    assert_eq!(
        command.last().unwrap(),
        fixture
            .root
            .join("exitbind.json")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(value["works"][0]["ledgerProducer"].is_object());
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["unreadable"][0]["work"], corrupt["work"]);
    assert_eq!(value["nextAction"]["safe"], true);
}

#[test]
fn resume_uses_valid_focus_while_preserving_unreadable_history() {
    let fixture = Fixture::new_single();
    let focused = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "focused current work",
            "--check-command",
            "true",
        ],
        None,
    );
    let corrupt = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "unreadable older history",
            "--check-command",
            "true",
        ],
        None,
    );
    let focused_work = focused["work"].as_str().unwrap();
    fixture.value(&["work", "focus", focused_work], None);
    let corrupt_work = corrupt["work"].as_str().unwrap();
    fs::write(
        fixture.root.join(fixture.ledger(corrupt_work)),
        b"not-json\n",
    )
    .unwrap();

    let value = fixture.value(&["work", "resume"], None);
    assert_eq!(value["status"], "resumed");
    assert_eq!(value["work"], focused_work);
    assert_eq!(value["selection"]["basis"], "current_work_focus");
    assert_eq!(value["works"].as_array().unwrap().len(), 0);
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["unreadable"][0]["work"], corrupt_work);
    assert_eq!(value["unreadable"][0]["reason"], "corrupt_ledger");
    assert_eq!(value["unreadable"][0]["command"][1], "run");
    assert_eq!(value["unreadable"][0]["command"][2], "inspect");
}

#[test]
fn resume_refuses_focused_superseded_work_with_unreadable_successor() {
    let fixture = Fixture::new_single();
    let predecessor = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "superseded focused work",
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = predecessor["work"].as_str().unwrap();
    let predecessor_ledger = fixture.ledger(predecessor_work);
    let successor_work = "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let successor_ledger = fixture.ledger(successor_work);

    fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            "successor work",
            "--ledger",
            &successor_ledger,
            "--check-command",
            "true",
        ],
        None,
    );
    fs::write(fixture.root.join(&successor_ledger), b"not-json\n").unwrap();

    let predecessor_before = fs::read(fixture.root.join(&predecessor_ledger)).unwrap();
    let successor_before = fs::read(fixture.root.join(&successor_ledger)).unwrap();
    let focus_path = fixture.root.join(".exitbind/current-work.json");
    let focus_before = fs::read(&focus_path).unwrap();

    for _ in 0..2 {
        let output = fixture.call(&["work", "resume"], None);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "ambiguous");
        assert_eq!(value["reason"]["code"], "unreadable_candidate");
        assert_ne!(
            value.get("work").and_then(Value::as_str),
            Some(predecessor_work)
        );
        assert!(value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["work"] == successor_work));
    }

    assert_eq!(
        fs::read(fixture.root.join(&predecessor_ledger)).unwrap(),
        predecessor_before
    );
    assert_eq!(
        fs::read(fixture.root.join(&successor_ledger)).unwrap(),
        successor_before
    );
    assert_eq!(fs::read(focus_path).unwrap(), focus_before);
}

#[test]
fn resume_refuses_predecessor_when_successor_ledger_is_missing() {
    let fixture = Fixture::new_single();
    let predecessor = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "missing successor predecessor",
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = predecessor["work"].as_str().unwrap();
    let predecessor_ledger = fixture.ledger(predecessor_work);
    let successor_work = "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let successor_ledger = fixture.ledger(successor_work);
    fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            "missing successor",
            "--ledger",
            &successor_ledger,
            "--check-command",
            "true",
        ],
        None,
    );

    let predecessor_path = fixture.root.join(&predecessor_ledger);
    let successor_path = fixture.root.join(&successor_ledger);
    fs::remove_file(&successor_path).unwrap();
    let claim_path = fixture.root.join(format!("{predecessor_ledger}.supersede"));
    let focus_path = fixture.root.join(".exitbind/current-work.json");
    let predecessor_before = fs::read(&predecessor_path).unwrap();
    let claim_before = fs::read(&claim_path).unwrap();

    for focused in [true, false] {
        if !focused {
            fs::remove_file(&focus_path).unwrap();
        }
        let focus_before = fs::read(&focus_path).ok();
        for _ in 0..2 {
            let output = fixture.call(&["work", "resume"], None);
            assert!(output.status.success(), "{output:?}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["status"], "ambiguous");
            assert_eq!(value["reason"]["code"], "unreadable_candidate");
            assert_ne!(
                value.get("work").and_then(Value::as_str),
                Some(predecessor_work)
            );
            let unreadable = value["unreadable"].as_array().unwrap();
            let successor = unreadable
                .iter()
                .find(|candidate| candidate["ledger"] == successor_ledger)
                .expect("missing successor must be reported as unreadable");
            assert_eq!(successor["work"], successor_work);
            assert_eq!(
                successor["workingDirectory"],
                fixture.root.to_str().unwrap()
            );
            assert_eq!(successor["command"][1], "run");
            assert_eq!(successor["command"][2], "inspect");
            assert_eq!(successor["command"][4], "--json");
            assert_eq!(successor["command"][5], "--config");
            assert_eq!(
                successor["command"][6],
                fixture.root.join("exitbind.json").to_str().unwrap()
            );
            assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);
            assert_eq!(fs::read(&claim_path).unwrap(), claim_before);
            assert!(!successor_path.exists());
            assert_eq!(fs::read(&focus_path).ok(), focus_before);
        }
    }
}

#[test]
fn resume_refuses_focused_predecessor_with_invalid_successor_lineage() {
    let fixture = Fixture::new_single();
    let predecessor = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "lineage predecessor",
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = predecessor["work"].as_str().unwrap();
    let predecessor_ledger = fixture.ledger(predecessor_work);
    let successor_work = "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let successor_ledger = fixture.ledger(successor_work);

    let created = fixture.call(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            "lineage successor",
            "--ledger",
            &successor_ledger,
            "--check-command",
            "true",
        ],
        None,
    );
    assert!(created.status.success(), "{created:?}");
    let created_value: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created_value["work"], successor_work);
    assert_eq!(created_value["currentDetail"]["route"], "work detail");
    assert_eq!(created_value["currentDetail"]["readOnly"], true);
    assert_eq!(created_value["currentDetail"]["requiresFreshContext"], true);
    assert_eq!(created_value["currentDetail"]["command"]["argv"][1], "work");
    assert_eq!(
        created_value["currentDetail"]["command"]["argv"][2],
        "detail"
    );
    assert_eq!(
        created_value["currentDetail"]["command"]["argv"][3],
        successor_work
    );

    let successor_path = fixture.root.join(&successor_ledger);
    let mut events = fs::read_to_string(&successor_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    events[0]["supersedes"]["ledgerSha256"] = serde_json::json!("0".repeat(64));
    let mut previous = Value::Null;
    for event in &mut events {
        event["previousEventSha256"] = previous.clone();
        *event = rehash(event.clone());
        previous = event["eventSha256"].clone();
    }
    let source = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&successor_path, source).unwrap();

    let inspect = fixture.call(&["run", "inspect", &successor_ledger], None);
    assert!(!inspect.status.success(), "{inspect:?}");
    let inspect_text = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(
        inspect_text.contains("superseded predecessor provenance mismatch"),
        "{inspect_text}"
    );

    let predecessor_path = fixture.root.join(&predecessor_ledger);
    let claim_path = fixture.root.join(format!("{predecessor_ledger}.supersede"));
    let focus_path = fixture.root.join(".exitbind/current-work.json");
    let predecessor_before = fs::read(&predecessor_path).unwrap();
    let successor_before = fs::read(&successor_path).unwrap();
    let claim_before = fs::read(&claim_path).unwrap();
    let focus_before = fs::read(&focus_path).unwrap();

    for _ in 0..2 {
        let output = fixture.call(&["work", "resume"], None);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "ambiguous");
        assert_eq!(value["reason"]["code"], "unreadable_candidate");
        assert_ne!(
            value.get("work").and_then(Value::as_str),
            Some(predecessor_work)
        );
        assert!(value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["work"] == successor_work));
    }

    assert_eq!(fs::read(predecessor_path).unwrap(), predecessor_before);
    assert_eq!(fs::read(successor_path).unwrap(), successor_before);
    assert_eq!(fs::read(claim_path).unwrap(), claim_before);
    assert_eq!(fs::read(focus_path).unwrap(), focus_before);
}

#[test]
fn same_goal_successor_carries_replayed_accounting_and_fresh_work_detail() {
    let fixture = Fixture::new_single();
    let goal = "same-goal successor carry";
    let begin = fixture.value(
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
    let predecessor_work = begin["work"].as_str().unwrap().to_owned();
    let predecessor_ledger = fixture.ledger(&predecessor_work);
    let scope = fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            scope["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first worker result"),
    );
    let predecessor_path = fixture.root.join(&predecessor_ledger);
    let predecessor_state = fixture.value(&["run", "inspect", &predecessor_ledger], None);
    let predecessor_before = fs::read(&predecessor_path).unwrap();
    let successor_work = format!("smw_{}", "a".repeat(64));
    let successor_ledger = fixture.ledger(&successor_work);
    let args = [
        "run",
        "supersede",
        &predecessor_ledger,
        "--workflow",
        "change",
        "--goal",
        goal,
        "--ledger",
        &successor_ledger,
        "--check-command",
        "true",
    ];
    let created = fixture.value(&args, None);
    assert_eq!(created["work"], successor_work);
    assert_eq!(created["currentDetail"]["route"], "work detail");
    assert_eq!(created["currentDetail"]["readOnly"], true);
    assert_eq!(created["currentDetail"]["requiresFreshContext"], true);
    assert_eq!(created["currentDetail"]["command"]["argv"][2], "detail");
    assert_eq!(
        created["currentDetail"]["command"]["argv"][3],
        successor_work
    );

    let successor_path = fixture.root.join(&successor_ledger);
    let successor_before = fs::read(&successor_path).unwrap();
    let mut events = fs::read_to_string(&successor_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["carryProtocol"], 1);
    assert_eq!(events[0]["governorCarry"]["accounting"]["spent"], 1);
    assert_eq!(events[0]["governorCarry"]["accounting"]["state"], "ready");
    assert_eq!(
        events[0]["governorCarrySha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(canonical(&events[0]["governorCarry"]).as_bytes())
        )
    );
    let successor = fixture.value(&["run", "inspect", &successor_ledger], None);
    assert_eq!(successor["governor"]["spent"], 1);
    assert_eq!(
        successor["governor"]["seenEvidenceSha256"],
        predecessor_state["governor"]["seenEvidenceSha256"]
    );
    assert!(successor["governor"]["evidence"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(successor["governor"]["currentMutation"], Value::Null);
    assert!(successor["governor"]["seenMutations"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(successor["governor"]["consumedGrants"]
        .as_array()
        .unwrap()
        .is_empty());
    let fresh = fixture.value(&["work", "detail", &successor_work], None);
    assert_ne!(
        fresh["current"]["result"]["subject"]["sha256"],
        predecessor_state["subject"]["sha256"]
    );

    let repeated = fixture.value(&args, None);
    assert_eq!(repeated["runId"], created["runId"]);
    assert_eq!(repeated["work"], successor_work);
    let other_work = format!("smw_{}", "b".repeat(64));
    let other_ledger = fixture.ledger(&other_work);
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            goal,
            "--ledger",
            &other_ledger,
            "--check-command",
            "true",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);
    assert!(!fixture.root.join(&other_ledger).exists());

    events[0]["governorCarry"]["accounting"]["spent"] = serde_json::json!(0);
    events[0]["governorCarrySha256"] = serde_json::json!(format!(
        "{:x}",
        sha2::Sha256::digest(canonical(&events[0]["governorCarry"]).as_bytes())
    ));
    events[0] = rehash(events[0].clone());
    fs::write(
        &successor_path,
        events
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let inspect = fixture.call(&["run", "inspect", &successor_ledger], None);
    assert!(!inspect.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(diagnostic.contains("governor carry"), "{diagnostic}");
    assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);

    // The predecessor's persisted claim binds this successor to the carry.
    // Removing every marker and rehashing the start must not reset spent to 0.
    fs::write(&successor_path, &successor_before).unwrap();
    let mut stripped: Value = serde_json::from_slice(
        successor_before
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    let object = stripped.as_object_mut().unwrap();
    object.remove("carryProtocol");
    object.remove("governorCarry");
    object.remove("governorCarrySha256");
    stripped = rehash(stripped);
    fs::write(
        &successor_path,
        serde_json::to_string(&stripped).unwrap() + "\n",
    )
    .unwrap();
    let inspect = fixture.call(&["run", "inspect", &successor_ledger], None);
    assert!(!inspect.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(
        diagnostic.contains("carry marker is missing"),
        "{diagnostic}"
    );
    assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);
}

#[test]
fn same_goal_carry_accepts_current_subject_check_without_duplicate_governor_evidence() {
    let fixture = Fixture::new_single();
    let goal = "same-goal observed result carry";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "printf stable-result",
            "--review-policy",
            "required",
        ],
        None,
    );
    let predecessor_work = begin["work"].as_str().unwrap().to_owned();
    let predecessor_ledger = fixture.ledger(&predecessor_work);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"predecessor worker result"),
    );
    let predecessor_worker_event = fs::read_to_string(fixture.root.join(&predecessor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor_check = fixture.value(
        &[
            "run",
            "observe-check",
            &predecessor_ledger,
            "--target",
            &predecessor_worker_event,
        ],
        None,
    );
    assert_eq!(predecessor_check["event"]["acquisition"], "observed");
    assert_eq!(predecessor_check["event"]["result"]["code"], 0);

    // Change the checked subject while preserving the exact frozen command
    // and its output. The successor must reject the old target by identity,
    // then accept its current target without treating the repeated result as new evidence.
    fs::write(fixture.root.join("material-input.txt"), b"changed input").unwrap();
    let successor_work = format!("smw_{}", "c".repeat(64));
    let successor_ledger = fixture.ledger(&successor_work);
    let created = fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            goal,
            "--ledger",
            &successor_ledger,
            "--check-command",
            "printf stable-result",
            "--review-policy",
            "required",
        ],
        None,
    );
    assert_eq!(created["work"], successor_work);
    let successor_begin = fixture.value(&["work", "next", &successor_work], None);
    let successor_assignment = successor_begin["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor_state = fixture.value(&["run", "inspect", &predecessor_ledger], None);

    let old_target_before = fs::read(fixture.root.join(&successor_ledger)).unwrap();
    let stale_target = fixture.call(
        &[
            "run",
            "observe-check",
            &successor_ledger,
            "--target",
            &predecessor_worker_event,
        ],
        None,
    );
    assert!(!stale_target.status.success());
    assert_eq!(
        fs::read(fixture.root.join(&successor_ledger)).unwrap(),
        old_target_before
    );

    let successor_scope = fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            &successor_assignment,
            "--outcome",
            "scoped",
        ],
        Some(b"successor scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &successor_work,
            successor_scope["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    let successor_worker = fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            successor_scope["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"successor worker result"),
    );
    let successor_worker_event = fs::read_to_string(fixture.root.join(&successor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(successor_worker["effect"], "recorded");
    let successor_state_before_check = fixture.value(&["run", "inspect", &successor_ledger], None);
    let successor_subject = successor_state_before_check["subject"]["sha256"].clone();
    assert_ne!(successor_subject, predecessor_state["subject"]["sha256"]);
    let before_check = fs::read(fixture.root.join(&successor_ledger)).unwrap();
    let repeated = fixture.call(
        &[
            "run",
            "observe-check",
            &successor_ledger,
            "--target",
            &successor_worker_event,
        ],
        None,
    );
    assert!(
        repeated.status.success(),
        "current-subject check was not accepted: {repeated:?}"
    );
    let repeated: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(repeated["event"]["acquisition"], "observed");
    assert_eq!(repeated["event"]["targetEventSha256"], successor_worker_event);
    assert_eq!(repeated["event"]["subjectSha256"], successor_subject);
    assert_eq!(
        repeated["event"]["result"]["code"],
        predecessor_check["event"]["result"]["code"]
    );
    assert_eq!(
        repeated["event"]["stdout"]["sha256"],
        predecessor_check["event"]["stdout"]["sha256"]
    );
    assert_ne!(
        fs::read(fixture.root.join(&successor_ledger)).unwrap(),
        before_check
    );
    let successor_events = fs::read_to_string(fixture.root.join(&successor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    assert_eq!(
        successor_events
            .iter()
            .filter(|event| event["action"] == "check")
            .count(),
        1
    );
    assert_eq!(
        successor_events
            .iter()
            .filter(|event| event["action"] == "check_observation")
            .count(),
        1,
        "the current check execution still records its bounded admission evidence"
    );
    let after_repeated = fixture.value(&["run", "inspect", &successor_ledger], None);
    assert_eq!(after_repeated["governor"]["state"], "ready");
    assert_eq!(after_repeated["governor"]["noInformationStreak"], 1);
    assert_eq!(after_repeated["governor"]["afterReplan"], false);
    assert_eq!(after_repeated["governor"]["postReplanSpent"], 0);
    assert_eq!(after_repeated["governor"]["spent"], 2);
}

#[test]
fn same_goal_carry_admits_distinct_validated_result_without_reducing_spent() {
    let fixture = Fixture::new_single();
    let goal = "same-goal distinct observed result";
    fs::write(fixture.root.join("status.txt"), b"1").unwrap();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "status=$(cat status.txt); printf same-output; exit \"$status\"",
            "--review-policy",
            "required",
        ],
        None,
    );
    let predecessor_work = begin["work"].as_str().unwrap().to_owned();
    let predecessor_ledger = fixture.ledger(&predecessor_work);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"predecessor worker result"),
    );
    let predecessor_worker_event = fs::read_to_string(fixture.root.join(&predecessor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor_check = fixture.value(
        &[
            "run",
            "observe-check",
            &predecessor_ledger,
            "--target",
            &predecessor_worker_event,
        ],
        None,
    );
    assert_eq!(predecessor_check["event"]["result"]["code"], 1);
    assert_eq!(predecessor_check["event"]["stdout"]["bytes"], 11);
    assert_eq!(
        predecessor_check["event"]["stdout"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    fs::write(fixture.root.join("status.txt"), b"0").unwrap();
    let successor_work = format!("smw_{}", "d".repeat(64));
    let successor_ledger = fixture.ledger(&successor_work);
    let created = fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            goal,
            "--ledger",
            &successor_ledger,
            "--check-command",
            "status=$(cat status.txt); printf same-output; exit \"$status\"",
            "--review-policy",
            "required",
        ],
        None,
    );
    assert_eq!(created["work"], successor_work);
    let successor_detail = fixture.value(&["work", "next", &successor_work], None);
    let assignment = successor_detail["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let scope = fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            &assignment,
            "--outcome",
            "scoped",
        ],
        Some(b"successor scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &successor_work,
            scope["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            scope["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"successor worker result"),
    );
    let successor_worker_event = fs::read_to_string(fixture.root.join(&successor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let repeated_command = fixture.value(
        &[
            "run",
            "observe-check",
            &successor_ledger,
            "--target",
            &successor_worker_event,
        ],
        None,
    );
    assert_eq!(repeated_command["event"]["result"]["code"], 0);
    assert_eq!(repeated_command["event"]["stdout"]["bytes"], 11);
    assert_eq!(
        repeated_command["event"]["stdout"]["sha256"],
        predecessor_check["event"]["stdout"]["sha256"]
    );
    let state = fixture.value(&["run", "inspect", &successor_ledger], None);
    assert_eq!(state["governor"]["spent"], 2);
    assert_eq!(state["governor"]["noInformationStreak"], 0);
    assert_eq!(state["governor"]["state"], "ready");
}

#[test]
fn same_goal_carry_preserves_ready_cumulative_spent_at_three() {
    let fixture = Fixture::new_single();
    let goal = "same-goal ready cumulative telemetry carry";
    let check_command = "status=$(cat status.txt); printf same-output; exit \"$status\"";
    fs::write(fixture.root.join("status.txt"), b"1").unwrap();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            check_command,
            "--review-policy",
            "required",
        ],
        None,
    );
    let mut work = begin["work"].as_str().unwrap().to_owned();

    for (index, status) in [1u8, 2, 3].into_iter().enumerate() {
        let ledger = fixture.ledger(&work);
        let worker_event = complete_current_worker(&fixture, &work);
        let observed = fixture.value(
            &[
                "run",
                "observe-check",
                &ledger,
                "--target",
                &worker_event,
            ],
            None,
        );
        assert_eq!(observed["event"]["acquisition"], "observed");
        assert_eq!(observed["event"]["result"]["code"], status);
        let state = fixture.value(&["run", "inspect", &ledger], None);
        assert_eq!(state["governor"]["spent"], index as u64 + 1);
        assert_eq!(state["governor"]["state"], "ready");
        assert_eq!(state["governor"]["noInformationStreak"], 0);

        let next_status = status + 1;
        fs::write(fixture.root.join("status.txt"), next_status.to_string()).unwrap();
        let successor_work = format!(
            "smw_{}",
            char::from(b'a' + index as u8).to_string().repeat(64)
        );
        let successor_ledger = fixture.ledger(&successor_work);
        let created = fixture.value(
            &[
                "run",
                "supersede",
                &ledger,
                "--workflow",
                "change",
                "--goal",
                goal,
                "--ledger",
                &successor_ledger,
                "--check-command",
                check_command,
                "--review-policy",
                "required",
            ],
            None,
        );
        assert_eq!(created["work"], successor_work);
        let carried = fixture.value(&["run", "inspect", &successor_ledger], None);
        assert_eq!(carried["governor"]["spent"], index as u64 + 1);
        assert_eq!(carried["governor"]["state"], "ready");
        assert_eq!(carried["governor"]["noInformationStreak"], 0);
        assert_eq!(carried["governor"]["afterReplan"], false);
        assert_eq!(carried["governor"]["postReplanSpent"], 0);
        work = successor_work;
    }
}

#[test]
fn resume_with_only_corrupt_candidate_stays_unresolved() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "corrupt only",
            "--check-command",
            "true",
        ],
        None,
    );
    let ledger = fixture.ledger(begin["work"].as_str().unwrap());
    fs::write(fixture.root.join(ledger), b"not-json\n").unwrap();

    let output = fixture.call(&["work", "resume", "--json"], None);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "unresolved");
    assert_eq!(value["reason"]["code"], "unreadable_candidate");
    assert_eq!(value["effect"], "no-change");
    assert!(value["works"].as_array().unwrap().is_empty());
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["nextAction"]["type"], "inspect_candidates");
    assert_eq!(value["nextAction"]["safe"], true);
    assert!(value["nextAction"]["summary"]
        .as_str()
        .unwrap()
        .contains("read-only command shown for each unreadable candidate"));
    assert_eq!(
        value["unreadable"][0]["workingDirectory"],
        fixture.root.to_str().unwrap()
    );
    let command = value["unreadable"][0]["command"].as_array().unwrap();
    assert_eq!(command[1], "run");
    assert_eq!(command[2], "inspect");
    assert_eq!(command[4], "--json");
    assert_eq!(command[5], "--config");
    assert_eq!(
        command[6],
        fixture.root.join("exitbind.json").to_str().unwrap()
    );
}

#[test]
fn work_return_reports_protection_event_as_recorded() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "protected acceptance",
            "--check-command",
            "false",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let _lead = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let _completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker"),
    );
    let check = fixture.recorded_check_failure(&work);
    assert_eq!(check["next"]["action"], "lead_decision");
    assert_eq!(check["next"]["role"], "lead");
    let accepting_lead = fixture.value(&["work", "next", &work], None);
    let output = fixture.call(
        &[
            "work",
            "return",
            &work,
            accepting_lead["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["effect"], "recorded");
    assert_eq!(value["reason"]["code"], "recorded_then_failed");
    assert_eq!(value["nextAction"]["type"], "inspect");
    assert_eq!(value["nextAction"]["safe"], true);
    assert_eq!(value["event"]["action"], "protect");
    assert_eq!(
        value["reference"]["headEventSha256"],
        value["event"]["eventSha256"]
    );
    assert_eq!(
        value["reference"]["eventSha256"],
        value["event"]["eventSha256"]
    );
    assert!(value["nextAction"]["command"].is_array());

    let accepting_again = fixture.value(&["work", "next", &work], None);
    let second = fixture.call(
        &[
            "work",
            "return",
            &work,
            accepting_again["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept again"),
    );
    assert!(second.status.success(), "{second:?}");
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["effect"], "recorded");
    assert_eq!(second["event"]["action"], "protect");
    assert_ne!(
        value["event"]["eventSha256"],
        second["event"]["eventSha256"]
    );
    assert_eq!(
        second["reference"]["eventSha256"],
        second["event"]["eventSha256"]
    );
    assert_eq!(
        second["reference"]["headEventSha256"],
        second["event"]["eventSha256"]
    );
}

#[test]
fn work_return_stale_assignment_does_not_report_recorded() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "stale assignment",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap();
    let ledger = fixture.ledger(work);
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let output = fixture.call(
        &[
            "work",
            "return",
            work,
            "sma_0000000000000000000000000000000000000000000000000000000000000000",
            "--outcome",
            "scoped",
        ],
        Some(b"stale"),
    );
    assert!(!output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("recorded"));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn skill_presentation_is_exact_and_packaged_copy_matches() {
    const ROUTINE: &str = "[Neuro] Exitbind progress: N%.";
    let canonical = include_bytes!("../skills/exitbind/SKILL.md");
    let packaged = include_bytes!("../plugins/exitbind/skills/exitbind/SKILL.md");
    assert_eq!(canonical, packaged);
    let text = std::str::from_utf8(canonical).unwrap();
    assert!(!text.contains(ROUTINE));
    // Human status is product-owned; no model remembers or formats progress.
    assert!(!text.contains("Neuro\nExitbind progress"));
    assert!(text.contains("product's English `goalProgress.systemText`"));
    assert!(text.contains("a model must not render the legacy\npercentage"));
    assert!(text.contains("Supported native and host paths surface that text automatically"));
    assert!(text.contains("remember a progress command"));
    // The product supplies the terminal block and the host copies it without
    // rebuilding it from state or progress.
    assert!(text.contains(
        "carries a non-null `terminal`, that value is the terminal\nstatus block: print it on a line of its own, exactly as given"
    ));
    assert!(text.contains("Never\nreconstruct terminal wording from `exitState` or `progress`."));
    assert!(text.contains("Do not add a routine preservation progress report."));
    assert!(
        text.contains("keep one handle\nand wait for completion or a meaningful state transition")
    );
    assert!(
        text.contains("A timeout is not a\nfailure and never authorizes restarting the command")
    );
}

#[test]
fn skill_keeps_readiness_lead_owned_and_preservation_narrow() {
    let text = std::str::from_utf8(include_bytes!("../skills/exitbind/SKILL.md"))
        .unwrap()
        .to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for contract in [
        "complex but has no material semantic-preservation risk",
        "does not add a preservation route",
        "tool and agent selection remains the lead's responsibility",
        "clarify the accepted behavior, invariants, allowed changes",
        "freeze that accepted meaning",
        "preserve the frozen meaning",
        "read back the same accepted meaning",
        "current implementation subject",
        "trivial work has no preservation ceremony",
        "minimizer may reduce mechanism",
        "does not choose the goal, tools, or agents",
        "does not own final acceptance",
        "does not create a second ledger",
        "separate preservation installation",
        "preservation_missing",
        "preservation_failed",
        "do not install a separate preservation tool for this path",
    ] {
        assert!(
            text.contains(contract),
            "missing Exitbind skill contract: {contract}"
        );
    }
    assert!(!text.contains("coffee"));
    assert!(!text.contains("holytail chooses tools"));
}

#[test]
fn work_facade_surfaces_stale_worker_and_requires_fresh_evidence() {
    let fixture = Fixture::new();
    let check = "test -f marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "subject recovery",
            "--check-command",
            check,
        ],
        None,
    );
    assert_progress(&begin["next"]);
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);

    let _scope = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker_a = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_a["next"]);
    let worker_a_return = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_a["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker A"),
    );
    let worker_a_next = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_a_next["next"]);
    assert_eq!(worker_a_return["next"]["action"], "check");

    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    let checked_a = fixture.value(&["work", "check", &work], None);
    let worker_b = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_b["next"]);
    assert_eq!(checked_a["next"]["action"], "spawn");
    let worker_b_return = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_b["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker B"),
    );
    let worker_b_next = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_b_next["next"]);
    assert_eq!(worker_b_return["next"]["action"], "check");

    let events: Vec<Value> = fs::read_to_string(fixture.root.join(&ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let worker_b_event = events
        .iter()
        .find(|event| event["action"] == "submit" && event["agent"] == "worker_two")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let checked_b = fixture.call(
        &[
            "run",
            "record-check",
            &ledger,
            "--target",
            &worker_b_event,
            "--check-command",
            check,
            "--exit-code",
            "0",
            "--json",
        ],
        None,
    );
    assert!(checked_b.status.success(), "{checked_b:?}");

    let stale = fixture.value(&["work", "next", &work], None);
    assert_progress(&stale["next"]);
    assert_eq!(stale["next"]["action"], "check");

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    let reviewer = fixture.submit_low_level(
        "reviewer",
        &ledger,
        "approved",
        ".exitbind/artifacts/reviewer.md",
    );
    assert!(reviewer.status.success(), "{reviewer:?}");
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let premature =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(
        !premature.status.success(),
        "stale A check was accepted: {premature:?}"
    );

    let recovered_check = fixture.value(&["work", "check", &work], None);
    let recovered_next = fixture.value(&["work", "next", &work], None);
    assert_progress(&recovered_next["next"]);
    assert_eq!(recovered_check["next"]["action"], "lead_decision");
    let stale_acceptance = fixture.call(
        &[
            "work",
            "return",
            &work,
            recovered_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(!stale_acceptance.status.success(), "{stale_acceptance:?}");
    let stale_acceptance_text = format!(
        "{}{}",
        String::from_utf8_lossy(&stale_acceptance.stdout),
        String::from_utf8_lossy(&stale_acceptance.stderr)
    );
    assert!(
        stale_acceptance_text.contains("canonical acceptance requires reviewer approval"),
        "{stale_acceptance:?}"
    );

    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            recovered_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"fresh worker repair"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let fresh_worker = fixture.value(&["work", "next", &work], None);
    let held = fixture.value(
        &[
            "work",
            "return",
            &work,
            fresh_worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh worker continuation"),
    );
    assert_eq!(held["effect"], "held", "{held}");
    let next = fixture.value(&["work", "next", &work], None);
    assert_eq!(next["next"]["role"], "worker");
    assert_ne!(next["next"]["progress"]["state"], "READY");
}

#[test]
fn work_resume_projects_residual_packet_without_repeating_valid_work() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "residual resume",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["action"], "spawn");
    assert_eq!(checked["next"]["role"], "reviewer");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], work);
    assert_eq!(resumed["next"]["role"], "reviewer");
    assert_eq!(resumed["residual"]["next"], "spawn");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check"));
    assert!(resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "current_check" && item["status"] == "passed"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "review"));
    assert_eq!(
        resumed["residual"]["invalidation"]["sessionRestartInvalidates"],
        false
    );
    assert_eq!(
        resumed["residual"]["invalidation"]["subjectChangeInvalidates"],
        true
    );

    let reviewed = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"review"),
    );
    assert_eq!(reviewed["next"]["action"], "lead_decision");
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "lead_acceptance"));

    assert_progress(&scoped["next"]);
}

#[test]
fn preservation_requirement_is_separate_from_functional_check_and_resume_reuses_it() {
    let fixture = Fixture::new_single();
    let functional = fixture.counting_command("functional.count");
    let preservation = fixture.counting_command("preservation.count");
    let (functional, preservation) = (functional.as_str(), preservation.as_str());
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve accepted precedence",
            "--check-command",
            functional,
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    assert_eq!(completed["next"]["check"]["kind"], "check");

    let functional_checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(functional_checked["next"]["action"], "check");
    assert_eq!(functional_checked["next"]["check"]["kind"], "preservation");
    assert_eq!(
        functional_checked["next"]["check"]["requirementId"],
        "precedence"
    );

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    assert!(fixture
        .submit_low_level(
            "reviewer",
            &ledger,
            "approved",
            ".exitbind/artifacts/reviewer.md",
        )
        .status
        .success());
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let premature =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(
        !premature.status.success(),
        "accepted without preservation check: {premature:?}"
    );
    let premature_text = format!(
        "{}{}",
        String::from_utf8_lossy(&premature.stdout),
        String::from_utf8_lossy(&premature.stderr)
    );
    assert!(
        premature_text.contains("preservation_missing"),
        "{premature:?}"
    );

    let preserved = fixture.value(&["work", "check", &work], None);
    assert_eq!(fixture.count("functional.count"), "1\n");
    assert_eq!(fixture.count("preservation.count"), "1\n");
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(fixture.count("functional.count"), "1\n");
    assert_eq!(fixture.count("preservation.count"), "1\n");
    assert!(resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "preservation" && item["requirementId"] == "precedence"));
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "preservation:precedence"));
    assert_eq!(preserved["next"]["action"], "lead_decision");
    let premature = fixture.call(
        &[
            "work",
            "return",
            &work,
            preserved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(!premature.status.success(), "{premature:?}");
    let premature_text = format!(
        "{}{}",
        String::from_utf8_lossy(&premature.stdout),
        String::from_utf8_lossy(&premature.stderr)
    );
    assert!(
        premature_text.contains("canonical acceptance requires reviewer approval"),
        "{premature:?}"
    );

    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            preserved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"fresh implementation"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    fixture.value(&["work", "check", &work], None);
    let preserved_again = fixture.value(&["work", "check", &work], None);
    assert_eq!(fixture.count("functional.count"), "2\n");
    assert_eq!(fixture.count("preservation.count"), "2\n");
    assert_eq!(preserved_again["next"]["role"], "reviewer");
    let reviewed = fixture.value(
        &[
            "work",
            "return",
            &work,
            preserved_again["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"fresh review"),
    );
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            reviewed["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

#[test]
fn failed_preservation_blocks_acceptance_without_claiming_functional_failure() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "detect weakened requirement",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "test ! -f preservation-fail",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    fs::write(fixture.root.join("preservation-fail"), b"weakened").unwrap();
    fixture.value(&["work", "check", &work], None);
    let failed = fixture.recorded_check_failure(&work);
    assert_eq!(failed["next"]["requiresExpansion"], true);
    assert_eq!(failed["next"]["action"], "lead_decision");
    assert_eq!(failed["next"]["role"], "lead");
    let failed_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][1]["status"],
        "failed"
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][1]["requirementId"],
        "precedence"
    );

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    let reviewer = fixture.submit_low_level(
        "reviewer",
        &ledger,
        "approved",
        ".exitbind/artifacts/reviewer.md",
    );
    assert!(
        !reviewer.status.success(),
        "reviewer approval bypassed failed check"
    );
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let refused =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(!refused.status.success(), "{refused:?}");
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("preservation_failed"), "{refused:?}");
}

#[test]
fn residual_packet_does_not_invent_review_before_initial_scope() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "initial lead resume",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    assert_eq!(begin["next"]["action"], "lead_decision");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], begin["work"]);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert!(resumed["next"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "scoped"));

    assert!(!resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "current_review" && item["status"] == "approved"));
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));
    assert!(!resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "lead_acceptance"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "scope"));
}

#[test]
fn residual_packet_keeps_stale_subject_evidence_out_of_reuse() {
    let fixture = Fixture::new();
    let check = "test -f marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "subject invalidation",
            "--check-command",
            check,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker_a = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_a["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker A"),
    );
    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    fixture.value(&["work", "check", &work], None);
    let worker_b = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_b["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker B"),
    );

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "check");
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "check"));
}

#[test]
fn preservation_checks_rerun_after_rework_changes_subject() {
    let fixture = Fixture::new_single();
    let functional = fixture.counting_command("functional.count");
    let preservation = fixture.counting_command("preservation.count");
    let (functional, preservation) = (functional.as_str(), preservation.as_str());
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preservation recheck after rework",
            "--check-command",
            functional,
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first implementation"),
    );
    fixture.value(&["work", "check", &work], None);
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"needs another implementation"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"second implementation"),
    );

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "check");
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check" || item == "preservation:precedence"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "check"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "preservation" && item["requirementId"] == "precedence"));

    fixture.value(&["work", "check", &work], None);
    fixture.value(&["work", "check", &work], None);
    assert_eq!(fixture.count("functional.count"), "2\n");
    assert_eq!(fixture.count("preservation.count"), "2\n");
}

#[test]
fn failed_check_surfaces_rework_and_requires_fresh_acceptance_path() {
    let fixture = Fixture::new_single();
    let check = "test -f pass-marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "failed check rework",
            "--check-command",
            check,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first attempt"),
    );
    assert_progress(&scoped["next"]);
    assert_eq!(completed["next"]["action"], "check");

    let failed = fixture.recorded_check_failure(&work);
    assert_eq!(failed["next"]["action"], "lead_decision");
    assert_eq!(failed["next"]["role"], "lead");
    let failed_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        failed["next"]["resolvedActor"],
        failed_next["residual"]["humanHelp"]["nextAction"]["actor"]
    );
    assert_eq!(
        failed_next["next"]["packet"]["context"]["next"]["owner"],
        failed["next"]["resolvedActor"]
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][0]["status"],
        "failed"
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let failed_ledger = fs::read(fixture.root.join(&ledger)).unwrap();

    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            failed["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"repair requested"),
    );
    assert_eq!(reworked["next"]["action"], "spawn");
    assert_eq!(reworked["next"]["role"], "worker");
    let reworked_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(reworked_next["next"]["packet"]["attempt"], 2);
    let after_rework = fs::read(fixture.root.join(&ledger)).unwrap();
    assert!(after_rework.starts_with(&failed_ledger));

    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let fresh_completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            reworked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh worker result"),
    );
    assert_eq!(fresh_completed["next"]["action"], "check");
    let fresh_check = fixture.value(&["work", "check", &work], None);
    assert_eq!(fresh_check["next"]["action"], "spawn");
    assert_eq!(fresh_check["next"]["role"], "reviewer");
    let fresh_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        fresh_next["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );

    let approved = fixture.value(
        &[
            "work",
            "return",
            &work,
            fresh_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"fresh review"),
    );
    assert_eq!(approved["next"]["action"], "lead_decision");
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            approved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"fresh acceptance"),
    );
    assert_eq!(done["next"]["action"], "done");
    let final_state = fixture.value(&["work", "next", &work], None);
    assert_eq!(final_state["next"]["progress"]["percent"], 100);
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

#[test]
fn evidence_after_failed_check_rework_carries_the_current_identity() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "evidence after failed check rework",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let mut next = fixture.value(&["work", "next", &work], None);
    let first = fixture.value(
        &[
            "work",
            "return",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first attempt"),
    );
    assert_eq!(first["next"]["action"], "check");
    let failed = fixture.recorded_check_failure(&work);
    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            failed["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"repair requested"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    next = reworked;

    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let second = fixture.value(
        &[
            "work",
            "return",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"second attempt"),
    );
    assert_eq!(second["next"]["action"], "check", "{second}");
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    next = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"continue repair"),
    );
    assert_eq!(next["next"]["role"], "worker");
    next = fixture.value(
        &[
            "work",
            "replan",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--hypothesis",
            "repair the next worker attempt",
        ],
        None,
    );
    assert_eq!(next["next"]["role"], "worker");
    let third = fixture.value(
        &[
            "work",
            "return",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"third attempt"),
    );
    assert_eq!(third["next"]["action"], "check", "{third}");
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    next = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"continue repair"),
    );
    assert_eq!(next["next"]["role"], "worker");
    fs::write(fixture.root.join("pass-marker"), b"updated after check").unwrap();

    let ledger = fixture.ledger(&work);

    let evidence = fixture.value(
        &[
            "work",
            "evidence",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--artifact",
            "pass-marker",
        ],
        None,
    );
    assert_eq!(
        evidence["event"]["governorEvent"]["identityTransition"],
        "carried_mutation_v1"
    );
    assert_eq!(evidence["next"]["action"], "spawn");

    let mut events: Vec<Value> = fs::read_to_string(fixture.root.join(ledger.clone()))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let last = events.last_mut().unwrap();
    let forged_subject = "f".repeat(64);
    last["subjectSha256"] = serde_json::json!(forged_subject);
    let governor = last["governorEvent"].as_object_mut().unwrap();
    governor["subjectSha256"] = serde_json::json!(forged_subject);
    governor["evidence"]["subjectSha256"] = serde_json::json!(forged_subject);
    let governor = Value::Object(governor.clone());
    last["governorEvent"] = rehash(governor);
    let forged = rehash(last.clone());
    *events.last_mut().unwrap() = forged;
    let forged_ledger = ".exitbind/runs/forged-evidence-identity.jsonl";
    fs::write(
        fixture.root.join(forged_ledger),
        events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let rejected = fixture.call(&["run", "inspect", forged_ledger, "--json"], None);
    assert!(
        !rejected.status.success(),
        "forged evidence subject was accepted"
    );
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&rejected.stdout),
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(
        diagnostic.contains("stale run identity")
            || diagnostic.contains("stale or mismatched for subjectSha256"),
        "{diagnostic}"
    );

    let mut input_events: Vec<Value> = fs::read_to_string(fixture.root.join(&ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let input_last = input_events.last_mut().unwrap();
    input_last["governorEvent"]["evidence"]["inputSha256"] = serde_json::json!("e".repeat(64));
    let governor = Value::Object(input_last["governorEvent"].as_object().unwrap().clone());
    input_last["governorEvent"] = rehash(governor);
    *input_events.last_mut().unwrap() = rehash(input_last.clone());
    let forged_input_ledger = ".exitbind/runs/forged-evidence-input.jsonl";
    fs::write(
        fixture.root.join(forged_input_ledger),
        input_events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let rejected_input = fixture.call(&["run", "inspect", forged_input_ledger, "--json"], None);
    assert!(
        !rejected_input.status.success(),
        "forged evidence input was accepted"
    );
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&map[key])
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => serde_json::to_string(other).unwrap(),
    }
}

fn rehash(mut event: Value) -> Value {
    event.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(canonical(&event).as_bytes());
    event["eventSha256"] = Value::String(format!("{digest:x}"));
    event
}

#[test]
fn preservation_without_functional_policy_is_refused_at_creation_and_replay() {
    let fixture = Fixture::new_single();
    let refused = fixture.call(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "preserve without a functional check",
            "--ledger",
            ".exitbind/runs/unchecked.jsonl",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "true",
        ],
        None,
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("checked run"),
        "{refused:?}"
    );
    assert!(!fixture.root.join(".exitbind/runs/unchecked.jsonl").exists());

    // Replay: a start event that claims preservation but no checkPolicy is
    // rejected even when its hash chain is internally valid.
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "true",
        ],
        None,
    );
    let ledger = fixture.ledger(begin["work"].as_str().unwrap());
    let text = fs::read_to_string(fixture.root.join(ledger)).unwrap();
    let mut start: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(start["version"], 8);
    start.as_object_mut().unwrap().remove("checkPolicy");
    let forged = ".exitbind/runs/forged.jsonl";
    fs::write(
        fixture.root.join(forged),
        format!("{}\n", serde_json::to_string(&rehash(start)).unwrap()),
    )
    .unwrap();
    let replay = fixture.call(&["run", "inspect", forged, "--json"], None);
    assert!(!replay.status.success(), "{replay:?}");
    assert!(
        String::from_utf8_lossy(&replay.stdout).contains("preservation requires checkPolicy"),
        "{replay:?}"
    );
}

/// Build governed work up to the final lead decision with a passing functional
/// check, a passing preservation check (whose checker lives in the product
/// root), and a current review.
fn prepared_for_acceptance(fixture: &Fixture) -> String {
    fs::create_dir_all(fixture.root.join("checks")).unwrap();
    fs::create_dir_all(fixture.root.join("src")).unwrap();
    fs::write(
        fixture.root.join("checks/preserve.sh"),
        b"grep -q env-first src/config.txt\n",
    )
    .unwrap();
    fs::write(fixture.root.join("src/config.txt"), b"env-first\n").unwrap();
    let functional = fixture.counting_command("functional.count");
    let preservation = format!(
        "sh checks/preserve.sh && {}",
        fixture.counting_command("preservation.count")
    );
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "simplify configuration",
            "--check-command",
            &functional,
            "--preserve-requirement",
            "precedence:environment wins over file",
            "--preservation-check-command",
            &preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    drive_to_lead_decision(fixture, &work);
    work
}

/// Follow the façade until a lead decision is pending: return scripted
/// worker/reviewer results and run pending checks. Scripted review requests are
/// counted separately; they are not model reviews.
fn drive_to_lead_decision(fixture: &Fixture, work: &str) -> usize {
    let mut review_requests = 0;
    loop {
        let response = fixture.value(&["work", "next", work], None);
        assert_one_state_basis(fixture, work, &response);
        let next = response["next"].clone();
        match next["action"].as_str().unwrap() {
            "check" => {
                fixture.value(&["work", "check", work], None);
            }
            "spawn" => {
                let role = next["role"].as_str().unwrap();
                let outcome = if role == "reviewer" {
                    "approved"
                } else {
                    "completed"
                };
                if role == "reviewer" {
                    review_requests += 1;
                }
                fixture.value(
                    &[
                        "work",
                        "return",
                        work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        outcome,
                    ],
                    Some(b"scripted result"),
                );
            }
            "lead_decision" if next["outcomes"][0] == "scoped" => {
                fixture.value(
                    &[
                        "work",
                        "return",
                        work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        "scoped",
                    ],
                    Some(b"scope"),
                );
            }
            "lead_decision" => return review_requests,
            other => panic!("unexpected action {other}"),
        }
    }
}

struct Consumed {
    result: String,
    reason: String,
    executed: Vec<Vec<String>>,
}

/// A tiny deterministic consumer: it reads the saved packet, asks the public
/// validator, and follows only the validator's bounded Exitbind action. It
/// knows nothing about the scenario being tested.
fn consume(fixture: &Fixture, work: &str, packet: &std::path::Path) -> Consumed {
    let verdict = fixture.value(
        &[
            "work",
            "validate",
            work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    let mut executed = Vec::new();
    let mut help = verdict["humanHelp"].clone();
    while help["nextAction"]["actor"] == "exitbind" {
        let args: Vec<String> = help["nextAction"]["command"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(help["nextAction"]["command"]["program"], "exitbind");
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        fixture.value(&borrowed, None);
        executed.push(args);
        let fresh = fixture.value(&["work", "next", work], None);
        help = fresh["residual"]["humanHelp"].clone();
    }
    Consumed {
        result: verdict["result"].as_str().unwrap().to_owned(),
        reason: verdict["reason"].as_str().unwrap().to_owned(),
        executed,
    }
}

fn save_packet(fixture: &Fixture, work: &str, name: &str) -> std::path::PathBuf {
    let packet = fixture.value(&["work", "next", work], None)["residual"].clone();
    let path = fixture.counter(name);
    fs::write(&path, serde_json::to_vec(&packet).unwrap()).unwrap();
    path
}

#[test]
fn validated_packet_reuses_unchanged_evidence_without_rerunning_checks() {
    let fixture = Fixture::new_single();
    let work = prepared_for_acceptance(&fixture);
    let packet = save_packet(&fixture, &work, "packet.json");
    let saved: Value = serde_json::from_slice(&fs::read(&packet).unwrap()).unwrap();
    assert_eq!(saved["version"], 2);
    assert!(saved["snapshot"]["inputsSha256"].is_string());
    assert_eq!(saved["humanHelp"]["nextAction"]["actor"], "lead");
    assert_eq!(saved["humanHelp"]["ownerDecision"], "not_required");

    let consumed = consume(&fixture, &work, &packet);
    assert_eq!(
        (consumed.result.as_str(), consumed.reason.as_str()),
        ("usable", "current")
    );
    assert!(consumed.executed.is_empty());
    assert_eq!(fixture.count("functional.count"), "1\n");
    assert_eq!(fixture.count("preservation.count"), "1\n");
    let usable = fixture.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        usable["packet"], saved,
        "usable still returns the canonical packet"
    );

    // Unsupported, contradictory, and oversized packets never permit skipping.
    let mut contradictory = saved.clone();
    contradictory["doNotRepeat"]
        .as_array_mut()
        .unwrap()
        .push(Value::from("implementation"));
    let mut unsupported = saved.clone();
    unsupported["version"] = Value::from(1);
    let mut goal = saved.clone();
    goal["goal"] = Value::from("drop the precedence requirement");
    let mut subject = saved.clone();
    subject["currentSubject"] = serde_json::json!({"sha256": "0".repeat(64)});
    let mut remaining = saved.clone();
    remaining["remaining"] = serde_json::json!([]);
    let mut next_action = saved.clone();
    next_action["next"] = Value::from("done");
    let mut missing = saved.clone();
    missing.as_object_mut().unwrap().remove("stillValid");
    for (mutated, expected) in [
        (contradictory, "claims_disagree_with_state"),
        (unsupported, "unsupported_packet_version"),
        (goal, "claims_disagree_with_state"),
        (subject, "claims_disagree_with_state"),
        (remaining, "claims_disagree_with_state"),
        (next_action, "claims_disagree_with_state"),
        (missing, "malformed_packet"),
    ] {
        let path = fixture.counter("mutated.json");
        fs::write(&path, serde_json::to_vec(&mutated).unwrap()).unwrap();
        let verdict = fixture.value(
            &[
                "work",
                "validate",
                &work,
                "--packet",
                path.to_str().unwrap(),
            ],
            None,
        );
        assert_ne!(verdict["result"], "usable");
        assert_eq!(verdict["reason"], expected);
        assert_eq!(verdict["reuse"], serde_json::json!([]));
        assert_eq!(verdict["packet"]["version"], 2);
    }
    let huge = fixture.counter("huge.json");
    fs::write(&huge, vec![b' '; 300 * 1024]).unwrap();
    let refused = fixture.call(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            huge.to_str().unwrap(),
        ],
        None,
    );
    assert!(!refused.status.success());

    let done = fixture.value(&["work", "next", &work], None);
    let accepted = fixture.value(
        &[
            "work",
            "return",
            &work,
            done["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert_eq!(fixture.count("functional.count"), "1\n");
}

#[test]
fn invalid_or_authority_conflicting_saved_packet_cannot_be_reused() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "reject invalid saved packets",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    let packet = fixture.counter("invalid-packet.json");
    fs::write(&packet, b"not json\n").unwrap();
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let invalid = fixture.call(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert!(invalid.status.success(), "{invalid:?}");
    let invalid: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(invalid["result"], "cannot_establish_applicability");
    assert_eq!(invalid["reason"], "packet_not_an_object");
    assert_eq!(invalid["reuse"], serde_json::json!([]));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);

    let current = fixture.value(&["work", "next", &work], None)["residual"].clone();
    let mut conflicting = current;
    conflicting["basis"] = serde_json::json!({"sha256": "f".repeat(64)});
    fs::write(&packet, serde_json::to_vec(&conflicting).unwrap()).unwrap();
    let verdict = fixture.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(verdict["result"], "refresh_required");
    assert_eq!(verdict["reason"], "claims_disagree_with_state");
    assert_eq!(verdict["reuse"], serde_json::json!([]));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);

    let current = fixture.value(&["work", "next", &work], None)["residual"].clone();
    let mut conflicting_policy = current;
    conflicting_policy["reviewPolicy"] = serde_json::json!({
        "version": 1,
        "decision": "required",
        "source": "forged",
        "reason": "conflicting policy",
        "previousSha256": null,
        "sha256": "f".repeat(64)
    });
    fs::write(&packet, serde_json::to_vec(&conflicting_policy).unwrap()).unwrap();
    let verdict = fixture.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(verdict["result"], "refresh_required");
    assert_eq!(verdict["reason"], "claims_disagree_with_state");
    assert_eq!(verdict["reuse"], serde_json::json!([]));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn source_or_checker_edit_without_new_submission_invalidates_reuse_and_acceptance() {
    for edited in ["src/config.txt", "checks/preserve.sh"] {
        let fixture = Fixture::new_single();
        let work = prepared_for_acceptance(&fixture);
        let packet = save_packet(&fixture, &work, "packet.json");
        let validated = consume(&fixture, &work, &packet);
        assert_eq!(validated.result, "usable", "{edited}");

        // Change after validation, before acceptance: the earlier validation
        // does not authorize acceptance.
        let mut bytes = fs::read(fixture.root.join(edited)).unwrap();
        bytes.extend_from_slice(b"# edited\n");
        fs::write(fixture.root.join(edited), &bytes).unwrap();
        fs::write(
            fixture.root.join(".exitbind/artifacts/lead.md"),
            b"accept\n",
        )
        .unwrap();
        let ledger = fixture.ledger(&work);
        let premature =
            fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
        assert!(!premature.status.success(), "{edited}: {premature:?}");
        let next = fixture.value(&["work", "next", &work], None);
        assert_eq!(next["next"]["action"], "check", "{edited}");
        assert!(next["residual"]["humanHelp"]["whatHappened"]
            .as_str()
            .unwrap()
            .contains("no longer applies"));

        // A fresh consumer with the old packet is told to refresh and obtains
        // real new executions before any acceptance.
        let consumed = consume(&fixture, &work, &packet);
        assert_eq!(
            (consumed.result.as_str(), consumed.reason.as_str()),
            ("refresh_required", "tested_inputs_changed"),
            "{edited}"
        );
        assert_eq!(consumed.executed.len(), 2, "{edited}");
        assert_eq!(fixture.count("functional.count"), "2\n", "{edited}");
        assert_eq!(fixture.count("preservation.count"), "2\n", "{edited}");

        // The review approved other tested files, so it is not reused.
        let stale_review = fixture.value(&["work", "next", &work], None);
        assert_eq!(stale_review["next"]["action"], "lead_decision");
        assert!(!stale_review["residual"]["doNotRepeat"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "review"));
        let refused =
            fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
        assert!(!refused.status.success(), "{edited}: stale review accepted");
        let old_packet = consume(&fixture, &work, &packet);
        assert_eq!(old_packet.result, "refresh_required");
        assert!(old_packet.executed.is_empty());

        fixture.value(
            &[
                "work",
                "return",
                &work,
                stale_review["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "rework",
            ],
            Some(b"review is stale"),
        );
        let review_requests = drive_to_lead_decision(&fixture, &work);
        assert_eq!(review_requests, 1, "{edited}");
        let decision = fixture.value(&["work", "next", &work], None);
        let accepted = fixture.value(
            &[
                "work",
                "return",
                &work,
                decision["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "accepted",
            ],
            Some(b"accept"),
        );
        assert_eq!(accepted["next"]["progress"]["state"], "READY", "{edited}");
    }
}

#[test]
fn changed_preservation_policy_cannot_inherit_predecessor_evidence() {
    let fixture = Fixture::new_single();
    let work = prepared_for_acceptance(&fixture);
    let ledger = fixture.ledger(&work);
    let changed = fixture.call(
        &[
            "run",
            "supersede",
            &ledger,
            "--workflow",
            "change",
            "--goal",
            "simplify configuration",
            "--ledger",
            ".exitbind/runs/successor.jsonl",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:file wins over environment",
            "--preservation-check-command",
            "true",
            "--json",
        ],
        None,
    );
    assert!(!changed.status.success(), "{changed:?}");
    assert!(
        String::from_utf8_lossy(&changed.stdout).contains("differs"),
        "{changed:?}"
    );
}

/// Cross-version read compatibility against a pinned `v0.18.0` binary. The
/// binary is supplied by path so the user's installation is never touched; the
/// test is skipped (and says so) when it is not provided.
#[test]
fn pinned_v0_18_reader_refuses_v6_and_current_reader_keeps_v5_guarantees() {
    let Some(old) = std::env::var_os("EXITBIND_V018_BIN") else {
        eprintln!("skipped: set EXITBIND_V018_BIN to a v0.18.0 exitbind binary");
        return;
    };
    let old = PathBuf::from(old);
    let run_with =
        |binary: &std::path::Path, root: &std::path::Path, args: &[&str], input: Option<&[u8]>| {
            let mut command = Command::new(binary);
            command
                .current_dir(root)
                .args(args)
                .arg("--config")
                .arg(root.join("exitbind.json"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.unwrap_or_default())
                .unwrap();
            child.wait_with_output().unwrap()
        };
    let json = |output: Output| -> Value {
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    };

    // Old reader, new ledger: explicit refusal, never partial acceptance.
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "v6",
            "--check-command",
            "true",
        ],
        None,
    );
    let ledger = fixture.ledger(begin["work"].as_str().unwrap());
    let refused = run_with(
        &old,
        &fixture.root,
        &["run", "inspect", &ledger, "--json"],
        None,
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stdout).contains("invalid event header"),
        "{refused:?}"
    );

    // Old writer, current reader: a v5 run at its final lead decision stays
    // readable, projects no tested-input claim, and cannot be validated as
    // input-applicable.
    let legacy = Fixture::new_single();
    let work = json(run_with(
        &old,
        &legacy.root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "v5",
            "--check-command",
            "true",
        ],
        None,
    ))["work"]
        .as_str()
        .unwrap()
        .to_owned();
    loop {
        let next =
            json(run_with(&old, &legacy.root, &["work", "next", &work], None))["next"].clone();
        match next["action"].as_str().unwrap() {
            "check" => {
                json(run_with(
                    &old,
                    &legacy.root,
                    &["work", "check", &work],
                    None,
                ));
            }
            "spawn" | "lead_decision" if next["outcomes"][0] != "accepted" => {
                let outcome = match next["role"].as_str().unwrap() {
                    "lead" => "scoped",
                    "reviewer" => "approved",
                    _ => "completed",
                };
                json(run_with(
                    &old,
                    &legacy.root,
                    &[
                        "work",
                        "return",
                        &work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        outcome,
                    ],
                    Some(b"legacy"),
                ));
            }
            _ => break,
        }
    }
    let legacy_ledger = legacy.ledger(&work);
    let events = fs::read_to_string(legacy.root.join(&legacy_ledger)).unwrap();
    assert!(events
        .lines()
        .all(|line| serde_json::from_str::<Value>(line).unwrap()["version"] == 5));
    let current = legacy.value(&["work", "next", &work], None);
    assert_eq!(current["next"]["action"], "lead_decision");
    assert!(current["residual"]["snapshot"]["inputsSha256"].is_null());
    assert_eq!(
        current["residual"]["invalidation"]["testedInputChangeInvalidates"],
        false
    );
    let packet = legacy.counter("legacy-packet.json");
    fs::write(&packet, serde_json::to_vec(&current["residual"]).unwrap()).unwrap();
    let verdict = legacy.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(verdict["result"], "cannot_establish_applicability");
    assert_eq!(verdict["reason"], "tested_inputs_not_bound");
    let accepted = legacy.value(
        &[
            "work",
            "return",
            &work,
            current["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert!(fs::read_to_string(legacy.root.join(&legacy_ledger))
        .unwrap()
        .lines()
        .all(|line| serde_json::from_str::<Value>(line).unwrap()["version"] == 5));
}

/// Every `work next` response must describe one revision: its next action and
/// its residual packet agree, and the packet names the ledger head on disk.
fn assert_one_state_basis(fixture: &Fixture, work: &str, response: &Value) {
    assert_eq!(response["next"]["action"], response["residual"]["next"]);
    let ledger = fs::read_to_string(fixture.root.join(fixture.ledger(work))).unwrap();
    let head: Value = serde_json::from_str(ledger.lines().last().unwrap()).unwrap();
    assert_eq!(
        response["residual"]["snapshot"]["headEventSha256"],
        head["eventSha256"]
    );
    assert_eq!(
        response["residual"]["snapshot"]["eventCount"],
        ledger.lines().count()
    );
}

#[test]
fn terminal_history_never_authorizes_current_continuation() {
    let fixture = Fixture::new_single();
    let work = prepared_for_acceptance(&fixture);
    let running_packet = save_packet(&fixture, &work, "running.json");
    let decision = fixture.value(&["work", "next", &work], None);
    let accepted = fixture.value(
        &[
            "work",
            "return",
            &work,
            decision["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    let accepted_packet = save_packet(&fixture, &work, "accepted.json");

    // Covered source edit after acceptance: ledger and delivery are unchanged.
    fs::write(fixture.root.join("src/config.txt"), b"file-first\n").unwrap();
    let history = fixture.call(&["run", "inspect", &fixture.ledger(&work), "--json"], None);
    assert!(history.status.success(), "{history:?}");
    let now = fixture.value(&["work", "next", &work], None);
    let residual = &now["residual"];
    assert_eq!(residual["historical"], true);
    assert!(residual["snapshot"]["inputsSha256"].is_null());
    assert_eq!(residual["doNotRepeat"], serde_json::json!([]));
    assert_eq!(residual["stillValid"], serde_json::json!([]));
    assert!(residual["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap()
        .contains("history"));
    for packet in [&running_packet, &accepted_packet] {
        let verdict = fixture.value(
            &[
                "work",
                "validate",
                &work,
                "--packet",
                packet.to_str().unwrap(),
            ],
            None,
        );
        assert_eq!(verdict["result"], "not_continuable");
        assert_eq!(verdict["reuse"], serde_json::json!([]));
        assert_eq!(verdict["packet"]["historical"], true);
    }
    assert_eq!(fixture.count("functional.count"), "1\n");

    // A blocked terminal run is not presented as a completed outcome.
    let blocked = Fixture::new_single();
    let begin = blocked.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "g",
            "--check-command",
            "true",
        ],
        None,
    );
    let blocked_work = begin["work"].as_str().unwrap().to_owned();
    blocked.value(
        &[
            "work",
            "return",
            &blocked_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"cannot proceed"),
    );
    let ended = blocked.value(&["work", "next", &blocked_work], None);
    let help = &ended["residual"]["humanHelp"];
    assert_eq!(ended["residual"]["historical"], true);
    assert_eq!(help["ownerDecision"], "required");
    assert!(help["whatHappened"]
        .as_str()
        .unwrap()
        .contains("not accepted"));
}

/// Both installed guidance copies must carry the same terminal rule. The
/// bootstrap is what a fresh host reads first, so a looser instruction there
/// is what a lead actually follows.
#[test]
fn every_distributed_guidance_copy_states_the_terminal_block_rule() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "skills/exitbind/SKILL.md",
        "plugins/exitbind/skills/exitbind/SKILL.md",
        "skills/exitbind-bootstrap/SKILL.md",
    ] {
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        assert!(
            text.contains("print it on a line of its own, exactly as given"),
            "{relative} does not state the terminal block rule"
        );
        assert!(
            text.contains("not inside a sentence, not wrapped in emphasis"),
            "{relative} does not forbid decorating the block"
        );
        // Nothing may invite the host to assemble the line itself.
        assert!(
            !text.contains("Report the exit state the CLI gives you (`EXIT READY`"),
            "{relative} still invites paraphrasing the exit state"
        );
    }
}

#[test]
fn distributed_guidance_limits_unchanged_status_polling() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "skills/exitbind/SKILL.md",
        "plugins/exitbind/skills/exitbind/SKILL.md",
        "skills/exitbind-bootstrap/SKILL.md",
    ] {
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        let lower = text.to_lowercase();
        assert!(
            lower.contains("do not start a fresh watch process"),
            "{relative}"
        );
        assert!(
            lower.contains("poll status at short intervals")
                || (lower.contains("poll unchanged status") && lower.contains("short intervals")),
            "{relative}"
        );
        assert!(lower.contains("a timeout is not a"), "{relative}");
        assert!(
            lower.contains("explicit human status request"),
            "{relative}"
        );
    }
}

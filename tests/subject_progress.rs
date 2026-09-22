#![cfg(unix)]

mod support;

use serde_json::Value;
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

fn assert_progress(action: &Value) {
    assert_eq!(action["progress"]["applicable"], true);
    assert!(
        action["progress"]["percent"].is_number(),
        "missing progress: {action}"
    );
    assert!(action["progress"]["weights"]["worker"].is_number());
}

fn assert_opaque_reference(reference: &Value, expected: Value) {
    assert_eq!(reference, &expected);
    let serialized = reference.to_string();
    assert!(!serialized.contains("ledger"));
    assert!(!serialized.contains(".exitbind"));
    assert!(!serialized.contains("runs/"));
}

fn assert_opaque_envelope(value: &Value) {
    let serialized = value.to_string();
    for marker in [".exitbind", "runs/", "artifacts/", "state/"] {
        assert!(
            !serialized.contains(marker),
            "opaque envelope leaked {marker}: {value}"
        );
    }
    fn visit(value: &Value) {
        match value {
            Value::Array(items) => items.iter().for_each(visit),
            Value::Object(object) => {
                for key in [
                    "path",
                    "root",
                    "ledgerPath",
                    "profilePath",
                    "artifactPathHint",
                    "sourcePath",
                ] {
                    assert!(
                        !object.contains_key(key),
                        "opaque envelope leaked {key}: {value}"
                    );
                }
                if object.get("exact") == Some(&serde_json::json!(true)) {
                    assert!(matches!(
                        object.get("kind").and_then(Value::as_str),
                        Some(
                            "ledger_history"
                                | "ledger_event"
                                | "check_log"
                                | "evidence"
                                | "stdout"
                                | "stderr",
                        )
                    ));
                }
                object.values().for_each(visit);
            }
            _ => {}
        }
    }
    visit(value);
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
    let log_id = checked["next"]["packet"]["context"]["evidence"]
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
                assert_eq!(
                    output.status.code(),
                    Some(1),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "config_drift");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "supersede");
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
                assert_eq!(output.status.code(), Some(1));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "artifact_drift");
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "inspect");
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
                assert_eq!(
                    output.status.code(),
                    Some(1),
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "memory_drift");
                assert_eq!(value["effect"], "no-change");
                assert_eq!(value["nextAction"]["type"], "supersede");
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
                assert_eq!(output.status.code(), Some(1));
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["reason"]["code"], "profile_drift");
                assert_eq!(value["effect"], "no-change");
                assert_opaque_reference(&value["reference"], serde_json::json!({"work": work}));
                assert_eq!(value["nextAction"]["type"], "supersede");
                assert_eq!(value["nextAction"]["safe"], true);
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
fn skill_presentation_is_exact_and_packaged_copy_matches() {
    const ROUTINE: &str = "[Neuro] Exitbind progress: N%.";
    let canonical = include_bytes!("../skills/exitbind/SKILL.md");
    let packaged = include_bytes!("../plugins/exitbind/skills/exitbind/SKILL.md");
    assert_eq!(canonical, packaged);
    let text = std::str::from_utf8(canonical).unwrap();
    assert!(text.contains(ROUTINE));
    // One line, never a two-line header, and never an invented percentage.
    assert!(!text.contains("Neuro\nExitbind progress"));
    assert!(
        text.contains("never estimate the\nnumber and never show one when no governed run applies")
    );
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
fn work_facade_surfaces_stale_worker_and_recovers_to_ready() {
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

    let scope = fixture.value(
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
    assert_progress(&scope["next"]);
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
    assert_progress(&worker_a_return["next"]);
    assert_eq!(worker_a_return["next"]["action"], "check");

    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    let checked_a = fixture.value(&["work", "check", &work], None);
    assert_progress(&checked_a["next"]);
    assert_eq!(checked_a["next"]["action"], "spawn");
    let worker_b = fixture.value(&["work", "next", &work], None);
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
    assert_progress(&worker_b_return["next"]);
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
    assert_progress(&recovered_check["next"]);
    assert_eq!(recovered_check["next"]["action"], "lead_decision");
    let done = fixture.value(
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
    assert_eq!(done["next"]["action"], "done");
    assert_eq!(done["next"]["progress"]["percent"], 100);
    assert_eq!(done["next"]["progress"]["state"], "READY");
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
    let done = fixture.value(
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
    let failed = fixture.value(&["work", "check", &work], None);
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][1]["status"],
        "failed"
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][1]["requirementId"],
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

    let failed = fixture.value(&["work", "check", &work], None);
    assert_eq!(failed["next"]["action"], "spawn");
    assert_eq!(failed["next"]["role"], "reviewer");
    let failed_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        failed["next"]["resolvedActor"],
        failed_next["residual"]["humanHelp"]["nextAction"]["actor"]
    );
    assert_eq!(
        failed["next"]["packet"]["context"]["next"]["owner"],
        failed["next"]["resolvedActor"]
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][0]["status"],
        "failed"
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"]
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
    assert_eq!(reworked["next"]["packet"]["attempt"], 2);
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
    assert_eq!(
        fresh_check["next"]["packet"]["checkEvidence"][0]["status"],
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
    assert_eq!(done["next"]["progress"]["percent"], 100);
    assert_eq!(done["next"]["progress"]["state"], "READY");
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
    use sha2::{Digest, Sha256};
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

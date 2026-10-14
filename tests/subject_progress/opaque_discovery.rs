use super::*;

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

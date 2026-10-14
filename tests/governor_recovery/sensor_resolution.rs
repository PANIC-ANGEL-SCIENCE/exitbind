use super::*;

#[test]
fn sensor_request_and_inert_result_bind_without_spending_or_replacing_mutation() {
    let fixture = Fixture::new();
    fixture.prepare_replanned_mutation(Some(true));
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "evidence_required");
    assert_eq!(inspected["governor"]["spent"], 3);
    let actions = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["governorEvent"]["action"].as_str())
        .collect::<Vec<_>>();
    assert!(actions.iter().any(|action| *action == "sensor_request"));
    assert!(actions.iter().any(|action| *action == "sensor"));
    let sensor = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["governorEvent"]["action"] == "sensor")
        .unwrap();
    assert_eq!(sensor["governorEvent"]["assessment"], "unavailable");
}

#[test]
fn unresolved_sensor_blocks_direct_owner_recovery_before_claim_or_start() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit_with_sensor(Some(false));
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert!(inspected["governor"]["currentSensorRequest"].is_object());
    assert!(inspected["governor"]["seenSensors"]
        .as_array()
        .unwrap()
        .is_empty());
    let recovery_path = fixture.write_recovery_artifacts_from_inspect(&inspected, &detail);
    let successor = ".exitbind/runs/work-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
            "--owner-recovery",
            &recovery_path,
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "unresolved sensor recovery succeeded"
    );
    assert!(
        display(&refused).contains("unresolved governor sensor request"),
        "wrong refusal boundary: {}",
        display(&refused)
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    assert!(!fixture.root.join(successor).exists());
    assert!(!fixture.root.join(claim).exists());
    fixture.assert_prior_ledgers_unchanged();
}

#[test]
fn newer_unanswered_sensor_is_not_resolved_by_earlier_inert_result() {
    let fixture = Fixture::new();
    let (old_bytes, detail, newer_digest) = fixture.block_after_newer_unanswered_sensor();
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert_eq!(
        inspected["governor"]["seenSensors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        inspected["governor"]["currentSensorRequest"]["requestDigest"],
        newer_digest
    );
    let latest_request = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|event| event["governorEvent"]["action"] == "sensor_request")
        .unwrap();
    assert_eq!(
        latest_request["governorEvent"]["requestDigest"],
        newer_digest
    );
    let recovery_path = fixture.write_recovery_artifacts_from_inspect(&inspected, &detail);
    let successor = ".exitbind/runs/work-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
            "--owner-recovery",
            &recovery_path,
        ],
        b"",
    );
    assert!(!refused.status.success());
    assert!(
        display(&refused).contains("supersession refused unresolved governor sensor request"),
        "wrong refusal boundary: {}",
        display(&refused)
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    assert!(!fixture.root.join(successor).exists());
    assert!(!fixture.root.join(claim).exists());
}

#[test]
fn resolved_sensor_allows_owner_recovery_to_start_grantless() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit_with_sensor(Some(true));
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert!(inspected["governor"]["currentSensorRequest"].is_object());
    assert_eq!(
        inspected["governor"]["seenSensors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let recovery_draft = detail["actionForms"]["recovery"].get("ownerRecoveryDraft");
    let recovery_path = fixture.write_recovery_artifacts_from_inspect(&inspected, &detail);
    let successor = ".exitbind/runs/work-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    let recovered = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
            "--owner-recovery",
            &recovery_path,
        ],
        b"",
    );
    assert!(
        recovered.status.success(),
        "resolved sensor should enable the valid owner recovery; draft={recovery_draft:?}; {}",
        display(&recovered)
    );
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["work"], format!("smw_{}", "e".repeat(64)));
    assert_eq!(recovery_draft.unwrap()["approved"], false);
    let successor_view = fixture.json(&["run", "inspect", successor], b"");
    assert_eq!(successor_view["governor"]["state"], "evidence_required");
    assert_eq!(successor_view["governor"]["spent"], 3);
    assert!(successor_view["governor"]["currentMutation"].is_null());
    assert!(successor_view["governor"]["consumedGrants"]
        .as_array()
        .unwrap()
        .is_empty());
    let start: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(start["governorCarry"]["accounting"]["state"], "blocked");
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    assert!(fixture.root.join(claim).exists());
    fixture.assert_prior_ledgers_unchanged();
}

#[test]
fn resolved_sensor_allows_ordinary_same_goal_carry() {
    let fixture = Fixture::new();
    let worker = fixture.json(&["work", "next", &fixture.work, "--full"], b"");
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    let grant = fixture.permit_edit(assignment);
    assert!(grant.status.success(), "{}", display(&grant));
    let request = fixture.sensor_request(assignment);
    assert_eq!(
        request["event"]["governorEvent"]["action"],
        "sensor_request"
    );
    let result = fixture.inert_sensor_result(assignment);
    assert_eq!(
        result["event"]["governorEvent"]["assessment"],
        "unavailable"
    );
    let old_bytes = fs::read(fixture.root.join(&fixture.old_ledger)).unwrap();
    let successor = ".exitbind/runs/work-9999999999999999999999999999999999999999999999999999999999999999.jsonl";
    let carried = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            successor,
        ],
        b"",
    );
    assert!(carried.status.success(), "{}", display(&carried));
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    let successor_view = fixture.json(&["run", "inspect", successor], b"");
    assert_eq!(successor_view["governor"]["state"], "ready");
    assert_eq!(successor_view["governor"]["spent"], 1);
    assert!(successor_view["governor"]["currentMutation"].is_null());
    assert!(successor_view["governor"]["currentGrantEventSha256"].is_null());
}

#[test]
fn sensor_event_rejects_mutation_retry_identity_on_read() {
    let fixture = Fixture::new();
    let worker = fixture.json(&["work", "next", &fixture.work, "--full"], b"");
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    let grant = fixture.permit_edit(assignment);
    assert!(grant.status.success(), "{}", display(&grant));
    let request = fixture.sensor_request(assignment);
    assert_eq!(
        request["event"]["governorEvent"]["action"],
        "sensor_request"
    );

    let path = fixture.root.join(&fixture.old_ledger);
    let source = fs::read_to_string(&path).unwrap();
    let mut lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut event: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    event["governorEvent"]["requestId"] = json!("forbidden-sensor-retry-id");
    rehash_event(&mut event["governorEvent"]);
    rehash_event(&mut event);
    *lines.last_mut().unwrap() = serde_json::to_string(&event).unwrap();
    fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

    let refused = fixture.call(&["run", "inspect", &fixture.old_ledger], b"");
    assert!(
        !refused.status.success(),
        "sensor retry identity was accepted"
    );
    assert!(
        display(&refused).contains("sensor event cannot use mutation retry identity"),
        "wrong refusal boundary: {}",
        display(&refused)
    );
}

#[test]
#[cfg(any())]
fn historical_v1_to_v4_and_unchecked_v5_remain_non_applicable_with_checked_v5_control() {
    let legacy = |label: &str| {
        let root = support::temp(label);
        let output = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        root
    };

    let v1_root = legacy("transition-history-v1");
    let v1 = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v1",
            "--ledger",
            ".soulmate/runs/v1.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v1.status.success(), "{}", text(&v1));
    let v1_status = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v1.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v1_status.status.success(), "{}", text(&v1_status));
    let v1_next = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v1.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v1_next.status.success(), "{}", text(&v1_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v1_next.stdout).unwrap(),
        "historical_run",
    );
    let old_receipt = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v1_root)
        .args([
            "receipt",
            ".soulmate/runs/v1.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_receipt_matches_progress(
        &old_receipt,
        &serde_json::from_slice::<Value>(&v1_next.stdout).unwrap()["progress"],
    );

    let v2_root = legacy("transition-history-v2");
    let manifest = json!({
        "$schema": "https://raw.githubusercontent.com/envy404/soulmate/v0.4.0/schema/harness-manifest.schema.json",
        "version": 1,
        "project": {"id": "history", "session": "history"},
        "harness": {"name": "history", "version": "2026.09.13"},
        "activations": [{"kind": "skill", "name": "soulmate", "evidence": "configured"}]
    });
    write_json(&v2_root.join("harness-manifest.json"), &manifest);
    let planned = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "plan",
            "change",
            "--goal",
            "v2",
            "--receipt",
            ".soulmate/harness-receipt.json",
            "--harness-manifest",
            "harness-manifest.json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(planned.status.success(), "{}", text(&planned));
    let v2 = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v2",
            "--ledger",
            ".soulmate/runs/v2.jsonl",
            "--harness-receipt",
            ".soulmate/harness-receipt.json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v2.status.success(), "{}", text(&v2));
    let v2_status = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v2.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v2_status.status.success(), "{}", text(&v2_status));
    let v2_next = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v2.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v2_next.status.success(), "{}", text(&v2_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v2_next.stdout).unwrap(),
        "historical_run",
    );
    let v2_receipt = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v2_root)
        .args([
            "receipt",
            ".soulmate/runs/v2.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_exit_1(&v2_receipt);

    let v3_root = legacy("transition-history-v3");
    let v3_start = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v3",
            "--ledger",
            ".soulmate/runs/v3.jsonl",
            "--check-command",
            CHECK,
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v3_start.status.success(), "{}", text(&v3_start));
    let v3_path = v3_root.join(".soulmate/runs/v3.jsonl");
    let mut v3_event: Value = serde_json::from_str(&fs::read_to_string(&v3_path).unwrap()).unwrap();
    v3_event["version"] = json!(3);
    v3_event["eventSha256"] = json!(value_hash(&without(&v3_event, "eventSha256")));
    fs::write(
        &v3_path,
        format!("{}\n", serde_json::to_string(&v3_event).unwrap()),
    )
    .unwrap();
    let v3_status = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v3.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v3_status.status.success(), "{}", text(&v3_status));
    let v3_next = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v3.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v3_next.status.success(), "{}", text(&v3_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v3_next.stdout).unwrap(),
        "historical_run",
    );
    let v3_receipt = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v3_root)
        .args([
            "receipt",
            ".soulmate/runs/v3.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_exit_1(&v3_receipt);

    let v4_root = legacy("transition-history-v4");
    let v4 = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "v4",
            "--ledger",
            ".soulmate/runs/v4.jsonl",
            "--check-command",
            CHECK,
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v4.status.success(), "{}", text(&v4));
    let v4_status = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "run",
            "status",
            ".soulmate/runs/v4.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v4_status.status.success(), "{}", text(&v4_status));
    let v4_next = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "run",
            "next",
            ".soulmate/runs/v4.jsonl",
            "--json",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(v4_next.status.success(), "{}", text(&v4_next));
    assert_not_ready_progress(
        &serde_json::from_slice(&v4_next.stdout).unwrap(),
        "historical_run",
    );
    let v4_receipt = super::support::git_topology::command(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&v4_root)
        .args([
            "receipt",
            ".soulmate/runs/v4.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert_exit_1(&v4_receipt);

    let unchecked = Fixture::new("transition-unchecked-v5");
    let unchecked_start = unchecked.call(&[
        "run",
        "start",
        "change",
        "--goal",
        "unchecked",
        "--ledger",
        ".exitbind/runs/unchecked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert!(
        unchecked_start.status.success(),
        "{}",
        text(&unchecked_start)
    );
    let _unchecked_status = unchecked.status(".exitbind/runs/unchecked.jsonl");
    let unchecked_next = unchecked.json(&[
        "run",
        "next",
        ".exitbind/runs/unchecked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_not_ready_progress(&unchecked_next, "unchecked_run");
    let unchecked_receipt = unchecked.call(&[
        "receipt",
        ".exitbind/runs/unchecked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_receipt_matches_progress(&unchecked_receipt, &unchecked_next["progress"]);

    let checked = Fixture::new("transition-checked-v5-control");
    let receipt = checked.accepted_single(".exitbind/runs/checked.jsonl");
    let checked_status = checked.json(&[
        "run",
        "next",
        ".exitbind/runs/checked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(checked_status["progress"]["applicable"], true);
    assert_eq!(checked_status["progress"]["state"], "READY");
    let verified = checked.call(&["verify", &receipt, "--config", "exitbind.json"]);
    assert!(verified.status.success(), "{}", text(&verified));
    assert_eq!(
        serde_json::from_slice::<Value>(&verified.stdout).unwrap()["outcome"],
        "READY"
    );

    let _ = fs::remove_dir_all(v1_root);
    let _ = fs::remove_dir_all(v2_root);
    let _ = fs::remove_dir_all(v3_root);
    let _ = fs::remove_dir_all(v4_root);
}

#[test]
#[cfg(any())]
fn accepted_historical_and_unchecked_states_project_through_work_next_without_ready_upgrade() {
    let token_v1 = "1".repeat(64);
    let v1 = LegacyFixture::new("transition-accepted-v1");
    let ledger_v1 = format!(".soulmate/runs/work-{token_v1}.jsonl");
    v1.start(&ledger_v1, false, None);
    v1.submit("lead", &ledger_v1, "scoped", "v1-scope");
    v1.submit("worker", &ledger_v1, "completed", "v1-worker");
    v1.submit("reviewer", &ledger_v1, "approved", "v1-review");
    v1.submit("lead", &ledger_v1, "accepted", "v1-accept");
    assert_accepted_historical(&v1, &token_v1, &ledger_v1);

    let token_v2 = "2".repeat(64);
    let v2 = LegacyFixture::new("transition-accepted-v2");
    let manifest = json!({
        "$schema": "https://raw.githubusercontent.com/envy404/soulmate/v0.4.0/schema/harness-manifest.schema.json",
        "version": 1,
        "project": {"id": "accepted-v2", "session": "accepted-v2"},
        "harness": {"name": "history", "version": "2026.09.13"},
        "activations": [{"kind": "skill", "name": "soulmate", "evidence": "configured"}]
    });
    write_json(&v2.root.join("harness-manifest.json"), &manifest);
    let planned = v2.call(&[
        "plan",
        "change",
        "--goal",
        "v2",
        "--receipt",
        ".soulmate/harness-receipt.json",
        "--harness-manifest",
        "harness-manifest.json",
        "--config",
        "soulmate.json",
    ]);
    assert!(planned.status.success(), "{}", text(&planned));
    let ledger_v2 = format!(".soulmate/runs/work-{token_v2}.jsonl");
    v2.start(&ledger_v2, false, Some(".soulmate/harness-receipt.json"));
    v2.submit("lead", &ledger_v2, "scoped", "v2-scope");
    v2.submit("worker", &ledger_v2, "completed", "v2-worker");
    v2.submit("reviewer", &ledger_v2, "approved", "v2-review");
    v2.submit("lead", &ledger_v2, "accepted", "v2-accept");
    assert_accepted_historical(&v2, &token_v2, &ledger_v2);

    let token_v3 = "3".repeat(64);
    let v3 = LegacyFixture::new("transition-accepted-v3");
    let ledger_v3 = format!(".soulmate/runs/work-{token_v3}.jsonl");
    v3.start(&ledger_v3, true, None);
    downgrade_start_to_v3(&v3.root.join(&ledger_v3));
    v3.submit("lead", &ledger_v3, "scoped", "v3-scope");
    let worker_v3 = v3.submit("worker", &ledger_v3, "completed", "v3-worker");
    let events_v3: Vec<Value> = fs::read_to_string(v3.root.join(&ledger_v3))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let target_v3 = events_v3
        .iter()
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(worker_v3["event"]["version"], 3);
    v3.check(&ledger_v3, &target_v3, 0);
    v3.submit("reviewer", &ledger_v3, "approved", "v3-review");
    v3.submit("lead", &ledger_v3, "accepted", "v3-accept");
    assert_accepted_historical(&v3, &token_v3, &ledger_v3);

    let frozen = LegacyFixture::new("transition-frozen-v3-bytes");
    let frozen_token = "f".repeat(64);
    let frozen_ledger = format!(".soulmate/runs/work-{frozen_token}.jsonl");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-v3.jsonl");
    fs::copy(fixture, frozen.root.join(&frozen_ledger)).unwrap();
    let inspected = frozen.call(&[
        "run",
        "inspect",
        &frozen_ledger,
        "--config",
        "soulmate.json",
    ]);
    assert!(inspected.status.success(), "{}", text(&inspected));
    let inspected_value: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected_value["events"][0]["version"], 3);
    assert_eq!(inspected_value["events"][0]["producer"]["name"], "soulmate");

    let token_v4 = "4".repeat(64);
    let v4 = LegacyFixture::new("transition-accepted-v4");
    let ledger_v4 = format!(".soulmate/runs/work-{token_v4}.jsonl");
    v4.start(&ledger_v4, true, None);
    v4.submit("lead", &ledger_v4, "scoped", "v4-scope");
    let worker_v4 = v4.submit("worker", &ledger_v4, "completed", "v4-worker");
    let target_v4 = worker_v4["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    v4.check(&ledger_v4, &target_v4, 0);
    v4.submit("reviewer", &ledger_v4, "approved", "v4-review");
    v4.submit("lead", &ledger_v4, "accepted", "v4-accept");
    assert_accepted_historical(&v4, &token_v4, &ledger_v4);

    let unchecked = Fixture::new("transition-accepted-unchecked-v5");
    let token_v5 = "5".repeat(64);
    let ledger_v5 = format!(".exitbind/runs/work-{token_v5}.jsonl");
    unchecked.start_unchecked(&ledger_v5);
    unchecked.submit_ok("lead", &ledger_v5, "scoped", "v5-scope");
    unchecked.submit_ok("worker", &ledger_v5, "completed", "v5-worker");
    unchecked.submit_ok("reviewer", &ledger_v5, "approved", "v5-review");
    unchecked.submit_ok("lead", &ledger_v5, "accepted", "v5-accept");
    let unchecked_next = unchecked.json(&["run", "next", &ledger_v5, "--config", "exitbind.json"]);
    assert_eq!(unchecked_next["status"], "accepted");
    assert_eq!(unchecked_next["progress"]["applicable"], false);
    assert!(unchecked_next["progress"]["percent"].is_null());
    assert_eq!(unchecked_next["progress"]["state"], "NOT_APPLICABLE");
    assert_eq!(
        unchecked_next["progress"]["reason"]["code"],
        "unchecked_run"
    );
    let unchecked_facade = unchecked.work_next(&token_v5);
    assert_eq!(unchecked_facade["next"]["action"], "done");
    assert_eq!(unchecked_facade["next"]["status"], "accepted");
    assert_eq!(
        unchecked_facade["next"]["progress"]["state"],
        "NOT_APPLICABLE"
    );
    assert_eq!(unchecked_facade["next"]["progress"]["applicable"], false);
    assert!(unchecked_facade["next"]["progress"]["percent"].is_null());
    assert_eq!(
        unchecked_facade["next"]["progress"]["reason"]["code"],
        "unchecked_run"
    );
    let unchecked_receipt = unchecked.call(&["receipt", &ledger_v5, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&unchecked_receipt, &unchecked_next["progress"]);

    let checked = Fixture::new("transition-accepted-checked-v5-control");
    let receipt = checked.accepted_single(".exitbind/runs/checked.jsonl");
    let checked_next = checked.json(&[
        "run",
        "next",
        ".exitbind/runs/checked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(checked_next["progress"]["applicable"], true);
    assert_eq!(checked_next["progress"]["state"], "READY");
    assert_eq!(checked_next["progress"]["percent"], 100);
    let checked_receipt = checked.call(&[
        "receipt",
        ".exitbind/runs/checked.jsonl",
        "--config",
        "exitbind.json",
    ]);
    assert!(
        checked_receipt.status.success(),
        "{}",
        text(&checked_receipt)
    );
    let checked_receipt_value: Value = serde_json::from_slice(&checked_receipt.stdout).unwrap();
    assert_eq!(
        checked_receipt_value["outcome"],
        checked_next["progress"]["state"]
    );
    assert_eq!(
        checked_receipt_value["reason"]["code"],
        checked_next["progress"]["reason"]["code"]
    );
    assert_receipt_valid(&checked, &receipt);

    let (checked_work, mut checked_action) = checked.work_begin("work checked control");
    checked_action =
        checked.work_return_ok(&checked_work, &checked_action, "scoped", "scope")["next"].clone();
    let _ = checked.work_return_ok(&checked_work, &checked_action, "completed", "worker");
    fs::write(checked.root.join("actual-product-check"), b"pass").unwrap();
    let checked_work_check = checked.work_check(&checked_work);
    assert!(
        checked_work_check.status.success(),
        "{}",
        text(&checked_work_check)
    );
    checked_action =
        serde_json::from_slice::<Value>(&checked_work_check.stdout).unwrap()["next"].clone();
    checked_action = checked.work_return_ok(&checked_work, &checked_action, "approved", "review")
        ["next"]
        .clone();
    let checked_work_accepted =
        checked.work_return_ok(&checked_work, &checked_action, "accepted", "acceptance");
    assert_eq!(checked_work_accepted["next"]["progress"]["state"], "READY");
    let accepted_next = checked.work_next(&checked_work[4..]);
    assert_eq!(accepted_next["next"]["progress"]["applicable"], true);
    assert_eq!(accepted_next["next"]["progress"]["percent"], 100);
}

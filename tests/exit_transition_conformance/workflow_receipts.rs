use super::*;

#[test]
fn work_facade_drives_multi_worker_multi_reviewer_receipt() {
    let f = Fixture::new("transition-work-receipt");
    f.configure_roles(&["worker", "worker_two"], &["reviewer", "reviewer_two"]);
    let (work, mut action) = f.work_begin("work receipt completeness");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    assert_eq!(action["role"], "worker");
    action = f.work_return_ok(&work, &action, "completed", "worker")["next"].clone();
    assert_eq!(action["action"], "check");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let first_check = f.work_check(&work);
    assert!(first_check.status.success(), "{}", text(&first_check));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["agent"], "worker_two", "{action}");
    action = f.work_return_ok(&work, &action, "completed", "worker_two")["next"].clone();
    assert_eq!(action["action"], "check");
    let repair_first = f.work_check(&work);
    assert!(repair_first.status.success(), "{}", text(&repair_first));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["action"], "check");
    let second_check = f.work_check(&work);
    assert!(second_check.status.success(), "{}", text(&second_check));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["role"], "reviewer");
    let approved = f.work_return_ok(&work, &action, "approved", "reviewer");
    assert_eq!(approved["next"]["requiresExpansion"], true);
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["agent"], "reviewer_two");
    action = f.work_return_ok(&work, &action, "approved", "reviewer_two")["next"].clone();
    let accepted = f.work_return_ok(&work, &action, "accepted", "acceptance");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let receipt_path = ".exitbind/receipts/work-multi.json";
    let receipt = f.call(&[
        "receipt",
        &ledger,
        "--output",
        receipt_path,
        "--config",
        "exitbind.json",
    ]);
    assert!(receipt.status.success(), "{}", text(&receipt));
    let receipt_value: Value =
        serde_json::from_slice(&fs::read(f.root.join(receipt_path)).unwrap()).unwrap();
    assert_eq!(receipt_value["artifacts"].as_array().unwrap().len(), 5);
    assert_eq!(
        receipt_value["check"]["targets"].as_array().unwrap().len(),
        2
    );
    let events = f.events(&ledger);
    let last_reviewer = events
        .iter()
        .rev()
        .find(|event| event["role"] == "reviewer" && event["outcome"] == "approved")
        .unwrap();
    assert_eq!(
        receipt_value["review"]["eventSha256"],
        last_reviewer["eventSha256"]
    );
    assert_eq!(
        receipt_value["review"]["artifactSha256"],
        last_reviewer["artifact"]["sha256"]
    );
    let verified = f.call(&["verify", receipt_path, "--config", "exitbind.json"]);
    assert!(verified.status.success(), "{}", text(&verified));
    assert_eq!(
        serde_json::from_slice::<Value>(&verified.stdout).unwrap()["outcome"],
        "READY"
    );
}

#[test]
fn failed_work_check_reaches_rework_then_only_fresh_attempt_reaches_ready() {
    let f = Fixture::new("transition-work-rework");
    let (work, mut action) = f.work_begin("failed check recovery");
    assert_eq!(action["action"], "lead_decision");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    assert_eq!(action["action"], "spawn");
    assert_eq!(action["role"], "worker");
    action = f.work_return_ok(&work, &action, "completed", "attempt one")["next"].clone();
    assert_eq!(action["action"], "check");
    let checked = f.work_check(&work);
    assert!(
        !checked.status.success(),
        "failed check unexpectedly succeeded"
    );
    let checked_value: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked_value["compact"], true);
    assert_eq!(checked_value["effect"], "recorded");
    assert_eq!(checked_value["result"]["code"], 1);
    assert_eq!(checked_value["next"]["progress"]["state"], "REFUSED");
    assert_eq!(
        checked_value["next"]["progress"]["reason"]["code"],
        "check_failed"
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let receipt = f.call(&["receipt", &ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &checked_value["next"]["progress"]);
    action = checked_value["next"].clone();
    assert_eq!(action["action"], "lead_decision");
    assert_eq!(action["role"], "lead");
    let rework = f.work_return_ok(&work, &action, "rework", "repair request");
    assert_eq!(rework["next"]["requiresExpansion"], true);
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["action"], "spawn");
    assert_eq!(action["role"], "worker");
    assert_eq!(action["packet"]["attempt"], 2);
    assert_eq!(
        action["packet"]["upstreamArtifacts"][0]["attemptStatus"],
        "prior"
    );
    action = f.work_return_ok(&work, &action, "completed", "attempt two")["next"].clone();
    assert_eq!(action["action"], "check");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    action = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(action["role"], "reviewer");
    action = f.work_return_ok(&work, &action, "approved", "fresh review")["next"].clone();
    assert_eq!(action["action"], "lead_decision");
    let accepted = f.work_return_ok(&work, &action, "accepted", "lead acceptance");
    assert_eq!(accepted["next"]["action"], "done");
    let accepted_next = f.work_next(&work[4..]);
    assert_eq!(accepted_next["next"]["status"], "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert_eq!(accepted_next["next"]["progress"]["percent"], 100);
}

#[test]
fn historical_check_input_change_replays_through_review_and_receipt() {
    let f = Fixture::new("transition-check-input-replay");
    let (work, action) = f.work_begin("historical check input replay");
    let action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    let action = f.work_return_ok(&work, &action, "completed", "worker result")["next"].clone();
    assert_eq!(action["action"], "check");
    let failed = f.work_check(&work);
    assert!(!failed.status.success(), "{}", text(&failed));

    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let passed = f.work_check(&work);
    assert!(passed.status.success(), "{}", text(&passed));

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let checks = f
        .events(&ledger)
        .into_iter()
        .filter(|event| event["action"] == "check")
        .collect::<Vec<_>>();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0]["result"]["code"], 1);
    assert_eq!(checks[1]["result"]["code"], 0);
    assert_ne!(checks[0]["inputsSha256"], checks[1]["inputsSha256"]);

    let inspected = f.json(&[
        "run",
        "inspect",
        &ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(inspected["inputsSha256"], checks[1]["inputsSha256"]);
    let action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["role"], "reviewer");
    let action = f.work_return_ok(&work, &action, "approved", "current review")["next"].clone();
    assert_eq!(action["role"], "lead");
    let accepted = f.work_return_ok(&work, &action, "accepted", "acceptance");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    let receipt = f.json(&["receipt", &ledger, "--json", "--config", "exitbind.json"]);
    assert_eq!(receipt["outcome"], "READY");
}

#[test]
fn missing_and_fabricated_review_calls_preserve_ledger_until_current_reviewer_controls() {
    let f = Fixture::new("transition-review-authority");
    let (work, mut action) = f.work_begin("review authority");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    let _ = f.work_return_ok(&work, &action, "completed", "worker");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    action = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(action["role"], "reviewer");

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before = f.events(&ledger);
    let fabricated = f.work_return(
        &work,
        &json!({"assignment": format!("sma_{}", "0".repeat(64))}),
        "approved",
        "fabricated review",
    );
    assert_exit_1(&fabricated);
    assert!(text(&fabricated).contains("assignment is not the current pending work action"));
    assert_eq!(f.events(&ledger), before);

    let missing = f.submit("lead", &ledger, "accepted", "missing-review");
    assert_exit_1(&missing);
    assert!(
        text(&missing).contains("agent 'lead' is not currently pending"),
        "{}",
        text(&missing)
    );
    assert_eq!(f.events(&ledger), before);

    action = f.work_return_ok(&work, &action, "approved", "real current review")["next"].clone();
    assert_eq!(action["action"], "lead_decision");
    let accepted = f.work_return_ok(&work, &action, "accepted", "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn attempt_one_review_does_not_authorize_attempt_two_until_fresh_review() {
    let f = Fixture::new("transition-stale-review");
    let ledger = ".exitbind/runs/attempts.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let first = f.submit_ok("worker", ledger, "completed", "attempt-one");
    let first_target = Fixture::worker_target(&first);
    f.check_ok(ledger, &first_target, 0);
    f.submit_ok("reviewer", ledger, "approved", "review-one");
    f.submit_ok("lead", ledger, "rework", "request-rework");
    let second = f.submit_ok("worker", ledger, "completed", "attempt-two");
    let second_target = Fixture::worker_target(&second);
    f.check_ok(ledger, &second_target, 0);

    let status = f.status(ledger);
    assert_eq!(status["attempt"], 2);
    assert_eq!(status["review"]["status"], "absent");
    assert_eq!(status["checks"]["status"], "passed");
    let before = f.events(ledger);
    let stale = f.submit("lead", ledger, "accepted", "stale-review-acceptance");
    assert_exit_1(&stale);
    assert!(
        text(&stale).contains("not currently pending"),
        "{}",
        text(&stale)
    );
    assert_eq!(f.events(ledger), before);

    f.submit_ok("reviewer", ledger, "approved", "review-two");
    let accepted = f.submit("lead", ledger, "accepted", "accepted-two");
    assert!(accepted.status.success(), "{}", text(&accepted));
    assert_eq!(
        serde_json::from_slice::<Value>(&accepted.stdout).unwrap()["status"],
        "accepted"
    );
}

#[test]
fn multi_receipt_binds_all_current_artifacts_and_keeps_last_review_singular() {
    let f = Fixture::new("transition-receipt-completeness");
    f.configure_roles(&["worker", "worker_two"], &["reviewer", "reviewer_two"]);
    let ledger = ".exitbind/runs/multi.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let mut targets = Vec::new();
    for worker in ["worker", "worker_two"] {
        let submission = f.submit_ok(worker, ledger, "completed", worker);
        targets.push(Fixture::worker_target(&submission));
    }
    for target in &targets {
        f.check_ok(ledger, target, 0);
    }
    for reviewer in ["reviewer", "reviewer_two"] {
        f.submit_ok(reviewer, ledger, "approved", reviewer);
    }
    f.submit_ok("lead", ledger, "accepted", "acceptance");
    let receipt_path = ".exitbind/receipts/multi.json";
    let receipt = f.call(&[
        "receipt",
        ledger,
        "--output",
        receipt_path,
        "--config",
        "exitbind.json",
    ]);
    assert!(receipt.status.success(), "{}", text(&receipt));
    let original: Value =
        serde_json::from_slice(&fs::read(f.root.join(receipt_path)).unwrap()).unwrap();
    assert_eq!(original["outcome"], "READY");
    assert_eq!(original["check"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(original["artifacts"].as_array().unwrap().len(), 5);
    let artifact_paths = original["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        artifact_paths,
        vec![
            ".exitbind/artifacts/worker.md",
            ".exitbind/artifacts/worker_two.md",
            ".exitbind/artifacts/reviewer.md",
            ".exitbind/artifacts/reviewer_two.md",
            ".exitbind/artifacts/acceptance.md",
        ]
    );
    assert_eq!(original["review"].as_object().unwrap().len(), 2);
    assert!(original["review"].get("eventSha256").is_some());
    assert!(original["review"].get("artifactSha256").is_some());
    assert_receipt_valid(&f, receipt_path);

    // Every current worker/reviewer/acceptance artifact gets omission and hash-tamper cells.
    let artifact_count = original["artifacts"].as_array().unwrap().len();
    for index in 0..artifact_count {
        let mut omitted = original.clone();
        omitted["artifacts"].as_array_mut().unwrap().remove(index);
        let omitted_path = format!(".exitbind/receipts/multi-omitted-{index}.json");
        write_json(&f.root.join(&omitted_path), &omitted);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &omitted_path,
            "receipt artifacts binding changed",
        );

        let mut tampered = original.clone();
        tampered["artifacts"][index]["sha256"] = json!("0".repeat(64));
        let tampered_path = format!(".exitbind/receipts/multi-tampered-{index}.json");
        write_json(&f.root.join(&tampered_path), &tampered);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &tampered_path,
            "receipt artifacts binding changed",
        );
    }

    // Every worker check target gets omission and target-hash tamper cells.
    let target_count = original["check"]["targets"].as_array().unwrap().len();
    for index in 0..target_count {
        let mut omitted = original.clone();
        omitted["check"]["targets"]
            .as_array_mut()
            .unwrap()
            .remove(index);
        let omitted_path = format!(".exitbind/receipts/multi-check-omitted-{index}.json");
        write_json(&f.root.join(&omitted_path), &omitted);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &omitted_path,
            "receipt check binding changed",
        );

        let mut tampered = original.clone();
        tampered["check"]["targets"][index]["targetEventSha256"] = json!("0".repeat(64));
        let tampered_path = format!(".exitbind/receipts/multi-check-tampered-{index}.json");
        write_json(&f.root.join(&tampered_path), &tampered);
        assert_receipt_variant_refused(
            &f,
            receipt_path,
            &tampered_path,
            "receipt check binding changed",
        );
    }

    // The singular review binding gets omission, fabrication, event, and artifact cells.
    let review_variants = ["missing", "fabricated", "event", "artifact"];
    for variant in review_variants {
        let mut mutated = original.clone();
        match variant {
            "missing" => {
                mutated.as_object_mut().unwrap().remove("review");
            }
            "fabricated" => {
                mutated["review"] = json!({
                    "eventSha256": "0".repeat(64),
                    "artifactSha256": "1".repeat(64)
                })
            }
            "event" => mutated["review"]["eventSha256"] = json!("0".repeat(64)),
            "artifact" => mutated["review"]["artifactSha256"] = json!("0".repeat(64)),
            _ => unreachable!(),
        }
        let path = format!(".exitbind/receipts/multi-review-{variant}.json");
        write_json(&f.root.join(&path), &mutated);
        assert_receipt_variant_refused(&f, receipt_path, &path, "receipt review binding changed");
    }
}

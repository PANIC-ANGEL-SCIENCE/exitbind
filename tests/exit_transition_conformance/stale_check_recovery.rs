use super::*;

#[test]
fn stale_subject_check_is_missing_until_reobserved_at_current_subject() {
    let f = Fixture::new("transition-stale-subject");
    f.configure_roles(&["worker", "worker_two"], &["reviewer"]);
    let ledger = ".exitbind/runs/stale.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let a = f.submit_ok("worker", ledger, "completed", "worker-a");
    let a_target = Fixture::worker_target(&a);
    f.check_ok(ledger, &a_target, 0);
    let b = f.submit_ok("worker_two", ledger, "completed", "worker-b");
    let b_target = Fixture::worker_target(&b);
    f.check_ok(ledger, &b_target, 0);
    f.submit_ok("reviewer", ledger, "approved", "review");

    let before = f.events(ledger);
    let refused = f.submit("lead", ledger, "accepted", "stale-acceptance");
    assert_exit_1(&refused);
    assert!(
        text(&refused).contains("check_missing"),
        "{}",
        text(&refused)
    );
    let after = f.events(ledger);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after.last().unwrap()["action"], "protect");
    assert_eq!(after.last().unwrap()["reason"], "check_missing");
    let status = f.status(ledger);
    let targets = status["checks"]["targets"].as_array().unwrap();
    assert_eq!(targets[0]["status"], "missing");
    assert_eq!(targets[1]["status"], "passed");
    let next = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    let receipt = f.call(&["receipt", ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &next["progress"]);

    // The same A target is re-observed against S2, but the earlier review is
    // still stale because it predates that check event.
    f.check_ok(ledger, &a_target, 0);
    let reobserved = f.status(ledger);
    let targets = reobserved["checks"]["targets"].as_array().unwrap();
    assert_eq!(targets[0]["status"], "passed");
    assert_eq!(targets[1]["status"], "passed");
    let stale_review = f.submit("lead", ledger, "accepted", "accept-current-subject");
    assert_exit_1(&stale_review);
    assert!(
        text(&stale_review).contains("canonical acceptance requires reviewer approval"),
        "{}",
        text(&stale_review)
    );
    let rework = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    assert_eq!(rework["assignments"][0]["role"], "lead");
    assert_eq!(rework["progress"]["state"], "IN_PROGRESS");
}

#[test]
fn stale_subject_recovery_is_visible_through_the_work_facade() {
    let f = Fixture::new("transition-stale-subject-work");
    f.configure_roles(&["worker", "worker_two"], &["reviewer"]);
    let (work, mut action) = f.work_begin("work stale subject");
    assert!(work.starts_with("smw_"));
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    assert_eq!(action["role"], "worker");
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    action = f.work_return_ok(&work, &action, "completed", "worker-a")["next"].clone();
    assert_eq!(action["action"], "check");
    let first_check = f.work_check(&work);
    assert!(first_check.status.success(), "{}", text(&first_check));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["agent"], "worker_two", "{action}");
    action = f.work_return_ok(&work, &action, "completed", "worker-b")["next"].clone();
    assert_eq!(action["action"], "check");
    assert_eq!(action["progress"]["state"], "BLOCKED");
    assert_eq!(action["progress"]["reason"]["code"], "check_missing");
    let work_ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let receipt = f.call(&["receipt", &work_ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &action["progress"]);

    // The façade selects A's stale target first, then B's missing target.
    let repair_a = f.work_check(&work);
    assert!(repair_a.status.success(), "{}", text(&repair_a));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["action"], "check");
    assert_eq!(action["progress"]["state"], "BLOCKED");
    let repair_b = f.work_check(&work);
    assert!(repair_b.status.success(), "{}", text(&repair_b));
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["role"], "reviewer");
    assert_eq!(action["progress"]["state"], "IN_PROGRESS");
    action = f.work_return_ok(&work, &action, "approved", "review")["next"].clone();
    let accepted = f.work_return_ok(&work, &action, "accepted", "acceptance");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");

    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let events = f.events(&ledger);
    assert!(ledger
        .strip_prefix(".exitbind/runs/")
        .is_some_and(|name| name.starts_with("work-")));
    let checks = events
        .iter()
        .filter(|event| event["action"] == "check")
        .collect::<Vec<_>>();
    assert_eq!(checks.len(), 3);
    let s1 = checks[0]["subjectSha256"].as_str().unwrap();
    let s2 = checks[1]["subjectSha256"].as_str().unwrap();
    assert_ne!(s1, s2);
    assert_eq!(checks[2]["subjectSha256"], s2);
    assert_eq!(checks[0]["acquisition"], "observed");
    assert_eq!(checks[1]["acquisition"], "observed");
    assert_eq!(checks[2]["acquisition"], "observed");
    assert_eq!(
        checks[0]["targetEventSha256"],
        checks[1]["targetEventSha256"]
    );
    assert_ne!(
        checks[1]["targetEventSha256"],
        checks[2]["targetEventSha256"]
    );
}

#[test]
fn caller_reported_failed_check_reaches_rework_and_fresh_low_level_attempt() {
    let f = Fixture::new("transition-low-level-rework");
    let ledger = ".exitbind/runs/low-level-rework.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let first = f.submit_ok("worker", ledger, "completed", "attempt-one");
    let first_target = Fixture::worker_target(&first);
    let failed = f.check(ledger, &first_target, 1);
    assert!(failed.status.success(), "{}", text(&failed));
    let status = f.status(ledger);
    assert_eq!(status["checks"]["status"], "blocked");
    assert_eq!(status["checks"]["targets"][0]["status"], "failed");
    let next = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    let receipt = f.call(&["receipt", ledger, "--config", "exitbind.json"]);
    assert_receipt_matches_progress(&receipt, &next["progress"]);
    assert_eq!(next["progress"]["state"], "REFUSED");
    assert_eq!(next["progress"]["reason"]["code"], "check_failed");
    assert_eq!(next["assignments"][0]["role"], "lead");

    f.submit_ok("lead", ledger, "rework", "rework");
    let attempt_two = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    assert_eq!(attempt_two["attempt"], 2);
    assert_eq!(attempt_two["progress"]["state"], "IN_PROGRESS");
    let second = f.submit_ok("worker", ledger, "completed", "attempt-two");
    let second_target = Fixture::worker_target(&second);
    let fresh = f.check(ledger, &second_target, 0);
    assert!(fresh.status.success(), "{}", text(&fresh));
    f.submit_ok("reviewer", ledger, "approved", "review-two");
    let accepted = f.submit("lead", ledger, "accepted", "accept-two");
    assert!(accepted.status.success(), "{}", text(&accepted));

    let events = f.events(ledger);
    assert!(events.iter().any(|event| {
        event["action"] == "check"
            && event["acquisition"] == "reported"
            && event["result"]["kind"] == "exit"
            && event["result"]["code"] == 1
    }));
    let subjects = events
        .iter()
        .filter(|event| event["action"] == "submit" && event["role"] == "worker")
        .map(|event| event["subjectSha256"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(subjects.len(), 2);
    assert_ne!(subjects[0], subjects[1]);
}

#[test]
fn real_attempt_one_reviewer_assignment_is_rejected_on_attempt_two_by_work_facade() {
    let f = Fixture::new("transition-work-stale-review");
    let (work, mut action) = f.work_begin("work stale review");
    action = f.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    fs::write(f.root.join("actual-product-check"), b"pass").unwrap();
    let _ = f.work_return_ok(&work, &action, "completed", "attempt-one");
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let reviewer_one = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(reviewer_one["role"], "reviewer");
    let reworked = f.work_return_ok(&work, &reviewer_one, "rework", "rework");
    assert_eq!(reworked["next"]["requiresExpansion"], true);
    action = f.work_next(&work[4..])["next"].clone();
    assert_eq!(action["packet"]["attempt"], 2);
    let _ = f.work_return_ok(&work, &action, "completed", "attempt-two");
    let checked = f.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let reviewer_two = serde_json::from_slice::<Value>(&checked.stdout).unwrap()["next"].clone();
    assert_eq!(reviewer_two["role"], "reviewer");
    assert_ne!(reviewer_one["assignment"], reviewer_two["assignment"]);
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before = f.events(&ledger);
    let stale = f.work_return(&work, &reviewer_one, "approved", "stale-review");
    assert_exit_1(&stale);
    assert!(text(&stale).contains("assignment is not the current pending work action"));
    assert_eq!(f.events(&ledger), before);
    let accepted_review = f.work_return_ok(&work, &reviewer_two, "approved", "review-two");
    let accepted = f.work_return_ok(&work, &accepted_review["next"], "accepted", "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn no_reviewer_configuration_reaches_canonical_missing_review_guard() {
    let f = Fixture::new("transition-missing-review-canonical");
    f.configure_roles(&["worker"], &[]);
    let ledger = ".exitbind/runs/no-reviewer.jsonl";
    f.start(ledger);
    f.submit_ok("lead", ledger, "scoped", "scope");
    let worker = f.submit_ok("worker", ledger, "completed", "worker");
    f.check_ok(ledger, &Fixture::worker_target(&worker), 0);
    let next = f.json(&["run", "next", ledger, "--json", "--config", "exitbind.json"]);
    assert_eq!(next["assignments"][0]["role"], "lead");
    let before = f.events(ledger);
    let missing = f.submit("lead", ledger, "accepted", "missing-review");
    assert_exit_1(&missing);
    assert!(text(&missing).contains("canonical acceptance requires reviewer approval"));
    assert_eq!(f.events(ledger), before);

    // The same checked, no-reviewer configuration is visible through the
    // facade: work next exposes the pending lead decision, but work return
    // reaches the canonical missing-review guard without appending an event.
    let facade = Fixture::new("transition-missing-review-work");
    facade.configure_roles(&["worker"], &[]);
    let (work, mut action) = facade.work_begin("work missing review");
    action = facade.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    let _ = facade.work_return_ok(&work, &action, "completed", "worker");
    fs::write(facade.root.join("actual-product-check"), b"pass").unwrap();
    let checked = facade.work_check(&work);
    assert!(checked.status.success(), "{}", text(&checked));
    let checked_value: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked_value["next"]["requiresExpansion"], true);
    let pending = facade.work_next(&work[4..])["next"].clone();
    assert_eq!(pending["action"], "lead_decision");
    assert_eq!(pending["progress"]["state"], "IN_PROGRESS");
    assert_eq!(
        pending["progress"]["reason"]["code"],
        "prerequisites_incomplete"
    );
    assert_eq!(checked_value["next"]["assignment"], pending["assignment"]);
    assert_eq!(checked_value["next"]["action"], pending["action"]);
    let facade_ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let facade_before = facade.events(&facade_ledger);
    let facade_missing = facade.work_return(&work, &pending, "accepted", "missing-review");
    assert_exit_1(&facade_missing);
    assert!(text(&facade_missing).contains("canonical acceptance requires reviewer approval"));
    assert_eq!(facade.events(&facade_ledger), facade_before);

    // The unchanged normal configuration is the paired valid control.
    let control = Fixture::new("transition-missing-review-control");
    let receipt = control.accepted_single(".exitbind/runs/control.jsonl");
    let verified = control.call(&["verify", &receipt, "--config", "exitbind.json"]);
    assert!(verified.status.success(), "{}", text(&verified));
    assert_eq!(
        serde_json::from_slice::<Value>(&verified.stdout).unwrap()["outcome"],
        "READY"
    );

    let (control_work, mut control_action) = control.work_begin("work reviewed control");
    control_action =
        control.work_return_ok(&control_work, &control_action, "scoped", "scope")["next"].clone();
    let _ = control.work_return_ok(&control_work, &control_action, "completed", "worker");
    fs::write(control.root.join("actual-product-check"), b"pass").unwrap();
    let control_checked = control.work_check(&control_work);
    assert!(
        control_checked.status.success(),
        "{}",
        text(&control_checked)
    );
    control_action =
        serde_json::from_slice::<Value>(&control_checked.stdout).unwrap()["next"].clone();
    control_action = control.work_return_ok(&control_work, &control_action, "approved", "review")
        ["next"]
        .clone();
    let control_accepted =
        control.work_return_ok(&control_work, &control_action, "accepted", "acceptance");
    assert_eq!(control_accepted["next"]["progress"]["state"], "READY");
}

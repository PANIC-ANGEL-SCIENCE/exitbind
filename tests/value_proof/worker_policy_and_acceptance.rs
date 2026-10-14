use super::*;

#[test]
fn named_worker_replan_uses_the_current_assignment_and_keeps_spent_authority() {
    let root = project("value-proof-named-worker-replan");
    configure_workers(&root, &["worker_named"]);
    let started = invoke(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "replan the current named worker",
            "--check-command",
            CHECK,
            "--review-policy",
            "required",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(started.status.success(), "{}", text(&started));
    let work = json_output(&started)["work"].as_str().unwrap().to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let scope = state_artifact(
        &root,
        "named-worker-scope.md",
        "bounded named worker scope\n",
    );
    submit(&root, "lead", &ledger, "scoped", &scope);
    let detail = json_output(&invoke(
        &root,
        &[
            "work",
            "detail",
            &work,
            "--json",
            "--config",
            "exitbind.json",
        ],
    ));
    assert_eq!(detail["recipient"]["agent"], "worker_named");
    let assignment = detail["recipient"]["assignment"].as_str().unwrap();
    for (operation, request) in [
        ("first-bounded-step", "named-first"),
        ("second-bounded-step", "named-second"),
    ] {
        let granted = invoke(
            &root,
            &[
                "work",
                "permit",
                &work,
                assignment,
                "--operation",
                operation,
                "--request-id",
                request,
                "--config",
                "exitbind.json",
            ],
        );
        assert!(granted.status.success(), "{}", text(&granted));
        assert_eq!(json_output(&granted)["allowed"], true);
    }
    let detail = json_output(&invoke(
        &root,
        &[
            "work",
            "detail",
            &work,
            "--json",
            "--config",
            "exitbind.json",
        ],
    ));
    assert_eq!(
        detail["actionForms"]["nextRequest"]["requiredAction"],
        "work replan"
    );
    let replanned = invoke(
        &root,
        &[
            "work",
            "replan",
            &work,
            assignment,
            "--hypothesis",
            "Inspect the exact failed check before choosing another bounded mutation.",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(replanned.status.success(), "{}", text(&replanned));
    let result = json_output(&replanned);
    assert_eq!(result["event"]["agent"], "worker_named");
    assert_eq!(result["governor"]["spent"], 2);
    assert_eq!(result["governor"]["replanCount"], 1);
    assert_eq!(result["governor"]["state"], "ready");
    assert!(result["governor"]["currentGrantEventSha256"].is_null());
    let before = fs::read(root.join(&ledger)).unwrap();
    let wrong_assignment = invoke(
        &root,
        &[
            "work",
            "replan",
            &work,
            &format!("sma_{}", "0".repeat(64)),
            "--hypothesis",
            "wrong assignment",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!wrong_assignment.status.success());
    assert!(text(&wrong_assignment).contains("not the current pending work action"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_runs_reject_no_worker_plans_and_early_lead_acceptance() {
    let no_worker = project("value-proof-no-worker");
    configure_workers(&no_worker, &[]);
    let rejected = invoke(
        &no_worker,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "no worker",
            "--ledger",
            ".exitbind/runs/no-worker.jsonl",
            "--check-command",
            CHECK,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert!(!no_worker.join(".exitbind/runs/no-worker.jsonl").exists());
    fs::remove_dir_all(no_worker).unwrap();

    let root = project("value-proof-early-acceptance");
    let ledger = ".exitbind/runs/early.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "early-lead.md", "lead\n");
    let before = fs::read(root.join(ledger)).unwrap();
    let accepted = invoke(
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
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!accepted.status.success(), "{}", text(&accepted));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    let status = json_output(&invoke(
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
    assert_eq!(status["checks"]["status"], "not_observed");
    assert_eq!(status["checks"]["targetCount"], 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn latest_check_result_controls_acceptance_and_failure_protection() {
    let root = project("value-proof-latest-check");
    let ledger = ".exitbind/runs/latest.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "latest-lead.md", "lead\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "latest-worker.md", "worker\n");
    let worker_submission = submit(&root, "worker", ledger, "completed", &worker);
    let target = event_hash(&worker_submission);
    let reviewer = state_artifact(&root, "latest-reviewer.md", "reviewer\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);

    assert!(record_check(&root, ledger, &target, "0").status.success());
    let passed = json_output(&invoke(
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
    assert_eq!(passed["checks"]["status"], "passed");
    assert!(record_check(&root, ledger, &target, "9").status.success());
    let failed = json_output(&invoke(
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
    assert_eq!(failed["checks"]["status"], "blocked");
    assert_eq!(failed["checks"]["observedCount"], 1);
    assert_eq!(failed["checks"]["failedCount"], 1);
    assert_eq!(failed["checks"]["targets"][0]["exitCode"], 9);

    let accepted = invoke(
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
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!accepted.status.success(), "{}", text(&accepted));
    let events = read_events(&root, ledger);
    assert_eq!(events.last().unwrap()["action"], "protect");
    assert_eq!(events.last().unwrap()["reason"], "check_failed");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn artifact_drift_blocks_check_and_protection_without_appending() {
    let root = project("value-proof-drift-no-append");
    let ledger = ".exitbind/runs/drift-no-append.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "drift-no-append-lead.md", "lead\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "drift-no-append-worker.md", "worker\n");
    let worker_submission = submit(&root, "worker", ledger, "completed", &worker);
    let target = event_hash(&worker_submission);
    let reviewer = state_artifact(&root, "drift-no-append-reviewer.md", "reviewer\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);
    fs::write(root.join(&worker), "changed after submission\n").unwrap();
    let before = fs::read(root.join(ledger)).unwrap();

    let check = record_check(&root, ledger, &target, "0");
    assert!(!check.status.success(), "{}", text(&check));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    let accepted = invoke(
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
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!accepted.status.success(), "{}", text(&accepted));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_supersession_inherits_policy_and_seals_predecessor() {
    let root = project("value-proof-supersede");
    let old = ".exitbind/runs/old.jsonl";
    let new = ".exitbind/runs/new.jsonl";
    checked_start(&root, old, "synthetic");
    let output = invoke(
        &root,
        &[
            "run",
            "supersede",
            old,
            "--workflow",
            "change",
            "--goal",
            "successor",
            "--ledger",
            new,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let successor = json_output(&invoke(
        &root,
        &["run", "inspect", new, "--json", "--config", "exitbind.json"],
    ));
    assert_eq!(successor["events"][0]["version"], 8);
    assert_eq!(successor["events"][0]["checkPolicy"]["command"], CHECK);
    assert_eq!(successor["events"][0]["checkPolicy"]["origin"], "synthetic");
    assert!(root.join(".exitbind/runs/old.jsonl.supersede").is_file());

    let worker = state_artifact(&root, "sealed-worker.md", "worker\n");
    let before = fs::read(root.join(old)).unwrap();
    let mutation = invoke(
        &root,
        &[
            "run",
            "submit",
            "worker",
            old,
            "--outcome",
            "completed",
            "--artifact",
            &worker,
            "--artifact-root",
            "state",
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!mutation.status.success(), "{}", text(&mutation));
    assert_eq!(fs::read(root.join(old)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

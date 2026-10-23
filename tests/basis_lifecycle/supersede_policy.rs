use super::*;

#[test]
fn supersede_preserves_explicit_basis_and_review_policy_options() {
    let fixture = Fixture::new("supersede-policy-propagation");
    let original_basis = json!({
        "version": 1,
        "constraints": ["preserve predecessor"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["successor records owner policy"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&original_basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let _lead = fixture.value(&["run", "next", fixture.ledger()]);
    let lead_artifact = fixture.artifact("supersede-lead.md", b"scope");
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &lead_artifact,
            "--artifact-root",
            "state",
        ],
        b"scope",
    );

    let successor_basis = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve predecessor", "require currentness"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["successor records owner policy"]
    }));
    let successor = fixture.value(&[
        "run",
        "supersede",
        fixture.ledger(),
        "--workflow",
        "change",
        "--goal",
        "successor goal",
        "--ledger",
        ".exitbind/runs/successor.jsonl",
        "--basis",
        &serde_json::to_string(&successor_basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let event = ledger_events_at(&fixture, ".exitbind/runs/successor.jsonl")[0].clone();
    assert_eq!(event["basis"], successor_basis);
    assert_eq!(event["reviewPolicy"]["decision"], "omitted");
    assert_eq!(event["subject"]["basisSha256"], successor_basis["sha256"]);
    assert!(event.get("recoveryProtocol").is_none());
    let next = fixture.value(&["run", "next", ".exitbind/runs/successor.jsonl"]);
    assert_eq!(
        next["assignments"][0]["basisSha256"],
        successor_basis["sha256"]
    );
    assert_eq!(
        next["assignments"][0]["reviewDecisionSha256"],
        event["reviewPolicy"]["sha256"]
    );
    assert!(successor["runId"].is_string());
}

#[test]
fn supersede_claim_is_atomic_and_idempotent_under_concurrency() {
    let fixture = Fixture::new("supersede-concurrent-claim");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "concurrent predecessor",
        "--ledger",
        fixture.ledger(),
    ]);

    let barrier = Arc::new(Barrier::new(2));
    let root = fixture.root.to_path_buf();
    // Different requests must choose one successor even if the OS schedules
    // their children sequentially. Identical requests may both validly succeed
    // because the later request replays the already committed claim.
    let workers = [
        ".exitbind/runs/concurrent-successor-a.jsonl",
        ".exitbind/runs/concurrent-successor-b.jsonl",
    ]
    .into_iter()
    .map(|ledger| {
        let barrier = Arc::clone(&barrier);
        let root = root.clone();
        thread::spawn(move || {
            barrier.wait();
            let output = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .current_dir(&root)
                .args([
                    "run",
                    "supersede",
                    ".exitbind/runs/basis.jsonl",
                    "--workflow",
                    "change",
                    "--goal",
                    "concurrent successor",
                    "--ledger",
                    ledger,
                    "--review-policy",
                    "omitted",
                    "--config",
                ])
                .arg(root.join("exitbind.json"))
                .output()
                .unwrap();
            (ledger, output)
        })
    })
    .collect::<Vec<_>>();
    let outputs = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outputs
            .iter()
            .filter(|(_, output)| output.status.success())
            .count(),
        1,
        "concurrent supersede did not choose one winner: {:?}",
        outputs
            .iter()
            .map(|(_, output)| String::from_utf8_lossy(&output.stderr).into_owned())
            .collect::<Vec<_>>()
    );
    assert!(outputs.iter().any(|(_, output)| {
        !output.status.success()
            && (String::from_utf8_lossy(&output.stderr).contains("busy")
                || String::from_utf8_lossy(&output.stderr).contains("already claimed"))
    }));

    let claim = fixture.root.join(".exitbind/runs/basis.jsonl.supersede");
    let (winner, _) = outputs
        .iter()
        .find(|(_, output)| output.status.success())
        .unwrap();
    let (loser, _) = outputs
        .iter()
        .find(|(_, output)| !output.status.success())
        .unwrap();
    let successor = fixture.root.join(winner);
    assert!(claim.is_file());
    assert!(successor.is_file());
    assert!(!fixture.root.join(loser).exists());
    assert_eq!(ledger_events_at(&fixture, winner).len(), 1);
    let claim_bytes = fs::read(&claim).unwrap();
    let successor_bytes = fs::read(&successor).unwrap();

    let rerun = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "concurrent successor",
            "--ledger",
            winner,
            "--review-policy",
            "omitted",
        ],
        None,
    );
    assert!(
        rerun.status.success(),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(fs::read(&claim).unwrap(), claim_bytes);
    assert_eq!(fs::read(&successor).unwrap(), successor_bytes);

    let conflicting = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "different concurrent successor",
            "--ledger",
            ".exitbind/runs/conflicting-successor.jsonl",
        ],
        None,
    );
    assert!(!conflicting.status.success());
    assert!(String::from_utf8_lossy(&conflicting.stderr).contains("already claimed"));
    assert!(!fixture
        .root
        .join(".exitbind/runs/conflicting-successor.jsonl")
        .exists());
}

#[test]
fn supersede_rejects_basis_only_or_invalid_review_without_durable_mutation() {
    let fixture = Fixture::new("supersede-invalid-extension");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
    ]);
    let original = fs::read(fixture.root.join(fixture.ledger())).unwrap();
    let basis = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve predecessor"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["invalid extension is inert"]
    }));

    let basis_only = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "basis-only successor",
            "--ledger",
            ".exitbind/runs/basis-only.jsonl",
            "--basis",
            &serde_json::to_string(&basis).unwrap(),
        ],
        None,
    );
    assert!(!basis_only.status.success());
    assert!(String::from_utf8_lossy(&basis_only.stderr)
        .contains("--basis requires an explicit --review-policy"));
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        original
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/basis-only.jsonl")
        .exists());
    assert!(!fixture
        .root
        .join(format!("{}.supersede", fixture.ledger()))
        .exists());

    let invalid_review = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "invalid-review successor",
            "--ledger",
            ".exitbind/runs/invalid-review.jsonl",
            "--review-policy",
            "unsupported",
        ],
        None,
    );
    assert!(!invalid_review.status.success());
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        original
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/invalid-review.jsonl")
        .exists());
}

#[test]
fn supersede_review_policy_only_creates_marked_basisless_successor_assignment() {
    let fixture = Fixture::new("supersede-review-policy-only");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
    ]);
    fixture.value(&[
        "run",
        "supersede",
        fixture.ledger(),
        "--workflow",
        "change",
        "--goal",
        "review-only successor",
        "--ledger",
        ".exitbind/runs/review-only.jsonl",
        "--review-policy",
        "omitted",
    ]);

    let event = ledger_events_at(&fixture, ".exitbind/runs/review-only.jsonl")[0].clone();
    assert_eq!(event["reviewPolicy"]["decision"], "omitted");
    assert!(event["basisProtocol"].is_number());
    assert!(event["recoveryProtocol"].is_number());
    assert!(event.get("basis").is_none());
    assert!(event["subject"].get("basisSha256").is_none());
    let next = fixture.value(&["run", "next", ".exitbind/runs/review-only.jsonl"]);
    assert!(next["assignments"][0]["basisSha256"].is_null());
    assert_eq!(
        next["assignments"][0]["reviewDecisionSha256"],
        event["reviewPolicy"]["sha256"]
    );
}

#[test]
fn supersede_without_options_keeps_historical_unmarked_compatibility() {
    let fixture = Fixture::new("supersede-historical-no-options");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
    ]);
    fixture.value(&[
        "run",
        "supersede",
        fixture.ledger(),
        "--workflow",
        "change",
        "--goal",
        "historical successor",
        "--ledger",
        ".exitbind/runs/historical-successor.jsonl",
    ]);

    let event = ledger_events_at(&fixture, ".exitbind/runs/historical-successor.jsonl")[0].clone();
    assert!(event.get("basisProtocol").is_none());
    assert!(event.get("basis").is_none());
    assert!(event.get("reviewPolicy").is_none());
    assert!(event["subject"].get("basisSha256").is_none());
    let next = fixture.value(&["run", "next", ".exitbind/runs/historical-successor.jsonl"]);
    assert!(next["assignments"][0]["basisSha256"].is_null());
    assert!(next["assignments"][0]["reviewDecisionSha256"].is_null());
}

#[test]
fn marked_basis_contradiction_successor_preserves_governor_and_currentness() {
    let fixture = Fixture::new("basis-lifecycle");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["contradiction routes to Lead"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "basis lifecycle",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let start_event = ledger_events(&fixture).pop().unwrap();
    assert_eq!(start_event["version"], 8);
    assert_eq!(start_event["basisProtocol"], 1);
    let original_basis_sha = start_event["basis"]["sha256"].as_str().unwrap().to_owned();
    let initial_governor = fixture.value(&["run", "inspect", fixture.ledger()])["governor"].clone();

    let duplicate_policy = fixture.call(
        &[
            "run",
            "review-policy",
            "lead",
            fixture.ledger(),
            "--decision",
            "required",
        ],
        None,
    );
    assert!(!duplicate_policy.status.success());
    assert!(
        String::from_utf8_lossy(&duplicate_policy.stderr).contains("duplicate")
            || String::from_utf8_lossy(&duplicate_policy.stdout).contains("duplicate")
    );
    assert_eq!(ledger_events(&fixture).len(), 1);

    let lead = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(lead["assignments"][0]["role"], "lead");
    let lead_artifact = fixture.artifact("lead.md", b"scoped basis work");
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &lead_artifact,
            "--artifact-root",
            "state",
        ],
        b"scoped basis work",
    );

    let worker = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(worker["assignments"][0]["role"], "worker");
    let worker_artifact = fixture.artifact("worker.md", b"contradiction finding");
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &worker_artifact,
            "--artifact-root",
            "state",
            "--reason",
            "basis assumption failed",
        ],
        b"contradiction finding",
    );
    let finding_sha = contradiction["event"]["eventSha256"].as_str().unwrap();
    let pending = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(pending["assignments"][0]["role"], "lead");
    assert_eq!(pending["assignments"][0]["stage"], 4);

    let successor = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve history", "require currentness"],
        "openZones": ["worker implementation"],
        "decisiveCases": [
            "contradiction routes to Lead",
            "successor rechecks contradiction"
        ]
    }));
    let disposition = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding_sha],
        "basisSha256": original_basis_sha,
        "causalAssumption": "the original basis omitted a required currentness constraint",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "successor basis and fresh worker evidence",
        "decisiveRegression": "contradiction routes through Lead disposition",
        "invalidatedEvidence": [finding_sha],
        "successorBasis": successor,
    }));
    let disposition_text = serde_json::to_string(&disposition).unwrap();
    let disposition_artifact = fixture.artifact("disposition.md", b"Lead disposition");
    let events_before_invalid = ledger_events(&fixture).len();
    let mut wrong_finding = disposition.clone();
    wrong_finding["findingSha256s"] = json!(["0".repeat(64)]);
    let wrong_finding = disposition_with_hash(wrong_finding);
    let mut wrong_basis = disposition.clone();
    wrong_basis["basisSha256"] = json!("0".repeat(64));
    let wrong_basis = disposition_with_hash(wrong_basis);
    let mut wrong_category = disposition.clone();
    wrong_category["category"] = json!("unknown_category");
    let wrong_category = disposition_with_hash(wrong_category);
    for (agent, raw) in [
        ("lead", serde_json::to_string(&wrong_finding).unwrap()),
        ("lead", serde_json::to_string(&wrong_basis).unwrap()),
        ("lead", serde_json::to_string(&wrong_category).unwrap()),
        ("worker", disposition_text.clone()),
    ] {
        let refused = fixture.call(
            &[
                "run",
                "submit",
                agent,
                fixture.ledger(),
                "--outcome",
                "disposition",
                "--artifact",
                &disposition_artifact,
                "--artifact-root",
                "state",
                "--disposition",
                &raw,
            ],
            None,
        );
        assert!(
            !refused.status.success(),
            "invalid disposition unexpectedly accepted"
        );
        assert_eq!(ledger_events(&fixture).len(), events_before_invalid);
    }
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &disposition_artifact,
            "--artifact-root",
            "state",
            "--disposition",
            &disposition_text,
        ],
        b"Lead disposition",
    );

    let fresh_worker = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(fresh_worker["assignments"][0]["role"], "worker");
    assert_ne!(
        fresh_worker["assignments"][0]["basisSha256"],
        original_basis_sha
    );
    let state = fixture.value(&["run", "status", fixture.ledger(), "--json"]);
    let inspect = fixture.value(&["run", "inspect", fixture.ledger()]);
    assert_eq!(state["status"], "running");
    assert_eq!(
        inspect["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["action"] == "submit" && event["outcome"] == "disposition")
            .count(),
        1
    );
    assert_eq!(inspect["governor"]["enabled"], true);
    assert!(inspect["governor"]["spent"].is_number());
    assert!(inspect["governor"]["consumedGrants"].is_array());
    assert_eq!(inspect["governor"]["spent"], initial_governor["spent"]);
    assert_eq!(
        inspect["governor"]["consumedGrants"],
        initial_governor["consumedGrants"]
    );
    assert_eq!(
        inspect["governor"]["lineageSha256"],
        initial_governor["lineageSha256"]
    );

    let fresh_artifact = fixture.artifact("fresh-worker.md", b"fresh successor evidence");
    let completed = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "completed",
            "--artifact",
            &fresh_artifact,
            "--artifact-root",
            "state",
        ],
        b"fresh successor evidence",
    );
    fixture.value(&[
        "run",
        "record-check",
        fixture.ledger(),
        "--target",
        completed["event"]["eventSha256"].as_str().unwrap(),
        "--check-command",
        "true",
        "--exit-code",
        "0",
        "--duration-ms",
        "1",
    ]);
    fs::write(
        fixture.root.join(&fresh_artifact),
        b"tampered successor evidence",
    )
    .unwrap();
    let stale = fixture.value(&["run", "status", fixture.ledger(), "--json"]);
    assert_eq!(
        stale["artifact"]["status"], "drifted",
        "tampered artifact unexpectedly remained current: {stale}"
    );

    let events = ledger_events(&fixture);
    assert!(events
        .iter()
        .any(|event| event["action"] == "submit" && event["outcome"] == "contradiction"));
    assert!(events
        .iter()
        .any(|event| event["action"] == "submit" && event["outcome"] == "disposition"));
    assert!(events
        .iter()
        .any(|event| event["action"] == "start" && event["version"] == 8));
    let events_after_disposition = events.len();
    let duplicate = fixture.call(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &disposition_artifact,
            "--artifact-root",
            "state",
            "--disposition",
            &disposition_text,
        ],
        None,
    );
    assert!(!duplicate.status.success());
    assert_eq!(ledger_events(&fixture).len(), events_after_disposition);
}

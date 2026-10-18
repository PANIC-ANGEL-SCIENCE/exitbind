use super::*;

#[test]
fn implementation_correction_is_a_basis_noop_and_restarts_worker() {
    let fixture = Fixture::new("basis-noop-correction");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["correction keeps the basis"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "basis no-op correction",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let events = ledger_events(&fixture);
    let start = &events[0];
    assert!(start.get("recoveryProtocol").is_none());
    let original_basis = start["basis"]["sha256"].as_str().unwrap().to_owned();
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &fixture.artifact("noop-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &fixture.artifact("noop-finding.md", b"finding"),
            "--artifact-root",
            "state",
        ],
        b"finding",
    );
    let finding = contradiction["event"]["eventSha256"].as_str().unwrap();
    let disposition = disposition_with_hash(json!({
        "category": "implementation_defect",
        "findingSha256s": [finding],
        "basisSha256": original_basis,
        "causalAssumption": "the implementation changed without changing the basis",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "fresh worker evidence",
        "decisiveRegression": "the original basis remains authoritative",
        "invalidatedEvidence": [finding],
        "successorBasis": null
    }));
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &fixture.artifact("noop-disposition.md", b"disposition"),
            "--artifact-root",
            "state",
            "--disposition",
            &serde_json::to_string(&disposition).unwrap(),
        ],
        b"disposition",
    );
    let next = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(next["assignments"][0]["role"], "worker");
    assert_eq!(next["assignments"][0]["basisSha256"], original_basis);
}

#[test]
fn design_correction_narrows_open_zone_and_preserves_bound_claims() {
    let fixture = Fixture::new("basis-narrowing-correction");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation", "lead clarification"],
        "decisiveCases": ["the original finding is recorded"],
        "boundarySha256": "a".repeat(64),
        "preservationSha256": "b".repeat(64)
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "narrow basis correction",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let original_basis = ledger_events(&fixture)[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &fixture.artifact("narrow-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &fixture.artifact("narrow-finding.md", b"finding"),
            "--artifact-root",
            "state",
        ],
        b"finding",
    );
    let finding = contradiction["event"]["eventSha256"].as_str().unwrap();
    let successor = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve history", "require currentness"],
        "openZones": ["worker implementation"],
        "decisiveCases": [
            "the original finding is recorded",
            "successor regression"
        ],
        "boundarySha256": "a".repeat(64),
        "preservationSha256": "b".repeat(64)
    }));
    let disposition = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding],
        "basisSha256": original_basis,
        "causalAssumption": "the open zone was too broad",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "narrowed successor basis and fresh evidence",
        "decisiveRegression": "the successor retains the bound claims",
        "invalidatedEvidence": [finding],
        "successorBasis": successor
    }));
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &fixture.artifact("narrow-disposition.md", b"disposition"),
            "--artifact-root",
            "state",
            "--disposition",
            &serde_json::to_string(&disposition).unwrap(),
        ],
        b"disposition",
    );
    let disposition_event = ledger_events(&fixture)
        .into_iter()
        .find(|event| event["outcome"] == "disposition")
        .unwrap();
    assert_eq!(
        disposition_event["disposition"]["successorBasis"]["openZones"],
        json!(["worker implementation"])
    );
    assert_eq!(
        disposition_event["disposition"]["successorBasis"]["boundarySha256"],
        "a".repeat(64)
    );
    assert_eq!(
        disposition_event["disposition"]["successorBasis"]["preservationSha256"],
        "b".repeat(64)
    );
    let next = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(next["assignments"][0]["role"], "worker");
    assert_eq!(
        next["assignments"][0]["basisSha256"],
        disposition_event["disposition"]["successorBasis"]["sha256"]
    );
}

#[test]
fn blocked_or_superseded_correction_refuses_without_mutation() {
    let blocked = Fixture::new("blocked-correction-refusal");
    blocked.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "blocked correction",
        "--ledger",
        blocked.ledger(),
        "--review-policy",
        "omitted",
    ]);
    blocked.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            blocked.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &blocked.artifact("blocked-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    blocked.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            blocked.ledger(),
            "--outcome",
            "blocked",
            "--artifact",
            &blocked.artifact("blocked-worker.md", b"blocked"),
            "--artifact-root",
            "state",
        ],
        b"blocked",
    );
    let before = fs::read(blocked.root.join(blocked.ledger())).unwrap();
    let artifacts = blocked.artifact_snapshot();
    let refused = blocked.call(
        &[
            "run",
            "submit",
            "lead",
            blocked.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            ".exitbind/artifacts/blocked-worker.md",
            "--artifact-root",
            "state",
            "--disposition",
            "{}",
        ],
        Some(b"should not persist"),
    );
    assert!(!refused.status.success());
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("terminal"), "{refused_text}");
    assert_eq!(
        fs::read(blocked.root.join(blocked.ledger())).unwrap(),
        before
    );
    assert_eq!(blocked.artifact_snapshot(), artifacts);

    let superseded = Fixture::new("superseded-correction-refusal");
    superseded.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "superseded correction",
        "--ledger",
        superseded.ledger(),
    ]);
    superseded.value(&[
        "run",
        "supersede",
        superseded.ledger(),
        "--workflow",
        "change",
        "--goal",
        "successor",
        "--ledger",
        ".exitbind/runs/successor.jsonl",
    ]);
    let stale_artifact = superseded.artifact("stale-correction.md", b"stale");
    let before = fs::read(superseded.root.join(superseded.ledger())).unwrap();
    let artifacts = superseded.artifact_snapshot();
    let refused = superseded.call(
        &[
            "run",
            "submit",
            "lead",
            superseded.ledger(),
            "--outcome",
            "blocked",
            "--artifact",
            &stale_artifact,
            "--artifact-root",
            "state",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("superseded"));
    assert_eq!(
        fs::read(superseded.root.join(superseded.ledger())).unwrap(),
        before
    );
    assert_eq!(superseded.artifact_snapshot(), artifacts);
}

#[test]
fn invalid_successor_or_claim_is_inert() {
    let fixture = Fixture::new("invalid-correction-noop");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["invalid correction is inert"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "invalid correction",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let original_basis = ledger_events(&fixture)[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &fixture.artifact("invalid-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &fixture.artifact("invalid-finding.md", b"finding"),
            "--artifact-root",
            "state",
        ],
        b"finding",
    );
    let finding = contradiction["event"]["eventSha256"].as_str().unwrap();
    let invalid = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding],
        "basisSha256": original_basis,
        "causalAssumption": "invalid widening",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "none",
        "decisiveRegression": "rejected before artifact creation",
        "invalidatedEvidence": [finding],
        "successorBasis": basis_with_hash(json!({
            "version": 1,
            "constraints": ["preserve history"],
            "openZones": ["worker implementation"],
            "decisiveCases": ["dropped decisive case"]
        }))
    }));
    let invalid_artifact = fixture.artifact("invalid-disposition.md", b"must not persist");
    let before = fs::read(fixture.root.join(fixture.ledger())).unwrap();
    let artifacts = fixture.artifact_snapshot();
    let refused = fixture.call(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &invalid_artifact,
            "--artifact-root",
            "state",
            "--disposition",
            &serde_json::to_string(&invalid).unwrap(),
        ],
        None,
    );
    assert!(!refused.status.success());
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("decisive case"), "{refused_text}");
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        before
    );
    assert_eq!(fixture.artifact_snapshot(), artifacts);

    let claim = fixture.root.join(format!("{}.supersede", fixture.ledger()));
    fs::write(&claim, b"not a valid supersession claim\n").unwrap();
    let before = fs::read(fixture.root.join(fixture.ledger())).unwrap();
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "invalid claim successor",
            "--ledger",
            ".exitbind/runs/invalid-claim.jsonl",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("claim"));
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        before
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/invalid-claim.jsonl")
        .exists());
    assert_eq!(
        fs::read(&claim).unwrap(),
        b"not a valid supersession claim\n"
    );
}

#[test]
fn successor_retains_a_real_governor_grant_and_owner_lineage() {
    let fixture = Fixture::new("basis-governor-successor");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve governor lineage"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["review rework gets a Lead disposition"]
    });
    let begin = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "governor successor",
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let work = begin["work"].as_str().unwrap().to_owned();
    let lead = begin["next"].clone();
    let lead_handle = lead["assignment"].as_str().unwrap();
    let scoped = fixture.value_with_input(
        &["work", "return", &work, lead_handle, "--outcome", "scoped"],
        b"scope",
    );
    let worker = scoped["next"].clone();
    let worker_handle = worker["assignment"].as_str().unwrap();
    let permit = fixture.value(&[
        "work",
        "permit",
        &work,
        worker_handle,
        "--operation",
        "successor worker mutation",
    ]);
    assert_eq!(permit["governor"]["enabled"], true);
    assert!(
        permit["governor"].is_object(),
        "invalid governor projection: {permit}"
    );
    let granted = fixture.value_with_input(
        &[
            "work",
            "return",
            &work,
            worker_handle,
            "--outcome",
            "completed",
        ],
        b"worker evidence",
    );
    let after_grant = fixture.value(&[
        "run",
        "inspect",
        &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
    ]);
    assert_eq!(
        after_grant["governor"]["currentMutation"]["eventSha256"],
        after_grant["governor"]["headSha256"]
    );
    fixture.value(&["work", "check", &work]);
    let reviewer_response = fixture.value(&["work", "next", &work]);
    let reviewer = support::resolved_work_next(
        &fixture.root,
        &work,
        &reviewer_response,
        env!("CARGO_BIN_EXE_exitbind"),
    );
    assert_eq!(reviewer["role"], "reviewer");
    let rework = fixture.value_with_input(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        b"review finding",
    );
    assert_eq!(rework["next"]["role"], "lead");
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before = fixture.value(&["run", "inspect", &ledger]);
    let events_before = ledger_events_at(&fixture, &ledger);
    let finding = events_before
        .iter()
        .find(|event| event["outcome"] == "rework")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let current_basis = events_before[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let successor = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve governor lineage", "preserve review finding"],
        "openZones": ["worker implementation"],
        "decisiveCases": [
            "review rework gets a Lead disposition",
            "fresh worker evidence"
        ]
    }));
    let disposition = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding],
        "basisSha256": current_basis,
        "causalAssumption": "review rework identifies a design correction",
        "affectedPaths": ["reviewed worker path"],
        "repairBoundary": "successor basis and fresh evidence",
        "decisiveRegression": "the next worker is bound to the successor",
        "invalidatedEvidence": [finding],
        "successorBasis": successor,
        // A reviewer finding is decided with a named Lead decision.
        "decision": "supersede",
        "reason": "the review exposed a design correction",
    }));
    let lead_after_rework = rework["next"]["assignment"].as_str().unwrap();
    let after = fixture.value_with_input(
        &[
            "work",
            "return",
            &work,
            lead_after_rework,
            "--outcome",
            "disposition",
            "--disposition",
            &serde_json::to_string(&disposition).unwrap(),
        ],
        b"Lead disposition",
    );
    assert_eq!(after["next"]["role"], "worker");
    assert_eq!(after["next"]["resolvedActor"], "worker");
    assert_eq!(after["next"]["requiresExpansion"], true);
    assert!(after["next"]["packet"].is_null());
    let successor_packet = fixture.value(&["work", "next", &work, "--full"]);
    assert!(successor_packet["next"]["packet"]["context"].is_object());
    let after_inspect = fixture.value(&["run", "inspect", &ledger]);
    assert_eq!(before["governor"]["enabled"], true);
    assert!(before["governor"]["spent"].as_u64().unwrap_or(0) > 0);
    assert!(!before["governor"]["consumedGrants"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(after_inspect["governor"]["enabled"], true);
    assert_eq!(
        after_inspect["governor"]["spent"],
        before["governor"]["spent"]
    );
    assert_eq!(
        after_inspect["governor"]["consumedGrants"],
        before["governor"]["consumedGrants"]
    );
    assert_eq!(
        after_inspect["governor"]["lineageSha256"],
        before["governor"]["lineageSha256"]
    );
    let _ = granted;
}

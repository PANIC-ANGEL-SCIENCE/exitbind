use super::*;

#[test]
fn owner_recovery_preserves_old_ledger_accounting_and_requires_new_evidence() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    let _decision_path = fixture.write_recovery_artifacts(draft);
    let successor = format!(".exitbind/runs/work-{}.jsonl", "a".repeat(64));
    let recovered = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
    assert!(recovered.status.success(), "{}", display(&recovered));
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();
    assert_eq!(recovered["work"], format!("smw_{}", "a".repeat(64)));
    let start: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(start["carryProtocol"], 2);
    assert_eq!(start["governorCarry"]["accounting"]["state"], "blocked");
    let carry_sha = value_sha256(&start["governorCarry"]);
    assert_eq!(start["governorCarrySha256"], carry_sha);
    let claim_path = format!("{}.supersede", fixture.old_ledger);
    let claim_bytes = fs::read(fixture.root.join(&claim_path)).unwrap();
    let claim: Value = serde_json::from_slice(&claim_bytes).unwrap();
    assert_eq!(claim["carryProtocol"], 2);
    assert_eq!(claim["governorCarrySha256"], carry_sha);
    let successor_work = format!("smw_{}", "a".repeat(64));
    let inspected = fixture.json(&["run", "inspect", &successor], b"");
    assert_eq!(inspected["governor"]["state"], "evidence_required");
    assert_eq!(inspected["governor"]["spent"], 3);
    assert_eq!(inspected["governor"]["currentMutation"], Value::Null);
    assert_eq!(
        inspected["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        inspected["events"][0]["reviewPolicy"]["decision"],
        "required"
    );
    assert!(inspected["events"][0]["checkPolicy"].is_object());
    assert_eq!(inspected["governorRecovery"]["protocol"], 2);
    assert_eq!(inspected["governorRecovery"]["state"], "evidence_required");
    let status = fixture.json(&["run", "status", &successor, "--json"], b"");
    assert_eq!(status["governorRecovery"]["state"], "evidence_required");

    // The frozen predecessor reader must still read the old Work and ledger
    // after the protocol-2 successor claim and start have been written.
    if let Ok(old_binary) = std::env::var("EXITBIND_OWNER_RECOVERY_OLD_READER") {
        for args in [
            vec![
                "work",
                "detail",
                fixture.work.as_str(),
                "--json",
                "--config",
                "exitbind.json",
            ],
            vec![
                "run",
                "inspect",
                fixture.old_ledger.as_str(),
                "--config",
                "exitbind.json",
            ],
        ] {
            let output = Command::new(&old_binary)
                .current_dir(&fixture.root)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", display(&output));
        }
    }
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();

    // The exact same successor request replays the existing claim/start.
    let replay = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
    assert!(replay.status.success(), "{}", display(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["work"], recovered["work"]);
    let replayed = fixture.json(&["run", "inspect", &successor], b"");
    assert_eq!(replayed["governor"]["spent"], 3);
    assert_eq!(replayed["governor"]["state"], "evidence_required");

    let other_target = ".exitbind/runs/work-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.jsonl";
    let refused_other_target = fixture.execute_form_replacing(
        &detail["actionForms"]["recovery"],
        b"",
        ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl",
        other_target,
    );
    assert!(
        !refused_other_target.status.success(),
        "different target replay was accepted"
    );
    assert!(!fixture.root.join(other_target).exists());

    let initial_recipient = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(initial_recipient["next"]["role"], "lead");
    let lead_detail = fixture.json(&["work", "detail", &successor_work, "--json"], b"");
    assert!(lead_detail["actionForms"].get("beforeEditing").is_none());
    let scope_form = Fixture::choice(&lead_detail["actionForms"]["choices"], "scoped");
    let scope = fixture.execute_form(&scope_form, b"fresh scope for the recovered successor\n");
    assert!(scope.status.success(), "{}", display(&scope));

    let worker_detail = fixture.json(&["work", "detail", &successor_work, "--json"], b"");
    let pending = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(
        pending["next"]["packet"]["context"]["loop"]["state"],
        "evidence_required"
    );
    let evidence_path = fixture.root.join(".exitbind/artifacts/fresh.md");
    fs::write(evidence_path, b"new exact evidence\n").unwrap();
    let evidence_form = Fixture::choice(&worker_detail["actionForms"]["choices"], "evidence");
    assert_eq!(
        evidence_form["inputConstraints"]["ARTIFACT_ROOT"]["allowedValues"],
        json!(["product", "state"])
    );
    let evidence = fixture.execute_form(&evidence_form, b"");
    assert!(evidence.status.success(), "{}", display(&evidence));
    let after_evidence: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        after_evidence["governorCarry"]["accounting"]["state"],
        "blocked"
    );
    assert_eq!(after_evidence["governorCarrySha256"], carry_sha);
    assert_eq!(
        fs::read(fixture.root.join(&claim_path)).unwrap(),
        claim_bytes
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    let ready = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(ready["next"]["packet"]["context"]["loop"]["state"], "ready");
    assert!(ready["next"]["packet"]["context"]["loop"]["spent"] == 3);
    let ready_detail = fixture.json(&["work", "detail", &successor_work, "--json"], b"");
    let permit_form = &ready_detail["actionForms"]["beforeEditing"];
    assert!(permit_form.is_object());
    let argv = permit_form["command"]["argv"].as_array().unwrap();
    let request_index = argv
        .iter()
        .position(|argument| argument == "--request-id")
        .unwrap();
    assert_eq!(
        argv[request_index + 1],
        format!(
            "permit-{}",
            ready_detail["actionForms"]["binding"].as_str().unwrap()
        )
    );
    let permit = fixture.execute_form(permit_form, b"");
    assert!(permit.status.success(), "{}", display(&permit));
    let grant: Value = serde_json::from_slice(&permit.stdout).unwrap();
    assert_eq!(grant["allowed"], true);
    assert!(grant["currentGrant"].is_object());
    let granted = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(granted["next"]["packet"]["context"]["loop"]["spent"], 4);
    assert_eq!(
        granted["next"]["packet"]["context"]["loop"]["currentMutation"]["runId"],
        granted["next"]["packet"]["context"]["run"]["id"]
    );
    let granted_bytes = fs::read(fixture.root.join(&successor)).unwrap();
    let replay = fixture.execute_form(permit_form, b"");
    assert!(replay.status.success(), "{}", display(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["idempotent"], true);
    assert_eq!(
        fs::read(fixture.root.join(&successor)).unwrap(),
        granted_bytes
    );
    let replayed = fixture.json(&["work", "next", &successor_work, "--full"], b"");
    assert_eq!(replayed["next"]["packet"]["context"]["loop"]["spent"], 4);
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();

    // Altered protocol-2 marker evidence is refused on successor read.
    let successor_bytes = fs::read(fixture.root.join(&successor)).unwrap();
    let mut first: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    first.as_object_mut().unwrap().remove("governorCarry");
    first.as_object_mut().unwrap().remove("carryProtocol");
    first.as_object_mut().unwrap().remove("governorCarrySha256");
    rehash_event(&mut first);
    fs::write(
        fixture.root.join(&successor),
        format!("{}\n", serde_json::to_string(&first).unwrap()),
    )
    .unwrap();
    let stripped = fixture.call(&["run", "inspect", &successor], b"");
    assert!(
        !stripped.status.success(),
        "stripped recovery marker was accepted"
    );
    fs::write(fixture.root.join(&successor), &successor_bytes).unwrap();
    assert!(fixture
        .call(&["run", "inspect", &successor], b"")
        .status
        .success());

    // Even a correctly rehashed successor event cannot change the accounting
    // that the predecessor claim committed to.
    let mut changed: Value = serde_json::from_str(
        fs::read_to_string(fixture.root.join(&successor))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    changed["governorCarry"]["accounting"]["spent"] = json!(2);
    let carry_sha = value_sha256(&changed["governorCarry"]);
    changed["governorCarrySha256"] = json!(carry_sha);
    rehash_event(&mut changed);
    fs::write(
        fixture.root.join(&successor),
        format!("{}\n", serde_json::to_string(&changed).unwrap()),
    )
    .unwrap();
    let altered_accounting = fixture.call(&["run", "inspect", &successor], b"");
    assert!(
        !altered_accounting.status.success(),
        "rehash allowed carried accounting to diverge from the predecessor claim"
    );
    fs::write(fixture.root.join(&successor), &successor_bytes).unwrap();
}

#[test]
fn duplicate_blocked_protocol1_predecessor_refuses_direct_evidence_without_appending() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let artifact = fixture.root.join(".exitbind/artifacts/blocked.md");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(&artifact, b"evidence cannot reopen a blocked predecessor\n").unwrap();
    let assignment = detail["recipient"]["assignment"].as_str().unwrap();
    let attempt = fixture.call(
        &[
            "work",
            "evidence",
            &fixture.work,
            assignment,
            "--artifact-root",
            "state",
            "--artifact",
            ".exitbind/artifacts/blocked.md",
        ],
        b"",
    );
    assert!(
        !attempt.status.success(),
        "direct evidence reopened a blocked protocol-1 predecessor: {}",
        display(&attempt)
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    let inspected = fixture.json(&["run", "inspect", &fixture.old_ledger], b"");
    assert_eq!(inspected["governor"]["state"], "blocked");
}

#[test]
fn owner_recovery_rejects_changed_owner_decision_bindings_before_claim() {
    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    let successor = ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl";
    let claim = format!("{}.supersede", fixture.old_ledger);
    for case in [
        "unapproved",
        "malformed",
        "config",
        "predecessor",
        "assignment",
        "operation",
        "effect_digest",
    ] {
        fixture.write_recovery_artifacts(draft);
        let decision_path = fixture.root.join(".exitbind/artifacts/recovery.json");
        let mut decision: Value =
            serde_json::from_slice(&fs::read(&decision_path).unwrap()).unwrap();
        match case {
            "unapproved" => decision["approved"] = json!(false),
            "malformed" => {
                decision.as_object_mut().unwrap().remove("invocation");
            }
            "config" => decision["currentConfigSha256"] = json!("0".repeat(64)),
            "predecessor" => {
                decision["predecessor"]["headEventSha256"] = json!("0".repeat(64));
            }
            "assignment" => {
                decision["identity"]["assignment"] = json!(format!("smw_{}", "0".repeat(64)));
            }
            "operation" => decision["identity"]["operation"] = json!("different operation"),
            "effect_digest" => {
                decision["effectsEvidence"]["sha256"] = json!("0".repeat(64));
            }
            _ => unreachable!(),
        }
        fs::write(&decision_path, serde_json::to_vec(&decision).unwrap()).unwrap();
        let refused = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
        assert!(!refused.status.success(), "{case} decision was accepted");
        assert!(
            !fixture.root.join(successor).exists(),
            "{case} created a successor"
        );
        assert!(
            !fixture.root.join(&claim).exists(),
            "{case} created a predecessor claim"
        );
        assert_eq!(
            fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
            old_bytes
        );
        fixture.assert_prior_ledgers_unchanged();
    }
}

#[test]
fn owner_recovery_requires_a_complete_effect_inventory_and_exact_same_goal() {
    let fixture = Fixture::new();
    let (_old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    let decision_path = fixture.write_recovery_artifacts(draft);
    let effects = fixture.root.join(".exitbind/artifacts/effects.json");
    let mut inventory: Value = serde_json::from_slice(&fs::read(&effects).unwrap()).unwrap();
    inventory["inventory"]["processStarts"] = json!("unknown");
    let effects_bytes = serde_json::to_vec(&inventory).unwrap();
    fs::write(&effects, &effects_bytes).unwrap();
    let decision_file = fixture.root.join(".exitbind/artifacts/recovery.json");
    let mut decision: Value = serde_json::from_slice(&fs::read(&decision_file).unwrap()).unwrap();
    decision["effectsEvidence"]["sha256"] = json!(hex(Sha256::digest(&effects_bytes)));
    decision["effectsEvidence"]["bytes"] = json!(effects_bytes.len());
    fs::write(&decision_file, serde_json::to_vec(&decision).unwrap()).unwrap();
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
            ".exitbind/runs/refused-no-effects.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "unknown effect inventory was accepted"
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/refused-no-effects.jsonl")
        .exists());

    let decision_path = fixture.write_recovery_artifacts(draft);
    fs::write(
        fixture.root.join("late-product-change.md"),
        b"changed after the draft\n",
    )
    .unwrap();
    let stale_inputs = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            &fixture.goal,
            "--ledger",
            ".exitbind/runs/refused-stale-inputs.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !stale_inputs.status.success(),
        "stale product snapshot was accepted"
    );
    fs::remove_file(fixture.root.join("late-product-change.md")).unwrap();

    let different_goal = fixture.call(
        &[
            "run",
            "supersede",
            &fixture.old_ledger,
            "--workflow",
            "change",
            "--goal",
            "a changed goal must not use duplicate-permit recovery",
            "--ledger",
            ".exitbind/runs/refused-different-goal.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !different_goal.status.success(),
        "changed goal was accepted"
    );
}

#[cfg(unix)]
#[test]
fn owner_recovery_refuses_symlinked_effect_evidence_before_claiming_successor() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let (_old_bytes, detail) = fixture.block_duplicate_permit();
    let decision_path =
        fixture.write_recovery_artifacts(&detail["actionForms"]["recovery"]["ownerRecoveryDraft"]);
    let artifacts = fixture.root.join(".exitbind/artifacts");
    fs::rename(
        artifacts.join("effects.json"),
        artifacts.join("effects-target.json"),
    )
    .unwrap();
    symlink("effects-target.json", artifacts.join("effects.json")).unwrap();
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
            ".exitbind/runs/refused-symlink.jsonl",
            "--owner-recovery",
            &decision_path,
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "symlinked effects evidence was accepted"
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/refused-symlink.jsonl")
        .exists());
}

#[cfg(unix)]
#[test]
fn held_completion_blocks_owner_successor_without_changing_old_ledger_or_bytes() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let (old_bytes, detail) = fixture.block_duplicate_permit();
    let draft = &detail["actionForms"]["recovery"]["ownerRecoveryDraft"];
    fixture.write_recovery_artifacts(draft);
    let assignment = fixture.json(&["work", "next", &fixture.work, "--full"], b"")["next"]
        ["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ordinary_result = fixture.call(
        &[
            "work",
            "return",
            &fixture.work,
            &assignment,
            "--outcome",
            "completed",
        ],
        b"a completion after durable governor refusal",
    );
    assert!(
        !ordinary_result.status.success(),
        "blocked completion unexpectedly succeeded"
    );
    let held_dir = fixture.root.join(".exitbind/held");
    assert!(!held_dir.exists() || fs::read_dir(&held_dir).unwrap().next().is_none());

    let result_bytes = b"valid fault-injected held bytes for the exact assignment";
    let work_part = fixture.work.strip_prefix("smw_").unwrap();
    let assignment_sha = hex(Sha256::digest(assignment.as_bytes()));
    let bytes_sha = hex(Sha256::digest(result_bytes));
    let reference = format!("hld_{work_part}_{assignment_sha}_{bytes_sha}");
    fs::create_dir_all(&held_dir).unwrap();
    fs::set_permissions(&held_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let held_path = held_dir.join(format!("{reference}.bin"));
    fs::write(&held_path, result_bytes).unwrap();
    fs::set_permissions(&held_path, fs::Permissions::from_mode(0o600)).unwrap();

    // The previously emitted command is stale with respect to held ownership
    // and must refuse before claim/start, even though its evidence was valid.
    let refused = fixture.execute_form(&detail["actionForms"]["recovery"], b"");
    assert!(
        !refused.status.success(),
        "held result did not block successor admission"
    );
    let successor = ".exitbind/runs/work-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jsonl";
    assert!(!fixture.root.join(successor).exists());
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
    fixture.assert_prior_ledgers_unchanged();

    let after = fixture.json(&["work", "detail", &fixture.work, "--json"], b"");
    assert!(after["actionForms"].get("beforeEditing").is_none());
    let current = fixture.json(&["work", "next", &fixture.work, "--full"], b"");
    assert_eq!(current["next"]["held"]["sha256"], bytes_sha);
    assert_eq!(current["next"]["held"]["bytes"], result_bytes.len());
    assert_eq!(current["next"]["held"]["reference"], reference);
    assert_eq!(
        fs::read(held_dir.join(format!("{reference}.bin"))).unwrap(),
        result_bytes
    );
}

#[test]
fn frozen_old_reader_can_read_the_exact_blocked_work_detail_when_configured() {
    let Ok(old_binary) = std::env::var("EXITBIND_OWNER_RECOVERY_OLD_READER") else {
        eprintln!("COVERAGE UNAVAILABLE: set EXITBIND_OWNER_RECOVERY_OLD_READER to exercise the frozen 9098 executable against the blocked predecessor");
        return;
    };
    let fixture = Fixture::new();
    let (old_bytes, _) = fixture.block_duplicate_permit();
    let output = Command::new(old_binary)
        .current_dir(&fixture.root)
        .args([
            "work",
            "detail",
            &fixture.work,
            "--json",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", display(&output));
    let detail: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(detail["work"], fixture.work);
    assert_eq!(
        fs::read(fixture.root.join(&fixture.old_ledger)).unwrap(),
        old_bytes
    );
}

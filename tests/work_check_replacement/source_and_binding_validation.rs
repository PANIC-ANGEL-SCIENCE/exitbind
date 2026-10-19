use super::*;

#[test]
fn replacement_requires_current_architecture_source_before_claim() {
    let f = Fixture::new();
    let contract_path = f.root.join("architecture.json");
    let contract = include_bytes!("../fixtures/architecture-contract.json");
    fs::write(&contract_path, contract).unwrap();
    let contract_value: Value = serde_json::from_slice(contract).unwrap();
    let config_path = f.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["architectureContract"] = json!({
        "sourcePath": "architecture.json",
        "sourceSha256": format!("{:x}", Sha256::digest(contract)),
        "revision": contract_value["revision"],
    });
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();

    let (work, _) = f.begin_failed_check();
    let binding = f.ok(&["work", "next", &work, "--json"])["current"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    let preview = f.ok(&refs(&replacement_args(&work, &binding)));
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let before_runs = run_entries(&f);
    let before_ledger = fs::read(f.ledger(&work)).unwrap();
    fs::write(&contract_path, b"{}").unwrap();
    let refused = f.call(&refs(&apply));
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("architecture contract source changed"),
        "{refused:?}"
    );
    assert_eq!(run_entries(&f), before_runs);
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before_ledger);

    fs::write(&contract_path, contract).unwrap();
    let recovered = f.ok(&refs(&apply));
    assert_eq!(recovered["state"], "successor_created");
    let successor = recovered["successor"]["work"].as_str().unwrap();
    let successor_start: Value = serde_json::from_slice(
        fs::read(f.ledger(successor))
            .unwrap()
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    let predecessor_start: Value =
        serde_json::from_slice(before_ledger.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(successor_start["plan"], predecessor_start["plan"]);
    assert!(successor_start["plan"]["architectureContract"].is_object());
}

#[test]
fn nonexistent_cargo_test_target_is_recorded_and_continued_as_a_new_contract() {
    let f = Fixture::new();
    let package = f.root.join(".exitbind/r46-missing-target-package");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname = \"r46-missing-target-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(package.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();

    let (work, failed) = f.begin_failed_check_with(
        "cargo test --offline --manifest-path .exitbind/r46-missing-target-package/Cargo.toml --test absent_r46_target",
        "the configured test target is absent",
    );
    assert_eq!(
        failed["result"]["code"], 101,
        "an actual Cargo missing-target diagnostic is distinct from command-not-found (127)"
    );
    let before = fs::read(f.ledger(&work)).unwrap();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let preview = f.ok(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "touch replacement-check-must-not-run",
        "--reason",
        "The frozen command names a test target absent from this package.",
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
    ]);
    assert_eq!(
        preview["decision"]["source"]["eventSha256"],
        failed["eventSha256"]
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect::<Vec<_>>();
    let committed = f.ok(&apply);
    assert_eq!(committed["state"], "successor_created");
    assert!(
        !f.root.join("replacement-check-must-not-run").exists(),
        "continuation records the new test contract without running it"
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "the failed Cargo event and frozen policy remain byte-identical"
    );
}

#[test]
fn destructive_count_check_is_exercised_once_and_its_source_stays_immutable() {
    let f = Fixture::new();
    let evidence_dir = f.root.join(".exitbind/r46-destructive-count");
    fs::create_dir_all(&evidence_dir).unwrap();
    fs::write(evidence_dir.join("count-evidence"), b"1\n").unwrap();
    fs::write(
        evidence_dir.join("destroy-count.sh"),
        "rm -f .exitbind/r46-destructive-count/count-evidence\ntest \"$(cat .exitbind/r46-destructive-count/count-evidence)\" = 1\n",
    )
    .unwrap();
    let (work, failed) = f.begin_failed_check_with(
        "exec sh .exitbind/r46-destructive-count/destroy-count.sh",
        "the count checker destroys the evidence before reading it",
    );
    assert_eq!(failed["result"]["code"], 1);
    assert!(
        !evidence_dir.join("count-evidence").exists(),
        "the controlled reproduction records the check's known destructive side effect"
    );
    let before = fs::read(f.ledger(&work)).unwrap();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let preview = f.ok(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "touch replacement-check-must-not-run",
        "--reason",
        "The frozen count command deletes its own required input before counting.",
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
    ]);
    assert_eq!(
        preview["decision"]["source"]["eventSha256"],
        failed["eventSha256"]
    );
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect::<Vec<_>>();
    let committed = f.ok(&apply);
    assert_eq!(committed["state"], "successor_created");
    assert!(
        !f.root.join("replacement-check-must-not-run").exists(),
        "the successor checker is not run during replacement"
    );
    assert!(
        !evidence_dir.join("count-evidence").exists(),
        "replacement never replays the destructive predecessor check"
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "the predecessor keeps the single original failed check and its policy"
    );
}

#[test]
fn destructive_bound_input_observation_never_becomes_replacement_source_credit() {
    let f = Fixture::new();
    let evidence_dir = f.root.join(".exitbind/r46-destructive-bound-input");
    fs::create_dir_all(&evidence_dir).unwrap();
    fs::write(f.root.join("count-evidence"), b"1\n").unwrap();
    fs::write(
        evidence_dir.join("destroy-count.sh"),
        "printf x >> .exitbind/r46-destructive-bound-input/attempts\nrm -f count-evidence\ntest \"$(cat count-evidence)\" = 1\n",
    )
    .unwrap();

    let work = f.begin_completed_worker(
        "exec sh .exitbind/r46-destructive-bound-input/destroy-count.sh",
        "the checker mutates its bound product input before counting",
    );
    let failed = f.call(&["work", "check", &work, "--timeout-ms", "2000"]);
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    let failure: Value = serde_json::from_slice(&failed.stdout)
        .unwrap_or_else(|error| panic!("{error}: {failed:?}"));
    assert_eq!(
        failure["observationFailure"]["facts"]["diagnostic"],
        "observation binding changed before commit; no check was recorded"
    );
    assert_eq!(failure["observationFailure"]["facts"]["process"]["code"], 1);
    assert_eq!(
        failure["observationFailure"]["facts"]["termination"],
        "ended"
    );
    assert_eq!(
        failure["observationFailure"]["facts"]["storage"],
        "not_committed"
    );
    assert_eq!(
        String::from_utf8_lossy(&failed.stderr),
        "Session Goal: not requested | Result readiness: BLOCKED | Next: lead_decision\n"
    );
    assert_eq!(fs::read(evidence_dir.join("attempts")).unwrap(), b"x");
    assert!(!f.root.join("count-evidence").exists());

    let ledger_after_observation = fs::read(f.ledger(&work)).unwrap();
    let events: Vec<Value> = std::str::from_utf8(&ledger_after_observation)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "check_observation"),
        "the execution admission remains durable: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "check_observation_failed"),
        "the ended execution failure is durably classified: {events:?}"
    );
    assert!(
        !events.iter().any(|event| event["action"] == "check"),
        "input-mutating execution has no failed-check source credit: {events:?}"
    );

    let entries_before = run_entries(&f);
    let current = f.ok(&["work", "next", &work, "--json"]);
    let binding = current["current"]["binding"].as_str().unwrap();
    for apply in [false, true] {
        let mut args = replacement_args(&work, binding);
        if apply {
            args.push("--apply".into());
        }
        let refused = f.call(&refs(&args));
        assert_eq!(refused.status.code(), Some(1), "{refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains(
                "predecessor contains no exact failed-check or blocked-worker source evidence"
            ),
            "{stderr}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), ledger_after_observation);
        assert_eq!(
            run_entries(&f),
            entries_before,
            "no successor or claim is created"
        );
        assert_eq!(
            fs::read(evidence_dir.join("attempts")).unwrap(),
            b"x",
            "replacement must not replay the destructive check"
        );
    }
}

#[test]
fn stale_binding_and_non_defective_or_missing_sources_refuse_without_mutation() {
    let f = Fixture::new();
    fs::write(f.root.join("preserved-evidence"), b"retain").unwrap();
    let (work, source) = f.begin_failed_check();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap().to_owned();
    let before = fs::read(f.ledger(&work)).unwrap();

    let mut stale_args = replacement_args(&work, &"0".repeat(64));
    stale_args.push("--apply".into());
    let output = f.call(&refs(&stale_args));
    assert!(!output.status.success(), "stale binding: {output:?}");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    let preview = f.ok(&refs(&replacement_args(&work, &binding)));
    let mut wrong_class = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let class_index = wrong_class
        .iter()
        .position(|arg| arg == "--classification")
        .unwrap()
        + 1;
    wrong_class[class_index] = "ordinary-failure".into();
    assert!(
        !f.call(&refs(&wrong_class)).status.success(),
        "ordinary product failure is not defective-contract authorization"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    assert_eq!(
        fs::read(f.root.join("preserved-evidence")).unwrap(),
        b"retain"
    );
    assert!(source["eventSha256"].is_string());
    prepared_replacement_claim_round_trips_with_exact_legacy_and_pair_validation();
}

fn prepared_replacement_claim_round_trips_with_exact_legacy_and_pair_validation() {
    let f = Fixture::new();
    let config_path = f.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = json!({
        "version": 1,
        "pythonBytecode": "disabled",
        "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();

    let (work, _) = f.begin_failed_check();
    let binding = f.ok(&["work", "next", &work, "--json"])["current"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    let preview = f.ok(&refs(&replacement_args(&work, &binding)));
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok(&refs(&apply));
    let successor = applied["successor"]["work"].as_str().unwrap();
    let claim_path = format!("{}.supersede", f.ledger(&work).display());
    let claim: Value = serde_json::from_slice(&fs::read(&claim_path).unwrap()).unwrap();
    assert_eq!(claim["verificationPreparationProtocol"], 1);
    assert!(claim["verificationCheckerSources"].is_array());
    let successor_events = f.events(successor);
    assert_eq!(
        claim["verificationPreparationProtocol"],
        successor_events[0]["verificationPreparationProtocol"]
    );
    assert_eq!(
        claim["verificationCheckerSources"],
        successor_events[0]["verificationCheckerSources"],
        "the claim preserves the successor's exact checker-source binding, including a valid empty list"
    );
    let predecessor_detail = f.ok(&["work", "detail", &work, "--json"]);
    assert_eq!(predecessor_detail["work"], work);
    assert_eq!(
        predecessor_detail["checkReplacement"]["successor"]["work"],
        successor
    );
    assert_eq!(predecessor_detail["checkReplacement"]["state"], "verified");

    // Removing the optional pair from a prepared claim while its successor
    // remains prepared must invalidate lineage and refuse recovery.
    let mut stripped_claim = claim.clone();
    stripped_claim
        .as_object_mut()
        .unwrap()
        .remove("verificationPreparationProtocol");
    stripped_claim
        .as_object_mut()
        .unwrap()
        .remove("verificationCheckerSources");
    fs::write(&claim_path, serde_json::to_vec(&stripped_claim).unwrap()).unwrap();
    assert_eq!(
        f.ok(&["work", "detail", &work, "--json"])["checkReplacement"]["state"],
        "unverified"
    );
    let mut retry = replacement_args(&work, &binding);
    retry.push("--apply".into());
    let refused = f.call(&refs(&retry));
    assert!(!refused.status.success(), "{refused:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        diagnostic
            .contains("verification preparation differs from its persisted predecessor claim"),
        "{diagnostic}"
    );

    // A genuine pre-preparation successor has no pair in either the claim or
    // the successor start and remains readable as a legacy lineage.
    let legacy_fixture = Fixture::new();
    let (legacy_work, _) = legacy_fixture.begin_failed_check();
    let legacy_binding = legacy_fixture.ok(&["work", "next", &legacy_work, "--json"])["current"]
        ["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    let legacy_preview = legacy_fixture.ok(&refs(&replacement_args(&legacy_work, &legacy_binding)));
    let legacy_apply = legacy_preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let legacy_applied = legacy_fixture.ok(&refs(&legacy_apply));
    let legacy_successor = legacy_applied["successor"]["work"].as_str().unwrap();
    let legacy_claim_path = format!(
        "{}.supersede",
        legacy_fixture.ledger(&legacy_work).display()
    );
    let legacy_claim: Value =
        serde_json::from_slice(&fs::read(legacy_claim_path).unwrap()).unwrap();
    assert!(legacy_claim
        .get("verificationPreparationProtocol")
        .is_none());
    assert!(legacy_claim.get("verificationCheckerSources").is_none());
    let legacy_start = &legacy_fixture.events(legacy_successor)[0];
    assert!(legacy_start
        .get("verificationPreparationProtocol")
        .is_none());
    assert!(legacy_start.get("verificationCheckerSources").is_none());
    assert_eq!(
        legacy_fixture.ok(&["work", "detail", &legacy_work, "--json"])["checkReplacement"]["state"],
        "verified"
    );

    let mut invalid_claims = Vec::new();
    let mut partial = claim.clone();
    partial
        .as_object_mut()
        .unwrap()
        .remove("verificationCheckerSources");
    invalid_claims.push(("partial pair", partial));
    let mut bad_protocol = claim.clone();
    bad_protocol["verificationPreparationProtocol"] = json!(2);
    invalid_claims.push(("unsupported protocol", bad_protocol));
    let mut malformed_sources = claim.clone();
    malformed_sources["verificationCheckerSources"] = json!([{
        "path": "relative/checker.sh",
        "sha256": "0".repeat(64)
    }]);
    invalid_claims.push(("malformed checker sources", malformed_sources));
    let mut unknown_field = claim.clone();
    unknown_field["unexpected"] = json!(true);
    invalid_claims.push(("unknown field", unknown_field));

    for (label, invalid) in invalid_claims {
        fs::write(&claim_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let refused = f.call(&["work", "detail", &work, "--json"]);
        assert!(!refused.status.success(), "{label}: {refused:?}");
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&refused.stdout),
            String::from_utf8_lossy(&refused.stderr)
        );
        assert!(
            diagnostic.contains("invalid supersession claim"),
            "{label}: {diagnostic}"
        );
    }

    // A schema-valid optional checker list that differs from the committed
    // successor must invalidate lineage and block existing-successor recovery.
    let source_path = fs::canonicalize(f.root.join("checker.sh")).unwrap();
    let source_sha = format!("{:x}", Sha256::digest(fs::read(&source_path).unwrap()));
    let source_path = source_path.to_string_lossy().into_owned();
    let mut mismatched_sources = claim.clone();
    mismatched_sources["verificationCheckerSources"] = json!([{
        "path": source_path,
        "argumentPath": source_path,
        "sha256": source_sha
    }]);
    assert_ne!(
        mismatched_sources["verificationCheckerSources"],
        claim["verificationCheckerSources"]
    );
    let predecessor_bytes = fs::read(f.ledger(&work)).unwrap();
    let successor_bytes = fs::read(f.ledger(successor)).unwrap();
    fs::write(
        &claim_path,
        serde_json::to_vec(&mismatched_sources).unwrap(),
    )
    .unwrap();
    let detail = f.ok(&["work", "detail", &work, "--json"]);
    assert_eq!(detail["checkReplacement"]["state"], "unverified");
    let mut retry = replacement_args(&work, &binding);
    retry.push("--apply".into());
    let refused = f.call(&refs(&retry));
    assert!(!refused.status.success(), "{refused:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        diagnostic
            .contains("verification preparation differs from its persisted predecessor claim"),
        "{diagnostic}"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), predecessor_bytes);
    assert_eq!(fs::read(f.ledger(successor)).unwrap(), successor_bytes);

    // A well-formed claim whose claimed successor identity differs from the
    // actual successor is still refused by replacement recovery.
    let mut mismatched = claim;
    mismatched["newRunId"] = json!("0".repeat(64));
    fs::write(&claim_path, serde_json::to_vec(&mismatched).unwrap()).unwrap();
    let mut retry = replacement_args(&work, &binding);
    retry.push("--apply".into());
    let refused = f.call(&refs(&retry));
    assert!(!refused.status.success(), "{refused:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        diagnostic.contains("successor claim provenance mismatch"),
        "{diagnostic}"
    );
}

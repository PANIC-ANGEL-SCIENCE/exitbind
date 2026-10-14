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
}

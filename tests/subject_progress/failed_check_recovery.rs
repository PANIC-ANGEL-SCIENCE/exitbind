use super::*;

#[test]
fn evidence_after_failed_check_rework_carries_the_current_identity() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "evidence after failed check rework",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let mut next = fixture.value(&["work", "next", &work], None);
    let first = fixture.value(
        &[
            "work",
            "return",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first attempt"),
    );
    assert_eq!(first["next"]["action"], "check");
    let failed = fixture.recorded_check_failure(&work);
    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            failed["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"repair requested"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    next = reworked;

    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let second = fixture.value(
        &[
            "work",
            "return",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"second attempt"),
    );
    assert_eq!(second["next"]["action"], "check", "{second}");
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    next = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"continue repair"),
    );
    assert_eq!(next["next"]["role"], "worker");
    next = fixture.value(
        &[
            "work",
            "replan",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--hypothesis",
            "repair the next worker attempt",
        ],
        None,
    );
    assert_eq!(next["next"]["role"], "worker");
    let third = fixture.value(
        &[
            "work",
            "return",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"third attempt"),
    );
    assert_eq!(third["next"]["action"], "check", "{third}");
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    next = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"continue repair"),
    );
    assert_eq!(next["next"]["role"], "worker");
    fs::write(fixture.root.join("pass-marker"), b"updated after check").unwrap();

    let ledger = fixture.ledger(&work);

    let evidence = fixture.value(
        &[
            "work",
            "evidence",
            &work,
            next["next"]["assignment"].as_str().unwrap(),
            "--artifact",
            "pass-marker",
        ],
        None,
    );
    assert_eq!(
        evidence["event"]["governorEvent"]["identityTransition"],
        "carried_mutation_v1"
    );
    assert_eq!(evidence["next"]["action"], "spawn");

    let mut events: Vec<Value> = fs::read_to_string(fixture.root.join(ledger.clone()))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let last = events.last_mut().unwrap();
    let forged_subject = "f".repeat(64);
    last["subjectSha256"] = serde_json::json!(forged_subject);
    let governor = last["governorEvent"].as_object_mut().unwrap();
    governor["subjectSha256"] = serde_json::json!(forged_subject);
    governor["evidence"]["subjectSha256"] = serde_json::json!(forged_subject);
    let governor = Value::Object(governor.clone());
    last["governorEvent"] = rehash(governor);
    let forged = rehash(last.clone());
    *events.last_mut().unwrap() = forged;
    let forged_ledger = ".exitbind/runs/forged-evidence-identity.jsonl";
    fs::write(
        fixture.root.join(forged_ledger),
        events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let rejected = fixture.call(&["run", "inspect", forged_ledger, "--json"], None);
    assert!(
        !rejected.status.success(),
        "forged evidence subject was accepted"
    );
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&rejected.stdout),
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(
        diagnostic.contains("stale run identity")
            || diagnostic.contains("stale or mismatched for subjectSha256"),
        "{diagnostic}"
    );

    let mut input_events: Vec<Value> = fs::read_to_string(fixture.root.join(&ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let input_last = input_events.last_mut().unwrap();
    input_last["governorEvent"]["evidence"]["inputSha256"] = serde_json::json!("e".repeat(64));
    let governor = Value::Object(input_last["governorEvent"].as_object().unwrap().clone());
    input_last["governorEvent"] = rehash(governor);
    *input_events.last_mut().unwrap() = rehash(input_last.clone());
    let forged_input_ledger = ".exitbind/runs/forged-evidence-input.jsonl";
    fs::write(
        fixture.root.join(forged_input_ledger),
        input_events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let rejected_input = fixture.call(&["run", "inspect", forged_input_ledger, "--json"], None);
    assert!(
        !rejected_input.status.success(),
        "forged evidence input was accepted"
    );
}

#[test]
fn preservation_without_functional_policy_is_refused_at_creation_and_replay() {
    let fixture = Fixture::new_single();
    let refused = fixture.call(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "preserve without a functional check",
            "--ledger",
            ".exitbind/runs/unchecked.jsonl",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "true",
        ],
        None,
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("checked run"),
        "{refused:?}"
    );
    assert!(!fixture.root.join(".exitbind/runs/unchecked.jsonl").exists());

    // Replay: a start event that claims preservation but no checkPolicy is
    // rejected even when its hash chain is internally valid.
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "true",
        ],
        None,
    );
    let ledger = fixture.ledger(begin["work"].as_str().unwrap());
    let text = fs::read_to_string(fixture.root.join(ledger)).unwrap();
    let mut start: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(start["version"], 8);
    start.as_object_mut().unwrap().remove("checkPolicy");
    let forged = ".exitbind/runs/forged.jsonl";
    fs::write(
        fixture.root.join(forged),
        format!("{}\n", serde_json::to_string(&rehash(start)).unwrap()),
    )
    .unwrap();
    let replay = fixture.call(&["run", "inspect", forged, "--json"], None);
    assert!(!replay.status.success(), "{replay:?}");
    assert!(
        String::from_utf8_lossy(&replay.stdout).contains("preservation requires checkPolicy"),
        "{replay:?}"
    );
}

#[test]
fn validated_packet_reuses_unchanged_evidence_without_rerunning_checks() {
    let fixture = Fixture::new_single();
    let work = prepared_for_acceptance(&fixture);
    let packet = save_packet(&fixture, &work, "packet.json");
    let saved: Value = serde_json::from_slice(&fs::read(&packet).unwrap()).unwrap();
    assert_eq!(saved["version"], 2);
    assert!(saved["snapshot"]["inputsSha256"].is_string());
    assert_eq!(saved["humanHelp"]["nextAction"]["actor"], "lead");
    assert_eq!(saved["humanHelp"]["ownerDecision"], "not_required");

    let consumed = consume(&fixture, &work, &packet);
    assert_eq!(
        (consumed.result.as_str(), consumed.reason.as_str()),
        ("usable", "current")
    );
    assert!(consumed.executed.is_empty());
    assert_eq!(fixture.count("functional.count"), "1\n");
    assert_eq!(fixture.count("preservation.count"), "1\n");
    let usable = fixture.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        usable["packet"], saved,
        "usable still returns the canonical packet"
    );

    // Unsupported, contradictory, and oversized packets never permit skipping.
    let mut contradictory = saved.clone();
    contradictory["doNotRepeat"]
        .as_array_mut()
        .unwrap()
        .push(Value::from("implementation"));
    let mut unsupported = saved.clone();
    unsupported["version"] = Value::from(1);
    let mut goal = saved.clone();
    goal["goal"] = Value::from("drop the precedence requirement");
    let mut subject = saved.clone();
    subject["currentSubject"] = serde_json::json!({"sha256": "0".repeat(64)});
    let mut remaining = saved.clone();
    remaining["remaining"] = serde_json::json!([]);
    let mut next_action = saved.clone();
    next_action["next"] = Value::from("done");
    let mut missing = saved.clone();
    missing.as_object_mut().unwrap().remove("stillValid");
    for (mutated, expected) in [
        (contradictory, "claims_disagree_with_state"),
        (unsupported, "unsupported_packet_version"),
        (goal, "claims_disagree_with_state"),
        (subject, "claims_disagree_with_state"),
        (remaining, "claims_disagree_with_state"),
        (next_action, "claims_disagree_with_state"),
        (missing, "malformed_packet"),
    ] {
        let path = fixture.counter("mutated.json");
        fs::write(&path, serde_json::to_vec(&mutated).unwrap()).unwrap();
        let verdict = fixture.value(
            &[
                "work",
                "validate",
                &work,
                "--packet",
                path.to_str().unwrap(),
            ],
            None,
        );
        assert_ne!(verdict["result"], "usable");
        assert_eq!(verdict["reason"], expected);
        assert_eq!(verdict["reuse"], serde_json::json!([]));
        assert_eq!(verdict["packet"]["version"], 2);
    }
    let huge = fixture.counter("huge.json");
    fs::write(&huge, vec![b' '; 300 * 1024]).unwrap();
    let refused = fixture.call(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            huge.to_str().unwrap(),
        ],
        None,
    );
    assert!(!refused.status.success());

    let done = fixture.value(&["work", "next", &work], None);
    let accepted = fixture.value(
        &[
            "work",
            "return",
            &work,
            done["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert_eq!(fixture.count("functional.count"), "1\n");
}

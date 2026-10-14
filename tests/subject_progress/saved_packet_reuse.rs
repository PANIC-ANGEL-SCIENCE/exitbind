use super::*;

#[test]
fn invalid_or_authority_conflicting_saved_packet_cannot_be_reused() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "reject invalid saved packets",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    let packet = fixture.counter("invalid-packet.json");
    fs::write(&packet, b"not json\n").unwrap();
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let invalid = fixture.call(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert!(invalid.status.success(), "{invalid:?}");
    let invalid: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(invalid["result"], "cannot_establish_applicability");
    assert_eq!(invalid["reason"], "packet_not_an_object");
    assert_eq!(invalid["reuse"], serde_json::json!([]));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);

    let current = fixture.value(&["work", "next", &work], None)["residual"].clone();
    let mut conflicting = current;
    conflicting["basis"] = serde_json::json!({"sha256": "f".repeat(64)});
    fs::write(&packet, serde_json::to_vec(&conflicting).unwrap()).unwrap();
    let verdict = fixture.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(verdict["result"], "refresh_required");
    assert_eq!(verdict["reason"], "claims_disagree_with_state");
    assert_eq!(verdict["reuse"], serde_json::json!([]));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);

    let current = fixture.value(&["work", "next", &work], None)["residual"].clone();
    let mut conflicting_policy = current;
    conflicting_policy["reviewPolicy"] = serde_json::json!({
        "version": 1,
        "decision": "required",
        "source": "forged",
        "reason": "conflicting policy",
        "previousSha256": null,
        "sha256": "f".repeat(64)
    });
    fs::write(&packet, serde_json::to_vec(&conflicting_policy).unwrap()).unwrap();
    let verdict = fixture.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(verdict["result"], "refresh_required");
    assert_eq!(verdict["reason"], "claims_disagree_with_state");
    assert_eq!(verdict["reuse"], serde_json::json!([]));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn source_or_checker_edit_without_new_submission_invalidates_reuse_and_acceptance() {
    for edited in ["src/config.txt", "checks/preserve.sh"] {
        let fixture = Fixture::new_single();
        let work = prepared_for_acceptance(&fixture);
        let packet = save_packet(&fixture, &work, "packet.json");
        let validated = consume(&fixture, &work, &packet);
        assert_eq!(validated.result, "usable", "{edited}");

        // Change after validation, before acceptance: the earlier validation
        // does not authorize acceptance.
        let mut bytes = fs::read(fixture.root.join(edited)).unwrap();
        bytes.extend_from_slice(b"# edited\n");
        fs::write(fixture.root.join(edited), &bytes).unwrap();
        fs::write(
            fixture.root.join(".exitbind/artifacts/lead.md"),
            b"accept\n",
        )
        .unwrap();
        let ledger = fixture.ledger(&work);
        let premature =
            fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
        assert!(!premature.status.success(), "{edited}: {premature:?}");
        let next = fixture.value(&["work", "next", &work], None);
        assert_eq!(next["next"]["action"], "check", "{edited}");
        assert!(next["residual"]["humanHelp"]["whatHappened"]
            .as_str()
            .unwrap()
            .contains("no longer applies"));

        // A fresh consumer with the old packet is told to refresh and obtains
        // real new executions before any acceptance.
        let consumed = consume(&fixture, &work, &packet);
        assert_eq!(
            (consumed.result.as_str(), consumed.reason.as_str()),
            ("refresh_required", "tested_inputs_changed"),
            "{edited}"
        );
        assert_eq!(consumed.executed.len(), 2, "{edited}");
        assert_eq!(fixture.count("functional.count"), "2\n", "{edited}");
        assert_eq!(fixture.count("preservation.count"), "2\n", "{edited}");

        // The review approved other tested files, so it is not reused.
        let stale_review = fixture.value(&["work", "next", &work], None);
        assert_eq!(stale_review["next"]["action"], "lead_decision");
        assert!(!stale_review["residual"]["doNotRepeat"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "review"));
        let refused =
            fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
        assert!(!refused.status.success(), "{edited}: stale review accepted");
        let old_packet = consume(&fixture, &work, &packet);
        assert_eq!(old_packet.result, "refresh_required");
        assert!(old_packet.executed.is_empty());

        fixture.value(
            &[
                "work",
                "return",
                &work,
                stale_review["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "rework",
            ],
            Some(b"review is stale"),
        );
        let review_requests = drive_to_lead_decision(&fixture, &work);
        assert_eq!(review_requests, 1, "{edited}");
        let decision = fixture.value(&["work", "next", &work], None);
        let accepted = fixture.value(
            &[
                "work",
                "return",
                &work,
                decision["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "accepted",
            ],
            Some(b"accept"),
        );
        assert_eq!(accepted["next"]["progress"]["state"], "READY", "{edited}");
    }
}

#[test]
fn changed_preservation_policy_cannot_inherit_predecessor_evidence() {
    let fixture = Fixture::new_single();
    let work = prepared_for_acceptance(&fixture);
    let ledger = fixture.ledger(&work);
    let changed = fixture.call(
        &[
            "run",
            "supersede",
            &ledger,
            "--workflow",
            "change",
            "--goal",
            "simplify configuration",
            "--ledger",
            ".exitbind/runs/successor.jsonl",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:file wins over environment",
            "--preservation-check-command",
            "true",
            "--json",
        ],
        None,
    );
    assert!(!changed.status.success(), "{changed:?}");
    assert!(
        String::from_utf8_lossy(&changed.stdout).contains("differs"),
        "{changed:?}"
    );
}

/// Cross-version read compatibility against a pinned `v0.18.0` binary. The
/// binary is supplied by path so the user's installation is never touched; the
/// test is skipped (and says so) when it is not provided.
#[test]
fn pinned_v0_18_reader_refuses_v6_and_current_reader_keeps_v5_guarantees() {
    let Some(old) = std::env::var_os("EXITBIND_V018_BIN") else {
        eprintln!("skipped: set EXITBIND_V018_BIN to a v0.18.0 exitbind binary");
        return;
    };
    let old = PathBuf::from(old);
    let run_with =
        |binary: &std::path::Path, root: &std::path::Path, args: &[&str], input: Option<&[u8]>| {
            let mut command = Command::new(binary);
            command
                .current_dir(root)
                .args(args)
                .arg("--config")
                .arg(root.join("exitbind.json"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.unwrap_or_default())
                .unwrap();
            child.wait_with_output().unwrap()
        };
    let json = |output: Output| -> Value {
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    };

    // Old reader, new ledger: explicit refusal, never partial acceptance.
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "v6",
            "--check-command",
            "true",
        ],
        None,
    );
    let ledger = fixture.ledger(begin["work"].as_str().unwrap());
    let refused = run_with(
        &old,
        &fixture.root,
        &["run", "inspect", &ledger, "--json"],
        None,
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stdout).contains("invalid event header"),
        "{refused:?}"
    );

    // Old writer, current reader: a v5 run at its final lead decision stays
    // readable, projects no tested-input claim, and cannot be validated as
    // input-applicable.
    let legacy = Fixture::new_single();
    let work = json(run_with(
        &old,
        &legacy.root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "v5",
            "--check-command",
            "true",
        ],
        None,
    ))["work"]
        .as_str()
        .unwrap()
        .to_owned();
    loop {
        let next =
            json(run_with(&old, &legacy.root, &["work", "next", &work], None))["next"].clone();
        match next["action"].as_str().unwrap() {
            "check" => {
                json(run_with(
                    &old,
                    &legacy.root,
                    &["work", "check", &work],
                    None,
                ));
            }
            "spawn" | "lead_decision" if next["outcomes"][0] != "accepted" => {
                let outcome = match next["role"].as_str().unwrap() {
                    "lead" => "scoped",
                    "reviewer" => "approved",
                    _ => "completed",
                };
                json(run_with(
                    &old,
                    &legacy.root,
                    &[
                        "work",
                        "return",
                        &work,
                        next["assignment"].as_str().unwrap(),
                        "--outcome",
                        outcome,
                    ],
                    Some(b"legacy"),
                ));
            }
            _ => break,
        }
    }
    let legacy_ledger = legacy.ledger(&work);
    let events = fs::read_to_string(legacy.root.join(&legacy_ledger)).unwrap();
    assert!(events
        .lines()
        .all(|line| serde_json::from_str::<Value>(line).unwrap()["version"] == 5));
    let current = legacy.value(&["work", "next", &work], None);
    assert_eq!(current["next"]["action"], "lead_decision");
    assert!(current["residual"]["snapshot"]["inputsSha256"].is_null());
    assert_eq!(
        current["residual"]["invalidation"]["testedInputChangeInvalidates"],
        false
    );
    let packet = legacy.counter("legacy-packet.json");
    fs::write(&packet, serde_json::to_vec(&current["residual"]).unwrap()).unwrap();
    let verdict = legacy.value(
        &[
            "work",
            "validate",
            &work,
            "--packet",
            packet.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(verdict["result"], "cannot_establish_applicability");
    assert_eq!(verdict["reason"], "tested_inputs_not_bound");
    let accepted = legacy.value(
        &[
            "work",
            "return",
            &work,
            current["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert!(fs::read_to_string(legacy.root.join(&legacy_ledger))
        .unwrap()
        .lines()
        .all(|line| serde_json::from_str::<Value>(line).unwrap()["version"] == 5));
}

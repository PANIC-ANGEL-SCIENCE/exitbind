use super::*;

#[test]
fn preservation_requirement_is_separate_from_functional_check_and_resume_reuses_it() {
    let fixture = Fixture::new_single();
    let functional = fixture.counting_command("functional.count");
    let preservation = fixture.counting_command("preservation.count");
    let (functional, preservation) = (functional.as_str(), preservation.as_str());
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve accepted precedence",
            "--check-command",
            functional,
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
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
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    assert_eq!(completed["next"]["check"]["kind"], "check");

    let functional_checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(functional_checked["next"]["action"], "check");
    assert_eq!(functional_checked["next"]["check"]["kind"], "preservation");
    assert_eq!(
        functional_checked["next"]["check"]["requirementId"],
        "precedence"
    );

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    assert!(fixture
        .submit_low_level(
            "reviewer",
            &ledger,
            "approved",
            ".exitbind/artifacts/reviewer.md",
        )
        .status
        .success());
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let premature =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(
        !premature.status.success(),
        "accepted without preservation check: {premature:?}"
    );
    let premature_text = format!(
        "{}{}",
        String::from_utf8_lossy(&premature.stdout),
        String::from_utf8_lossy(&premature.stderr)
    );
    assert!(
        premature_text.contains("preservation_missing"),
        "{premature:?}"
    );

    let preserved = fixture.value(&["work", "check", &work], None);
    assert_eq!(fixture.count("functional.count"), "1\n");
    assert_eq!(fixture.count("preservation.count"), "1\n");
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(fixture.count("functional.count"), "1\n");
    assert_eq!(fixture.count("preservation.count"), "1\n");
    assert!(resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "preservation" && item["requirementId"] == "precedence"));
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "preservation:precedence"));
    assert_eq!(preserved["next"]["action"], "lead_decision");
    let premature = fixture.call(
        &[
            "work",
            "return",
            &work,
            preserved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(!premature.status.success(), "{premature:?}");
    let premature_text = format!(
        "{}{}",
        String::from_utf8_lossy(&premature.stdout),
        String::from_utf8_lossy(&premature.stderr)
    );
    assert!(
        premature_text.contains("canonical acceptance requires reviewer approval"),
        "{premature:?}"
    );

    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            preserved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"fresh implementation"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    fixture.value(&["work", "check", &work], None);
    let preserved_again = fixture.value(&["work", "check", &work], None);
    assert_eq!(fixture.count("functional.count"), "2\n");
    assert_eq!(fixture.count("preservation.count"), "2\n");
    assert_eq!(preserved_again["next"]["role"], "reviewer");
    let reviewed = fixture.value(
        &[
            "work",
            "return",
            &work,
            preserved_again["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"fresh review"),
    );
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            reviewed["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

#[test]
fn failed_preservation_blocks_acceptance_without_claiming_functional_failure() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "detect weakened requirement",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "test ! -f preservation-fail",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
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
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    fs::write(fixture.root.join("preservation-fail"), b"weakened").unwrap();
    fixture.value(&["work", "check", &work], None);
    let failed = fixture.recorded_check_failure(&work);
    assert_eq!(failed["next"]["requiresExpansion"], true);
    assert_eq!(failed["next"]["action"], "lead_decision");
    assert_eq!(failed["next"]["role"], "lead");
    let failed_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][1]["status"],
        "failed"
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][1]["requirementId"],
        "precedence"
    );

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    let reviewer = fixture.submit_low_level(
        "reviewer",
        &ledger,
        "approved",
        ".exitbind/artifacts/reviewer.md",
    );
    assert!(
        !reviewer.status.success(),
        "reviewer approval bypassed failed check"
    );
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let refused =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(!refused.status.success(), "{refused:?}");
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("preservation_failed"), "{refused:?}");
}

#[test]
fn residual_packet_does_not_invent_review_before_initial_scope() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "initial lead resume",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    assert_eq!(begin["next"]["action"], "lead_decision");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], begin["work"]);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert!(resumed["next"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "scoped"));

    assert!(!resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "current_review" && item["status"] == "approved"));
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));
    assert!(!resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "lead_acceptance"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "scope"));
}

#[test]
fn residual_packet_keeps_stale_subject_evidence_out_of_reuse() {
    let fixture = Fixture::new();
    let check = "test -f marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "subject invalidation",
            "--check-command",
            check,
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
    let worker_a = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_a["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker A"),
    );
    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    fixture.value(&["work", "check", &work], None);
    let worker_b = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_b["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker B"),
    );

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "check");
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "check"));
}

#[test]
fn preservation_checks_rerun_after_rework_changes_subject() {
    let fixture = Fixture::new_single();
    let functional = fixture.counting_command("functional.count");
    let preservation = fixture.counting_command("preservation.count");
    let (functional, preservation) = (functional.as_str(), preservation.as_str());
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preservation recheck after rework",
            "--check-command",
            functional,
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            preservation,
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
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first implementation"),
    );
    fixture.value(&["work", "check", &work], None);
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"needs another implementation"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"second implementation"),
    );

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "check");
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check" || item == "preservation:precedence"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "check"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "preservation" && item["requirementId"] == "precedence"));

    fixture.value(&["work", "check", &work], None);
    fixture.value(&["work", "check", &work], None);
    assert_eq!(fixture.count("functional.count"), "2\n");
    assert_eq!(fixture.count("preservation.count"), "2\n");
}

#[test]
fn failed_check_surfaces_rework_and_requires_fresh_acceptance_path() {
    let fixture = Fixture::new_single();
    let check = "test -f pass-marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "failed check rework",
            "--check-command",
            check,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    let scoped = fixture.value(
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
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first attempt"),
    );
    assert_progress(&scoped["next"]);
    assert_eq!(completed["next"]["action"], "check");

    let failed = fixture.recorded_check_failure(&work);
    assert_eq!(failed["next"]["action"], "lead_decision");
    assert_eq!(failed["next"]["role"], "lead");
    let failed_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        failed["next"]["resolvedActor"],
        failed_next["residual"]["humanHelp"]["nextAction"]["actor"]
    );
    assert_eq!(
        failed_next["next"]["packet"]["context"]["next"]["owner"],
        failed["next"]["resolvedActor"]
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"][0]["status"],
        "failed"
    );
    assert_eq!(
        failed_next["next"]["packet"]["checkEvidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let failed_ledger = fs::read(fixture.root.join(&ledger)).unwrap();

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
    assert_eq!(reworked["next"]["action"], "spawn");
    assert_eq!(reworked["next"]["role"], "worker");
    let reworked_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(reworked_next["next"]["packet"]["attempt"], 2);
    let after_rework = fs::read(fixture.root.join(&ledger)).unwrap();
    assert!(after_rework.starts_with(&failed_ledger));

    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let fresh_completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            reworked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh worker result"),
    );
    assert_eq!(fresh_completed["next"]["action"], "check");
    let fresh_check = fixture.value(&["work", "check", &work], None);
    assert_eq!(fresh_check["next"]["action"], "spawn");
    assert_eq!(fresh_check["next"]["role"], "reviewer");
    let fresh_next = fixture.value(&["work", "next", &work], None);
    assert_eq!(
        fresh_next["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );

    let approved = fixture.value(
        &[
            "work",
            "return",
            &work,
            fresh_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"fresh review"),
    );
    assert_eq!(approved["next"]["action"], "lead_decision");
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            approved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"fresh acceptance"),
    );
    assert_eq!(done["next"]["action"], "done");
    let final_state = fixture.value(&["work", "next", &work], None);
    assert_eq!(final_state["next"]["progress"]["percent"], 100);
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

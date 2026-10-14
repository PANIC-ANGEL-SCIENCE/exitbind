use super::*;

#[test]
fn work_return_reports_protection_event_as_recorded() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "protected acceptance",
            "--check-command",
            "false",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let _lead = fixture.value(
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
    let _completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker"),
    );
    let check = fixture.recorded_check_failure(&work);
    assert_eq!(check["next"]["action"], "lead_decision");
    assert_eq!(check["next"]["role"], "lead");
    let accepting_lead = fixture.value(&["work", "next", &work], None);
    let output = fixture.call(
        &[
            "work",
            "return",
            &work,
            accepting_lead["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["effect"], "recorded");
    assert_eq!(value["reason"]["code"], "recorded_then_failed");
    assert_eq!(value["nextAction"]["type"], "inspect");
    assert_eq!(value["nextAction"]["safe"], true);
    assert_eq!(value["event"]["action"], "protect");
    assert_eq!(
        value["reference"]["headEventSha256"],
        value["event"]["eventSha256"]
    );
    assert_eq!(
        value["reference"]["eventSha256"],
        value["event"]["eventSha256"]
    );
    assert!(value["nextAction"]["command"].is_array());

    let accepting_again = fixture.value(&["work", "next", &work], None);
    let second = fixture.call(
        &[
            "work",
            "return",
            &work,
            accepting_again["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept again"),
    );
    assert!(second.status.success(), "{second:?}");
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["effect"], "recorded");
    assert_eq!(second["event"]["action"], "protect");
    assert_ne!(
        value["event"]["eventSha256"],
        second["event"]["eventSha256"]
    );
    assert_eq!(
        second["reference"]["eventSha256"],
        second["event"]["eventSha256"]
    );
    assert_eq!(
        second["reference"]["headEventSha256"],
        second["event"]["eventSha256"]
    );
}

#[test]
fn work_return_stale_assignment_does_not_report_recorded() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "stale assignment",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap();
    let ledger = fixture.ledger(work);
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let output = fixture.call(
        &[
            "work",
            "return",
            work,
            "sma_0000000000000000000000000000000000000000000000000000000000000000",
            "--outcome",
            "scoped",
        ],
        Some(b"stale"),
    );
    assert!(!output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("recorded"));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn skill_presentation_is_exact_and_packaged_copy_matches() {
    const ROUTINE: &str = "[Neuro] Exitbind progress: N%.";
    let canonical = include_bytes!("../../skills/exitbind/SKILL.md");
    let packaged = include_bytes!("../../plugins/exitbind/skills/exitbind/SKILL.md");
    assert_eq!(canonical, packaged);
    let text = std::str::from_utf8(canonical).unwrap();
    assert!(!text.contains(ROUTINE));
    // Human status is product-owned; no model remembers or formats progress.
    assert!(!text.contains("Neuro\nExitbind progress"));
    assert!(text.contains("product's English `goalProgress.systemText`"));
    assert!(text.contains("a model must not render the legacy\npercentage"));
    assert!(text.contains("Supported native and host paths surface that text automatically"));
    assert!(text.contains("remember a progress command"));
    // The product supplies the terminal block and the host copies it without
    // rebuilding it from state or progress.
    assert!(text.contains(
        "carries a non-null `terminal`, that value is the terminal\nstatus block: print it on a line of its own, exactly as given"
    ));
    assert!(text.contains("Never\nreconstruct terminal wording from `exitState` or `progress`."));
    assert!(text.contains("Do not add a routine preservation progress report."));
    assert!(
        text.contains("keep one handle\nand wait for completion or a meaningful state transition")
    );
    assert!(
        text.contains("A timeout is not a\nfailure and never authorizes restarting the command")
    );
}

#[test]
fn skill_keeps_readiness_lead_owned_and_preservation_narrow() {
    let text = std::str::from_utf8(include_bytes!("../../skills/exitbind/SKILL.md"))
        .unwrap()
        .to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for contract in [
        "complex but has no material semantic-preservation risk",
        "does not add a preservation route",
        "tool and agent selection remains the lead's responsibility",
        "clarify the accepted behavior, invariants, allowed changes",
        "freeze that accepted meaning",
        "preserve the frozen meaning",
        "read back the same accepted meaning",
        "current implementation subject",
        "trivial work has no preservation ceremony",
        "minimizer may reduce mechanism",
        "does not choose the goal, tools, or agents",
        "does not own final acceptance",
        "does not create a second ledger",
        "separate preservation installation",
        "preservation_missing",
        "preservation_failed",
        "do not install a separate preservation tool for this path",
    ] {
        assert!(
            text.contains(contract),
            "missing Exitbind skill contract: {contract}"
        );
    }
    assert!(!text.contains("coffee"));
    assert!(!text.contains("holytail chooses tools"));
}

#[test]
fn work_facade_surfaces_stale_worker_and_requires_fresh_evidence() {
    let fixture = Fixture::new();
    let check = "test -f marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "subject recovery",
            "--check-command",
            check,
        ],
        None,
    );
    assert_progress(&begin["next"]);
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);

    let _scope = fixture.value(
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
    assert_progress(&worker_a["next"]);
    let worker_a_return = fixture.value(
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
    let worker_a_next = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_a_next["next"]);
    assert_eq!(worker_a_return["next"]["action"], "check");

    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    let checked_a = fixture.value(&["work", "check", &work], None);
    let worker_b = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_b["next"]);
    assert_eq!(checked_a["next"]["action"], "spawn");
    let worker_b_return = fixture.value(
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
    let worker_b_next = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_b_next["next"]);
    assert_eq!(worker_b_return["next"]["action"], "check");

    let events: Vec<Value> = fs::read_to_string(fixture.root.join(&ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let worker_b_event = events
        .iter()
        .find(|event| event["action"] == "submit" && event["agent"] == "worker_two")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let checked_b = fixture.call(
        &[
            "run",
            "record-check",
            &ledger,
            "--target",
            &worker_b_event,
            "--check-command",
            check,
            "--exit-code",
            "0",
            "--json",
        ],
        None,
    );
    assert!(checked_b.status.success(), "{checked_b:?}");

    let stale = fixture.value(&["work", "next", &work], None);
    assert_progress(&stale["next"]);
    assert_eq!(stale["next"]["action"], "check");

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
    assert!(reviewer.status.success(), "{reviewer:?}");
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let premature =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(
        !premature.status.success(),
        "stale A check was accepted: {premature:?}"
    );

    let recovered_check = fixture.value(&["work", "check", &work], None);
    let recovered_next = fixture.value(&["work", "next", &work], None);
    assert_progress(&recovered_next["next"]);
    assert_eq!(recovered_check["next"]["action"], "lead_decision");
    let stale_acceptance = fixture.call(
        &[
            "work",
            "return",
            &work,
            recovered_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert!(!stale_acceptance.status.success(), "{stale_acceptance:?}");
    let stale_acceptance_text = format!(
        "{}{}",
        String::from_utf8_lossy(&stale_acceptance.stdout),
        String::from_utf8_lossy(&stale_acceptance.stderr)
    );
    assert!(
        stale_acceptance_text.contains("canonical acceptance requires reviewer approval"),
        "{stale_acceptance:?}"
    );

    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            recovered_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"fresh worker repair"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let fresh_worker = fixture.value(&["work", "next", &work], None);
    let held = fixture.value(
        &[
            "work",
            "return",
            &work,
            fresh_worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh worker continuation"),
    );
    assert_eq!(held["effect"], "held", "{held}");
    let next = fixture.value(&["work", "next", &work], None);
    assert_eq!(next["next"]["role"], "worker");
    assert_ne!(next["next"]["progress"]["state"], "READY");
}

#[test]
fn work_resume_projects_residual_packet_without_repeating_valid_work() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "residual resume",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
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
        Some(b"implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["action"], "spawn");
    assert_eq!(checked["next"]["role"], "reviewer");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], work);
    assert_eq!(resumed["next"]["role"], "reviewer");
    assert_eq!(resumed["residual"]["next"], "spawn");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check"));
    assert!(resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "current_check" && item["status"] == "passed"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "review"));
    assert_eq!(
        resumed["residual"]["invalidation"]["sessionRestartInvalidates"],
        false
    );
    assert_eq!(
        resumed["residual"]["invalidation"]["subjectChangeInvalidates"],
        true
    );

    let reviewed = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"review"),
    );
    assert_eq!(reviewed["next"]["action"], "lead_decision");
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "lead_acceptance"));

    assert_progress(&scoped["next"]);
}

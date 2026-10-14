use super::*;

#[test]
fn same_goal_successor_carries_replayed_accounting_and_fresh_work_detail() {
    let fixture = Fixture::new_single();
    let goal = "same-goal successor carry";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = begin["work"].as_str().unwrap().to_owned();
    let predecessor_ledger = fixture.ledger(&predecessor_work);
    let scope = fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            scope["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first worker result"),
    );
    let predecessor_path = fixture.root.join(&predecessor_ledger);
    let predecessor_state = fixture.value(&["run", "inspect", &predecessor_ledger], None);
    let predecessor_before = fs::read(&predecessor_path).unwrap();
    let successor_work = format!("smw_{}", "a".repeat(64));
    let successor_ledger = fixture.ledger(&successor_work);
    let args = [
        "run",
        "supersede",
        &predecessor_ledger,
        "--workflow",
        "change",
        "--goal",
        goal,
        "--ledger",
        &successor_ledger,
        "--check-command",
        "true",
    ];
    let created = fixture.value(&args, None);
    assert_eq!(created["work"], successor_work);
    assert_eq!(created["currentDetail"]["route"], "work detail");
    assert_eq!(created["currentDetail"]["readOnly"], true);
    assert_eq!(created["currentDetail"]["requiresFreshContext"], true);
    assert_eq!(created["currentDetail"]["command"]["argv"][2], "detail");
    assert_eq!(
        created["currentDetail"]["command"]["argv"][3],
        successor_work
    );

    let successor_path = fixture.root.join(&successor_ledger);
    let successor_before = fs::read(&successor_path).unwrap();
    let mut events = fs::read_to_string(&successor_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["carryProtocol"], 1);
    assert_eq!(events[0]["governorCarry"]["accounting"]["spent"], 1);
    assert_eq!(events[0]["governorCarry"]["accounting"]["state"], "ready");
    assert_eq!(
        events[0]["governorCarrySha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(canonical(&events[0]["governorCarry"]).as_bytes())
        )
    );
    let successor = fixture.value(&["run", "inspect", &successor_ledger], None);
    assert_eq!(successor["governor"]["spent"], 1);
    assert_eq!(
        successor["governor"]["seenEvidenceSha256"],
        predecessor_state["governor"]["seenEvidenceSha256"]
    );
    assert!(successor["governor"]["evidence"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(successor["governor"]["currentMutation"], Value::Null);
    assert!(successor["governor"]["seenMutations"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(successor["governor"]["consumedGrants"]
        .as_array()
        .unwrap()
        .is_empty());
    let fresh = fixture.value(&["work", "detail", &successor_work], None);
    assert_ne!(
        fresh["current"]["result"]["subject"]["sha256"],
        predecessor_state["subject"]["sha256"]
    );

    let repeated = fixture.value(&args, None);
    assert_eq!(repeated["runId"], created["runId"]);
    assert_eq!(repeated["work"], successor_work);
    let other_work = format!("smw_{}", "b".repeat(64));
    let other_ledger = fixture.ledger(&other_work);
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            goal,
            "--ledger",
            &other_ledger,
            "--check-command",
            "true",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);
    assert!(!fixture.root.join(&other_ledger).exists());

    events[0]["governorCarry"]["accounting"]["spent"] = serde_json::json!(0);
    events[0]["governorCarrySha256"] = serde_json::json!(format!(
        "{:x}",
        sha2::Sha256::digest(canonical(&events[0]["governorCarry"]).as_bytes())
    ));
    events[0] = rehash(events[0].clone());
    fs::write(
        &successor_path,
        events
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let inspect = fixture.call(&["run", "inspect", &successor_ledger], None);
    assert!(!inspect.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(diagnostic.contains("governor carry"), "{diagnostic}");
    assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);

    // The predecessor's persisted claim binds this successor to the carry.
    // Removing every marker and rehashing the start must not reset spent to 0.
    fs::write(&successor_path, &successor_before).unwrap();
    let mut stripped: Value = serde_json::from_slice(
        successor_before
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    let object = stripped.as_object_mut().unwrap();
    object.remove("carryProtocol");
    object.remove("governorCarry");
    object.remove("governorCarrySha256");
    stripped = rehash(stripped);
    fs::write(
        &successor_path,
        serde_json::to_string(&stripped).unwrap() + "\n",
    )
    .unwrap();
    let inspect = fixture.call(&["run", "inspect", &successor_ledger], None);
    assert!(!inspect.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(
        diagnostic.contains("carry marker is missing"),
        "{diagnostic}"
    );
    assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);
}

#[test]
fn same_goal_carry_accepts_current_subject_check_without_duplicate_governor_evidence() {
    let fixture = Fixture::new_single();
    let goal = "same-goal observed result carry";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "printf stable-result",
            "--review-policy",
            "required",
        ],
        None,
    );
    let predecessor_work = begin["work"].as_str().unwrap().to_owned();
    let predecessor_ledger = fixture.ledger(&predecessor_work);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"predecessor worker result"),
    );
    let predecessor_worker_event = fs::read_to_string(fixture.root.join(&predecessor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor_check = fixture.value(
        &[
            "run",
            "observe-check",
            &predecessor_ledger,
            "--target",
            &predecessor_worker_event,
        ],
        None,
    );
    assert_eq!(predecessor_check["event"]["acquisition"], "observed");
    assert_eq!(predecessor_check["event"]["result"]["code"], 0);

    // Change the checked subject while preserving the exact frozen command
    // and its output. The successor must reject the old target by identity,
    // then accept its current target without treating the repeated result as new evidence.
    fs::write(fixture.root.join("material-input.txt"), b"changed input").unwrap();
    let successor_work = format!("smw_{}", "c".repeat(64));
    let successor_ledger = fixture.ledger(&successor_work);
    let created = fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            goal,
            "--ledger",
            &successor_ledger,
            "--check-command",
            "printf stable-result",
            "--review-policy",
            "required",
        ],
        None,
    );
    assert_eq!(created["work"], successor_work);
    let successor_begin = fixture.value(&["work", "next", &successor_work], None);
    let successor_assignment = successor_begin["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor_state = fixture.value(&["run", "inspect", &predecessor_ledger], None);

    let old_target_before = fs::read(fixture.root.join(&successor_ledger)).unwrap();
    let stale_target = fixture.call(
        &[
            "run",
            "observe-check",
            &successor_ledger,
            "--target",
            &predecessor_worker_event,
        ],
        None,
    );
    assert!(!stale_target.status.success());
    assert_eq!(
        fs::read(fixture.root.join(&successor_ledger)).unwrap(),
        old_target_before
    );

    let successor_scope = fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            &successor_assignment,
            "--outcome",
            "scoped",
        ],
        Some(b"successor scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &successor_work,
            successor_scope["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    let successor_worker = fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            successor_scope["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"successor worker result"),
    );
    let successor_worker_event = fs::read_to_string(fixture.root.join(&successor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(successor_worker["effect"], "recorded");
    let successor_state_before_check = fixture.value(&["run", "inspect", &successor_ledger], None);
    let successor_subject = successor_state_before_check["subject"]["sha256"].clone();
    assert_ne!(successor_subject, predecessor_state["subject"]["sha256"]);
    let before_check = fs::read(fixture.root.join(&successor_ledger)).unwrap();
    let repeated = fixture.call(
        &[
            "run",
            "observe-check",
            &successor_ledger,
            "--target",
            &successor_worker_event,
        ],
        None,
    );
    assert!(
        repeated.status.success(),
        "current-subject check was not accepted: {repeated:?}"
    );
    let repeated: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(repeated["event"]["acquisition"], "observed");
    assert_eq!(
        repeated["event"]["targetEventSha256"],
        successor_worker_event
    );
    assert_eq!(repeated["event"]["subjectSha256"], successor_subject);
    assert_eq!(
        repeated["event"]["result"]["code"],
        predecessor_check["event"]["result"]["code"]
    );
    assert_eq!(
        repeated["event"]["stdout"]["sha256"],
        predecessor_check["event"]["stdout"]["sha256"]
    );
    assert_ne!(
        fs::read(fixture.root.join(&successor_ledger)).unwrap(),
        before_check
    );
    let successor_events = fs::read_to_string(fixture.root.join(&successor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    assert_eq!(
        successor_events
            .iter()
            .filter(|event| event["action"] == "check")
            .count(),
        1
    );
    assert_eq!(
        successor_events
            .iter()
            .filter(|event| event["action"] == "check_observation")
            .count(),
        1,
        "the current check execution still records its bounded admission evidence"
    );
    let after_repeated = fixture.value(&["run", "inspect", &successor_ledger], None);
    assert_eq!(after_repeated["governor"]["state"], "ready");
    assert_eq!(after_repeated["governor"]["noInformationStreak"], 1);
    assert_eq!(after_repeated["governor"]["afterReplan"], false);
    assert_eq!(after_repeated["governor"]["postReplanSpent"], 0);
    assert_eq!(after_repeated["governor"]["spent"], 2);
}

#[test]
fn same_goal_carry_admits_distinct_validated_result_without_reducing_spent() {
    let fixture = Fixture::new_single();
    let goal = "same-goal distinct observed result";
    fs::write(fixture.root.join("status.txt"), b"1").unwrap();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "status=$(cat status.txt); printf same-output; exit \"$status\"",
            "--review-policy",
            "required",
        ],
        None,
    );
    let predecessor_work = begin["work"].as_str().unwrap().to_owned();
    let predecessor_ledger = fixture.ledger(&predecessor_work);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            &predecessor_work,
            scoped["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"predecessor worker result"),
    );
    let predecessor_worker_event = fs::read_to_string(fixture.root.join(&predecessor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor_check = fixture.value(
        &[
            "run",
            "observe-check",
            &predecessor_ledger,
            "--target",
            &predecessor_worker_event,
        ],
        None,
    );
    assert_eq!(predecessor_check["event"]["result"]["code"], 1);
    assert_eq!(predecessor_check["event"]["stdout"]["bytes"], 11);
    assert_eq!(
        predecessor_check["event"]["stdout"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    fs::write(fixture.root.join("status.txt"), b"0").unwrap();
    let successor_work = format!("smw_{}", "d".repeat(64));
    let successor_ledger = fixture.ledger(&successor_work);
    let created = fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            goal,
            "--ledger",
            &successor_ledger,
            "--check-command",
            "status=$(cat status.txt); printf same-output; exit \"$status\"",
            "--review-policy",
            "required",
        ],
        None,
    );
    assert_eq!(created["work"], successor_work);
    let successor_detail = fixture.value(&["work", "next", &successor_work], None);
    let assignment = successor_detail["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let scope = fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            &assignment,
            "--outcome",
            "scoped",
        ],
        Some(b"successor scope"),
    );
    fixture.value(
        &[
            "work",
            "permit",
            &successor_work,
            scope["next"]["assignment"].as_str().unwrap(),
            "--operation",
            "same-goal carry worker mutation",
        ],
        None,
    );
    fixture.value(
        &[
            "work",
            "return",
            &successor_work,
            scope["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"successor worker result"),
    );
    let successor_worker_event = fs::read_to_string(fixture.root.join(&successor_ledger))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let repeated_command = fixture.value(
        &[
            "run",
            "observe-check",
            &successor_ledger,
            "--target",
            &successor_worker_event,
        ],
        None,
    );
    assert_eq!(repeated_command["event"]["result"]["code"], 0);
    assert_eq!(repeated_command["event"]["stdout"]["bytes"], 11);
    assert_eq!(
        repeated_command["event"]["stdout"]["sha256"],
        predecessor_check["event"]["stdout"]["sha256"]
    );
    let state = fixture.value(&["run", "inspect", &successor_ledger], None);
    assert_eq!(state["governor"]["spent"], 2);
    assert_eq!(state["governor"]["noInformationStreak"], 0);
    assert_eq!(state["governor"]["state"], "ready");
}

#[test]
fn same_goal_carry_preserves_ready_cumulative_spent_at_three() {
    let fixture = Fixture::new_single();
    let goal = "same-goal ready cumulative telemetry carry";
    let check_command = "status=$(cat status.txt); printf same-output; exit \"$status\"";
    fs::write(fixture.root.join("status.txt"), b"1").unwrap();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            check_command,
            "--review-policy",
            "required",
        ],
        None,
    );
    let mut work = begin["work"].as_str().unwrap().to_owned();

    for (index, status) in [1u8, 2, 3].into_iter().enumerate() {
        let ledger = fixture.ledger(&work);
        let worker_event = complete_current_worker(&fixture, &work);
        let observed = fixture.value(
            &["run", "observe-check", &ledger, "--target", &worker_event],
            None,
        );
        assert_eq!(observed["event"]["acquisition"], "observed");
        assert_eq!(observed["event"]["result"]["code"], status);
        let state = fixture.value(&["run", "inspect", &ledger], None);
        assert_eq!(state["governor"]["spent"], index as u64 + 1);
        assert_eq!(state["governor"]["state"], "ready");
        assert_eq!(state["governor"]["noInformationStreak"], 0);

        let next_status = status + 1;
        fs::write(fixture.root.join("status.txt"), next_status.to_string()).unwrap();
        let successor_work = format!(
            "smw_{}",
            char::from(b'a' + index as u8).to_string().repeat(64)
        );
        let successor_ledger = fixture.ledger(&successor_work);
        let created = fixture.value(
            &[
                "run",
                "supersede",
                &ledger,
                "--workflow",
                "change",
                "--goal",
                goal,
                "--ledger",
                &successor_ledger,
                "--check-command",
                check_command,
                "--review-policy",
                "required",
            ],
            None,
        );
        assert_eq!(created["work"], successor_work);
        let carried = fixture.value(&["run", "inspect", &successor_ledger], None);
        assert_eq!(carried["governor"]["spent"], index as u64 + 1);
        assert_eq!(carried["governor"]["state"], "ready");
        assert_eq!(carried["governor"]["noInformationStreak"], 0);
        assert_eq!(carried["governor"]["afterReplan"], false);
        assert_eq!(carried["governor"]["postReplanSpent"], 0);
        work = successor_work;
    }
}

#[test]
fn resume_with_only_corrupt_candidate_stays_unresolved() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "corrupt only",
            "--check-command",
            "true",
        ],
        None,
    );
    let ledger = fixture.ledger(begin["work"].as_str().unwrap());
    fs::write(fixture.root.join(ledger), b"not-json\n").unwrap();

    let output = fixture.call(&["work", "resume", "--json"], None);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "unresolved");
    assert_eq!(value["reason"]["code"], "unreadable_candidate");
    assert_eq!(value["effect"], "no-change");
    assert!(value["works"].as_array().unwrap().is_empty());
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["nextAction"]["type"], "inspect_candidates");
    assert_eq!(value["nextAction"]["safe"], true);
    assert!(value["nextAction"]["summary"]
        .as_str()
        .unwrap()
        .contains("read-only command shown for each unreadable candidate"));
    assert_eq!(
        value["unreadable"][0]["workingDirectory"],
        fixture.root.to_str().unwrap()
    );
    let command = value["unreadable"][0]["command"].as_array().unwrap();
    assert_eq!(command[1], "run");
    assert_eq!(command[2], "inspect");
    assert_eq!(command[4], "--json");
    assert_eq!(command[5], "--config");
    assert_eq!(
        command[6],
        fixture.root.join("exitbind.json").to_str().unwrap()
    );
}

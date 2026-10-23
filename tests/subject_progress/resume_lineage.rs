use super::*;

#[test]
fn resume_lists_healthy_and_corrupt_candidates_without_selecting_one() {
    let fixture = Fixture::new_single();
    let healthy = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "healthy candidate",
            "--check-command",
            "true",
        ],
        None,
    );
    let corrupt = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "corrupt candidate",
            "--check-command",
            "true",
        ],
        None,
    );
    let corrupt_work = corrupt["work"].as_str().unwrap();
    let corrupt_ledger = fixture.ledger(corrupt_work);
    fs::write(fixture.root.join(corrupt_ledger), b"not-json\n").unwrap();

    let output = fixture.call(&["work", "resume", "--json"], None);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ambiguous");
    assert_eq!(value["reason"]["code"], "unreadable_candidate");
    assert_eq!(value["effect"], "no-change");
    assert_eq!(value["works"].as_array().unwrap().len(), 1);
    assert_eq!(value["works"][0]["work"], healthy["work"]);
    assert_eq!(value["works"][0]["command"][1], "work");
    assert_eq!(value["works"][0]["command"][2], "next");
    let command = value["works"][0]["command"].as_array().unwrap();
    assert_eq!(command[command.len() - 2], "--config");
    assert_eq!(
        command.last().unwrap(),
        fixture
            .root
            .join("exitbind.json")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(value["works"][0]["ledgerProducer"].is_object());
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["unreadable"][0]["work"], corrupt["work"]);
    assert_eq!(value["nextAction"]["safe"], true);
}

#[test]
fn resume_uses_valid_focus_while_preserving_unreadable_history() {
    let fixture = Fixture::new_single();
    let focused = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "focused current work",
            "--check-command",
            "true",
        ],
        None,
    );
    let corrupt = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "unreadable older history",
            "--check-command",
            "true",
        ],
        None,
    );
    let focused_work = focused["work"].as_str().unwrap();
    fixture.value(&["work", "focus", focused_work], None);
    let corrupt_work = corrupt["work"].as_str().unwrap();
    fs::write(
        fixture.root.join(fixture.ledger(corrupt_work)),
        b"not-json\n",
    )
    .unwrap();

    let value = fixture.value(&["work", "resume"], None);
    assert_eq!(value["status"], "resumed");
    assert_eq!(value["work"], focused_work);
    assert_eq!(value["selection"]["basis"], "current_work_focus");
    assert_eq!(value["works"].as_array().unwrap().len(), 0);
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
    assert_eq!(value["unreadable"][0]["work"], corrupt_work);
    assert_eq!(value["unreadable"][0]["reason"], "corrupt_ledger");
    assert_eq!(value["unreadable"][0]["command"][1], "run");
    assert_eq!(value["unreadable"][0]["command"][2], "inspect");
}

#[test]
fn resume_refuses_focused_superseded_work_with_unreadable_successor() {
    let fixture = Fixture::new_single();
    let predecessor = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "superseded focused work",
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = predecessor["work"].as_str().unwrap();
    let predecessor_ledger = fixture.ledger(predecessor_work);
    let successor_work = "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let successor_ledger = fixture.ledger(successor_work);

    fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            "successor work",
            "--ledger",
            &successor_ledger,
            "--check-command",
            "true",
        ],
        None,
    );
    fs::write(fixture.root.join(&successor_ledger), b"not-json\n").unwrap();

    let predecessor_before = fs::read(fixture.root.join(&predecessor_ledger)).unwrap();
    let successor_before = fs::read(fixture.root.join(&successor_ledger)).unwrap();
    let focus_path = fixture.root.join(".exitbind/current-work.json");
    let focus_before = fs::read(&focus_path).unwrap();

    for _ in 0..2 {
        let output = fixture.call(&["work", "resume"], None);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "ambiguous");
        assert_eq!(value["reason"]["code"], "unreadable_candidate");
        assert_ne!(
            value.get("work").and_then(Value::as_str),
            Some(predecessor_work)
        );
        assert!(value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["work"] == successor_work));
    }

    assert_eq!(
        fs::read(fixture.root.join(&predecessor_ledger)).unwrap(),
        predecessor_before
    );
    assert_eq!(
        fs::read(fixture.root.join(&successor_ledger)).unwrap(),
        successor_before
    );
    assert_eq!(fs::read(focus_path).unwrap(), focus_before);
}

#[test]
fn resume_refuses_predecessor_when_successor_ledger_is_missing() {
    let fixture = Fixture::new_single();
    let predecessor = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "missing successor predecessor",
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = predecessor["work"].as_str().unwrap();
    let predecessor_ledger = fixture.ledger(predecessor_work);
    let successor_work = "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let successor_ledger = fixture.ledger(successor_work);
    fixture.value(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            "missing successor",
            "--ledger",
            &successor_ledger,
            "--check-command",
            "true",
        ],
        None,
    );

    let predecessor_path = fixture.root.join(&predecessor_ledger);
    let successor_path = fixture.root.join(&successor_ledger);
    fs::remove_file(&successor_path).unwrap();
    let claim_path = fixture.root.join(format!("{predecessor_ledger}.supersede"));
    let focus_path = fixture.root.join(".exitbind/current-work.json");
    let predecessor_before = fs::read(&predecessor_path).unwrap();
    let claim_before = fs::read(&claim_path).unwrap();

    for focused in [true, false] {
        if !focused {
            fs::remove_file(&focus_path).unwrap();
        }
        let focus_before = fs::read(&focus_path).ok();
        for _ in 0..2 {
            let output = fixture.call(&["work", "resume"], None);
            assert!(output.status.success(), "{output:?}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["status"], "ambiguous");
            assert_eq!(value["reason"]["code"], "unreadable_candidate");
            assert_ne!(
                value.get("work").and_then(Value::as_str),
                Some(predecessor_work)
            );
            let unreadable = value["unreadable"].as_array().unwrap();
            let successor = unreadable
                .iter()
                .find(|candidate| candidate["ledger"] == successor_ledger)
                .expect("missing successor must be reported as unreadable");
            assert_eq!(successor["work"], successor_work);
            assert_eq!(
                successor["workingDirectory"],
                fixture.root.to_str().unwrap()
            );
            assert_eq!(successor["command"][1], "run");
            assert_eq!(successor["command"][2], "inspect");
            assert_eq!(successor["command"][4], "--json");
            assert_eq!(successor["command"][5], "--config");
            assert_eq!(
                successor["command"][6],
                fixture.root.join("exitbind.json").to_str().unwrap()
            );
            assert_eq!(fs::read(&predecessor_path).unwrap(), predecessor_before);
            assert_eq!(fs::read(&claim_path).unwrap(), claim_before);
            assert!(!successor_path.exists());
            assert_eq!(fs::read(&focus_path).ok(), focus_before);
        }
    }
}

#[test]
fn resume_refuses_focused_predecessor_with_invalid_successor_lineage() {
    let fixture = Fixture::new_single();
    let predecessor = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "lineage predecessor",
            "--check-command",
            "true",
        ],
        None,
    );
    let predecessor_work = predecessor["work"].as_str().unwrap();
    let predecessor_ledger = fixture.ledger(predecessor_work);
    let successor_work = "smw_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let successor_ledger = fixture.ledger(successor_work);

    let created = fixture.call(
        &[
            "run",
            "supersede",
            &predecessor_ledger,
            "--workflow",
            "change",
            "--goal",
            "lineage successor",
            "--ledger",
            &successor_ledger,
            "--check-command",
            "true",
        ],
        None,
    );
    assert!(created.status.success(), "{created:?}");
    let created_value: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created_value["work"], successor_work);
    assert_eq!(created_value["currentDetail"]["routeName"], "work detail");
    assert_eq!(created_value["currentDetail"]["readOnly"], true);
    assert_eq!(created_value["currentDetail"]["requiresFreshContext"], true);
    assert_eq!(created_value["currentDetail"]["command"]["argv"][1], "work");
    assert_eq!(
        created_value["currentDetail"]["command"]["argv"][2],
        "detail"
    );
    assert_eq!(
        created_value["currentDetail"]["command"]["argv"][3],
        successor_work
    );

    let successor_path = fixture.root.join(&successor_ledger);
    let mut events = fs::read_to_string(&successor_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    events[0]["supersedes"]["ledgerSha256"] = serde_json::json!("0".repeat(64));
    let mut previous = Value::Null;
    for event in &mut events {
        event["previousEventSha256"] = previous.clone();
        *event = rehash(event.clone());
        previous = event["eventSha256"].clone();
    }
    let source = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&successor_path, source).unwrap();

    let inspect = fixture.call(&["run", "inspect", &successor_ledger], None);
    assert!(!inspect.status.success(), "{inspect:?}");
    let inspect_text = format!(
        "{}{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert!(
        inspect_text.contains("superseded predecessor provenance mismatch"),
        "{inspect_text}"
    );

    let predecessor_path = fixture.root.join(&predecessor_ledger);
    let claim_path = fixture.root.join(format!("{predecessor_ledger}.supersede"));
    let focus_path = fixture.root.join(".exitbind/current-work.json");
    let predecessor_before = fs::read(&predecessor_path).unwrap();
    let successor_before = fs::read(&successor_path).unwrap();
    let claim_before = fs::read(&claim_path).unwrap();
    let focus_before = fs::read(&focus_path).unwrap();

    for _ in 0..2 {
        let output = fixture.call(&["work", "resume"], None);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "ambiguous");
        assert_eq!(value["reason"]["code"], "unreadable_candidate");
        assert_ne!(
            value.get("work").and_then(Value::as_str),
            Some(predecessor_work)
        );
        assert!(value["unreadable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["work"] == successor_work));
    }

    assert_eq!(fs::read(predecessor_path).unwrap(), predecessor_before);
    assert_eq!(fs::read(successor_path).unwrap(), successor_before);
    assert_eq!(fs::read(claim_path).unwrap(), claim_before);
    assert_eq!(fs::read(focus_path).unwrap(), focus_before);
}

use super::*;

#[test]
fn canonical_goal_updates_obligations_and_invalidates_prior_closure() {
    let project = Project::new("goal-revision");
    let evidence_work = begin(&project, false);
    let evidence_decision = project.drive_until(&evidence_work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &evidence_work,
            evidence_decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    let first = project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-a",
            "--goal",
            "ship",
            "--obligation",
            "first",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ],
        None,
    );
    assert_eq!(first["revision"], 1);
    assert_eq!(first["obligations"][0]["disposition"], "open");

    let accepted = project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-a",
            "--goal",
            "ship",
            "--obligation",
            "first",
            "--disposition",
            "accepted",
            "--result-ref",
            &evidence_work,
        ],
        None,
    );
    assert_eq!(accepted["revision"], 2);
    assert_eq!(accepted["obligations"].as_array().unwrap().len(), 1);
    assert_eq!(accepted["obligations"][0]["disposition"], "accepted");
    project.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-a",
            "--result-ref",
            &evidence_work,
        ],
        None,
    );

    let successor = project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-a",
            "--goal",
            "ship the follow-up",
        ],
        None,
    );
    assert_eq!(successor["revision"], 4);
    assert_eq!(successor["closure"]["closed"], false);
    assert_eq!(project.value(&["goal", "status"], None)["revision"], 4);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn unresolved_goal_facts_block_close_but_outside_scope_action_does_not() {
    let finding = Project::new("goal-finding");
    finding.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-f",
            "--goal",
            "ship",
            "--finding",
            "open review finding",
        ],
        None,
    );
    let blocked = finding.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-f",
            "--result-ref",
            "run-f",
        ],
        None,
    );
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("unresolved"));
    fs::remove_dir_all(finding.root).unwrap();

    let in_scope = Project::new("goal-external-in-scope");
    in_scope.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-i",
            "--goal",
            "ship",
            "--external-action",
            "publish release",
            "--scope",
            "in_scope",
        ],
        None,
    );
    let blocked = in_scope.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-i",
            "--result-ref",
            "run-i",
        ],
        None,
    );
    assert!(!blocked.status.success());
    fs::remove_dir_all(in_scope.root).unwrap();

    let external = Project::new("goal-external");
    let evidence_work = begin(&external, false);
    let evidence_decision = external.drive_until(&evidence_work, "lead_decision");
    external.value(
        &[
            "work",
            "return",
            &evidence_work,
            evidence_decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    external.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-e",
            "--goal",
            "ship",
            "--external-action",
            "publish release",
            "--scope",
            "outside_scope",
            "--result-ref",
            &evidence_work,
            "--none-applicable",
            "obligations,findings,blockers,decisions",
        ],
        None,
    );
    let closed = external.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-e",
            "--result-ref",
            &evidence_work,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    fs::remove_dir_all(external.root).unwrap();
}

#[test]
fn goal_close_requires_explicit_categories_and_current_governed_result() {
    let project = Project::new("goal-close-guards");
    project.value(
        &["goal", "incorporate", "--goal-id", "g", "--goal", "ship"],
        None,
    );
    let omitted = project.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "g",
            "--result-ref",
            "arbitrary",
        ],
        None,
    );
    assert!(!omitted.status.success());
    assert!(String::from_utf8_lossy(&omitted.stderr).contains("unresolved"));

    let explicit = Project::new("goal-result-guard");
    explicit.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let invalid = explicit.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "g",
            "--result-ref",
            "does-not-exist",
        ],
        None,
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("not a governed result"));
    let history = fs::read_to_string(explicit.root.join(".exitbind/session-goal.jsonl")).unwrap();
    assert_eq!(
        history.lines().count(),
        1,
        "rejected closure must not append"
    );
    fs::remove_dir_all(project.root).unwrap();
    fs::remove_dir_all(explicit.root).unwrap();
}

#[test]
fn goal_close_keeps_accepted_result_after_config_drift_and_warns() {
    let project = Project::new("goal-close-config-drift");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );

    let config = project.root.join("exitbind.json");
    let mut config_bytes = fs::read(&config).unwrap();
    config_bytes.push(b'\n');
    fs::write(config, config_bytes).unwrap();
    let closed = project.call(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    assert!(
        closed.status.success(),
        "{}{}",
        String::from_utf8_lossy(&closed.stdout),
        String::from_utf8_lossy(&closed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&closed.stderr).contains("config_drift"),
        "{}",
        String::from_utf8_lossy(&closed.stderr)
    );
    let value: Value = serde_json::from_slice(&closed.stdout).unwrap();
    assert_eq!(value["closure"]["closed"], true);
    assert_eq!(value["closure"]["resultRefs"][0], work);

    let history = fs::read_to_string(project.root.join(".exitbind/session-goal.jsonl")).unwrap();
    assert!(!history.contains("testedInputsSha256"));
    assert!(!history.contains("\"warnings\""));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn session_goal_history_is_hash_bound_and_legacy_is_refused() {
    let project = Project::new("goal-history-integrity");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let history_path = project.root.join(".exitbind/session-goal.jsonl");
    let original = fs::read_to_string(&history_path).unwrap();
    let tampered = original.replace("\"goal\":\"ship\"", "\"goal\":\"tampered\"");
    fs::write(&history_path, &tampered).unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("event hash mismatch"));
    assert_eq!(fs::read_to_string(&history_path).unwrap(), tampered);

    fs::remove_file(&history_path).unwrap();
    fs::write(project.root.join(".exitbind/session-goal.json"), original).unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("legacy session goal format"));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn hash_valid_but_broken_goal_chain_links_are_refused() {
    let project = Project::new("goal-chain-integrity");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &["goal", "incorporate", "--goal-id", "g", "--goal", "ship"],
        None,
    );
    let path = project.root.join(".exitbind/session-goal.jsonl");
    let valid = fs::read_to_string(&path).unwrap();

    let mut lines = valid
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    lines[1]["revision"] = Value::from(99);
    reseal(&mut lines[1]);
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("revision is not continuous"));

    fs::write(&path, &valid).unwrap();
    let mut lines = valid
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    lines[1]["predecessor"]["revision"] = Value::from(99);
    reseal(&mut lines[1]);
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("predecessor linkage mismatch"));

    fs::write(&path, &valid).unwrap();
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "successor",
            "--goal",
            "follow-up",
        ],
        None,
    );
    let valid_successor = fs::read_to_string(&path).unwrap();
    let mut lines = valid_successor
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    lines[2]["successorOf"] = Value::Null;
    reseal(&mut lines[2]);
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("successor linkage mismatch"));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn concurrent_goal_incorporation_preserves_each_successful_revision() {
    let project = Project::new("goal-concurrent");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let handles = (0..8)
        .map(|index| {
            let root = project.root.clone();
            thread::spawn(move || {
                let finding = format!("finding-{index}");
                let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
                    .current_dir(&root)
                    .args([
                        "goal",
                        "incorporate",
                        "--goal-id",
                        "g",
                        "--goal",
                        "ship",
                        "--finding",
                    ])
                    .arg(&finding)
                    .arg("--config")
                    .arg(root.join("exitbind.json"))
                    .output()
                    .unwrap();
                (finding, output)
            })
        })
        .collect::<Vec<_>>();
    let mut successful = Vec::new();
    for handle in handles {
        let (finding, output) = handle.join().unwrap();
        if output.status.success() {
            successful.push(finding);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("run ledger is busy"), "{stderr}");
        }
    }
    assert!(
        !successful.is_empty(),
        "all competing mutations were refused"
    );
    let history = fs::read_to_string(project.root.join(".exitbind/session-goal.jsonl")).unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), successful.len() + 1);
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record["revision"], index as u64 + 1);
    }
    let final_record = records.last().unwrap();
    for finding in successful {
        assert!(final_record["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == finding));
    }
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn resolved_goal_facts_are_not_projected_as_pending_card_items() {
    let project = Project::new("goal-resolved-card");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--finding",
            "review finding",
            "--none-applicable",
            "obligations,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--finding",
            "review finding",
            "--disposition",
            "accepted",
            "--result-ref",
            &work,
        ],
        None,
    );
    project.value(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --themed --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert_eq!(text.matches(SESSION_GOAL_CARD_TEST).count(), 1, "{text}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn all_five_resolved_categories_emit_then_new_open_item_suppresses_card() {
    let project = Project::new("goal-five-categories");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    for (category, item) in [
        ("obligation", "obligation-one"),
        ("finding", "finding-one"),
        ("blocker", "blocker-one"),
        ("decision", "decision-one"),
        ("external-action", "external-one"),
    ] {
        let flag = match category {
            "obligation" => "--obligation",
            "finding" => "--finding",
            "blocker" => "--blocker",
            "decision" => "--decision",
            "external-action" => "--external-action",
            _ => unreachable!(),
        };
        let mut open = vec![
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            flag,
            item,
        ];
        if category == "external-action" {
            open.extend(["--scope", "in_scope"]);
        }
        project.value(&open, None);
        let mut resolve = vec![
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            flag,
            item,
            "--disposition",
            "accepted",
            "--result-ref",
            &work,
        ];
        if category == "external-action" {
            resolve.extend(["--scope", "in_scope"]);
        }
        project.value(&resolve, None);
    }
    project.value(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --themed --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert_eq!(text.matches(SESSION_GOAL_CARD_TEST).count(), 1, "{text}");

    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--blocker",
            "new-blocker",
        ],
        None,
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert_eq!(text.matches(SESSION_GOAL_CARD_TEST).count(), 0, "{text}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn stale_closure_is_suppressed_before_and_after_display_cache_loss() {
    let project = Project::new("goal-stale-card");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --themed --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    fs::write(project.root.join("source.txt"), b"drifted\n").unwrap();
    let stale_goal = project.value(&["goal", "status", "--json"], None);
    assert_eq!(stale_goal["closure"]["closed"], true);
    assert_eq!(stale_goal["currentReadiness"]["state"], "stale");
    let first = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let first_text = String::from_utf8_lossy(&first.stdout).replace('\r', "");
    assert_eq!(
        first_text.matches(SESSION_GOAL_CARD_TEST).count(),
        0,
        "{first_text}"
    );
    let cache = project
        .root
        .join(".exitbind/presentation/session-goal.json");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(&cache, b"corrupt cache").unwrap();
    let corrupt_stale = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let corrupt_stale_text = String::from_utf8_lossy(&corrupt_stale.stdout).replace('\r', "");
    assert_eq!(
        corrupt_stale_text.matches(SESSION_GOAL_CARD_TEST).count(),
        0,
        "{corrupt_stale_text}"
    );
    fs::write(project.root.join("source.txt"), b"env-first\n").unwrap();
    let current = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let current_text = String::from_utf8_lossy(&current.stdout).replace('\r', "");
    assert_eq!(
        current_text.matches(SESSION_GOAL_CARD_TEST).count(),
        1,
        "{current_text}"
    );
    fs::write(project.root.join("source.txt"), b"drifted\n").unwrap();
    fs::remove_file(&cache).unwrap();
    let second = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let second_text = String::from_utf8_lossy(&second.stdout).replace('\r', "");
    assert_eq!(
        second_text.matches(SESSION_GOAL_CARD_TEST).count(),
        0,
        "{second_text}"
    );
    fs::remove_dir_all(project.root).unwrap();
}

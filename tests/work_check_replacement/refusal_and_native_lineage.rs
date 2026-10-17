use super::*;

#[test]
fn blocked_worker_has_a_bound_sibling_lead_replacement_without_fake_submission() {
    let f = Fixture::new();
    let begin = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "continue blocked work",
        "--check-command",
        "test -f missing-generated-target",
        "--review-policy",
        "required",
    ]);
    let work = begin["work"].as_str().unwrap().to_owned();
    f.ok(&[
        "work",
        "return",
        &work,
        begin["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = f.ok(&["work", "next", &work, "--json"]);
    let blocked = f.ok(&[
        "work",
        "return",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "blocked",
    ]);
    let source_sha = blocked["event"]["eventSha256"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            fs::read_to_string(f.ledger(&work))
                .unwrap()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find(|event| {
                    event["action"] == "submit"
                        && event["role"] == "worker"
                        && event["outcome"] == "blocked"
                })
                .unwrap()["eventSha256"]
                .as_str()
                .unwrap()
                .to_owned()
        });
    let detail = f.ok(&["work", "detail", &work, "--json"]);
    let binding = detail["binding"]
        .as_str()
        .or(detail["current"]["binding"].as_str())
        .unwrap()
        .to_owned();
    let action_form = &detail["actionForms"];
    assert_eq!(action_form["state"], "blocked_lead_check_replacement");
    assert_eq!(action_form["leadChoiceRequired"], true);
    assert_eq!(action_form["choices"].as_array().unwrap().len(), 1);
    let before = fs::read(f.ledger(&work)).unwrap();

    let preview_args = [
        "work",
        "replace-check",
        &work,
        "--check-command",
        "rm -f preserved-evidence; exit 9",
        "--reason",
        "blocked worker evidence shows the check contract cannot proceed",
        "--classification",
        "defective-contract",
        "--current-binding",
        &binding,
    ];
    let preview = f.ok(&preview_args);
    assert_eq!(preview["decision"]["source"]["kind"], "blocked_worker");
    assert_eq!(preview["decision"]["source"]["eventSha256"], source_sha);
    assert!(preview["decision"]["leadOwner"]["agent"].is_string());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);

    let apply_args = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    let applied = f.ok(&apply_args);
    assert_eq!(applied["state"], "successor_created");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    let events: Vec<Value> = fs::read_to_string(f.ledger(&work))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let previous_events: Vec<Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let lead_submissions = |events: &[Value]| {
        events
            .iter()
            .filter(|event| event["action"] == "submit" && event["role"] == "lead")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(
        lead_submissions(&events) == lead_submissions(&previous_events),
        "replacement adds no Lead submission; the original Lead scope decision remains"
    );
}

#[test]
fn accepted_predecessor_and_unclassified_failure_are_refused() {
    let f = Fixture::new();
    let begin = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "accepted stays closed",
        "--check-command",
        "true",
        "--review-policy",
        "required",
    ]);
    let work = begin["work"].as_str().unwrap().to_owned();
    f.ok(&[
        "work",
        "return",
        &work,
        begin["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "scoped",
    ]);
    let worker = f.ok(&["work", "next", &work, "--json"]);
    f.ok(&[
        "work",
        "return",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "completed",
    ]);
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"]);
    let reviewer = f.ok(&["work", "next", &work, "--json"]);
    assert_eq!(reviewer["next"]["role"], "reviewer");
    f.ok(&[
        "work",
        "return",
        &work,
        reviewer["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "approved",
    ]);
    let lead = f.ok(&["work", "next", &work, "--json"]);
    assert_eq!(lead["next"]["role"], "lead");
    f.ok(&[
        "work",
        "return",
        &work,
        lead["next"]["assignment"].as_str().unwrap(),
        "--outcome",
        "accepted",
    ]);
    let terminal = f.ok(&["work", "next", &work, "--json"]);
    let binding = terminal["current"]["binding"].as_str().unwrap().to_owned();
    let before = fs::read(f.ledger(&work)).unwrap();
    let rejected = f.call(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "false",
        "--reason",
        "test",
        "--classification",
        "defective-contract",
        "--current-binding",
        &binding,
        "--apply",
    ]);
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("accepted predecessor"),
        "{rejected:?}"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);

    let (failed_work, _) = f.begin_failed_check();
    let failed_lead = f.ok(&["work", "next", &failed_work, "--json"]);
    let failed_binding = failed_lead["current"]["binding"].as_str().unwrap();
    let unclassified = f.call(&[
        "work",
        "replace-check",
        &failed_work,
        "--check-command",
        "true",
        "--reason",
        "ordinary assertion failure",
        "--current-binding",
        failed_binding,
        "--apply",
    ]);
    assert!(
        !unclassified.status.success(),
        "a failed assertion cannot be classified automatically"
    );
    assert!(
        String::from_utf8_lossy(&unclassified.stderr).contains("--classification"),
        "{unclassified:?}"
    );
}

#[test]
fn missing_environment_tool_is_not_silently_classified_as_a_defective_contract() {
    let f = Fixture::new();
    let (work, failure) = f.begin_failed_check_with(
        "r46_tool_that_is_deliberately_not_installed --version",
        "the check environment lacks a tool",
    );
    assert_eq!(
        failure["result"]["code"], 127,
        "missing executable remains distinct from the Cargo missing-target result"
    );
    let before = fs::read(f.ledger(&work)).unwrap();
    let lead = f.ok(&["work", "next", &work, "--json"]);
    let binding = lead["current"]["binding"].as_str().unwrap();
    let refused = f.call(&[
        "work",
        "replace-check",
        &work,
        "--check-command",
        "true",
        "--reason",
        "the worker's environment does not contain the required tool",
        "--current-binding",
        binding,
        "--apply",
    ]);
    assert!(
        !refused.status.success(),
        "environment failure cannot be silently promoted to a Lead defective-contract decision"
    );
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--classification"));
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        before,
        "no successor is created without an explicit Lead classification"
    );
}

#[test]
fn native_goal_work_cannot_be_replaced_from_a_foreign_session() {
    let f = Fixture::new();
    let store = f.root.join(".exitbind/native-test-session-goals");
    let owner_session = "r46-replacement-owner-session";
    let incorporate = f.call_in_session(
        owner_session,
        &store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "r46-session-bound-goal",
            "--goal",
            "The original conversation commitment remains in this session",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    assert!(incorporate.status.success(), "{incorporate:?}");
    let begin = f.call_in_session(
        owner_session,
        &store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A session-bound implementation task",
            "--check-command",
            "test -f absent-r46-generated-input",
        ],
    );
    assert!(begin.status.success(), "{begin:?}");
    let begin: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = begin["work"].as_str().unwrap().to_owned();
    let scoped = f.call_in_session(
        owner_session,
        &store,
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = f.call_in_session(owner_session, &store, &["work", "next", &work, "--json"]);
    assert!(worker.status.success(), "{worker:?}");
    let worker: Value = serde_json::from_slice(&worker.stdout).unwrap();
    let returned = f.call_in_session(
        owner_session,
        &store,
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
    );
    assert!(returned.status.success(), "{returned:?}");
    let failed = f.call_in_session(
        owner_session,
        &store,
        &["work", "check", &work, "--timeout-ms", "2000"],
    );
    assert!(
        !failed.status.success(),
        "the deliberately absent input must fail once: {failed:?}"
    );
    let events_before = fs::read(f.ledger(&work)).unwrap();
    let binding = f.ok_in_session(
        "r46-unrelated-session",
        &store,
        &["work", "next", &work, "--json"],
    )["current"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let foreign = f.call_in_session(
        "r46-unrelated-session",
        &store,
        &[
            "work",
            "replace-check",
            &work,
            "--check-command",
            "true",
            "--reason",
            "must stay bound to the original session",
            "--classification",
            "defective-contract",
            "--current-binding",
            &binding,
            "--apply",
        ],
    );
    assert!(
        !foreign.status.success(),
        "a fresh but foreign native session cannot continue this member: {foreign:?}"
    );
    let diagnostic = String::from_utf8_lossy(&foreign.stderr);
    assert!(
        diagnostic.contains("native Goal/session/project association"),
        "{diagnostic}"
    );
    assert_eq!(
        fs::read(f.ledger(&work)).unwrap(),
        events_before,
        "session mismatch must refuse before any successor or predecessor write"
    );
}

#[test]
fn native_goal_replacement_keeps_original_member_and_resolves_successor_lineage_read_only() {
    let f = Fixture::new();
    let session = "r46-native-lineage-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let (work, binding) = native_failed_work(&f, session, &store);
    let session_root = native_session_root(&store, session);
    let journal_path = session_root.join(".exitbind/session-goal.jsonl");
    let journal_before = fs::read(&journal_path).unwrap();
    let records_before = journal_before
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let member_before = records_before
        .iter()
        .rev()
        .find_map(|record| {
            record["journeyMembers"]
                .as_array()?
                .iter()
                .find(|member| member["work"] == work)
        })
        .expect("original native Goal member")
        .clone();
    assert_eq!(
        records_before.last().unwrap()["journeyMembers"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "this fixture begins with one original conversation member"
    );
    assert!(records_before.last().unwrap()["obligations"]
        .to_string()
        .contains("Keep the original side obligation intact"));
    let predecessor_before = fs::read(f.ledger(&work)).unwrap();

    let preview = f.ok_in_session(session, &store, &refs(&replacement_args(&work, &binding)));
    let argv = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok_in_session(session, &store, &refs(&argv));
    assert_eq!(applied["state"], "successor_created");
    let successor = applied["successor"]["work"].as_str().unwrap().to_owned();

    assert_eq!(fs::read(f.ledger(&work)).unwrap(), predecessor_before);
    assert_eq!(
        fs::read(&journal_path).unwrap(),
        journal_before,
        "replacement does not append a successor member or rewrite admissions/obligations"
    );
    let records_after = fs::read_to_string(&journal_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        records_after.last().unwrap()["journeyMembers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        records_after.last().unwrap()["journeyMembers"][0],
        member_before
    );
    assert!(records_after.last().unwrap()["obligations"]
        .to_string()
        .contains("Keep the original side obligation intact"));

    let detail = f.ok_in_session(session, &store, &["work", "detail", &successor, "--json"]);
    assert_eq!(
        detail["goalAssociation"]["kind"],
        "verified_successor_lineage"
    );
    assert_eq!(detail["goalAssociation"]["work"], successor);
    assert_eq!(detail["goalAssociation"]["originalMemberWork"], work);
    assert_eq!(detail["checkReplacement"]["predecessor"]["work"], work);
    assert_eq!(detail["checkReplacement"]["successor"]["work"], successor);

    let next = f.ok_in_session(session, &store, &["work", "next", &successor, "--json"]);
    assert_eq!(next["current"]["checkReplacement"]["state"], "verified");
    assert_eq!(
        next["current"]["checkReplacement"]["predecessor"]["work"],
        work
    );
    assert_eq!(next["current"]["goal"]["overallAvailable"], true);
    // A bounded next response can omit optional progress details. Its current
    // relation and exact section route survive; ordinary action detail carries
    // the verified original member without another full instruction read.
    assert!(next["current"]["details"]["tasks"]["readOnly"] == true);
    let action = f.ok_in_session(
        session,
        &store,
        &["work", "detail", &successor, "--view", "action", "--json"],
    );
    assert_eq!(action["checkReplacement"]["state"], "verified");
    assert_eq!(
        action["goalAssociation"]["kind"],
        "verified_successor_lineage"
    );
    assert_eq!(action["goalAssociation"]["originalMemberWork"], work);
}

#[test]
fn unrelated_missing_historical_config_does_not_poison_selected_lineage() {
    let f = Fixture::new();
    let session = "lineage-unrelated-missing-config";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let historical_config = f.root.join("historical-unrelated.json");
    fs::copy(f.root.join("exitbind.json"), &historical_config).unwrap();
    let historical_path = historical_config.to_string_lossy().into_owned();

    let (unrelated, _) = native_failed_work_with_goal_and_config(
        &f,
        session,
        &store,
        "An unrelated historical task",
        &historical_path,
    );
    assert!(f.ledger(&unrelated).is_file());
    let goal_journal = native_session_root(&store, session).join(".exitbind/session-goal.jsonl");
    let latest: Value = fs::read_to_string(&goal_journal)
        .unwrap()
        .lines()
        .last()
        .map(|line| serde_json::from_str(line).unwrap())
        .unwrap();
    assert_eq!(latest["journeyMembers"][0]["work"], unrelated);
    assert_eq!(latest["journeyMembers"][0]["configPath"], historical_path);
    fs::remove_file(&historical_config).unwrap();
    assert!(!historical_config.exists());

    let (work, binding) = native_failed_work(&f, session, &store);
    let preview = f.ok_in_session(session, &store, &refs(&replacement_args(&work, &binding)));
    assert_eq!(preview["state"], "preview");
    let apply = preview["applyCommand"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let applied = f.ok_in_session(session, &store, &refs(&apply));
    assert_eq!(applied["state"], "successor_created");
    let successor = applied["successor"]["work"].as_str().unwrap();
    let detail = f.ok_in_session(session, &store, &["work", "detail", successor, "--json"]);
    assert_eq!(
        detail["goalAssociation"]["kind"],
        "verified_successor_lineage"
    );
    assert_eq!(detail["goalAssociation"]["originalMemberWork"], work);
    let successor_binding = fail_native_successor(&f, session, &store, successor);
    let mut successor_args = replacement_args(successor, &successor_binding);
    let check_command_index = successor_args
        .iter()
        .position(|argument| argument == "--check-command")
        .expect("replacement arguments declare the check command")
        + 1;
    let frozen_command = successor_args[check_command_index].clone();
    let successor_start = f.events(successor).first().cloned().unwrap();
    let frozen_command_sha256 = successor_start["checkPolicy"]["commandSha256"]
        .as_str()
        .expect("successor start freezes its check command hash");
    assert_eq!(
        digest_text(&frozen_command),
        frozen_command_sha256,
        "replacement helper's default must match the actual frozen successor command"
    );
    let proposed_command = "exit 0";
    assert_ne!(
        digest_text(proposed_command),
        frozen_command_sha256,
        "second preview must propose a policy different from the frozen successor command"
    );
    successor_args[check_command_index] = proposed_command.to_owned();
    let successor_preview_output = f.call_in_session(session, &store, &refs(&successor_args));
    assert!(
        successor_preview_output.status.success(),
        "second replacement preview output={successor_preview_output:?}; detail={detail:#}"
    );
    let successor_preview: Value = serde_json::from_slice(&successor_preview_output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {successor_preview_output:?}"));
    assert_eq!(
        successor_preview["state"], "preview",
        "{successor_preview:#}"
    );
    assert_eq!(
        successor_preview["decision"]["newPolicy"]["commandSha256"],
        digest_text(proposed_command),
        "preview must bind the explicitly proposed second command"
    );
}

#[test]
fn missing_selected_ancestor_config_still_refuses_lineage() {
    let f = Fixture::new();
    let session = "lineage-selected-missing-config";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let selected_config = f.root.join("exitbind.json");
    assert!(
        f.root.is_absolute(),
        "fixture product root must be absolute"
    );
    let mut config_b: Value = serde_json::from_slice(&fs::read(&selected_config).unwrap()).unwrap();
    config_b["project"]["root"] = json!(f.root.to_string_lossy());
    let config_b_bytes = serde_json::to_vec_pretty(&config_b).unwrap();
    fs::write(&selected_config, &config_b_bytes).unwrap();
    let historical_config = f.root.join(".exitbind/historical-selected.json");
    fs::write(&historical_config, &config_b_bytes).unwrap();
    assert_eq!(
        fs::read(&historical_config).unwrap(),
        fs::read(&selected_config).unwrap(),
        "historical A and selected B must be byte-identical configs"
    );
    let historical_path = historical_config.to_string_lossy().into_owned();

    let (work, _) = native_failed_work_with_config(&f, session, &store, &historical_path);
    let goal_journal = native_session_root(&store, session).join(".exitbind/session-goal.jsonl");
    let latest: Value = fs::read_to_string(&goal_journal)
        .unwrap()
        .lines()
        .last()
        .map(|line| serde_json::from_str(line).unwrap())
        .unwrap();
    assert!(latest["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|member| { member["work"] == work && member["configPath"] == historical_path }));
    assert!(selected_config.exists(), "B must remain available");

    let first_detail = full_work_detail_in_session(&f, session, &store, &work);
    assert_eq!(
        first_detail["recipient"]["role"], "lead",
        "detail={first_detail:#}"
    );
    assert_eq!(
        first_detail["actionForms"]["state"], "failed_check",
        "detail={first_detail:#}"
    );
    let first_binding = first_detail["binding"]
        .as_str()
        .or(first_detail["current"]["binding"].as_str())
        .unwrap_or_else(|| panic!("A member detail under B has no binding: {first_detail:#}"));
    let first_preview_output = f.call_in_session(
        session,
        &store,
        &refs(&replacement_args(&work, first_binding)),
    );
    assert!(
        first_preview_output.status.success(),
        "first B-context replacement preview output={first_preview_output:?}; detail={first_detail:#}"
    );
    let first_preview: Value = serde_json::from_slice(&first_preview_output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {first_preview_output:?}"));
    assert_eq!(first_preview["state"], "preview", "{first_preview:#}");
    let apply = first_preview["applyCommand"]["argv"]
        .as_array()
        .unwrap_or_else(|| panic!("first preview lacks apply command: {first_preview:#}"))
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let apply_output = f.call_in_session(session, &store, &refs(&apply));
    assert!(
        apply_output.status.success(),
        "first replacement apply={apply_output:?}"
    );
    let applied: Value = serde_json::from_slice(&apply_output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {apply_output:?}"));
    assert_eq!(applied["state"], "successor_created", "{applied:#}");
    let successor = applied["successor"]["work"]
        .as_str()
        .unwrap_or_else(|| panic!("replacement lacks successor: {applied:#}"));
    let before_removal = full_work_detail_in_session(&f, session, &store, successor);
    assert_eq!(
        before_removal["goalAssociation"]["kind"], "verified_successor_lineage",
        "successor detail before removing A: {before_removal:#}"
    );
    assert_eq!(
        before_removal["goalAssociation"]["originalMemberWork"], work,
        "successor detail before removing A: {before_removal:#}"
    );

    fs::remove_file(&historical_config).unwrap();
    assert!(!historical_config.exists());
    assert!(
        selected_config.exists(),
        "only A may be removed; B must remain"
    );
    let current_detail = full_work_detail_in_session(&f, session, &store, successor);
    assert_eq!(
        current_detail["recipient"]["role"], "lead",
        "detail={current_detail:#}"
    );
    assert_eq!(
        current_detail["actionForms"]["state"], "initial_scope",
        "detail={current_detail:#}"
    );
    assert!(
        current_detail["recipient"]["assignment"].as_str().is_some(),
        "successor current detail lacks Lead assignment: {current_detail:#}"
    );
    let refreshed_binding = current_detail["binding"]
        .as_str()
        .or(current_detail["current"]["binding"].as_str())
        .unwrap_or_else(|| panic!("successor current detail has no binding: {current_detail:#}"));
    let output = f.call_in_session(
        session,
        &store,
        &refs(&replacement_args(successor, refreshed_binding)),
    );
    assert!(
        !output.status.success(),
        "missing selected ancestor config must still refuse: {output:?}; detail={current_detail:#}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("native Goal/session/project association"),
        "{output:?}; detail={current_detail:#}"
    );
}

fn incorporate_replacement_goal(f: &Fixture, session: &str, store: &std::path::Path) {
    let goal = f.call_in_session(
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "r46-replacement-goal",
            "--goal",
            "Preserve the original goal and its side obligation",
            "--obligation",
            "Keep the original side obligation intact",
        ],
    );
    assert!(goal.status.success(), "{goal:?}");
}

fn native_failed_work_with_config(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    config: &str,
) -> (String, String) {
    native_failed_work_with_goal_and_config(
        f,
        session,
        store,
        "A session-bound implementation task",
        config,
    )
}

fn native_failed_work_with_goal_and_config(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    goal: &str,
    config: &str,
) -> (String, String) {
    incorporate_replacement_goal(f, session, store);
    let begin = f.call_in_session(
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "false",
            "--config",
            config,
        ],
    );
    assert!(begin.status.success(), "{begin:?}");
    let begin: Value = serde_json::from_slice(&begin.stdout).unwrap();
    let work = begin["work"].as_str().unwrap().to_owned();
    let scoped = f.call_in_session(
        session,
        store,
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
            "--config",
            config,
        ],
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = configured_session_call(
        f,
        session,
        store,
        &["work", "next", &work, "--json"],
        config,
    );
    assert!(worker.status.success(), "{worker:?}");
    let worker: Value = serde_json::from_slice(&worker.stdout).unwrap();
    let returned = f.call_in_session(
        session,
        store,
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
            "--config",
            config,
        ],
    );
    assert!(returned.status.success(), "{returned:?}");
    let failed = configured_session_call(
        f,
        session,
        store,
        &["work", "check", &work, "--timeout-ms", "2000"],
        config,
    );
    assert!(
        !failed.status.success(),
        "expected checker failure: {failed:?}"
    );
    let current = configured_session_call(
        f,
        session,
        store,
        &["work", "next", &work, "--json"],
        config,
    );
    assert!(current.status.success(), "{current:?}");
    let current: Value = serde_json::from_slice(&current.stdout).unwrap();
    (
        work,
        current["current"]["binding"].as_str().unwrap().to_owned(),
    )
}

fn configured_session_call(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    args: &[&str],
    config: &str,
) -> Output {
    let mut owned = args
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    owned.extend(["--config".into(), config.into()]);
    f.call_in_session(session, store, &refs(&owned))
}

fn fail_native_successor(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    work: &str,
) -> String {
    let lead = full_work_detail_in_session(f, session, store, work);
    assert_eq!(lead["recipient"]["role"], "lead", "detail={lead:#}");
    assert_eq!(
        lead["actionForms"]["state"], "initial_scope",
        "detail={lead:#}"
    );
    let scoped_choice = emitted_choice(&lead, "scoped");
    assert_emitted_choice_context(&lead, &scoped_choice, work);
    assert!(
        scoped_choice["stdin"]["required"]
            .as_bool()
            .unwrap_or(false),
        "choice={scoped_choice:#}"
    );
    assert_eq!(
        scoped_choice["stdin"]["kind"], "result_artifact",
        "choice={scoped_choice:#}"
    );
    let scoped_args = emitted_choice_args(
        &scoped_choice,
        "lineage fixture scopes the emitted Lead assignment",
    );
    let scoped_artifact =
        b"Fixture result artifact: scoped the emitted Lead assignment for lineage regression.\n";
    let scoped = call_in_session_with_input(f, session, store, &scoped_args, scoped_artifact);
    assert!(
        scoped.status.success(),
        "scoped output={scoped:?}; detail={lead:#}"
    );

    let worker = full_work_detail_in_session(f, session, store, work);
    assert_eq!(worker["recipient"]["role"], "worker", "detail={worker:#}");
    assert_eq!(
        worker["actionForms"]["state"], "pending_assignment",
        "detail={worker:#}"
    );
    let completed_choice = emitted_choice(&worker, "completed");
    assert_emitted_choice_context(&worker, &completed_choice, work);
    assert!(
        completed_choice["stdin"]["required"]
            .as_bool()
            .unwrap_or(false),
        "choice={completed_choice:#}"
    );
    assert_eq!(
        completed_choice["stdin"]["kind"], "result_artifact",
        "choice={completed_choice:#}"
    );
    let completed_args = emitted_choice_args(
        &completed_choice,
        "lineage fixture completed the emitted Worker assignment",
    );
    let artifact = b"Fixture result artifact: completed the emitted worker assignment for lineage regression.\n";
    let returned = call_in_session_with_input(f, session, store, &completed_args, artifact);
    assert!(
        returned.status.success(),
        "completed output={returned:?}; detail={worker:#}"
    );

    let failed = f.call_in_session(
        session,
        store,
        &["work", "check", work, "--timeout-ms", "2000"],
    );
    assert!(
        !failed.status.success(),
        "expected actual frozen checker failure: {failed:?}"
    );
    let failed_detail = full_work_detail_in_session(f, session, store, work);
    assert_eq!(
        failed_detail["actionForms"]["state"], "failed_check",
        "failed checker output={failed:?}; detail={failed_detail:#}"
    );
    failed_detail["binding"]
        .as_str()
        .or(failed_detail["current"]["binding"].as_str())
        .unwrap_or_else(|| {
            panic!("failed check has no current replacement binding: {failed_detail:#}")
        })
        .to_owned()
}

fn full_work_detail_in_session(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    work: &str,
) -> Value {
    let output = f.call_in_session(session, store, &["work", "detail", work, "--json"]);
    assert!(
        output.status.success(),
        "full work detail output={output:?}"
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: full work detail output={output:?}"))
}

fn emitted_choice(detail: &Value, label: &str) -> Value {
    detail["actionForms"]["choices"]
        .as_array()
        .unwrap_or_else(|| panic!("detail lacks action choices: {detail:#}"))
        .iter()
        .find(|choice| choice["label"] == label)
        .cloned()
        .unwrap_or_else(|| panic!("detail lacks `{label}` action choice: {detail:#}"))
}

fn assert_emitted_choice_context(detail: &Value, choice: &Value, work: &str) {
    let assignment = detail["recipient"]["assignment"]
        .as_str()
        .unwrap_or_else(|| panic!("detail has no current recipient assignment: {detail:#}"));
    let binding = detail["binding"]
        .as_str()
        .or(detail["current"]["binding"].as_str())
        .unwrap_or_else(|| panic!("detail has no current binding: {detail:#}"));
    let argv = choice["command"]["argv"]
        .as_array()
        .unwrap_or_else(|| panic!("choice has no argv: {choice:#}"));
    let argv = argv
        .iter()
        .map(|argument| argument.as_str().expect("argv values are strings"))
        .collect::<Vec<_>>();
    assert!(argv.contains(&work), "work missing from choice: {detail:#}");
    assert!(
        argv.contains(&assignment),
        "current recipient assignment missing from choice: {detail:#} choice={choice:#}"
    );
    assert!(
        argv.contains(&binding),
        "current binding missing from choice: {detail:#} choice={choice:#}"
    );
}

fn emitted_choice_args(choice: &Value, reason: &str) -> Vec<String> {
    let mut replaced_reason = false;
    let args = choice["command"]["argv"]
        .as_array()
        .unwrap_or_else(|| panic!("choice has no argv: {choice:#}"))
        .iter()
        .map(|argument| {
            let argument = argument.as_str().expect("argv values are strings");
            if argument == "<REASON>" {
                replaced_reason = true;
                reason.to_owned()
            } else {
                argument.to_owned()
            }
        })
        .collect::<Vec<_>>();
    assert!(
        replaced_reason,
        "choice did not declare a reason placeholder: {choice:#}"
    );
    args
}

fn call_in_session_with_input(
    f: &Fixture,
    session: &str,
    store: &std::path::Path,
    args: &[String],
    input: &[u8],
) -> Output {
    let args = refs(args);
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&f.root)
        .args(command_args(&args))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("CODEX_THREAD_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !args.contains(&"--config") {
        command.args(["--config", "exitbind.json"]);
    }
    clear_native_selectors(&mut command);
    command
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("CODEX_THREAD_ID", session);
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn pending_native_admission_blocks_check_replacement_preview_and_apply() {
    let f = Fixture::new();
    let session = "r46-pending-admission-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let (work, _) = native_failed_work(&f, session, &store);
    let session_root = native_session_root(&store, session);

    // A separate unlinked Work start creates a valid admission against this
    // session's current member. Its pending ownership blocks replacement of
    // the already-associated failed Work as well.
    let admitted = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "An interrupted side request",
        "--check-command",
        "true",
    ]);
    let admitted_work = admitted["work"].as_str().unwrap();
    let admitted_ledger = fs::read(f.ledger(admitted_work)).unwrap();
    let start: Value =
        serde_json::from_slice(admitted_ledger.split(|byte| *byte == b'\n').next().unwrap())
            .unwrap();
    let journal_path = session_root.join(".exitbind/session-goal.jsonl");
    let history = fs::read_to_string(&journal_path).unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let parent = records.last().unwrap();
    let member = records
        .iter()
        .rev()
        .find_map(|record| {
            record["journeyMembers"]
                .as_array()?
                .iter()
                .find(|member| member["work"] == work)
        })
        .expect("native Work membership")
        .clone();
    let admission = admission_for_start(&member, admitted_work, &start, parent);
    append_goal_admission(&session_root, admission);

    let journal_with_admission = fs::read(&journal_path).unwrap();
    let latest: Value = serde_json::from_slice(
        journal_with_admission
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(latest["journeyAdmissions"].as_array().unwrap().len(), 1);
    assert!(latest["obligations"]
        .to_string()
        .contains("Keep the original side obligation intact"));

    let binding = f.ok_in_session(session, &store, &["work", "next", &work, "--json"])["current"]
        ["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let ledger_before = fs::read(f.ledger(&work)).unwrap();
    let entries_before = run_entries(&f);
    for apply in [false, true] {
        let mut args = replacement_args(&work, &binding);
        if apply {
            args.push("--apply".into());
        }
        let output = f.call_in_session(session, &store, &refs(&args));
        assert!(
            !output.status.success(),
            "pending admission must refuse: {output:?}"
        );
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic
                .contains("check replacement is blocked by unresolved native Goal admissions"),
            "{diagnostic}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), ledger_before);
        assert_eq!(
            run_entries(&f),
            entries_before,
            "refusal creates no claim or successor"
        );
        assert_eq!(fs::read(&journal_path).unwrap(), journal_with_admission);
        let latest: Value = serde_json::from_str(
            fs::read_to_string(&journal_path)
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert!(latest["obligations"]
            .to_string()
            .contains("Keep the original side obligation intact"));
    }
    assert_eq!(fs::read(f.ledger(admitted_work)).unwrap(), admitted_ledger);
}

#[test]
fn same_session_corrupt_native_companion_blocks_check_replacement() {
    let f = Fixture::new();
    let session = "r46-corrupt-companion-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    let (work, _) = native_failed_work(&f, session, &store);
    let ledger_before = fs::read(f.ledger(&work)).unwrap();
    let entries_before = run_entries(&f);
    let companion = f.root.join(format!(
        ".exitbind/runs/work-{}.session-goal.json",
        &work[4..]
    ));
    assert!(
        companion.is_file(),
        "fixture has the native association companion"
    );
    fs::write(&companion, b"corrupted fixture companion\n").unwrap();
    let corrupted = fs::read(&companion).unwrap();

    let binding = f.ok_in_session(session, &store, &["work", "next", &work, "--json"])["current"]
        ["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let mut args = replacement_args(&work, &binding);
    args.push("--apply".into());
    let output = f.call_in_session(session, &store, &refs(&args));
    assert!(
        !output.status.success(),
        "corrupt companion must refuse: {output:?}"
    );
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("native Goal/session/project association is malformed or foreign; check replacement is blocked"),
        "{diagnostic}"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), ledger_before);
    assert_eq!(
        run_entries(&f),
        entries_before,
        "refusal creates no claim or successor"
    );
    assert_eq!(fs::read(&companion).unwrap(), corrupted);
}

#[test]
fn prepared_native_child_blocks_replacement_until_its_ownership_resolves() {
    let f = Fixture::new();
    let session = "r46-prepared-child-session";
    let store = f.root.join(".exitbind/native-test-session-goals");
    f.ok_in_session(
        session,
        &store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "r46-prepared-child-goal",
            "--goal",
            "Keep this native Goal bound while child work is prepared",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let begin = f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "inspect unresolved child ownership",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "child-review:Inspect unresolved child ownership.",
            "--preservation-check-command",
            "true",
        ],
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let initialization = serde_json::json!({
        "action":"init",
        "sourceRef":"owner:r46-child-review",
        "sourceText":"Inspect unresolved child ownership.",
        "requirements":[{"id":"child-review","text":"Inspect unresolved child ownership."}]
    });
    let initialized = f.call_with_input(
        &["work", "record", &work],
        initialization.to_string().as_bytes(),
    );
    assert!(initialized.status.success(), "{initialized:?}");
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
    );
    let worker = f.ok_in_session(session, &store, &["work", "next", &work, "--json"]);
    assert_eq!(worker["next"]["role"], "worker");
    let continuation = f.ok_in_session(session, &store, &["work", "continuation", &work]);
    let continuation_binding = continuation["mutationContext"]["token"].as_str().unwrap();
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "bind",
            &work,
            "--context",
            continuation_binding,
            "--host",
            "codex",
            "--session",
            session,
            "--host-version",
            "0.1-test",
        ],
    );
    let continuation = f.ok_in_session(session, &store, &["work", "continuation", &work]);
    let continuation_binding = continuation["mutationContext"]["token"].as_str().unwrap();
    f.ok_in_session(
        session,
        &store,
        &[
            "work",
            "child",
            "prepare",
            &work,
            "inspect retained state",
            "--context",
            continuation_binding,
        ],
    );
    let blocked = f.call_in_session(
        session,
        &store,
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
    );
    assert!(blocked.status.success(), "{blocked:?}");
    let children = f.ok_in_session(session, &store, &["work", "continuation", &work]);
    assert_eq!(children["preparedChildren"][0]["state"], "prepared");
    let before = fs::read(f.ledger(&work)).unwrap();
    let events: Vec<Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let blocked_source = events
        .iter()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "blocked"
        })
        .expect("the refused replacement has exact blocked-worker source evidence");
    assert!(blocked_source["eventSha256"].is_string());
    let current = f.ok_in_session(session, &store, &["work", "next", &work, "--json"]);
    let binding = current["current"]["binding"].as_str().unwrap();
    let entries_before = run_entries(&f);
    for apply in [false, true] {
        let mut args = replacement_args(&work, binding);
        if apply {
            args.push("--apply".into());
        }
        let refused = f.call_in_session(session, &store, &refs(&args));
        assert!(
            !refused.status.success(),
            "a prepared child blocks preview and apply: {refused:?}"
        );
        let diagnostic = String::from_utf8_lossy(&refused.stderr);
        assert!(
            diagnostic.contains(
                "check replacement is blocked by a live, prepared, or retained child result"
            ),
            "unexpected earlier replacement guard; stderr={diagnostic}; stdout={}",
            String::from_utf8_lossy(&refused.stdout)
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
        assert_eq!(run_entries(&f), entries_before);
    }
}

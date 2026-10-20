use crate::fixture::*;
use serde_json::{json, Value};
use std::fs;

#[test]
fn goal_status_automatically_emits_goal_and_task_progress_without_model_formatting() {
    let project = Project::new("goal-progress-product-surface");
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "progress",
        "--goal",
        "Ship the user's external request",
        "--obligation",
        "Implement the requested behavior",
    ]);
    let status = project.value(&["goal", "status", "--json"]);
    assert_eq!(status["goalProgress"]["overall"], "in_progress");
    assert_eq!(status["goalProgress"]["decomposition"]["total"], 1);
    assert_eq!(status["goalProgress"]["decomposition"]["complete"], 0);
    assert_eq!(
        status["goalProgress"]["resultReadiness"]["state"],
        "unavailable"
    );
    let text = status["goalProgress"]["systemText"].as_str().unwrap();
    assert!(text.contains("Goal: IN PROGRESS"), "{text}");
    assert!(text.contains("Tasks: 0/1 complete"), "{text}");
    assert!(text.contains("Result readiness: unavailable"), "{text}");
    assert!(
        text.ends_with("| Journey: 0 members, closes by each member accepted and current; not-yet-current members: `goal status --json .journeyCompletion`"),
        "{text}"
    );
    assert!(!text.contains("Lessons:"), "{text}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn standalone_work_keeps_an_unrelated_canonical_goal_private() {
    let project = Project::new("goal-progress-work-binding");
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "unrelated-goal",
        "--goal",
        "Private external goal",
        "--obligation",
        "Private task",
    ]);
    let started = project.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Different Work goal",
        "--check-command",
        "true",
        "--standalone",
    ]);
    assert!(started["next"]["packet"].get("goalProgress").is_none());
    assert!(!started.to_string().contains("Private external goal"));
    assert!(!started.to_string().contains("Private task"));
    let progress = &started["next"]["goalProgress"];
    assert_eq!(progress["overall"], "unavailable");
    assert_eq!(progress["goal"], Value::Null);
    assert_eq!(progress["decomposition"]["available"], false);
    assert_eq!(
        progress["decomposition"]["reason"],
        "overall_goal_unavailable"
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn ordinary_work_gets_context_for_the_open_goal_without_completing_its_task() {
    let project = Project::new("goal-context-work-association");
    let session = "goal-context-work-association-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "session",
            "--goal",
            "Ship the user's broader session request",
            "--obligation",
            "Implement the shared behavior",
        ],
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A different but related implementation task",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    let association = &started["next"]["goalProgress"]["workAssociation"];
    // An owner-project Work begun from the plain route is a journey member.
    assert_eq!(association["kind"], "journey_member");
    assert_eq!(association["membershipProtocol"], 2);
    assert_eq!(association["goalId"], "session");
    assert_eq!(association["work"], work);
    assert_eq!(association["startEventSha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        started["next"]["goalProgress"]["goal"],
        "Ship the user's broader session request"
    );
    assert_eq!(
        started["next"]["goalProgress"]["decomposition"]["complete"],
        0
    );
    assert_eq!(
        started["next"]["goalProgress"]["tasks"][0]["disposition"],
        "open"
    );
    let status = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(status["goalProgress"]["decomposition"]["complete"], 0);

    let standalone = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Must not inherit context",
            "--check-command",
            "true",
            "--standalone",
        ],
    );
    assert!(standalone.status.success());
    let standalone: Value = serde_json::from_slice(&standalone.stdout).unwrap();
    assert_eq!(standalone["next"]["goalProgress"]["overall"], "unavailable");
    assert!(standalone["next"]["goalProgress"]
        .get("workAssociation")
        .is_none());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_late_side_work_reopens_a_closed_session_goal_and_suppresses_room_readiness() {
    let project = Project::new("goal-context-late-side-work");
    let session = "goal-context-late-side-work-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "journey",
            "--goal",
            "The whole conversation journey",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let closed = project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "close", "--direct", "--goal-id", "journey"],
    );
    assert_eq!(closed["closure"]["closed"], true);
    let before = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(before["closure"]["closed"], true);
    assert_eq!(
        before["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        1
    );

    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A later side request in this conversation",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["work"],
        work
    );
    let after = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(after["closure"]["closed"], false);
    assert_eq!(
        after["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(after["goalProgress"]["journeyCompletion"]["protocol"], 2);
    assert_eq!(
        after["goalProgress"]["journeyCompletion"]["pendingAdmissions"],
        0
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        0
    );

    let session_root = native_session_root(store, "codex", session);
    let history = session_root.join(".exitbind/session-goal.jsonl");
    let records = fs::read_to_string(history)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records
        .iter()
        .any(|record| { record["closure"]["closed"] == true && record["goalId"] == "journey" }));
    assert_eq!(records.last().unwrap()["closure"]["closed"], false);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_prestart_pending_admission_reopens_a_closed_goal_without_a_work_start() {
    let project = Project::new("goal-context-prestart-admission");
    let session = "goal-context-prestart-admission-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "journey",
            "--goal",
            "Every request in this conversation remains accounted for",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let closed = project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "close", "--direct", "--goal-id", "journey"],
    );
    assert_eq!(closed["closure"]["closed"], true);
    let before = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(before["closure"]["closed"], true);
    assert_eq!(
        before["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        1
    );

    let pending_work = format!("smw_{}", "9".repeat(64));
    let admission = json!({
        "work": pending_work,
        "startIntentSha256": "a".repeat(64),
        "parentGoalId": "journey",
        "parentObservedGoalEventSha256": closed["eventSha256"],
        "workGoalSha256": "b".repeat(64),
        "host": "codex",
        "sessionKey": "c".repeat(64),
        "ownerIdentitySha256": "d".repeat(64),
        "ownerProjectId": null,
        "projectIdentitySha256": "e".repeat(64),
        "localIdentitySha256": "f".repeat(64),
        "configPath": project.root.join("exitbind.json").display().to_string(),
        "configSha256": "1".repeat(64),
        "productRoot": project.root.display().to_string(),
        "stateRoot": project.root.join(".exitbind").display().to_string(),
    });
    append_goal_admission(&native_session_root(store, "codex", session), admission);

    let blocked = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(blocked["closure"]["closed"], false);
    assert_eq!(
        blocked["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(
        blocked["goalProgress"]["journeyCompletion"]["pendingAdmissions"],
        1
    );
    assert_eq!(
        project
            .pty_status_in_session("codex", session, store)
            .matches(CARD)
            .count(),
        0
    );
    let close = project.call_in_session(
        "codex",
        session,
        store,
        &["goal", "close", "--direct", "--goal-id", "journey"],
    );
    assert!(!close.status.success());
    assert!(String::from_utf8_lossy(&close.stderr)
        .contains("native session journey is not ready to close"));
    assert!(!project
        .root
        .join(format!(".exitbind/runs/work-{}.jsonl", "9".repeat(64)))
        .exists());

    let history = fs::read_to_string(
        native_session_root(store, "codex", session).join(".exitbind/session-goal.jsonl"),
    )
    .unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records
        .iter()
        .any(|record| record["closure"]["closed"] == true));
    assert_eq!(
        records.last().unwrap()["journeyAdmissions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn link_session_recovers_only_the_exact_admitted_start_without_mutating_work() {
    let project = Project::new("goal-context-link-session-recovery");
    let session = "goal-context-link-session-recovery-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "journey",
            "--goal",
            "Link only a start admitted by this conversation",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    let seed = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Seed current native membership",
            "--check-command",
            "true",
        ],
    );
    let session_root = native_session_root(store, "codex", session);
    let work = project.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "The interrupted side request",
        "--check-command",
        "true",
    ]);
    let work = work["work"].as_str().unwrap().to_owned();
    let token = work.strip_prefix("smw_").unwrap();
    let ledger = project
        .root
        .join(format!(".exitbind/runs/work-{token}.jsonl"));
    let ledger_before = fs::read(&ledger).unwrap();
    let start =
        serde_json::from_slice::<Value>(ledger_before.split(|byte| *byte == b'\n').next().unwrap())
            .unwrap();

    let history_path = session_root.join(".exitbind/session-goal.jsonl");
    let history = fs::read_to_string(&history_path).unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let parent = records.last().unwrap().clone();
    let seed_work = seed["work"].as_str().unwrap();
    let member = records
        .iter()
        .rev()
        .find_map(|record| {
            record["journeyMembers"]
                .as_array()?
                .iter()
                .find(|member| member["work"] == seed_work)
        })
        .unwrap()
        .clone();
    let admission = admission_for_start(&member, &work, &start, &parent);
    append_goal_admission(&session_root, admission);

    let wrong_session = project.call_in_session(
        "codex",
        "another-conversation",
        store,
        &["work", "link-session", &work, "--json"],
    );
    assert!(!wrong_session.status.success());
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    let pending = fs::read_to_string(&history_path).unwrap();
    let pending_record = pending.lines().last().unwrap();
    let pending_record: Value = serde_json::from_str(pending_record).unwrap();
    assert_eq!(
        pending_record["journeyAdmissions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let recovered = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "link-session", &work, "--json"],
    );
    assert_eq!(recovered["kind"], "native_session_goal_link");
    assert_eq!(recovered["state"], "linked");
    assert_eq!(recovered["workLedger"], "unchanged");
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    let after_first_link = fs::read(&history_path).unwrap();
    let companion_path = project
        .root
        .join(format!(".exitbind/runs/work-{token}.session-goal.json"));
    let companion = fs::read(&companion_path).unwrap();
    let repeated = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "link-session", &work, "--json"],
    );
    assert_eq!(repeated["state"], "linked");
    assert_eq!(fs::read(&history_path).unwrap(), after_first_link);
    assert_eq!(fs::read(&companion_path).unwrap(), companion);
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    let detail = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "detail", &work, "--json"],
    );
    assert_eq!(detail["goalAssociation"]["kind"], "journey_member");
    let final_history = fs::read_to_string(&history_path).unwrap();
    let final_record: Value = serde_json::from_str(final_history.lines().last().unwrap()).unwrap();
    assert!(final_record["journeyAdmissions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(final_record["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["work"] == work));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_goal_added_after_work_start_is_not_attached_retroactively() {
    let project = Project::new("goal-context-no-retroactive-link");
    let session = "goal-context-no-retroactive-link-session";
    let store = project.session_store.as_path();
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Start before the Lead has a canonical Goal",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["next"]["goalProgress"]["overall"], "not_requested");
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "later",
            "--goal",
            "A later session request",
            "--obligation",
            "A later task",
        ],
    );
    let detail =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(detail["sections"]["tasks"]["overall"], "unavailable");
    assert!(detail.get("goalAssociation").is_none());
    assert!(!detail.to_string().contains("A later session request"));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn conversation_members_survive_successors_but_work_context_stays_with_its_original_goal() {
    let project = Project::new("goal-context-goal-lifecycle");
    let session = "goal-context-goal-lifecycle-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original",
            "--goal",
            "Original goal",
            "--obligation",
            "Original task",
        ],
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Context only",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    let original_association = started["next"]["goalProgress"]["workAssociation"].clone();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original",
            "--goal",
            "Revised original goal",
            "--obligation",
            "Revised original task",
        ],
    );
    let same_goal =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(
        same_goal["sections"]["tasks"]["goal"],
        "Revised original goal"
    );
    assert_eq!(same_goal["goalAssociation"], original_association);

    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--direct",
            "--goal-id",
            "original",
            "--goal",
            "Revised original goal",
            "--finding",
            "Lead recorded a finding",
        ],
    );
    let direct_update =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(direct_update["goalAssociation"], original_association);

    let successor = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "successor",
            "--goal",
            "Successor goal",
            "--obligation",
            "Successor task",
        ],
    );
    assert!(successor.get("workAssociationProtocol").is_none());
    assert!(successor.get("workAssociations").is_none());
    assert_eq!(successor["journeyMembershipProtocol"], 2);
    assert_eq!(successor["journeyMembers"].as_array().unwrap().len(), 1);
    assert_eq!(successor["journeyMembers"][0]["work"], work);
    assert_eq!(successor["journeyMembers"][0]["parentGoalId"], "original");
    let old_work =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(old_work["goalAssociation"], original_association);
    assert_eq!(
        old_work["sections"]["tasks"]["goal"],
        "Revised original goal"
    );
    assert_eq!(old_work["sections"]["tasks"]["overall"], "in_progress");
    assert!(!old_work.to_string().contains("Successor goal"));
    let active_successor =
        project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(active_successor["goalId"], "successor");
    assert_eq!(active_successor["closure"]["closed"], false);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn context_association_does_not_create_named_requirement_coverage() {
    let project = Project::new("goal-context-named-coverage");
    let session = "goal-context-named-coverage-session";
    let store = project.session_store.as_path();
    fs::write(
        project.root.join("request.txt"),
        "The accepted behavior is precise.\n",
    )
    .unwrap();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "named",
            "--goal",
            "Named coverage remains explicit",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "require",
            "--goal-id",
            "named",
            "--requirement",
            "R1",
            "--obligation",
            "The accepted behavior is precise.",
            "--artifact",
            "request.txt",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "cover", "--goal-id", "named"],
    );
    let refused = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Named requirement contract",
            "--check-command",
            "true",
            "--goal-id",
            "named",
            "--requirement",
            "R1",
            "--artifact",
            "request.txt",
            "--standalone",
        ],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("cannot be combined"));
    assert_eq!(
        fs::read_dir(project.root.join(".exitbind/runs"))
            .unwrap()
            .count(),
        0
    );
    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Unmapped ordinary Work",
            "--check-command",
            "true",
        ],
    );
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "journey_member"
    );
    assert_eq!(started["next"]["goalProgress"]["coverageConfirmed"], true);
    assert_eq!(
        started["next"]["goalProgress"]["decomposition"]["complete"],
        0
    );
    assert_eq!(started["next"]["goalProgress"]["tasks"][0]["mapped"], false);
    assert_eq!(
        started["next"]["goalProgress"]["tasks"][0]["performed"],
        false
    );
    let explicitly_named = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Explicitly cover the named requirement",
            "--check-command",
            "test -f request.txt",
            "--review-policy",
            "required",
            "--goal-id",
            "named",
            "--requirement",
            "R1",
            "--artifact",
            "request.txt",
        ],
    );
    assert_eq!(
        explicitly_named["next"]["goalProgress"]["workAssociation"]["kind"],
        "journey_member"
    );
    assert!(explicitly_named["next"]
        .get("projectGoalProgress")
        .is_none());
    assert_eq!(
        explicitly_named["next"]["goalProgress"]["tasks"][0]["mapped"],
        true
    );
    assert_eq!(
        explicitly_named["next"]["goalProgress"]["tasks"][0]["performed"],
        false
    );
    fs::remove_dir_all(project.root).unwrap();
}

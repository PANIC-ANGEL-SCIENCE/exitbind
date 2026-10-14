use crate::{fixture::*, support};
use serde_json::Value;
use std::fs;

fn pty_status(project: &Project) -> String {
    let command = format!(
        "env -u CODEX_THREAD_ID -u CODEX_SESSION_ID -u CLAUDE_CODE_SESSION_ID -u EXITBIND_NATIVE_SESSION_ID -u EXITBIND_SESSION_GOALS_DIR {} goal status --themed --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).replace('\r', "")
}

fn assert_no_run_files(project: &Project) {
    let runs = project.root.join(".exitbind/runs");
    let count = fs::read_dir(runs).unwrap().count();
    assert_eq!(count, 0, "direct goal created governed run files");
}

#[test]
fn direct_goal_closes_and_emits_exact_card_without_a_run() {
    let project = Project::new("direct-goal-card");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_no_run_files(&project);

    let plain_command = format!(
        "{} goal status --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        project.root.join("exitbind.json").display()
    );
    let plain = support::pty(&plain_command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&plain.stdout).contains(CARD));
    assert_eq!(
        project.value(&["goal", "status", "--json"])["currentReadiness"]["state"],
        "current"
    );

    let first = pty_status(&project);
    assert_eq!(first.matches(CARD).count(), 1, "{first}");
    assert_eq!(first.lines().last(), Some("+----------------------------+"));
    let second = pty_status(&project);
    assert_eq!(second.matches(CARD).count(), 0, "{second}");
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn direct_completion_event_is_immediate_idempotent_and_recoverable_once() {
    let project = Project::new("direct-goal-completion-event");
    let session = "direct-goal-completion-event-session";
    establish_native_direct_goal(&project, session);
    let journal = native_goal_journal(&project, session);

    let first_output = project.call_in_session(
        "codex",
        session,
        &project.session_store,
        &[
            "goal",
            "close",
            "--direct",
            "--goal-id",
            "direct-event",
            "--view",
            "event",
            "--json",
        ],
    );
    assert!(first_output.status.success(), "{first_output:?}");
    assert!(
        first_output.stdout.len() <= 4096,
        "{} bytes",
        first_output.stdout.len()
    );
    let first: Value = serde_json::from_slice(&first_output.stdout).unwrap();
    assert_eq!(first["kind"], "goal_completion_response");
    assert_eq!(first["effect"], "committed");
    assert_eq!(first["completionEvent"]["kind"], "session_goal_completed");
    assert_eq!(first["completionEvent"]["mode"], "direct");
    assert_eq!(first["presentation"]["terminal"], CARD);
    assert!(first["completionEvent"].get("terminal").is_none());
    assert!(!first["presentation"]["terminal"]
        .as_str()
        .unwrap()
        .contains("EXIT READY"));
    let event_id = first["completionEvent"]["id"].as_str().unwrap();
    let committed_journal = fs::read(&journal).unwrap();

    let duplicate = project.value_in_session(
        "codex",
        session,
        &project.session_store,
        &[
            "goal",
            "close",
            "--direct",
            "--goal-id",
            "direct-event",
            "--view",
            "event",
            "--json",
        ],
    );
    assert_eq!(duplicate["effect"], "no-change");
    assert_eq!(duplicate["completionEvent"]["id"], event_id);
    assert_eq!(duplicate["presentation"]["terminal"], Value::Null);
    assert_eq!(fs::read(&journal).unwrap(), committed_journal);

    let recovered = project.value_in_session(
        "codex",
        session,
        &project.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(recovered["completionEvent"]["id"], event_id);
    assert_eq!(recovered["recovery"]["readOnly"], true);
    assert_eq!(recovered["presentation"]["terminal"], CARD);
    assert_eq!(fs::read(&journal).unwrap(), committed_journal);
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn completion_event_is_suppressed_for_unfinished_stale_and_foreign_goals() {
    let unfinished = Project::new("direct-goal-event-unfinished");
    let unfinished_session = "direct-goal-event-unfinished-session";
    unfinished.value_in_session(
        "codex",
        unfinished_session,
        &unfinished.session_store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "unfinished",
            "--goal",
            "Leave one agreed obligation open",
            "--obligation",
            "Finish the remaining task",
        ],
    );
    let unfinished_journal = native_goal_journal(&unfinished, unfinished_session);
    let before_unfinished = fs::read(&unfinished_journal).unwrap();
    let unfinished_status = unfinished.value_in_session(
        "codex",
        unfinished_session,
        &unfinished.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(unfinished_status["completionEvent"], Value::Null);
    assert_eq!(unfinished_status["presentation"]["terminal"], Value::Null);
    assert_eq!(fs::read(&unfinished_journal).unwrap(), before_unfinished);
    fs::remove_dir_all(unfinished.root).unwrap();

    let stale = Project::new("direct-goal-event-stale");
    let stale_session = "direct-goal-event-stale-session";
    fs::write(stale.root.join("source.txt"), b"before-close\n").unwrap();
    establish_native_direct_goal(&stale, stale_session);
    let stale_journal = native_goal_journal(&stale, stale_session);
    stale.value_in_session(
        "codex",
        stale_session,
        &stale.session_store,
        &["goal", "close", "--direct", "--goal-id", "direct-event"],
    );
    fs::write(stale.root.join("source.txt"), b"changed-after-close\n").unwrap();
    let before_stale_read = fs::read(&stale_journal).unwrap();
    let stale_status = stale.value_in_session(
        "codex",
        stale_session,
        &stale.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(stale_status["completionEvent"], Value::Null);
    assert_eq!(stale_status["presentation"]["terminal"], Value::Null);
    assert_eq!(fs::read(&stale_journal).unwrap(), before_stale_read);

    let foreign = Project::new("direct-goal-event-foreign");
    let before_foreign_read = fs::read(&stale_journal).unwrap();
    let foreign_status = foreign.value_in_session(
        "codex",
        stale_session,
        &stale.session_store,
        &["goal", "status", "--view", "event", "--json"],
    );
    assert_eq!(foreign_status["contextOnly"], true);
    assert!(foreign_status.get("completionEvent").is_none());
    assert!(foreign_status.get("presentation").is_none());
    assert_eq!(fs::read(&stale_journal).unwrap(), before_foreign_read);
    assert_no_run_files(&stale);
    fs::remove_dir_all(stale.root).unwrap();
    fs::remove_dir_all(foreign.root).unwrap();
}

#[test]
fn direct_item_completion_is_typed_and_unknown_or_open_facts_refuse_close() {
    let project = Project::new("direct-goal-item");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "direct finding",
    ]);
    let closed = project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_eq!(closed["closure"]["kind"], "direct");
    assert_eq!(closed["closure"]["resultRefs"].as_array().unwrap().len(), 0);
    fs::remove_dir_all(project.root).unwrap();

    let open = Project::new("direct-goal-open");
    open.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "open",
        "--goal",
        "open goal",
        "--finding",
        "open finding",
    ]);
    let refused = open.call(&["goal", "close", "--direct", "--goal-id", "open"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unresolved"));
    fs::remove_dir_all(open.root).unwrap();

    let unknown = Project::new("direct-goal-unknown");
    unknown.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "unknown",
        "--goal",
        "unknown goal",
    ]);
    let refused = unknown.call(&["goal", "close", "--direct", "--goal-id", "unknown"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unresolved"));
    fs::remove_dir_all(unknown.root).unwrap();
}

#[test]
fn mixed_direct_closure_cannot_bypass_missing_governed_evidence() {
    let project = Project::new("direct-goal-mixed");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "governed finding",
        "--disposition",
        "accepted",
        "--result-ref",
        "smw_missing",
    ]);
    let refused = project.call(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a governed result"));
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn direct_close_keeps_history_but_suppresses_current_room_after_input_drift() {
    let project = Project::new("direct-goal-drift");
    establish_direct_goal(&project);
    project.value(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "direct finding",
    ]);
    fs::write(project.root.join("source.txt"), b"drifted-input\n").unwrap();
    let closed = project.call(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert!(
        closed.status.success(),
        "{}{}",
        String::from_utf8_lossy(&closed.stdout),
        String::from_utf8_lossy(&closed.stderr)
    );
    assert!(String::from_utf8_lossy(&closed.stderr).contains("tested_inputs_drift"));
    let closed: Value = serde_json::from_slice(&closed.stdout).unwrap();
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(pty_status(&project).matches(CARD).count(), 0);
    let status = project.value(&["goal", "status", "--json"]);
    assert_eq!(status["closure"]["closed"], true);
    assert_eq!(status["currentReadiness"]["state"], "stale");
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn current_direct_room_survives_corrupt_or_missing_display_memo() {
    let project = Project::new("direct-goal-memo");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    let cache = project
        .root
        .join(".exitbind/presentation/session-goal.json");
    fs::write(&cache, b"corrupt cache").unwrap();
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    fs::remove_file(&cache).unwrap();
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);
    assert_eq!(
        project.value(&["goal", "status", "--json"])["currentReadiness"]["state"],
        "current"
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn new_goal_request_invalidates_direct_closure_and_machine_surfaces_stay_silent() {
    let project = Project::new("direct-goal-invalidate");
    establish_direct_goal(&project);
    project.value(&["goal", "close", "--direct", "--goal-id", "direct"]);
    let machine = project.value(&["goal", "status", "--json"]);
    assert_eq!(machine["closure"]["kind"], "direct");
    assert_eq!(machine["completionMode"], Value::Null);
    assert!(!serde_json::to_string(&machine).unwrap().contains(CARD));
    assert_eq!(pty_status(&project).matches(CARD).count(), 1);

    project.value(&[
        "goal",
        "incorporate",
        "--goal-id",
        "direct",
        "--goal",
        "finish directly",
        "--finding",
        "new finding",
    ]);
    let after = pty_status(&project);
    assert_eq!(after.matches(CARD).count(), 0, "{after}");

    let headless = project.call(&["goal", "status"]);
    assert!(!String::from_utf8_lossy(&headless.stdout).contains(CARD));
    let closed_stdin = project.call(&["goal", "status"]);
    assert!(!String::from_utf8_lossy(&closed_stdin.stdout).contains(CARD));
    assert_no_run_files(&project);
    fs::remove_dir_all(project.root).unwrap();
}

use crate::fixture::*;
use serde_json::Value;
use std::{fs, io::Write, process::Stdio};

fn return_in_session(
    project: &Project,
    session: &str,
    work: &str,
    assignment: &str,
    outcome: &str,
    bytes: &[u8],
) -> Value {
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&project.root)
        .args(["work", "return", work, assignment, "--outcome", outcome])
        .arg("--config")
        .arg(project.root.join("exitbind.json"))
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", &project.session_store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(bytes) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn complete_required_review_work(project: &Project, session: &str, work: &str) {
    for _ in 0..12 {
        let detail = project.value_in_session(
            "codex",
            session,
            &project.session_store,
            &["work", "next", work, "--json", "--full"],
        );
        let next = &detail["next"];
        match next["action"].as_str().unwrap() {
            "lead_decision" => {
                let assignment = next["assignment"].as_str().unwrap();
                let scoped = next["outcomes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|outcome| outcome == "scoped");
                return_in_session(
                    project,
                    session,
                    work,
                    assignment,
                    if scoped { "scoped" } else { "accepted" },
                    b"Deterministic fixture decision; not semantic review.\n",
                );
            }
            "spawn" if next["role"] == "worker" => {
                let assignment = next["assignment"].as_str().unwrap();
                project.value_in_session(
                    "codex",
                    session,
                    &project.session_store,
                    &[
                        "work",
                        "permit",
                        work,
                        assignment,
                        "--operation",
                        "Return deterministic fixture evidence",
                    ],
                );
                return_in_session(
                    project,
                    session,
                    work,
                    assignment,
                    "completed",
                    br#"{"summary":"fixture implementation complete"}"#,
                );
            }
            "spawn" => {
                let assignment = next["assignment"].as_str().unwrap();
                return_in_session(
                    project,
                    session,
                    work,
                    assignment,
                    "approved",
                    b"Deterministic fixture review; not semantic review.\n",
                );
            }
            "check" => {
                let checked = project.call_in_session(
                    "codex",
                    session,
                    &project.session_store,
                    &["work", "check", work],
                );
                assert!(checked.status.success(), "{checked:?}");
            }
            "done" => return,
            action => panic!("unexpected required-review action {action}: {next}"),
        }
    }
    panic!("required-review Work did not reach done: {work}");
}

#[test]
fn native_sessions_are_independent_by_identity_and_host_in_one_project() {
    let project = Project::new("goal-native-session-isolation");
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        "same-native-id",
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "codex-goal",
            "--goal",
            "Codex conversation request",
            "--obligation",
            "Codex conversation task",
        ],
    );
    project.value_in_session(
        "claude",
        "same-native-id",
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "claude-goal",
            "--goal",
            "Claude conversation request",
            "--obligation",
            "Claude conversation task",
        ],
    );
    project.value_in_session(
        "codex",
        "other-native-id",
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "other-codex-goal",
            "--goal",
            "Other Codex request",
            "--obligation",
            "Other Codex task",
        ],
    );

    let codex = project.value_in_session(
        "codex",
        "same-native-id",
        store,
        &["goal", "status", "--json"],
    );
    let claude = project.value_in_session(
        "claude",
        "same-native-id",
        store,
        &["goal", "status", "--json"],
    );
    let other = project.value_in_session(
        "codex",
        "other-native-id",
        store,
        &["goal", "status", "--json"],
    );
    assert_eq!(codex["goal"], "Codex conversation request");
    assert_eq!(claude["goal"], "Claude conversation request");
    assert_eq!(other["goal"], "Other Codex request");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn another_project_receives_session_goal_context_without_mutation_or_coverage_authority() {
    let owner = Project::new("goal-native-session-owner");
    let child = Project::new("goal-native-session-child");
    let session = "shared-conversation-session";
    let store = owner.session_store.as_path();
    owner.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "parent",
            "--goal",
            "One conversation across projects",
            "--obligation",
            "Owner project task",
        ],
    );

    let started = child.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Separate child-project task",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["next"]["goalProgress"]["overall"], "context_only");
    assert_eq!(
        started["next"]["goalProgress"]["goal"],
        "One conversation across projects"
    );
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "context_only"
    );

    let child_status =
        child.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(child_status["contextOnly"], true);
    assert_eq!(child_status["ownerRequired"], true);
    let mutation = child.call_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "hijack",
            "--goal",
            "Child mutation attempt",
            "--obligation",
            "Must be refused",
        ],
    );
    assert!(!mutation.status.success());
    assert!(
        String::from_utf8_lossy(&mutation.stderr).contains("owned by another configured project")
    );

    let detail =
        child.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(detail["goalAssociation"]["kind"], "context_only");
    assert_eq!(detail["sections"]["tasks"]["overall"], "context_only");

    // Optional context corruption must not hide the Work's required detail or
    // recovery surface. The malformed locator only removes its Goal preview.
    let token = work.strip_prefix("smw_").unwrap();
    let context_companion = child
        .root
        .join(".exitbind/runs")
        .join(format!("work-{token}.session-goal.json"));
    let mut companion: Value =
        serde_json::from_slice(&fs::read(&context_companion).unwrap()).unwrap();
    companion["protocol"] = serde_json::json!(99);
    fs::write(&context_companion, serde_json::to_vec(&companion).unwrap()).unwrap();
    let after_corruption =
        child.call_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert!(
        after_corruption.status.success(),
        "{}",
        String::from_utf8_lossy(&after_corruption.stderr)
    );
    let after_corruption: Value = serde_json::from_slice(&after_corruption.stdout).unwrap();
    assert_eq!(after_corruption["complete"], true);
    assert_eq!(
        after_corruption["sections"]["tasks"]["overall"],
        "unavailable"
    );
    assert!(after_corruption.get("goalAssociation").is_none());
    fs::remove_dir_all(owner.root).unwrap();
    fs::remove_dir_all(child.root).unwrap();
}

#[test]
fn invalid_or_unavailable_native_identity_never_falls_back_and_does_not_block_ordinary_work() {
    let project = Project::new("goal-native-session-invalid-selector");
    project.value(&[
        "goal",
        "incorporate",
        "--project",
        "--goal-id",
        "local",
        "--goal",
        "Local Goal must not be a native fallback",
        "--obligation",
        "Local-only task",
    ]);
    for selectors in [
        vec![
            ("CODEX_THREAD_ID", "codex-one"),
            ("CLAUDE_CODE_SESSION_ID", "claude-one"),
        ],
        vec![
            ("CODEX_THREAD_ID", "codex-one"),
            ("CODEX_SESSION_ID", "codex-two"),
        ],
    ] {
        let started = project.call_with_native_selectors(
            &selectors,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Still usable ordinary Work",
                "--check-command",
                "true",
            ],
        );
        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        let started: Value = serde_json::from_slice(&started.stdout).unwrap();
        let work = started["work"].as_str().unwrap();
        assert_eq!(started["next"]["goalProgress"]["overall"], "unavailable");
        assert!(started["next"]["goalProgress"]
            .get("workAssociation")
            .is_none());
        let detail =
            project.call_with_native_selectors(&selectors, &["work", "detail", work, "--json"]);
        assert!(
            detail.status.success(),
            "{}",
            String::from_utf8_lossy(&detail.stderr)
        );
    }
    let session_without_goal = "valid-but-not-yet-incorporated";
    let started = project.value_in_session(
        "codex",
        session_without_goal,
        &project.session_store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "No implicit project fallback",
            "--check-command",
            "true",
        ],
    );
    assert_eq!(started["next"]["goalProgress"]["overall"], "not_requested");
    assert!(started["next"]["goalProgress"]
        .get("workAssociation")
        .is_none());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn project_named_contract_can_coexist_with_native_parent_context() {
    let project = Project::new("goal-project-contract-with-parent-context");
    let session = "project-contract-parent-session";
    let store = project.session_store.as_path();
    fs::write(
        project.root.join("request.txt"),
        "Project obligation text.\n",
    )
    .unwrap();
    fs::write(
        project.root.join("check.sh"),
        "#!/bin/sh\ntest -f request.txt\n",
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
            "same",
            "--goal",
            "Session parent request",
            "--obligation",
            "Session parent task",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "same",
            "--goal",
            "Project-local requirement contract",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "require",
            "--project",
            "--goal-id",
            "same",
            "--requirement",
            "P1",
            "--obligation",
            "Project obligation text.",
            "--artifact",
            "request.txt",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &["goal", "cover", "--project", "--goal-id", "same"],
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
            "Project-local named Work",
            "--check-command",
            "sh check.sh",
            "--review-policy",
            "required",
            "--goal-id",
            "same",
            "--requirement",
            "P1",
            "--artifact",
            "check.sh",
            "--project-goal",
        ],
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(
        started["next"]["goalProgress"]["goal"],
        "Session parent request"
    );
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "journey_member"
    );
    assert_eq!(started["next"]["projectGoalProgress"]["scope"], "project");
    assert_eq!(started["next"]["projectGoalProgress"]["goalId"], "same");
    assert_eq!(
        started["next"]["projectGoalProgress"]["requirements"][0]["mapped"],
        true
    );
    assert_eq!(
        started["next"]["projectGoalProgress"]["requirements"][0]["state"],
        "pending"
    );
    assert_eq!(
        started["next"]["projectGoalProgress"]["requirements"][0]["support"]["state"],
        "pending"
    );
    let detail =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(detail["goalAssociation"]["kind"], "journey_member");
    assert_eq!(detail["projectGoalProgress"]["scope"], "project");

    // A malformed named-scope selector is not silently reinterpreted as a
    // contract in another Goal journal, but remains optional for Work detail.
    let token = work.strip_prefix("smw_").unwrap();
    let scope_companion = project
        .root
        .join(".exitbind/runs")
        .join(format!("work-{token}.named-goal-scope.json"));
    let mut companion: Value =
        serde_json::from_slice(&fs::read(&scope_companion).unwrap()).unwrap();
    companion["protocol"] = serde_json::json!(99);
    fs::write(&scope_companion, serde_json::to_vec(&companion).unwrap()).unwrap();
    let after_scope_corruption =
        project.call_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert!(
        after_scope_corruption.status.success(),
        "{}",
        String::from_utf8_lossy(&after_scope_corruption.stderr)
    );
    let after_scope_corruption: Value =
        serde_json::from_slice(&after_scope_corruption.stdout).unwrap();
    assert_eq!(
        after_scope_corruption["goalAssociation"]["kind"],
        "journey_member"
    );
    assert_eq!(
        after_scope_corruption["projectGoalProgress"]["state"],
        "unavailable"
    );
    let assignment = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "assign",
            "--project",
            "--goal-id",
            "same",
            "--requirement",
            "P1",
            "--result-ref",
            work,
        ],
    );
    assert!(!assignment.status.success());
    assert!(String::from_utf8_lossy(&assignment.stderr).contains("named Goal scope companion"));

    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "local-direct",
            "--goal",
            "Local direct completion",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
    );
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "close",
            "--project",
            "--direct",
            "--goal-id",
            "local-direct",
        ],
    );
    let parent = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(parent["goalId"], "same");
    assert_eq!(parent["closure"]["closed"], false);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn native_owner_work_guidance_uses_verified_journal_root_for_close_readiness() {
    let project = Project::new("native-owner-work-close-readiness");
    let session = "native-owner-work-close-readiness-session";
    let store = project.session_store.as_path();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "native-owner",
            "--goal",
            "Complete the current conversation request",
            "--obligation",
            "Record the current direct completion",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
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
            "Complete a governed side task",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    );
    let work = started["work"].as_str().unwrap();
    let journal = native_goal_journal(&project, session);

    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--direct",
            "--goal-id",
            "native-owner",
            "--goal",
            "Complete the current conversation request",
            "--obligation",
            "Record the current direct completion",
        ],
    );

    let status = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(
        status["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(status["goalProgress"]["nextAction"], Value::Null);
    let compact =
        project.value_in_session("codex", session, store, &["work", "next", work, "--json"]);
    let detail =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    assert_eq!(
        detail["sections"]["tasks"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(detail["sections"]["tasks"]["nextAction"], Value::Null);
    assert_eq!(
        compact["presentation"]["goalProgress"]["overall"],
        detail["sections"]["tasks"]["overall"]
    );
    assert!(compact["presentation"]["goalProgress"]
        .get("nextAction")
        .is_none());

    let before_refusal = fs::read(&journal).unwrap();
    let refusal = project.call_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "close",
            "--direct",
            "--goal-id",
            "native-owner",
            "--view",
            "event",
            "--json",
        ],
    );
    assert!(!refusal.status.success(), "{refusal:?}");
    let refusal: Value = serde_json::from_slice(&refusal.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON close refusal, got stdout={:?}, stderr={:?}, error={error}",
            String::from_utf8_lossy(&refusal.stdout),
            String::from_utf8_lossy(&refusal.stderr)
        )
    });
    assert_eq!(
        refusal["error"],
        "native session journey is not ready to close: journey_members_need_current_acceptance_or_checked_integration",
        "close refusal must preserve the exact native readiness reason"
    );
    assert_eq!(fs::read(&journal).unwrap(), before_refusal);

    complete_required_review_work(&project, session, work);
    let current = project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    assert_eq!(
        current["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert!(current["goalProgress"]["nextAction"]["command"].is_array());
    let ready_detail =
        project.value_in_session("codex", session, store, &["work", "detail", work, "--json"]);
    let route = ready_detail["sections"]["tasks"]["nextAction"]["command"]
        .as_array()
        .unwrap();
    assert!(route.iter().any(|arg| arg == "--direct"));
    assert_eq!(
        route.first().and_then(Value::as_str),
        Some(env!("CARGO_BIN_EXE_exitbind"))
    );
    let config_index = route
        .iter()
        .position(|arg| arg == "--config")
        .expect("emitted close route has an exact config");
    assert_eq!(
        route.get(config_index + 1).and_then(Value::as_str),
        Some(project.root.join("exitbind.json").to_str().unwrap())
    );
    let args = route
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--config")
        .map(|arg| arg.as_str().unwrap())
        .collect::<Vec<_>>();
    let closed = project.value_in_session("codex", session, store, &args);
    assert_eq!(closed["completionEvent"]["kind"], "session_goal_completed");
    assert_eq!(closed["effect"], "committed");
    fs::remove_dir_all(project.root).unwrap();
}
#[path = "native_context_recovery.rs"]
mod native_context_recovery;

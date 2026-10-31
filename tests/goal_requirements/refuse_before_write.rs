//! Goal ownership is enforced for each public action before a foreign
//! project's files or its session Goal store can be changed.
use super::goal_action::{Admission, GoalAction};
use super::setup_first_work::{fresh, setup, tree};
use super::*;

const FOREIGN: &str = "native session Goal is owned by another configured project";

fn foreign_session(label: &str) -> (Project, (String, PathBuf)) {
    let owner = Project::new(label).with_native_session(&format!("{label}-session"));
    incorporate_native_generic_goal(&owner, "owner-goal", "The owner project's Goal");
    let session = owner.native_session.clone().unwrap();
    (owner, session)
}

/// The action type is also the production dispatcher. Adding an action forces
/// a fixture and admission decision here; no CLI source parsing is involved.
#[test]
fn goal_actions_refuse_a_foreign_owner_before_writing() {
    let (owner, session) = foreign_session("p4-goal-owner");
    let store = session.1.clone();
    let mut project = Project::new("p4-goal-mutations");
    project.file("request.txt", "Reject invalid input before persistence.\n");
    project.native_session = Some(session);
    let config = project.root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let work = format!("smw_{}", "1".repeat(64));
    let other = format!("smw_{}", "2".repeat(64));
    let journal = "3".repeat(64);
    let binding = "4".repeat(64);
    for &action in GoalAction::ALL {
        assert_eq!(GoalAction::parse(action.name()), Some(action));
        let tail = match action {
            GoalAction::Prepare => vec![
                "--goal-id",
                "g",
                "--requirement",
                "R1",
                "--check-command",
                "sh check.sh R1",
            ],
            GoalAction::Require => vec![
                "--goal-id",
                "g",
                "--requirement",
                "R1",
                "--obligation",
                "Reject invalid input before persistence.",
                "--artifact",
                "request.txt",
            ],
            GoalAction::Cover => vec!["--goal-id", "g"],
            GoalAction::Assign => vec![
                "--goal-id",
                "g",
                "--requirement",
                "R1",
                "--result-ref",
                &work,
            ],
            GoalAction::Incorporate => vec![
                "--goal-id",
                "g",
                "--goal",
                "Mine",
                "--none-applicable",
                "obligations,findings,blockers,decisions,externalActions",
            ],
            GoalAction::Close => vec!["--goal-id", "g", "--result-ref", &work],
            GoalAction::Usage => vec!["--goal-id", "g", "--apply"],
            GoalAction::Recover => vec![
                "--source-config",
                config,
                "--request-id",
                "r1",
                "--reason",
                "recover",
                "--current-binding",
                &binding,
                "--apply",
            ],
            GoalAction::Adopt => vec!["--session-key", &journal],
            GoalAction::Quarantine => vec![
                &work,
                "--source-config",
                config,
                "--verification-work",
                &other,
                "--request-id",
                "r1",
                "--reason",
                "quarantine",
                "--current-binding",
                &binding,
                "--apply",
            ],
            GoalAction::Status => vec!["--json"],
        };
        let mut args = vec!["goal", action.name()];
        args.extend(tail);
        let input = if action == GoalAction::Usage {
            Some(
                r#"{"id":"foreign-usage","source":"host_reported","scope":"setup","phase":"setup","status":"observed","semantics":"delta","lifetime":"invocation","values":{"inputTokens":4}}"#,
            )
        } else {
            None
        };
        let before = (tree(&owner.root), tree(&project.root), tree(&store));
        let output = project.call(&args, input);
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if action == GoalAction::Status {
            assert!(output.status.success(), "{action:?}: {message}");
            let status: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(status["contextOnly"], true);
            assert_eq!(status["authority"], "none");
        } else {
            assert!(!output.status.success(), "{action:?}: {message}");
            assert!(message.contains(FOREIGN), "{action:?}: {message}");
            if action.admission() == Admission::ProjectOrOwner {
                assert!(
                    message.contains("rerun with --project"),
                    "{action:?}: {message}"
                );
            }
        }
        assert_eq!(
            (tree(&owner.root), tree(&project.root), tree(&store)),
            before,
            "{action:?} wrote before refusing or reading"
        );
    }
}

/// Positive actions and independent persisted state assertions prevent a
/// dispatcher that simply refuses every Goal operation from passing.
#[test]
fn owner_actions_succeed_and_wrong_session_and_goal_leave_no_writes() {
    let mut project = Project::new("p4-owner-positive").with_native_session("p4-owner-session");
    project.setup();
    let store = project.native_session.as_ref().unwrap().1.clone();
    let prepared = project.value(
        &[
            "goal",
            "prepare",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--check-command",
            "sh check.sh R1",
        ],
        None,
    );
    assert_eq!(prepared["selectedRequirementIds"], json!(["R1"]));
    assert_eq!(prepared["readOnly"], true);

    let work = project.begin("R1", "sh check.sh R1");
    let assigned = project.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--result-ref",
            &work,
        ],
        None,
    );
    assert_eq!(assigned["goalId"], "import");
    let require = [
        "goal",
        "require",
        "--goal-id",
        "import",
        "--requirement",
        "R4",
        "--obligation",
        REQUIREMENTS[0].1,
        "--artifact",
        "request.txt",
    ];
    project.value(&require, None);
    project.value(&["goal", "cover", "--goal-id", "import"], None);
    assert_eq!(
        project.status()["requirements"]["requirements"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    let event = r#"{"id":"owner-usage","source":"host_reported","scope":"setup","phase":"setup","status":"observed","semantics":"delta","lifetime":"invocation","values":{"inputTokens":4,"cachedInputTokens":1,"outputTokens":3}}"#;
    let usage = project.value(
        &["goal", "usage", "--goal-id", "import", "--apply"],
        Some(event),
    );
    assert_eq!(usage["status"], "recorded");
    let observed = project.value(&["goal", "usage", "--goal-id", "import"], None);
    assert_eq!(observed["totals"]["inputTokens"], 4);
    assert_eq!(observed["totals"]["outputTokens"], 3);

    project.set_native_session("p4-wrong-session");
    let before = (tree(&project.root), tree(&store));
    let wrong = project.call(&require, None);
    assert!(!wrong.status.success(), "{wrong:?}");
    assert!(
        String::from_utf8_lossy(&wrong.stderr)
            .contains("no native session Goal owner is established"),
        "{wrong:?}"
    );
    assert_eq!((tree(&project.root), tree(&store)), before);

    project.set_native_session("p4-owner-session");
    let stale = [
        "goal",
        "require",
        "--goal-id",
        "stale",
        "--requirement",
        "R5",
        "--obligation",
        REQUIREMENTS[0].1,
        "--artifact",
        "request.txt",
    ];
    let before = (tree(&project.root), tree(&store));
    let output = project.call(&stale, None);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("canonical goal identity does not match"),
        "{output:?}"
    );
    assert_eq!((tree(&project.root), tree(&store)), before);
}

/// A recovery binding becomes stale when the Goal changes. The same public
/// apply succeeds with a fresh preview and writes only after that comparison.
#[test]
fn stale_recovery_apply_writes_nothing_and_fresh_apply_succeeds() {
    let owner = Project::new("p4-recovery-owner").with_native_session("p4-recovery-session");
    let goal = "Complete the original conversation work with current accepted evidence";
    incorporate_native_generic_goal(&owner, "original-conversation", goal);
    let mut source = Project::new("p4-recovery-source");
    source.native_session = owner.native_session.clone();
    source.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    source.file("source-input.txt", "original source evidence\n");
    let work = source.begin_ordinary("Validate source evidence", "sh check.sh", &[]);
    source.drive(&work, true);
    let config = source.root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let preview_args = [
        "goal",
        "recover",
        "--source-config",
        config,
        "--request-id",
        "p4-recovery",
        "--reason",
        "Retain the verified source evidence",
    ];
    let stale = owner.value(&preview_args, None);
    owner.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "original-conversation",
            "--goal",
            goal,
            "--finding",
            "Concurrent Goal change",
            "--disposition",
            "open",
        ],
        None,
    );
    let store = &owner.native_session.as_ref().unwrap().1;
    let before = (tree(&owner.root), tree(&source.root), tree(store));
    let mut args = preview_args.to_vec();
    args.extend([
        "--current-binding",
        stale["currentBinding"].as_str().unwrap(),
        "--apply",
    ]);
    let refused = owner.call(&args, None);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("--current-binding does not match the exact current recovery preview"),
        "{refused:?}"
    );
    assert_eq!((tree(&owner.root), tree(&source.root), tree(store)), before);

    let fresh = owner.value(&preview_args, None);
    let mut args = preview_args.to_vec();
    args.extend([
        "--current-binding",
        fresh["currentBinding"].as_str().unwrap(),
        "--apply",
    ]);
    let applied = owner.value(&args, None);
    assert_eq!(applied["kind"], "native_goal_recovery_applied");
    assert_ne!(tree(store), before.2);
}

/// The R54 case: setup used to apply the configuration, then fail on the
/// task card. It now selects this project's journal before writing and says so.
#[test]
fn setup_task_card_under_a_foreign_session_goal_uses_the_project_journal_and_says_so() {
    let (_owner, session) = foreign_session("p4-setup-owner");
    let (mut project, _outside, check) = fresh("p4-setup-card");
    project.native_session = Some(session);
    let store = project.native_store_snapshot();
    let output = setup(&project, &check, "S1");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let card = &report["next"]["taskCard"];
    assert_eq!(card["scope"], "project", "{report}");
    assert_eq!(card["registration"]["scope"], "project", "{report}");
    for reason in [&card["scopeReason"], &card["registration"]["scopeReason"]] {
        assert!(
            reason
                .as_str()
                .is_some_and(|reason| reason.contains("owned by another configured project")),
            "{report}"
        );
    }
    assert_eq!(
        project.native_store_snapshot(),
        store,
        "the session store is untouched"
    );
    assert!(
        project.goal_journal().is_some(),
        "the project journal records the card"
    );
    let argv = report["next"]["argv"].as_array().unwrap();
    for command in &argv[..2] {
        assert!(
            command
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "--project"),
            "{command}"
        );
    }
    assert!(argv[2]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "--project-goal"));
}

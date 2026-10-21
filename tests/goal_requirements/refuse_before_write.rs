//! R55 S2 (pattern P4, FR54-09): every applying command resolves its Goal
//! scope and refusable inputs before its first write. Under a native session
//! Goal another configured project owns, each one leaves the project tree
//! and the session store byte-unchanged and names its route, or (setup)
//! selects the project journal and says so.
use super::setup_first_work::{fresh, setup, tree};
use super::*;

/// How an applying command meets a foreign-owned session Goal.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Route {
    /// Refuses before writing and names `--project`.
    Project,
    /// Owner-context only by design: refuses before writing.
    OwnerContext,
    /// Reads only.
    ReadOnly,
}

/// Every `goal` subcommand; a new one fails the inventory until listed.
const GOAL_COMMANDS: &[(&str, Route)] = &[
    ("prepare", Route::Project),
    ("require", Route::Project),
    ("cover", Route::Project),
    ("assign", Route::Project),
    ("incorporate", Route::Project),
    ("close", Route::Project),
    ("usage", Route::Project),
    ("recover", Route::OwnerContext),
    ("quarantine", Route::OwnerContext),
    ("status", Route::ReadOnly),
];

/// Applying commands outside `goal`, with the test that drives each one
/// under a foreign-owned session Goal.
const OTHER_COMMANDS: &[(&str, &str, &str)] = &[
    (
        "setup --apply --task-card",
        "tests/goal_requirements/refuse_before_write.rs",
        "fn setup_task_card_under_a_foreign_session_goal_uses_the_project_journal_and_says_so",
    ),
    (
        "work quarantine --apply",
        "tests/check_observation_recovery/quarantine.rs",
        "fn quarantine_apply_under_a_foreign_session_goal_writes_nothing",
    ),
];

const FOREIGN: &str = "native session Goal is owned by another configured project";

/// A session whose Goal is owned by `owner`'s configuration.
fn foreign_session(label: &str) -> (Project, (String, PathBuf)) {
    let owner = Project::new(label).with_native_session(&format!("{label}-session"));
    incorporate_native_generic_goal(&owner, "owner-goal", "The owner project's Goal");
    let session = owner.native_session.clone().unwrap();
    (owner, session)
}

#[test]
fn every_goal_mutation_refuses_a_foreign_session_goal_before_writing() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli/goal.rs")).unwrap();
    let mut arms: Vec<&str> = source
        .lines()
        .filter_map(|line| line.strip_prefix("        \""))
        .filter_map(|rest| rest.split_once("\" =>").map(|(name, _)| name))
        .collect();
    let mut listed: Vec<&str> = GOAL_COMMANDS.iter().map(|(name, _)| *name).collect();
    arms.sort_unstable();
    listed.sort_unstable();
    assert_eq!(arms, listed, "every goal subcommand is in the P4 inventory");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (command, file, test) in OTHER_COMMANDS {
        let text = fs::read_to_string(root.join(file)).unwrap();
        assert!(text.contains(test), "{command}: {file} lacks {test}");
    }

    let (_owner, session) = foreign_session("p4-goal-owner");
    let mut project = Project::new("p4-goal-mutations");
    project.file("request.txt", "Reject invalid input before persistence.\n");
    project.native_session = Some(session);
    let config = project.root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let work = format!("smw_{}", "1".repeat(64));
    let other = format!("smw_{}", "2".repeat(64));
    let cases: Vec<(&str, Vec<&str>, Option<&str>)> = vec![
        (
            "prepare",
            vec![
                "goal",
                "prepare",
                "--goal-id",
                "g",
                "--requirement",
                "R1",
                "--check-command",
                "sh check.sh R1",
            ],
            None,
        ),
        (
            "require",
            vec![
                "goal",
                "require",
                "--goal-id",
                "g",
                "--requirement",
                "R1",
                "--obligation",
                "Reject invalid input before persistence.",
                "--artifact",
                "request.txt",
            ],
            None,
        ),
        ("cover", vec!["goal", "cover", "--goal-id", "g"], None),
        (
            "assign",
            vec![
                "goal",
                "assign",
                "--goal-id",
                "g",
                "--requirement",
                "R1",
                "--result-ref",
                &work,
            ],
            None,
        ),
        (
            "incorporate",
            vec![
                "goal",
                "incorporate",
                "--goal-id",
                "g",
                "--goal",
                "Mine",
                "--none-applicable",
                "obligations,findings,blockers,decisions,externalActions",
            ],
            None,
        ),
        (
            "close",
            vec!["goal", "close", "--goal-id", "g", "--result-ref", &work],
            None,
        ),
        (
            "usage",
            vec!["goal", "usage", "--goal-id", "g", "--apply"],
            Some("{\"version\":1,\"goalId\":\"g\"}"),
        ),
        (
            "recover",
            vec![
                "goal",
                "recover",
                "--source-config",
                config,
                "--request-id",
                "r1",
                "--reason",
                "recover",
            ],
            None,
        ),
        (
            "quarantine",
            vec![
                "goal",
                "quarantine",
                &work,
                "--source-config",
                config,
                "--verification-work",
                &other,
                "--request-id",
                "r1",
                "--reason",
                "quarantine",
            ],
            None,
        ),
    ];
    for (name, args, input) in cases {
        let route = GOAL_COMMANDS
            .iter()
            .find(|(listed, _)| *listed == name)
            .unwrap()
            .1;
        let before = (tree(&project.root), project.native_store_snapshot());
        let output = project.call(&args, input);
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "{name}: {message}");
        assert!(message.contains(FOREIGN), "{name}: {message}");
        if route == Route::Project {
            assert!(
                message.contains("rerun with --project"),
                "{name}: {message}"
            );
        }
        assert_eq!(
            (tree(&project.root), project.native_store_snapshot()),
            before,
            "{name} wrote before refusing"
        );
    }
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

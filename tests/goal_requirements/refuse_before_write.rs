//! R55 S2 (pattern P4, FR54-09): every applying command resolves its Goal
//! scope and refusable inputs before its first write. Under a native session
//! Goal another configured project owns, each one leaves the project tree
//! and the session store byte-unchanged and names its route, or (setup)
//! selects the project journal and says so.
use super::cli_tables::{applying_in, families};
use super::rust_source::production;
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
    ("adopt", Route::OwnerContext),
    ("status", Route::ReadOnly),
];

/// How an applying command outside `goal` meets a foreign-owned session
/// Goal. The commands are read from the CLI (every option check that accepts
/// `--apply`); each one is classified here.
enum Applying {
    /// Driven under a foreign-owned session Goal by this test (file, fn).
    Driven(&'static str, &'static str),
    /// Dispatched to the same admission as another applying command.
    Shares(&'static str, &'static str),
    /// Never writes a native session Goal: what it writes instead.
    NoSessionGoal(&'static str),
}

const APPLYING: &[(&str, Applying)] = &[
    (
        "setup",
        Applying::Driven(
            "tests/goal_requirements/refuse_before_write.rs",
            "fn setup_task_card_under_a_foreign_session_goal_uses_the_project_journal_and_says_so",
        ),
    ),
    (
        "work quarantine",
        Applying::Driven(
            "tests/check_observation_recovery/quarantine/foreign_session_goal.rs",
            "fn quarantine_apply_under_a_foreign_session_goal_writes_nothing",
        ),
    ),
    (
        "work quarantine-followup",
        Applying::Shares(
            "work quarantine",
            "one dispatcher arm and one admission (work::quarantine::start)",
        ),
    ),
    (
        "work replace-check",
        Applying::NoSessionGoal(
            "a successor Work ledger; it reads the Work's Goal association and refuses a foreign one",
        ),
    ),
    (
        "work recover-check",
        Applying::NoSessionGoal("the Work's own check-observation ledger"),
    ),
    (
        "work recover-pre-return",
        Applying::NoSessionGoal("the Work's own pre-return reconciliation ledger"),
    ),
    (
        "work usage",
        Applying::NoSessionGoal("usage records under this project's state root"),
    ),
    (
        "memory choose",
        Applying::NoSessionGoal("task memory choices under the state root"),
    ),
    (
        "memory correct-policy",
        Applying::NoSessionGoal("a memory ledger's policy correction"),
    ),
    (
        "memory revalidate",
        Applying::NoSessionGoal("lesson ledgers under the state root"),
    ),
    (
        "migrate",
        Applying::NoSessionGoal("this project's own layout"),
    ),
    (
        "project agents",
        Applying::NoSessionGoal("native agent profile projections"),
    ),
    (
        "project architecture select",
        Applying::NoSessionGoal("this project's configuration"),
    ),
    (
        "project capabilities select",
        Applying::NoSessionGoal("this project's configuration"),
    ),
    (
        "project advance admit",
        Applying::NoSessionGoal("the project continuation journal; Work admission uses this project's Goal journal (ContractScope::Project)"),
    ),
    (
        "project advance amend",
        Applying::NoSessionGoal("the project continuation journal"),
    ),
    (
        "project advance recover",
        Applying::NoSessionGoal("the project continuation journal"),
    ),
    (
        "project advance select",
        Applying::NoSessionGoal("the project continuation journal"),
    ),
];

/// Every applying command outside `goal` that `APPLYING` does not classify,
/// and every classified one the CLI no longer has.
fn unclassified(applying: &[(String, String)], classified: &[(&str, Applying)]) -> Vec<String> {
    let names: Vec<&str> = applying
        .iter()
        .map(|(_, name)| name.as_str())
        .filter(|name| !name.starts_with("goal "))
        .collect();
    let mut failures: Vec<String> = names
        .iter()
        .filter(|name| !classified.iter().any(|(listed, _)| listed == *name))
        .map(|name| format!("applying command `{name}` is not in the P4 inventory"))
        .collect();
    for (listed, class) in classified {
        if !names.contains(listed) {
            failures.push(format!(
                "P4 inventory lists `{listed}`, which no option check accepts --apply for"
            ));
        }
        if let Applying::Shares(other, _) = class {
            if !classified
                .iter()
                .any(|(name, class)| name == other && matches!(class, Applying::Driven(..)))
            {
                failures.push(format!("`{listed}` shares `{other}`, which is not driven"));
            }
        }
    }
    failures
}

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
    let families = families();
    let mut arms = families
        .iter()
        .find(|(name, _)| name == "goal")
        .map(|(_, subs)| subs.clone())
        .expect("goal family");
    let mut listed: Vec<String> = GOAL_COMMANDS
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    arms.sort_unstable();
    listed.sort_unstable();
    assert_eq!(arms, listed, "every goal subcommand is in the P4 inventory");
    let applying = applying_in(&production());
    assert!(applying.len() >= 15, "{applying:?}");
    let failures = unclassified(&applying, APPLYING);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (command, class) in APPLYING {
        if let Applying::Driven(file, test) = class {
            let text = fs::read_to_string(root.join(file)).unwrap();
            assert!(text.contains(test), "{command}: {file} lacks {test}");
        }
    }

    let (_owner, session) = foreign_session("p4-goal-owner");
    let mut project = Project::new("p4-goal-mutations");
    project.file("request.txt", "Reject invalid input before persistence.\n");
    project.native_session = Some(session);
    let config = project.root.join("exitbind.json");
    let config = config.to_str().unwrap();
    let work = format!("smw_{}", "1".repeat(64));
    let other = format!("smw_{}", "2".repeat(64));
    let journal = "3".repeat(64);
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
            "adopt",
            vec!["goal", "adopt", "--session-key", &journal],
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

/// The P4 inventory is read from the CLI: an applying command added to a
/// dispatcher, with its option list inline, bound by `let` or under a
/// command name bound by `let`, is a member until it is classified.
#[test]
fn an_applying_command_added_to_the_dispatcher_is_a_p4_member() {
    let seeded = concat!(
        "pub(super) fn work_command(l: &Loaded, a: &Arguments) -> Result<(), String> {\n",
        "    match action {\n",
        "        \"frobnicate\" => {\n",
        "            args::assert_options(\n                \"work frobnicate\",\n                a,\n                &[\"config\", \"apply\"],\n            )?;\n",
        "            Ok(())\n",
        "        }\n",
        "        \"look\" => args::assert_options(\"work look\", a, &[\"config\", \"json\"]),\n",
        "        _ => Ok(()),\n",
        "    }\n",
        "}\n",
        "fn grow(a: &Arguments) -> Result<(), String> {\n",
        "    let mut allowed = vec![\"root\", \"apply\"];\n",
        "    allowed.push(\"json\");\n",
        "    let command = if a.flags.contains_key(\"x\") { \"grow fast\" } else { \"grow\" };\n",
        "    args::assert_options(command, a, &allowed)\n",
        "}\n",
        "// args::assert_options(\"work commented\", a, &[\"apply\"]);\n",
    );
    let applying = applying_in(&[("src/cli/seeded.rs".to_owned(), seeded.to_owned())]);
    let names: Vec<&str> = applying.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names, ["grow", "grow fast", "work frobnicate"]);
    let failures = unclassified(&applying, &[("grow", Applying::NoSessionGoal("x"))]);
    assert_eq!(
        failures,
        [
            "applying command `grow fast` is not in the P4 inventory",
            "applying command `work frobnicate` is not in the P4 inventory",
        ]
    );
    let stale = unclassified(&[], &[("gone", Applying::Shares("work quarantine", "x"))]);
    assert_eq!(stale.len(), 2, "{stale:?}");
}

/// Tuple-bound names belong to the selected field, never an error message or
/// pattern; each new applying command still needs an explicit classification.
#[test]
fn applying_inventory_reads_the_selected_tuple_field_and_keeps_unclassified_commands() {
    let seeded = concat!(
        "fn recovery(a: &Arguments, action: &str) {\n",
        "    let usage = \"work obsolete\";\n",
        "    let (usage, missing) = match action {\n",
        "        \"recover-check\" => (\"work recover-check\", \"decoy check message\"),\n",
        "        \"recover-pre-return\" => (\"work recover-pre-return\", \"decoy pre-return message\"),\n",
        "        _ => return Err(\"decoy unknown\".into()),\n",
        "    };\n",
        "    args::assert_options(usage, a, &[\"config\", \"apply\"])?;\n",
        "}\n",
        "fn reversed(a: &Arguments, action: &str) {\n",
        "    let (missing, usage) = match action {\n",
        "        \"fresh\" => (\"decoy missing\", \"work fresh\"),\n",
        "        _ => return Err(\"decoy unknown\".into()),\n",
        "    };\n",
        "    args::assert_options(usage, a, &[\"apply\"])?;\n",
        "}\n",
        "fn readonly(a: &Arguments, action: &str) {\n",
        "    let (usage, missing) = match action {\n",
        "        \"look\" => (\"work look\", \"decoy look message\"),\n",
        "        _ => return Err(\"decoy unknown\".into()),\n",
        "    };\n",
        "    args::assert_options(usage, a, &[\"json\"])?;\n",
        "}\n",
        "fn later_binding(a: &Arguments, action: &str) {\n",
        "    let (usage, missing) = match action { \"old\" => (\"work old\", \"decoy old\"), _ => return Err(\"decoy unknown\".into()) };\n",
        "    let usage = \"work current\";\n",
        "    args::assert_options(usage, a, &[\"apply\"])?;\n",
        "}\n",
        "fn unrelated(action: &str) {\n",
        "    let (usage, missing) = match action { \"x\" => (\"work phantom\", \"decoy phantom\"), _ => return };\n",
        "}\n",
        "fn parameter(a: &Arguments, usage: &str) {\n",
        "    args::assert_options(usage, a, &[\"apply\"])?;\n",
        "}\n",
        "fn closed_block(a: &Arguments, usage: &str, action: &str) {\n",
        "    { let (usage, missing) = match action { \"x\" => (\"work closed\", \"decoy closed\"), _ => return }; }\n",
        "    args::assert_options(usage, a, &[\"apply\"])?;\n",
        "}\n",
        "fn outer_binding(a: &Arguments, action: &str) {\n",
        "    let (usage, missing) = match action { \"x\" => (\"work outer\", \"decoy outer\"), _ => return };\n",
        "    if ready() { args::assert_options(usage, a, &[\"apply\"])?; }\n",
        "}\n",
    );
    let applying = applying_in(&[("src/cli/seeded.rs".to_owned(), seeded.to_owned())]);
    let names: Vec<&str> = applying.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "work current",
            "work fresh",
            "work outer",
            "work recover-check",
            "work recover-pre-return"
        ]
    );
    let mut classified = vec![
        ("work current", Applying::NoSessionGoal("x")),
        ("work fresh", Applying::NoSessionGoal("x")),
        ("work outer", Applying::NoSessionGoal("x")),
        ("work recover-check", Applying::NoSessionGoal("x")),
    ];
    assert_eq!(
        unclassified(&applying, &classified),
        ["applying command `work recover-pre-return` is not in the P4 inventory"]
    );
    classified.push(("work recover-pre-return", Applying::NoSessionGoal("x")));
    assert!(unclassified(&applying, &classified).is_empty());
    let stale = unclassified(&[], &[("work recover-check", Applying::NoSessionGoal("x"))]);
    assert_eq!(
        stale,
        ["P4 inventory lists `work recover-check`, which no option check accepts --apply for"]
    );
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

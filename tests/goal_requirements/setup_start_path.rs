//! R54 Step 1: `setup --apply --task-card` registers the card's step table and
//! prints the whole start path. A Lead who runs exactly the printed argv, in
//! order, has every card row mapped to the begun Work before any edit, with
//! three goal/work commands and no wrapper script for the checker.
use super::checker_derivation::Outside;
use super::*;

const CARD: &str = "# R9 task\n\nIntro table that is not the step table:\n\n| Name | Value |\n| --- | --- |\n| owner | fixture |\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 1 | Add the parser | The parser rejects empty input. |\n| 2 | Add the store | Each name is stored once. |\n| 3 | Add the report | The report shows the count. |\n\nLimits: none.\n";
const ROWS: [(&str, &str); 3] = [
    ("S1", "The parser rejects empty input."),
    ("S2", "Each name is stored once."),
    ("S3", "The report shows the count."),
];

/// A fresh Git project with the card and its project check script, but no
/// Exitbind configuration yet.
fn fresh(label: &str, session: Option<&str>) -> (Project, Outside, String) {
    let root = support::temp(label);
    support::git_topology::repository(&root);
    fs::create_dir_all(root.join("work/r9")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("work/r9/00_TASK.md"), CARD).unwrap();
    fs::write(
        root.join("scripts/check.sh"),
        "set -eu\ntest -s work/r9/00_TASK.md\n",
    )
    .unwrap();
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    let mut project = Project {
        fixture_store: root.with_file_name(format!("{name}-native-session-goals")),
        root,
        native_session: None,
    };
    if let Some(identity) = session {
        project.set_native_session(identity);
    }
    let outside = Outside::new(&project);
    let script = outside.script(
        "run.sh",
        &format!(
            "set -eu\ncd '{}'\nexec sh ./scripts/check.sh\n",
            project.root.to_str().unwrap()
        ),
    );
    (project, outside, format!("sh {script}"))
}

fn setup(project: &Project, check: &str, apply: bool) -> Value {
    let mut argv = [
        env!("CARGO_BIN_EXE_exitbind"),
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        project.root.to_str().unwrap(),
        "--scope",
        "worker",
        "--observe",
        "scripts,work",
        "--write",
        "scripts",
        "--commands",
        "sh scripts/check.sh",
        "--check-command",
        check,
        "--goal",
        "Ship the R9 parser, store and report",
        "--review-policy",
        "required",
        "--hosts",
        "codex",
        "--skip-skills",
        "--task-card",
        "work/r9/00_TASK.md",
    ]
    .map(str::to_owned)
    .to_vec();
    if apply {
        argv.push("--apply".into());
    }
    let output = project.call_exact_argv(&argv);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn printed(report: &Value) -> Vec<Vec<String>> {
    report["next"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argv| {
            argv.as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_owned())
                .collect()
        })
        .collect()
}

fn goal_work(argv: &[Vec<String>]) -> Vec<&Vec<String>> {
    argv.iter()
        .filter(|command| matches!(command[1].as_str(), "goal" | "work"))
        .collect()
}

fn option<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    let index = argv.iter().position(|item| item == name)?;
    argv.get(index + 1).map(String::as_str)
}

/// Run setup with `--apply`, then exactly its printed argv in order, and
/// check every card row is mapped to the begun Work before any edit.
fn start_from_setup(project: &Project, check: &str, scope: &str) {
    let report = setup(project, check, true);
    let card = &report["next"]["taskCard"];
    assert_eq!(card["goalId"], "r9", "{report}");
    assert_eq!(card["scope"], scope, "{report}");
    assert_eq!(card["registration"]["registered"], true, "{report}");
    assert_eq!(card["registration"]["scope"], scope, "{report}");
    assert_eq!(card["checker"]["path"], "scripts/check.sh", "{report}");
    let argv = printed(&report);
    let commands = goal_work(&argv);
    assert_eq!(
        commands.len(),
        3,
        "the Lead runs three goal/work commands: {argv:?}"
    );
    assert_eq!(argv.len(), 3, "apply prints only the start path: {argv:?}");
    let verbs = commands
        .iter()
        .map(|command| (command[1].as_str(), command[2].as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        verbs,
        [("goal", "cover"), ("goal", "prepare"), ("work", "begin")]
    );
    let project_scope = scope == "project";
    for command in &commands[..2] {
        assert_eq!(
            command.contains(&"--project".to_owned()),
            project_scope,
            "{command:?}"
        );
    }
    assert_eq!(
        commands[2].contains(&"--project-goal".to_owned()),
        project_scope
    );
    for command in &commands[1..] {
        assert_eq!(
            option(command, "--requirement"),
            Some("S1,S2,S3"),
            "{command:?}"
        );
        assert_eq!(
            option(command, "--artifact"),
            Some("scripts/check.sh"),
            "{command:?}"
        );
        assert_eq!(
            option(command, "--check-command"),
            Some(check),
            "{command:?}"
        );
    }
    assert_eq!(option(commands[2], "--review-policy"), Some("required"));

    let mut work = None;
    for command in &argv {
        let output = project.call_exact_argv(command);
        assert!(output.status.success(), "{command:?}: {output:?}");
        if command[1] == "work" {
            let started: Value = serde_json::from_slice(&output.stdout).unwrap();
            work = started["work"].as_str().map(str::to_owned);
        }
    }
    let work = work.expect("the printed work begin started a Work");
    let status = project.status();
    for (id, text) in ROWS {
        let row = row(&status, id);
        assert_eq!(row["text"], text, "{status}");
        assert_eq!(row["mapped"], true, "{status}");
        assert!(
            row["support"]
                .as_array()
                .unwrap()
                .iter()
                .any(|support| support["mapping"]["work"] == work.as_str()),
            "{status}"
        );
    }
}

#[test]
fn setup_task_card_start_path_maps_every_row_under_a_native_session() {
    let (project, _outside, check) =
        fresh("setup-task-card-session", Some("r54-setup-card-session"));
    start_from_setup(&project, &check, "session");
    assert!(
        project.goal_journal().is_none(),
        "a native session registers in the session scope, not the project journal"
    );
}

#[test]
fn setup_task_card_preview_then_project_journal_start_path() {
    let (project, _outside, check) = fresh("setup-task-card-project", None);
    let preview = setup(&project, &check, false);
    let card = &preview["next"]["taskCard"];
    let rows = card["requirements"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{preview}");
    for ((id, text), row) in ROWS.iter().zip(rows) {
        assert_eq!(row["id"], *id);
        assert_eq!(row["obligation"], *text);
        assert!(CARD.contains(text));
    }
    assert!(card["registration"].is_null(), "preview registers nothing");
    let argv = printed(&preview);
    let verbs = goal_work(&argv)
        .iter()
        .map(|command| (command[1].clone(), command[2].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        verbs,
        [
            ("goal".to_owned(), "cover".to_owned()),
            ("goal".to_owned(), "prepare".to_owned()),
            ("work".to_owned(), "begin".to_owned())
        ]
    );
    assert!(
        !project.root.join("exitbind.json").exists(),
        "preview writes no files"
    );
    start_from_setup(&project, &check, "project");
    assert!(
        project.goal_journal().is_some(),
        "no native session uses the project journal"
    );
}

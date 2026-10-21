//! R55 F1 (FR55-01): `setup --task-card CARD --first-work IDS` prints a start
//! path whose three commands, run unedited, open a Work on the named rows
//! alone while every card row stays registered; an ID the card lacks is
//! refused before setup writes anything.
use super::checker_derivation::Outside;
use super::*;

const CARD: &str = "# R9 task\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 0 | Baseline | The baseline is recorded. |\n| 1 | Parser | The parser rejects empty input. |\n| 2 | Store | Each name is stored once. |\n| 3 | Report | The report shows the count. |\n| 4 | Docs | The docs name the parser. |\n| 5 | Bench | The bench is recorded. |\n| 6 | Observe | The observation is recorded. |\n| 7 | Ship | The release is published. |\n";
const IDS: [&str; 8] = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7"];

pub(super) fn fresh(label: &str) -> (Project, Outside, String) {
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
    let project = Project {
        fixture_store: root.with_file_name(format!("{name}-native-session-goals")),
        root,
        native_session: None,
    };
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

pub(super) fn setup(project: &Project, check: &str, first_work: &str) -> Output {
    let argv = [
        env!("CARGO_BIN_EXE_exitbind"),
        "setup",
        "--json",
        "--apply",
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
        "Ship the R9 rows one Work at a time",
        "--review-policy",
        "required",
        "--hosts",
        "codex",
        "--skip-skills",
        "--task-card",
        "work/r9/00_TASK.md",
        "--first-work",
        first_work,
    ]
    .map(str::to_owned)
    .to_vec();
    project.call_exact_argv(&argv)
}

/// Every file under `root` except Git's own metadata, with its bytes.
pub(super) fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.push((format!("{relative}/"), Vec::new()));
                visit(root, &path, out);
            } else {
                out.push((relative, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn first_work_opens_one_row_and_keeps_every_card_row_registered() {
    let (project, _outside, check) = fresh("setup-first-work");
    let output = setup(&project, &check, "S1");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let card = &report["next"]["taskCard"];
    assert_eq!(card["firstWork"], json!(["S1"]), "{report}");
    assert_eq!(
        card["remaining"],
        json!(["S0", "S2", "S3", "S4", "S5", "S6", "S7"])
    );
    assert_eq!(card["registration"]["requirements"], 8);
    let argv: Vec<Vec<String>> = report["next"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| {
            command
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_owned())
                .collect()
        })
        .collect();
    assert_eq!(argv.len(), 3, "{argv:?}");
    let verbs: Vec<(&str, &str)> = argv
        .iter()
        .map(|command| (command[1].as_str(), command[2].as_str()))
        .collect();
    assert_eq!(
        verbs,
        [("goal", "cover"), ("goal", "prepare"), ("work", "begin")]
    );
    for command in &argv[1..] {
        let at = command
            .iter()
            .position(|item| item == "--requirement")
            .unwrap();
        assert_eq!(command[at + 1], "S1", "{command:?}");
    }

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
    for id in IDS {
        let row = row(&status, id);
        let supported = row["support"]
            .as_array()
            .unwrap()
            .iter()
            .any(|support| support["mapping"]["work"] == work.as_str());
        assert_eq!(supported, id == "S1", "{id}: {status}");
        assert_eq!(row["mapped"], id == "S1", "{id}: {status}");
    }
}

#[test]
fn unknown_first_work_row_is_refused_before_setup_writes_anything() {
    let (project, _outside, check) = fresh("setup-first-work-unknown");
    let before = tree(&project.root);
    let store = project.native_store_snapshot();
    let output = setup(&project, &check, "S1,S9");
    assert!(!output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--first-work S9 names no task card row; the card has S0, S1"),
        "{output:?}"
    );
    assert_eq!(tree(&project.root), before, "nothing is written");
    assert!(!project.root.join("exitbind.json").exists());
    assert_eq!(project.native_store_snapshot(), store);
}

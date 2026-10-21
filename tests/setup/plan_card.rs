//! R54 follow-up (plan approval): a Claude Code `PostToolUse` for
//! `ExitPlanMode` whose approved plan has a `Done when` table saves the plan as
//! a private task card that `git status` does not list, and gives the Lead one
//! line; `setup --task-card` on that card prints the start path. A plan
//! without a table, or another tool, writes and prints nothing.
use super::lesson_carry::{repository, Root};
use super::support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Output, Stdio},
};

const PLAN: &str = "# Plan: R9\n\n## Steps\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 1 | Add the parser | The parser rejects empty input. |\n| 2 | Add the report | The report shows the count. |\n";
const PLAN_FILE: &str = "/tmp/plans/r9-quiet-river.md";
const CARD: &str = ".exitbind/plan-cards/r9-quiet-river/TASK.md";

fn call(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let home = root.with_extension("home");
    fs::create_dir_all(&home).unwrap();
    // `hooks apply` asks the exitbind on PATH for its hook protocol.
    let path = format!(
        "{}:{}",
        Path::new(env!("CARGO_BIN_EXE_exitbind"))
            .parent()
            .unwrap()
            .display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(root)
        .args(args)
        .env(
            "EXITBIND_SESSION_GOALS_DIR",
            root.with_extension("session-goals"),
        )
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("HOME", &home)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn setup(root: &Path, card: Option<&str>) -> Value {
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("scripts/check.sh"), "true\n").unwrap();
    let mut args = vec![
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root.to_str().unwrap(),
        "--scope",
        "worker",
        "--observe",
        "scripts",
        "--write",
        "scripts",
        "--commands",
        "sh scripts/check.sh",
        "--check-command",
        "sh scripts/check.sh",
        "--goal",
        "Ship the R9 parser and report",
        "--review-policy",
        "required",
        "--hosts",
        "claude",
        "--skip-skills",
        "--apply",
    ];
    if let Some(card) = card {
        args.extend(["--task-card", card]);
    }
    let output = call(root, &args, b"");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn approve(root: &Path, tool: &str, plan: &str) -> Output {
    let payload = json!({"hook_event_name":"PostToolUse","session_id":"plan-probe",
        "cwd":root,"permission_mode":"default","tool_name":tool,"tool_input":{},
        "tool_response":{"plan":plan,"isAgent":false,"filePath":PLAN_FILE}});
    let output = call(root, &["hook-run"], payload.to_string().as_bytes());
    assert!(output.status.success(), "{output:?}");
    output
}

fn porcelain(root: &Path) -> String {
    let output = support::git_topology::git(root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn an_approved_plan_with_a_table_becomes_a_private_card_and_one_line() {
    let owned: Root = repository("plan-card");
    let root = owned.0.clone();
    setup(&root, None);

    // Another tool and a plan without a table: nothing written, nothing printed.
    let other = approve(&root, "Write", PLAN);
    assert!(other.stdout.is_empty(), "{other:?}");
    let plain = approve(&root, "ExitPlanMode", "# Plan\n\n1. Write bye.txt\n");
    assert!(plain.stdout.is_empty(), "{plain:?}");
    assert!(!root.join(".exitbind/plan-cards").exists());

    let output = approve(&root, "ExitPlanMode", PLAN);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    let hook: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(hook["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let line = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(!line.contains('\n'), "{line}");
    assert!(
        line.contains(&format!(
            "exitbind setup --apply --root {} --task-card {CARD}",
            root.display()
        )),
        "{line}"
    );
    assert!(hook.get("systemMessage").is_none(), "{hook}");
    assert_eq!(fs::read_to_string(root.join(CARD)).unwrap(), PLAN);
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join(CARD)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let status = porcelain(&root);
    assert!(!status.contains("plan-cards"), "{status}");

    // A second approval in the same session replaces the card; the exclude
    // line is written once.
    let again = approve(&root, "ExitPlanMode", PLAN);
    assert!(!again.stdout.is_empty());
    let exclude = support::git_topology::git(&root)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()
        .unwrap();
    let exclude = root.join(String::from_utf8(exclude.stdout).unwrap().trim());
    let excluded = fs::read_to_string(exclude).unwrap();
    assert_eq!(
        excluded
            .lines()
            .filter(|line| *line == "/.exitbind/plan-cards/")
            .count(),
        1,
        "{excluded}"
    );

    // The printed setup registers the card and prints the start path.
    let report = setup(&root, Some(CARD));
    let card = &report["next"]["taskCard"];
    assert_eq!(card["goalId"], "r9-quiet-river", "{report}");
    assert_eq!(card["registration"]["registered"], true, "{report}");
    assert_eq!(
        card["requirements"].as_array().unwrap().len(),
        2,
        "{report}"
    );
    let argv = report["next"]["argv"].as_array().unwrap();
    let verbs = argv
        .iter()
        .map(|argv| {
            format!(
                "{} {}",
                argv[1].as_str().unwrap(),
                argv[2].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        verbs,
        ["goal cover", "goal prepare", "work begin"],
        "{report}"
    );
}

#[test]
fn claude_hooks_apply_adds_the_plan_approval_record_with_its_matcher() {
    let owned: Root = repository("plan-card-hooks");
    let root = owned.0.clone();
    let output = call(
        &root,
        &[
            "hooks",
            "apply",
            "--hosts",
            "claude",
            "--root",
            root.to_str().unwrap(),
        ],
        b"",
    );
    assert!(output.status.success(), "{output:?}");
    let settings: Value =
        serde_json::from_slice(&fs::read(root.join(".claude/settings.json")).unwrap()).unwrap();
    let groups = settings["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{settings}");
    assert_eq!(groups[0]["matcher"], "ExitPlanMode");
    assert!(groups[0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .contains("exitbind hook-run"));
    assert!(settings["hooks"]["SessionStart"][0]
        .get("matcher")
        .is_none());
    let repeat = call(
        &root,
        &[
            "hooks",
            "status",
            "--hosts",
            "claude",
            "--root",
            root.to_str().unwrap(),
        ],
        b"",
    );
    assert!(
        String::from_utf8_lossy(&repeat.stdout).contains("\"installed\""),
        "{repeat:?}"
    );
}

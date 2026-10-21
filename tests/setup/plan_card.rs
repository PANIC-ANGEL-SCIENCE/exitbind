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
    hook(
        root,
        tool,
        json!({"plan":plan,"isAgent":false,"filePath":PLAN_FILE}),
    )
}

fn hook(root: &Path, tool: &str, response: Value) -> Output {
    let payload = json!({"hook_event_name":"PostToolUse","session_id":"plan-probe",
        "cwd":root,"permission_mode":"default","tool_name":tool,"tool_input":{},
        "tool_response":response});
    let output = call(root, &["hook-run"], payload.to_string().as_bytes());
    assert!(output.status.success(), "{output:?}");
    output
}

fn exclude_path(root: &Path) -> std::path::PathBuf {
    let exclude = support::git_topology::git(root)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()
        .unwrap();
    root.join(String::from_utf8(exclude.stdout).unwrap().trim())
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

    // A second approval of the same plan keeps the same card; the exclude
    // line is written once.
    let again = approve(&root, "ExitPlanMode", PLAN);
    assert_eq!(again.stdout, stdout.as_bytes());
    let excluded = fs::read_to_string(exclude_path(&root)).unwrap();
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

/// D1: the shared exclude file is read strictly and replaced atomically. An
/// unreadable (here non-UTF-8) exclude file is never rewritten and no card is
/// saved; a readable one keeps its bytes and permissions and gains one line.
#[test]
fn an_unreadable_exclude_file_is_kept_and_a_readable_one_keeps_its_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let owned: Root = repository("plan-card-exclude");
    let root = owned.0.clone();
    setup(&root, None);
    let exclude = exclude_path(&root);
    let latin1 = b"# keep\n/caf\xe9/\n".to_vec();
    fs::write(&exclude, &latin1).unwrap();
    let refused = approve(&root, "ExitPlanMode", PLAN);
    assert!(refused.stdout.is_empty(), "{refused:?}");
    assert_eq!(fs::read(&exclude).unwrap(), latin1);
    assert!(!root.join(CARD).exists());

    fs::write(&exclude, "# keep\n/build").unwrap();
    fs::set_permissions(&exclude, fs::Permissions::from_mode(0o640)).unwrap();
    let saved = approve(&root, "ExitPlanMode", PLAN);
    assert!(!saved.stdout.is_empty(), "{saved:?}");
    assert_eq!(
        fs::read_to_string(&exclude).unwrap(),
        "# keep\n/build\n/.exitbind/plan-cards/\n"
    );
    let mode = fs::metadata(&exclude).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    let leftovers = fs::read_dir(exclude.parent().unwrap())
        .unwrap()
        .filter(|entry| {
            let name = entry.as_ref().unwrap().file_name();
            name.to_string_lossy().ends_with(".tmp")
        })
        .count();
    assert_eq!(leftovers, 0);
}

/// D2: a card is written only through real directories under the project and
/// never over an existing file: symlinked components are refused, and a later
/// different plan gets a fresh card so an edited card survives.
#[test]
fn a_card_never_follows_symlinks_or_overwrites_an_existing_card() {
    let outside = support::temp("plan-card-outside");
    let owned: Root = repository("plan-card-symlink-dir");
    let root = owned.0.clone();
    setup(&root, None);
    fs::create_dir_all(root.join(".exitbind")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join(".exitbind/plan-cards")).unwrap();
    let refused = approve(&root, "ExitPlanMode", PLAN);
    assert!(refused.stdout.is_empty(), "{refused:?}");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

    let owned: Root = repository("plan-card-symlink-file");
    let root = owned.0.clone();
    setup(&root, None);
    let target = outside.join("target.md");
    fs::write(&target, "target\n").unwrap();
    fs::create_dir_all(root.join(CARD).parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, root.join(CARD)).unwrap();
    let moved = approve(&root, "ExitPlanMode", PLAN);
    assert_eq!(fs::read_to_string(&target).unwrap(), "target\n");
    let line = String::from_utf8(moved.stdout).unwrap();
    assert!(
        line.contains("plan-cards/r9-quiet-river-2/TASK.md"),
        "{line}"
    );

    let owned: Root = repository("plan-card-no-overwrite");
    let root = owned.0.clone();
    setup(&root, None);
    approve(&root, "ExitPlanMode", PLAN);
    let edited = PLAN.replace("rejects empty input", "rejects blank input");
    fs::write(root.join(CARD), &edited).unwrap();
    let second = PLAN.replace("shows the count", "shows the total");
    let output = approve(&root, "ExitPlanMode", &second);
    let line = String::from_utf8(output.stdout).unwrap();
    assert!(
        line.contains("plan-cards/r9-quiet-river-2/TASK.md"),
        "{line}"
    );
    assert_eq!(fs::read_to_string(root.join(CARD)).unwrap(), edited);
    assert_eq!(
        fs::read_to_string(root.join(".exitbind/plan-cards/r9-quiet-river-2/TASK.md")).unwrap(),
        second
    );
    let status = porcelain(&root);
    assert!(!status.contains("plan-cards"), "{status}");
    let _ = fs::remove_dir_all(outside);
}

/// D3: a sub-agent's plan approval (or one that does not say who approved)
/// writes no card and prints nothing.
#[test]
fn a_sub_agent_plan_approval_writes_and_prints_nothing() {
    let owned: Root = repository("plan-card-agent");
    let root = owned.0.clone();
    setup(&root, None);
    for response in [
        json!({"plan":PLAN,"isAgent":true,"filePath":PLAN_FILE}),
        json!({"plan":PLAN,"filePath":PLAN_FILE}),
    ] {
        let output = hook(&root, "ExitPlanMode", response);
        assert!(output.stdout.is_empty(), "{output:?}");
    }
    assert!(!root.join(".exitbind/plan-cards").exists());
}

/// D4: a PostToolUse record without the ExitPlanMode matcher would run on
/// every tool call; status reports it as an ownership conflict, not installed.
#[test]
fn a_plan_approval_record_without_its_matcher_is_not_installed() {
    let owned: Root = repository("plan-card-matcher");
    let root = owned.0.clone();
    let status = |root: &Path| -> Value {
        let output = call(
            root,
            &[
                "hooks",
                "status",
                "--hosts",
                "claude",
                "--json",
                "--root",
                root.to_str().unwrap(),
            ],
            b"",
        );
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["hosts"][0].clone()
    };
    let applied = call(
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
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(status(&root)["state"], "installed");
    let path = root.join(".claude/settings.json");
    let mut settings: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    settings["hooks"]["PostToolUse"][0]
        .as_object_mut()
        .unwrap()
        .remove("matcher");
    fs::write(&path, settings.to_string()).unwrap();
    let report = status(&root);
    assert_eq!(report["state"], "conflict", "{report}");
    assert_eq!(report["exactHandlers"]["PostToolUse"], 0, "{report}");
}

/// A Claude Code activation hook installed by an earlier release lacks the
/// plan-approval record. `host status` names the state and the one action that
/// completes it, and that action does.
#[test]
fn a_partial_claude_activation_hook_names_host_install() {
    let owned: Root = repository("plan-card-host-partial");
    let root = owned.0.clone();
    let home = root.with_extension("home");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let record = json!({"type": "command",
        "command": "command -v exitbind >/dev/null 2>&1 && exitbind hook-run || true", "timeout": 5});
    let group = json!([{ "hooks": [record] }]);
    fs::write(
        home.join(".claude/settings.json"),
        json!({"hooks": {"SessionStart": group, "SubagentStart": group, "SubagentStop": group}})
            .to_string(),
    )
    .unwrap();
    let status = |args: &[&str]| -> String {
        let output = call(&root, args, b"");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let report: Value =
        serde_json::from_str(&status(&["host", "status", "--hosts", "claude", "--json"])).unwrap();
    assert_eq!(report["hosts"][0]["activationHook"], "partial", "{report}");
    assert_eq!(
        report["hosts"][0]["activationHookAction"],
        "exitbind host install"
    );
    let text = status(&["host", "status", "--hosts", "claude"]);
    assert!(
        text.contains("activation hook: partial; run 'exitbind host install'"),
        "{text}"
    );
    status(&["host", "install", "--hosts", "claude", "--json"]);
    let report: Value =
        serde_json::from_str(&status(&["host", "status", "--hosts", "claude", "--json"])).unwrap();
    assert_eq!(
        report["hosts"][0]["activationHook"], "installed",
        "{report}"
    );
    assert!(report["hosts"][0].get("activationHookAction").is_none());
    let _ = fs::remove_dir_all(home);
}

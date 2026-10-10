mod support;

use std::{fs, path::PathBuf, process::Command};

const EXITBIND_SKILL: &[u8] = include_bytes!("../skills/exitbind/SKILL.md");

fn init(root: &PathBuf) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(root)
        .output()
        .unwrap()
}

#[test]
fn setup_projects_current_guidance_without_starting_work() {
    let root = support::temp("exitbind-external-selection");
    let output = init(&root);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Selection: not performed by setup"));
    assert!(stdout.contains("Activation: not performed by setup"));
    assert!(stdout.contains("Fresh-session discovery: unverified"));

    for relative in [
        ".agents/skills/exitbind/SKILL.md",
        ".claude/skills/exitbind/SKILL.md",
    ] {
        assert_eq!(fs::read(root.join(relative)).unwrap(), EXITBIND_SKILL);
    }
    assert!(root.join("exitbind.json").is_file());
    assert_eq!(
        fs::read_dir(root.join(".exitbind/runs"))
            .unwrap()
            .filter_map(Result::ok)
            .count(),
        0,
        "setup must not activate a task"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_begin_records_the_first_current_work_item() {
    let root = support::temp("exitbind-external-selection-begin");
    let output = init(&root);
    assert!(output.status.success(), "{output:?}");
    let begin = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "explicit current task",
            "--check-command",
            "true",
            "--review-policy",
            "required",
            "--detail",
        ])
        .output()
        .unwrap();
    assert!(begin.status.success(), "{begin:?}");
    let response: serde_json::Value = serde_json::from_slice(&begin.stdout).unwrap();
    assert!(response["work"].as_str().unwrap().starts_with("smw_"));
    assert!(root
        .join(".exitbind/runs")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_current_skill_refuses_init_without_partial_projection_or_config() {
    let root = support::temp("exitbind-external-selection-stale-init");
    let stale = b"<!-- exitbind-managed-skill:v1 -->\nstale managed bytes\n";
    let stale_path = root.join(".agents/skills/exitbind/SKILL.md");
    fs::create_dir_all(stale_path.parent().unwrap()).unwrap();
    fs::write(&stale_path, stale).unwrap();

    let output = init(&root);
    assert!(!output.status.success(), "{output:?}");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("differs from embedded bytes"));
    assert!(diagnostics.contains("init --refresh-skills --root"));
    assert_eq!(fs::read(&stale_path).unwrap(), stale);
    assert!(!root.join("exitbind.json").exists());
    assert!(!root.join(".claude/skills/exitbind/SKILL.md").exists());
    fs::remove_dir_all(root).unwrap();
}

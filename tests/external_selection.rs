mod support;

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const SOULMATE_SKILL: &[u8] = include_bytes!("../skills/soulmate/SKILL.md");

fn fixture() -> PathBuf {
    let root = support::temp("external-selection");
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Project-scoped selective preference: confirmed"));
    assert!(stdout.contains("before any scoped implementation or mutation"));
    assert!(stdout
        .contains("proceeding only after it succeeds and returns a work handle and next action"));
    assert!(stdout.contains("Projection/materialization: confirmed"));
    assert!(stdout.contains("Fresh-session discovery: unverified"));
    assert!(stdout.contains("Active-session discovery: unverified"));
    assert!(stdout.contains("Selection: not performed by setup"));
    assert!(stdout.contains("Activation: not performed by setup"));
    assert!(stdout
        .contains("do not prove session discovery, future compliance, selection, or activation"));
    assert!(!stdout.contains("Fresh-session discovery: confirmed"));
    assert!(!stdout.contains("Active-session discovery: confirmed"));
    assert!(!stdout.contains("should use high-level"));
    root
}

fn run(root: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(root)
        .args(args)
        .arg("--config")
        .arg(root.join("soulmate.json"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn selection_guidance_projects_exactly_and_begin_confirms_machine_state() {
    let root = fixture();
    let agents = root.join(".agents/skills/soulmate/SKILL.md");
    let claude = root.join(".claude/skills/soulmate/SKILL.md");
    assert_eq!(fs::read(agents).unwrap(), SOULMATE_SKILL);
    assert_eq!(fs::read(claude).unwrap(), SOULMATE_SKILL);

    let skill = String::from_utf8_lossy(SOULMATE_SKILL);
    let normalized = skill.split_whitespace().collect::<Vec<_>>().join(" ");
    for phrase in [
        "available",
        "discovered",
        "selected",
        "activated",
        "high-level `soulmate work begin`",
        "select Soulmate automatically",
        "The user need not name",
        "explicit blocker",
        "Keep tiny, obvious, reversible work direct",
        "do not prove session discovery",
        "future compliance",
        "selection, or activation",
    ] {
        assert!(
            normalized.contains(phrase),
            "missing selection guidance: {phrase}"
        );
    }
    for phrase in ["for every task", "always use Soulmate"] {
        assert!(!normalized.to_lowercase().contains(&phrase.to_lowercase()));
    }
    let order = [
        "At the start of each material task, classify once",
        "select Soulmate automatically",
        "run `soulmate work begin` before any scoped implementation or mutation",
        "proceed only after it succeeds and returns a work handle and next action",
        "If unavailable or activation fails, stop before scoped work",
        "never silently downgrade or relabel a material trigger as advisory",
        "Direct fallback is allowed only for a genuinely advisory trigger",
        "Keep tiny, obvious, reversible work direct without activation",
    ];
    for pair in order.windows(2) {
        assert!(
            normalized.find(pair[0]).unwrap() < normalized.find(pair[1]).unwrap(),
            "selection guidance order changed: {:?}",
            pair
        );
    }

    let before = fs::read_dir(root.join(".soulmate/runs"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("work-"))
        .count();
    assert_eq!(before, 0);
    let begin = run(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "eligible task",
            "--check-command",
            "true",
        ],
    );
    let work = begin["work"].as_str().unwrap();
    assert!(work.starts_with("smw_"));
    assert!(begin["next"]["action"] == "lead_decision" || begin["next"]["action"] == "spawn");
    let ledgers = fs::read_dir(root.join(".soulmate/runs"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("work-"))
        .count();
    assert_eq!(ledgers, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fresh_init_rejects_stale_skill_without_partial_projection_or_config() {
    let root = support::temp("external-selection-stale-init");
    let stale = b"<!-- soulmate-managed-skill:v1 -->\nstale managed bytes\n";
    let stale_path = root.join(".agents/skills/soulmate/SKILL.md");
    fs::create_dir_all(stale_path.parent().unwrap()).unwrap();
    fs::write(&stale_path, stale).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("differs from embedded bytes"));
    assert!(diagnostics.contains("init --refresh-skills --root"));
    assert_eq!(fs::read(&stale_path).unwrap(), stale);
    assert!(!root.join("soulmate.json").exists());
    assert!(!root.join(".claude/skills/soulmate/SKILL.md").exists());
    fs::remove_dir_all(root).unwrap();
}

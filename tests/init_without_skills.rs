mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn temp(label: &str) -> PathBuf {
    support::temp(&format!("init-without-skills-{label}"))
}

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn init(root: &Path, extra: &[&str]) -> Output {
    let mut arguments = vec!["init", "--mode", "portable", "--root"];
    arguments.push(root.to_str().unwrap());
    arguments.extend_from_slice(extra);
    invoke(&arguments)
}

#[cfg(unix)]
#[test]
fn default_init_refuses_unsafe_skill_symlink_without_writes() {
    use std::os::unix::fs::symlink;

    let base = temp("default-symlink");
    let root = base.join("project");
    let outside = base.join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir_all(outside.join("exitbind")).unwrap();
    fs::write(outside.join("exitbind/SKILL.md"), b"operator skill\n").unwrap();
    fs::create_dir_all(root.join(".agents")).unwrap();
    symlink(&outside, root.join(".agents/skills")).unwrap();

    let refused = init(&root, &[]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("symlink"), "{}", text(&refused));
    assert!(!root.join("exitbind.json").exists());
    assert_eq!(
        fs::read(outside.join("exitbind/SKILL.md")).unwrap(),
        b"operator skill\n"
    );

    fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn skip_skills_preserves_symlink_targets_and_creates_setup() {
    use std::os::unix::fs::symlink;

    let base = temp("skip-symlink");
    let root = base.join("project");
    let outside_agents = base.join("agents-skills");
    let outside_claude = base.join("claude-skills");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside_agents).unwrap();
    fs::create_dir(&outside_claude).unwrap();
    fs::write(outside_agents.join("operator.md"), b"operator\n").unwrap();
    fs::write(outside_claude.join("operator.md"), b"operator\n").unwrap();
    fs::create_dir_all(root.join(".agents")).unwrap();
    fs::create_dir_all(root.join(".claude")).unwrap();
    symlink(&outside_agents, root.join(".agents/skills")).unwrap();
    symlink(&outside_claude, root.join(".claude/skills")).unwrap();

    let initialized = init(&root, &["--skip-skills"]);
    assert!(initialized.status.success(), "{}", text(&initialized));
    let output = text(&initialized);
    assert!(output.contains("Project skill projection skipped"));
    assert!(output.contains("Configuration:"));
    assert!(!output.contains(".agents/skills/exitbind/SKILL.md"));
    assert!(!output.contains("Projection/materialization: confirmed"));
    assert!(root.join("exitbind.json").is_file());
    assert!(root.join("exitbind/agents/worker.md").is_file());
    assert!(root.join(".exitbind/runs").is_dir());
    assert!(root.join(".agents/skills").is_symlink());
    assert!(root.join(".claude/skills").is_symlink());
    assert_eq!(fs::read_dir(&outside_agents).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&outside_claude).unwrap().count(), 1);
    assert_eq!(
        fs::read(outside_agents.join("operator.md")).unwrap(),
        b"operator\n"
    );
    assert_eq!(
        fs::read(outside_claude.join("operator.md")).unwrap(),
        b"operator\n"
    );

    let checked = invoke(&[
        "check",
        "--json",
        "--config",
        root.join("exitbind.json").to_str().unwrap(),
    ]);
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(text(&checked).contains("\"valid\":true"));

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn skip_skills_on_fresh_root_does_not_create_host_skill_directories() {
    let root = temp("fresh");

    let initialized = init(&root, &["--skip-skills"]);
    assert!(initialized.status.success(), "{}", text(&initialized));
    assert!(root.join("exitbind.json").is_file());
    assert!(root.join("exitbind/agents/lead.md").is_file());
    assert!(root.join(".exitbind/runs").is_dir());
    assert!(!root.join(".agents").exists());
    assert!(!root.join(".claude").exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn skip_skills_leaves_tracked_operator_skill_unchanged() {
    let root = temp("tracked-skill");
    fs::create_dir_all(root.join(".agents/skills/exitbind")).unwrap();
    fs::write(
        root.join(".agents/skills/exitbind/SKILL.md"),
        b"operator-owned skill\n",
    )
    .unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["config", "user.name", "Exitbind Test"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "config",
            "user.email",
            "exitbind-test@users.noreply.github.com"
        ])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["add", ".agents/skills/exitbind/SKILL.md"])
        .status()
        .unwrap()
        .success());

    let refused = init(&root, &[]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("tracked"), "{}", text(&refused));
    assert!(!root.join("exitbind.json").exists());

    let initialized = init(&root, &["--skip-skills"]);
    assert!(initialized.status.success(), "{}", text(&initialized));
    assert_eq!(
        fs::read(root.join(".agents/skills/exitbind/SKILL.md")).unwrap(),
        b"operator-owned skill\n"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn skip_skills_conflicts_fail_before_any_write() {
    let fresh = temp("flag-conflicts");
    let before = fs::read_dir(&fresh).unwrap().count();
    for extra in [
        ["--skip-skills", "--refresh-skills"],
        ["--skip-skills", "--with-coffee"],
    ] {
        let refused = init(&fresh, &extra);
        assert!(!refused.status.success(), "{}", text(&refused));
        assert!(text(&refused).contains("cannot be combined"));
        assert_eq!(fs::read_dir(&fresh).unwrap().count(), before);
    }
    fs::remove_dir_all(&fresh).unwrap();

    let root = temp("conflict");
    let config = root.join("exitbind.json");
    fs::write(&config, b"operator config\n").unwrap();

    let before = fs::read(&config).unwrap();
    let refused = init(&root, &["--skip-skills"]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("already exists"),
        "{}",
        text(&refused)
    );
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(!root.join("exitbind").exists());
    assert!(!root.join(".exitbind").exists());

    let refresh = invoke(&[
        "init",
        "--skip-skills",
        "--refresh-skills",
        "--root",
        root.to_str().unwrap(),
    ]);
    assert!(!refresh.status.success());
    assert!(text(&refresh).contains("cannot be combined"));

    fs::remove_dir_all(root).unwrap();
}

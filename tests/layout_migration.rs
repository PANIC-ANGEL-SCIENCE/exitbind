use serde_json::{json, Value};
mod support;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .unwrap()
}

fn legacy_project(label: &str) -> (PathBuf, String, Vec<u8>) {
    let root = support::temp(&format!("layout-migration-{label}"));
    let initialized = invoke(&["init", "--root", root.to_str().unwrap()]);
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let config_path = root.join("soulmate.json");
    let config = config_path.to_string_lossy().into_owned();
    let target = root.join("soulmate/agents/worker.md");
    let bytes = fs::read(&target).unwrap();
    let legacy = root.join(".agents/profiles");
    fs::create_dir_all(&legacy).unwrap();
    fs::rename(target, legacy.join("worker.md")).unwrap();
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    value["agents"]["worker"]["profile"] = json!(".agents/profiles/worker.md");
    fs::write(
        config_path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    (root, config, bytes)
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn git(root: &Path, arguments: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn layout_migration_is_deterministic_explicit_and_preserves_old_evidence() {
    let (root, config, bytes) = legacy_project("apply");
    let old_config = fs::read(&config).unwrap();
    let old_evidence = b"historical ledger bytes stay opaque\n";
    fs::create_dir_all(root.join(".soulmate/runs")).unwrap();
    fs::write(root.join(".soulmate/runs/prior.jsonl"), old_evidence).unwrap();

    let first = invoke(&["migrate", "layout", "--config", &config]);
    let second = invoke(&["migrate", "layout", "--config", &config]);
    assert!(first.status.success(), "{}", text(&first));
    assert_eq!(first.stdout, second.stdout);
    let plan: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(plan["mode"], "dry-run");
    assert_eq!(plan["status"], "ready");
    assert_eq!(plan["operations"].as_array().unwrap().len(), 1);
    assert_eq!(
        plan["operations"][0]["source"],
        ".agents/profiles/worker.md"
    );
    assert_eq!(plan["operations"][0]["target"], "soulmate/agents/worker.md");
    assert_eq!(fs::read(&config).unwrap(), old_config);
    assert!(!root.join("soulmate/agents/worker.md").exists());

    let applied = invoke(&["migrate", "layout", "--apply", "--config", &config]);
    assert!(applied.status.success(), "{}", text(&applied));
    let result: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(result["mode"], "apply");
    assert_eq!(result["status"], "applied");
    assert_eq!(
        fs::read(root.join("soulmate/agents/worker.md")).unwrap(),
        bytes
    );
    assert!(!root.join(".agents/profiles/worker.md").exists());
    let migrated: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(
        migrated["agents"]["worker"]["profile"],
        "soulmate/agents/worker.md"
    );
    assert_eq!(
        fs::read(root.join(".soulmate/runs/prior.jsonl")).unwrap(),
        old_evidence
    );
    assert!(invoke(&["check", "--config", &config]).status.success());

    let unchanged = invoke(&["migrate", "layout", "--apply", "--config", &config]);
    assert!(unchanged.status.success(), "{}", text(&unchanged));
    assert_eq!(
        serde_json::from_slice::<Value>(&unchanged.stdout).unwrap()["status"],
        "unchanged"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn layout_migration_refuses_collisions_without_mutation() {
    let (root, config, _) = legacy_project("collision");
    let old_config = fs::read(&config).unwrap();
    let source = root.join(".agents/profiles/worker.md");
    let old_source = fs::read(&source).unwrap();
    let target = root.join("soulmate/agents/worker.md");
    fs::write(&target, "operator-owned target\n").unwrap();

    for arguments in [
        vec!["migrate", "layout", "--config", &config],
        vec!["migrate", "layout", "--apply", "--config", &config],
    ] {
        let refused = invoke(&arguments);
        assert!(!refused.status.success());
        assert!(text(&refused).contains("migration target already exists"));
    }
    assert_eq!(fs::read(&config).unwrap(), old_config);
    assert_eq!(fs::read(source).unwrap(), old_source);
    assert_eq!(fs::read(target).unwrap(), b"operator-owned target\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn layout_migration_plans_tracked_files_but_refuses_to_apply() {
    let (root, config, _) = legacy_project("git");
    assert!(git(&root, &["init", "-q"]).status.success());
    assert!(git(&root, &["config", "user.name", "Soulmate Test"])
        .status
        .success());
    assert!(git(
        &root,
        &[
            "config",
            "user.email",
            "soulmate-test@users.noreply.github.com"
        ]
    )
    .status
    .success());
    assert!(git(
        &root,
        &["add", "soulmate.json", ".agents/profiles/worker.md"]
    )
    .status
    .success());
    assert!(git(&root, &["commit", "-qm", "legacy fixture"])
        .status
        .success());
    let old_config = fs::read(&config).unwrap();

    let planned = invoke(&["migrate", "layout", "--config", &config]);
    assert!(planned.status.success(), "{}", text(&planned));
    let refused = invoke(&["migrate", "layout", "--apply", "--config", &config]);
    assert!(!refused.status.success());
    assert!(text(&refused).contains("tracked or staged"));
    assert_eq!(fs::read(&config).unwrap(), old_config);
    assert!(root.join(".agents/profiles/worker.md").is_file());
    assert!(!root.join("soulmate/agents/worker.md").exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn layout_migration_refuses_symlinked_profile_sources() {
    use std::os::unix::fs::symlink;

    let (root, config, _) = legacy_project("symlink");
    let source = root.join(".agents/profiles/worker.md");
    fs::remove_file(&source).unwrap();
    symlink(root.join("soulmate/agents/lead.md"), &source).unwrap();
    let old_config = fs::read(&config).unwrap();

    let refused = invoke(&["migrate", "layout", "--config", &config]);
    assert!(!refused.status.success());
    assert!(text(&refused).contains("must not contain symlinks"));
    assert_eq!(fs::read(&config).unwrap(), old_config);
    assert!(fs::symlink_metadata(source)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_dir_all(root).unwrap();
}

#[allow(dead_code)]
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tooling/repository/release")
        .join(name)
}

fn run_check(archive: Option<&Path>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = support::git_topology::command("python3");
    command.arg(script("check-plugin-distribution.py"));
    if let Some(archive) = archive {
        command.args(["--archive", archive.to_str().unwrap()]);
    }
    let output = command.current_dir(root).output().unwrap();
    assert!(output.status.success(), "plugin check failed: {output:?}");
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink(), "unexpected source symlink: {entry:?}");
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn check_at(root: &Path) -> Output {
    support::git_topology::command("python3")
        .arg(script("check-plugin-distribution.py"))
        .arg("--repo-root")
        .arg(root)
        .output()
        .unwrap()
}

#[test]
fn validator_uses_the_selected_cargo_version_and_refuses_invalid_metadata() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = support::temp("plugin-versions");
    for directory in ["plugins/exitbind", "skills/exitbind"] {
        copy_tree(&source.join(directory), &root.join(directory));
    }
    for file in [
        ".agents/plugins/marketplace.json",
        ".claude-plugin/marketplace.json",
    ] {
        let target = root.join(file);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(source.join(file), target).unwrap();
    }
    let manifests = [
        "plugins/exitbind/plugin.json",
        "plugins/exitbind/.codex-plugin/plugin.json",
        "plugins/exitbind/.claude-plugin/plugin.json",
    ];
    let cargo = root.join("Cargo.toml");
    for version in ["9.8.7-rc.1", "9.8.7-alpha--beta+build--123"] {
        for file in manifests {
            let path = root.join(file);
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["version"] = serde_json::json!(version);
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        fs::write(&cargo, format!("[package]\nversion = \"{version}\"\n")).unwrap();
        let accepted = check_at(&root);
        assert!(accepted.status.success(), "{version}: {accepted:?}");
    }

    fs::write(&cargo, "[package]\nversion = \"9.8.8\"\n").unwrap();
    let mismatch = check_at(&root);
    assert!(!mismatch.status.success(), "{mismatch:?}");
    assert!(String::from_utf8_lossy(&mismatch.stdout).contains("plugin.json.version"));

    for invalid in [
        "[package]\nname = \"fixture\"\n",
        "[package]\nversion = \"bad\"\n",
        "[package]\nversion = \"9.8.7-rc.01\"\n",
        "[package]\nversion = \"9.8.7\"\nversion = \"9.8.8\"\n",
    ] {
        fs::write(&cargo, invalid).unwrap();
        let refused = check_at(&root);
        assert!(!refused.status.success(), "{invalid}: {refused:?}");
        assert!(String::from_utf8_lossy(&refused.stdout).contains("invalid package version"));
    }

    #[cfg(unix)]
    {
        let outside = root.join("other.toml");
        fs::write(&outside, "[package]\nversion = \"9.8.7-rc.1\"\n").unwrap();
        fs::remove_file(&cargo).unwrap();
        std::os::unix::fs::symlink(&outside, &cargo).unwrap();
        let refused = check_at(&root);
        assert!(!refused.status.success(), "{refused:?}");
        assert!(String::from_utf8_lossy(&refused.stdout).contains("symbolic link"));
    }
}

#[test]
fn isolated_distribution_and_catalogs_pass_the_deterministic_check() {
    run_check(None);
}

#[test]
fn archive_is_reproducible_and_existing_output_is_not_overwritten() {
    let output_dir = support::temp("plugin-archive");
    let first = output_dir.join("one.tar.gz");
    let second = output_dir.join("two.tar.gz");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for output in [&first, &second] {
        let result = support::git_topology::command("sh")
            .arg(script("package-plugin.sh"))
            .arg(output)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(result.status.success(), "package failed: {result:?}");
    }
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    run_check(Some(&first));

    let result = support::git_topology::command("sh")
        .arg(script("package-plugin.sh"))
        .arg(&first)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!result.status.success(), "existing output was overwritten");
    fs::remove_dir_all(output_dir).unwrap();
}

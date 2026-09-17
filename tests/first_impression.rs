//! Exercise the literal README entry in an existing Git project, not a copied recipe.
#![cfg(unix)]

mod support;
use std::{fs, os::unix::fs::symlink, path::Path, process::Command};

fn run_readme(command: &str, root: &Path, bin: &Path, temporary: &Path) -> String {
    let path = std::env::join_paths(
        std::iter::once(bin.to_owned())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = Command::new("sh")
        .args(["-eu", "-c", command])
        .current_dir(root)
        .env("PATH", path)
        .env("TMPDIR", temporary)
        .env("SOULMATE_BINDINGS_DIR", temporary.join("bindings"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "README command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn conversation_first_readme_keeps_setup_and_proof_safe_inside_git() {
    let base = support::temp("first-impression");
    let project = base.join("project with spaces");
    let bin = base.join("bin");
    let temporary = base.join("temporary");
    for path in [&project, &bin, &temporary] {
        fs::create_dir(path).unwrap();
    }
    symlink(env!("CARGO_BIN_EXE_exitbind"), bin.join("exitbind")).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(&project)
        .status()
        .unwrap()
        .success());
    fs::write(project.join("work.txt"), b"unfinished user work\n").unwrap();
    let git_status = || {
        let out = Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=all"])
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(out.status.success());
        out.stdout
    };
    let before = git_status();
    let readme = include_str!("../README.md");
    assert!(readme.starts_with("# Exitbind\n"));
    assert!(readme.contains("Current stable release: `v0.19.1`"));
    assert!(readme.contains("https://github.com/envy404/exitbind"));
    assert!(readme.contains("URL-only"));
    assert!(readme.contains("Exitbind progress"));
    let init = "exitbind init --mode portable --root .";
    let output = run_readme(init, &project, &bin, &temporary);
    assert!(git_status().len() > before.len());
    assert_eq!(
        fs::read(project.join("work.txt")).unwrap(),
        b"unfinished user work\n"
    );
    assert!(!project.join("soulmate.json").exists());
    assert_eq!(
        fs::read_dir(&temporary).unwrap().count(),
        0,
        "proof must clean up"
    );

    let skill = project.join(".agents/skills/exitbind/SKILL.md");
    let config = project.join("exitbind.json");
    assert!(config.is_file());
    assert!(skill.is_file());
    assert!(output.contains(skill.to_str().unwrap()));
    assert!(output.contains(config.to_str().unwrap()));
    assert!(output.contains("Bounded setup facts for your existing root agent"));
    assert!(!output.contains("Replace TASK"));
    assert!(output.contains("Setup does not start agents or grant host permissions."));
    assert!(output.contains("Project-scoped selective preference: confirmed"));
    assert!(output.contains("Fresh-session discovery: unverified"));
    assert!(output.contains("Activation: not performed by setup"));
    assert_eq!(
        fs::read(project.join("work.txt")).unwrap(),
        b"unfinished user work\n"
    );
    fs::remove_dir_all(base).unwrap();
}

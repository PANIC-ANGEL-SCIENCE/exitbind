//! Environment readiness before the first Work record (R56 G6).
//!
//! One resolver states the tested-input breadth, Git worktree and baseline,
//! checker program, build-target environment and host write posture. `check`
//! prints it; `setup --apply` and `work begin` refuse before writing only for
//! a checker file that exists but cannot run, with one route.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn bin() -> Command {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
}

fn git() -> Command {
    support::git_topology::command("git")
}

fn init(root: &Path) -> PathBuf {
    let output = bin()
        .args(["init", "--mode", "portable", "--skip-skills", "--root"])
        .arg(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    root.join("exitbind.json")
}

fn text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn work_ledgers(root: &Path) -> Vec<String> {
    let runs = root.join(".exitbind/runs");
    let Ok(entries) = fs::read_dir(runs) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.starts_with("work-"))
        .collect()
}

#[test]
fn a_non_git_root_with_many_entries_states_every_fact_before_any_record() {
    let Some(root) = support::marker_free_temp("environment-readiness-100k") else {
        return;
    };
    let config = init(&root);
    let bulk = root.join("bulk");
    fs::create_dir(&bulk).unwrap();
    for index in 0..100_000u32 {
        fs::File::create(bulk.join(format!("entry-{index:06}"))).unwrap();
    }

    let started = Instant::now();
    let output = bin()
        .args(["check", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    eprintln!("check on a 100000-entry non-Git root took {elapsed:?}");
    assert!(output.status.success(), "{}", text(&output));
    assert!(
        elapsed < Duration::from_secs(5),
        "check on a 10^5-entry root took {elapsed:?}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stdout.contains("configuration is valid (portable mode)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Environment readiness: attention"),
        "{stdout}"
    );
    assert!(
        stdout.contains("over the tested-input walk limit"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Git: not a Git worktree (no baseline commit)"),
        "{stdout}"
    );
    assert!(stdout.contains("checker: not stated"), "{stdout}");
    assert!(stdout.contains("host write:"), "{stdout}");
    assert!(work_ledgers(&root).is_empty(), "check wrote a Work record");

    let json: Value = serde_json::from_slice(
        &bin()
            .args(["check", "--json", "--config"])
            .arg(&config)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(json["valid"], true);
    assert_eq!(json["readiness"]["state"], "attention");
    assert_eq!(json["readiness"]["root"]["basis"], "walk");
    assert_eq!(json["readiness"]["root"]["withinLimit"], false);
    assert!(json["readiness"]["root"]["files"].as_u64().unwrap() > 4096);
    assert_eq!(json["readiness"]["git"]["state"], "not-git");
    assert_eq!(json["readiness"]["git"]["baseline"], Value::Null);
    assert_eq!(json["readiness"]["checker"]["state"], "not-stated");
    assert!(json["readiness"]["hostWrite"].get("hosts").is_some());
    assert_eq!(
        json["readiness"]["checkerEnvironment"]["targetOwnership"],
        "not stated"
    );

    // The same root as a Git worktree: the bulk stays ignored, so the tested
    // input count is small and the baseline commit exists.
    fs::write(
        root.join(".gitignore"),
        "bulk/\n.exitbind/\n.agents/\n.claude/\n",
    )
    .unwrap();
    assert!(git()
        .args(["init", "-q"])
        .arg(&root)
        .status()
        .unwrap()
        .success());
    assert!(git()
        .args([
            "-c",
            "user.name=Readiness",
            "-c",
            "user.email=readiness@example.invalid"
        ])
        .arg("-C")
        .arg(&root)
        .args(["add", "exitbind.json", ".gitignore"])
        .status()
        .unwrap()
        .success());
    let commit = git()
        .args([
            "-c",
            "user.name=Readiness",
            "-c",
            "user.email=readiness@example.invalid",
        ])
        .arg("-C")
        .arg(&root)
        .args(["commit", "-q", "-m", "fixture baseline"])
        .output()
        .unwrap();
    assert!(commit.status.success(), "{commit:?}");

    let git_output = bin()
        .args(["check", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(git_output.status.success(), "{}", text(&git_output));
    let git_stdout = String::from_utf8_lossy(&git_output.stdout).into_owned();
    assert!(
        git_stdout.contains("Environment readiness: ready"),
        "{git_stdout}"
    );
    assert!(git_stdout.contains("baseline commit "), "{git_stdout}");
    assert!(
        git_stdout.contains("within the 4096-path tested-input limit"),
        "{git_stdout}"
    );
    let git_json: Value = serde_json::from_slice(
        &bin()
            .args(["check", "--json", "--config"])
            .arg(&config)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(git_json["readiness"]["state"], "ready");
    assert_eq!(git_json["readiness"]["git"]["state"], "worktree");
    assert_eq!(git_json["readiness"]["root"]["withinLimit"], true);
    assert_eq!(
        git_json["readiness"]["git"]["baseline"]
            .as_str()
            .map(str::len),
        Some(40)
    );
}

#[test]
fn a_non_executable_checker_refuses_before_the_first_write_with_one_route() {
    let root = support::temp("environment-readiness-checker");
    let config = init(&root);
    let checker = root.join("check.sh");
    fs::write(&checker, b"#!/bin/sh\nexit 0\n").unwrap();

    let output = bin()
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "readiness refusal fixture",
            "--review-policy",
            "omitted",
            "--standalone",
        ])
        .arg("--check-command")
        .arg(&checker)
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{}", text(&output));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("work begin refused before recording a Work"),
        "{stderr}"
    );
    assert!(stderr.contains("is not executable"), "{stderr}");
    assert!(
        stderr.contains(&format!("chmod +x {}", checker.display())),
        "{stderr}"
    );
    assert!(work_ledgers(&root).is_empty(), "refusal recorded a Work");
}

#[test]
fn setup_apply_refuses_a_non_executable_checker_before_writing() {
    let root = support::temp("environment-readiness-setup");
    let checker = root.join("check.sh");
    fs::write(&checker, b"#!/bin/sh\nexit 0\n").unwrap();

    let output = bin()
        .args([
            "setup", "--apply", "--mode", "portable", "--scope", "worker", "--root",
        ])
        .arg(&root)
        .arg("--check-command")
        .arg(&checker)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{}", text(&output));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("setup refused before writing; no files changed"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("chmod +x {}", checker.display())),
        "{stderr}"
    );
    assert!(
        root.join("exitbind.json").symlink_metadata().is_err(),
        "setup wrote the configuration despite the refusal"
    );
}

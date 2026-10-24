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
        stderr.contains(&format!("chmod +x '{}'", checker.display())),
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
        stderr.contains(&format!("chmod +x '{}'", checker.display())),
        "{stderr}"
    );
    assert!(
        root.join("exitbind.json").symlink_metadata().is_err(),
        "setup wrote the configuration despite the refusal"
    );
}

fn check_json(config: &Path) -> Value {
    let output = bin()
        .args(["check", "--json", "--config"])
        .arg(config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn begin(config: &Path, command: &str) -> Output {
    bin()
        .args([
            "work",
            "begin",
            "change",
            "--goal",
            "readiness rework fixture",
            "--review-policy",
            "omitted",
            "--standalone",
            "--check-command",
        ])
        .arg(command)
        .arg("--config")
        .arg(config)
        .output()
        .unwrap()
}

// B1: an unreadable subdirectory must not be reported as a complete count.
#[test]
fn an_unreadable_subdirectory_is_unknown_not_within_limit() {
    use std::os::unix::fs::PermissionsExt;

    let Some(root) = support::marker_free_temp("environment-readiness-unreadable") else {
        return;
    };
    let config = init(&root);
    let readable = root.join("a");
    fs::create_dir(&readable).unwrap();
    for index in 0..4u32 {
        fs::File::create(readable.join(format!("entry-{index:05}"))).unwrap();
    }
    let blocked = root.join("zz");
    fs::create_dir(&blocked).unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&blocked).is_ok() {
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("coverage unavailable: the operator can list a 0o000 directory");
        return;
    }

    let output = bin()
        .args(["check", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    let json = check_json(&config);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(output.status.success(), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stdout.contains("Environment readiness: attention"),
        "{stdout}"
    );
    assert!(stdout.contains("not counted"), "{stdout}");
    assert!(
        stdout.contains("zz"),
        "the cause must name the directory: {stdout}"
    );
    assert!(
        !stdout.contains("within the tested-input walk limit"),
        "{stdout}"
    );
    assert_eq!(json["readiness"]["root"]["withinLimit"], Value::Null);
    assert!(
        json["readiness"]["root"]["detail"]
            .as_str()
            .unwrap_or("")
            .contains("zz"),
        "{}",
        json["readiness"]["root"]["detail"]
    );
}

// B2: the printed route must run unedited, including roots with spaces.
#[test]
fn the_printed_route_runs_edited_in_a_root_with_spaces() {
    use std::os::unix::fs::PermissionsExt;

    let root = support::temp("environment readiness route");
    let config = init(&root);
    let checker = root.join("check.sh");
    fs::write(&checker, b"#!/bin/sh\nexit 0\n").unwrap();

    let output = begin(&config, "./check.sh");
    assert!(!output.status.success(), "{}", text(&output));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("is not executable"), "{stderr}");
    let start = stderr.find("run `").expect("a printed route") + 5;
    let end = start + stderr[start..].find('`').expect("route end");
    let route = &stderr[start..end];
    assert!(route.starts_with("chmod +x '"), "{route}");
    assert!(
        !route.contains("/./"),
        "the printed path must be normalised: {route}"
    );
    let status = support::git_topology::command("sh")
        .args(["-c", route])
        .status()
        .unwrap();
    assert!(status.success(), "the printed route did not run: {route}");
    assert_ne!(
        fs::metadata(&checker).unwrap().permissions().mode() & 0o111,
        0,
        "the printed route did not make the checker executable"
    );
}

// B3: no unconditional build-target claim; CARGO_TARGET_DIR is respected.
#[test]
fn build_target_facts_are_conditional_and_respect_cargo_target_dir() {
    let root = support::temp("environment-readiness-target");
    let config = init(&root);
    let target = root.join("shared-target");
    fs::create_dir(&target).unwrap();

    let output = bin()
        .env("CARGO_TARGET_DIR", &target)
        .args(["check", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.contains("checker environment:"), "{stdout}");
    assert!(stdout.contains("target directory present"), "{stdout}");
    assert!(
        stdout.contains(&target.display().to_string()),
        "the line must name the inherited target: {stdout}"
    );
    assert!(!stdout.contains("build-target conflict"), "{stdout}");
    let json: Value = serde_json::from_slice(
        &bin()
            .env("CARGO_TARGET_DIR", &target)
            .args(["check", "--json", "--config"])
            .arg(&config)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(
        json["readiness"]["checkerEnvironment"]["targetPresent"],
        Value::Bool(true)
    );
    assert_eq!(
        json["readiness"]["checkerEnvironment"]["cargoTargetDir"],
        Value::String(target.display().to_string())
    );

    let plain = support::temp("environment-readiness-no-target");
    let plain_config = init(&plain);
    let plain_output = bin()
        .env_remove("CARGO_TARGET_DIR")
        .args(["check", "--config"])
        .arg(&plain_config)
        .output()
        .unwrap();
    let plain_stdout = String::from_utf8_lossy(&plain_output.stdout).into_owned();
    assert!(
        !plain_stdout.contains("checker environment"),
        "a project without a build target states no target facts: {plain_stdout}"
    );
    assert!(
        !plain_stdout.contains("build-target conflict"),
        "{plain_stdout}"
    );
}

// D1: a failing git listing is unknown, never "0 paths".
#[test]
fn a_failing_git_listing_is_unknown_not_zero_paths() {
    let root = support::temp("environment-readiness-git-listing");
    let config = init(&root);
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
        .args(["add", "exitbind.json"])
        .status()
        .unwrap()
        .success());
    assert!(git()
        .args([
            "-c",
            "user.name=Readiness",
            "-c",
            "user.email=readiness@example.invalid",
        ])
        .arg("-C")
        .arg(&root)
        .args(["commit", "-q", "-m", "fixture baseline"])
        .status()
        .unwrap()
        .success());
    fs::write(root.join(".git/index"), b"broken index\n").unwrap();

    let output = bin()
        .args(["check", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.contains("not counted"), "{stdout}");
    assert!(
        stdout.contains("Git could not list the tested inputs"),
        "the cause must be stated: {stdout}"
    );
    assert!(!stdout.contains("0 paths listed by Git"), "{stdout}");
    let json = check_json(&config);
    assert_eq!(json["readiness"]["git"]["state"], "worktree");
    assert_eq!(json["readiness"]["root"]["withinLimit"], Value::Null);
    assert_eq!(json["readiness"]["root"]["files"], Value::Null);
    assert!(json["readiness"]["root"]["detail"].is_string());
}

// D2: the versioned output shape is registered, with a retirement condition.
#[test]
fn the_readiness_shape_is_registered_in_the_compatibility_table() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tooling/repository/checks/compatibility-markers.json");
    let table: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let row = table["markers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["marker"] == "environment-readiness-v1")
        .expect("compatibility-markers.json must register environment-readiness-v1");
    assert_eq!(row["kind"], "text");
    assert!(row["writers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|writer| writer == "src/project/readiness.rs"));
    assert!(!row["retire"].as_str().unwrap_or("").is_empty());
}

// D3: the human setup preview prints the same readiness section as check.
#[test]
fn setup_preview_prints_the_same_readiness_section() {
    let root = support::temp("environment-readiness-setup-preview");
    let checker = root.join("check.sh");
    fs::write(checker, b"#!/bin/sh\nexit 0\n").unwrap();

    let output = bin()
        .args(["setup", "--mode", "portable", "--scope", "worker", "--root"])
        .arg(&root)
        .arg("--check-command")
        .arg("./check.sh")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stdout.contains("Environment readiness: attention"),
        "{stdout}"
    );
    assert!(stdout.contains("exists but is not executable"), "{stdout}");
    assert!(stdout.contains("chmod +x '"), "{stdout}");
    assert!(
        !root.join("exitbind.json").exists(),
        "a preview must not write the configuration"
    );
}

// D4: classify_activation is live; no stale dead-code suppression.
#[test]
fn classify_activation_is_not_suppressed_as_dead_code() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/host/runtime/activation.rs");
    let source = fs::read_to_string(path).unwrap();
    let suppressed = source
        .lines()
        .zip(source.lines().skip(1))
        .any(|(line, next)| {
            line.trim() == "#[allow(dead_code)]"
                && next
                    .trim_start()
                    .starts_with("pub(crate) fn classify_activation(")
        });
    assert!(
        !suppressed,
        "classify_activation is live (assess_activation and work begin call it); remove the stale #[allow(dead_code)]"
    );
}

// D5: environment prefixes and quoted paths reach the same executable test.
#[test]
fn environment_assignments_and_quoted_paths_use_the_same_checker_test() {
    let root = support::temp("environment readiness quoted");
    let config = init(&root);
    let checker = root.join("check.sh");
    fs::write(&checker, b"#!/bin/sh\nexit 0\n").unwrap();

    for command in [
        "X=1 ./check.sh".to_owned(),
        format!("\"{}\"", checker.display()),
    ] {
        let output = begin(&config, &command);
        assert!(
            !output.status.success(),
            "the checker must be tested through the shell prefix/quoting: {command}\n{}",
            text(&output)
        );
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(stderr.contains("is not executable"), "{command}: {stderr}");
        assert!(stderr.contains("chmod +x '"), "{command}: {stderr}");
    }
    assert!(work_ledgers(&root).is_empty(), "a refusal recorded a Work");

    // A shell program that runs the script is not the script itself: no refusal.
    let output = begin(&config, "sh ./check.sh");
    assert!(
        output.status.success(),
        "a shell invocation must not be refused as a non-executable checker: {}",
        text(&output)
    );
}

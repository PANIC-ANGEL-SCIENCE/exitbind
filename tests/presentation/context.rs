#[path = "../support/mod.rs"]
mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
    thread,
};

const SESSION_GOAL_CARD_TEST: &str =
    "+----------------------------+\n| This room remains nothing. |\n+----------------------------+";

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn reseal(value: &mut Value) {
    value.as_object_mut().unwrap().remove("eventSha256");
    let mut digest = Sha256::new();
    digest.update(canonical(value).as_bytes());
    let hash = format!("{:x}", digest.finalize());
    value["eventSha256"] = Value::String(hash);
}

struct Project {
    root: std::path::PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        Self { root }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.root).args(args);
        if args.first() == Some(&"work") && matches!(args.get(1), Some(&"next" | &"resume")) {
            command.arg("--full");
        }
        command
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(bytes) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            return child.wait_with_output().unwrap();
        }
        command.output().unwrap()
    }

    fn value(&self, args: &[&str], input: Option<&[u8]>) -> Value {
        let output = self.call(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn recorded_check_failure(&self, work: &str) -> Value {
        let output = self.call(&["work", "check", work], None);
        assert!(
            !output.status.success(),
            "failed check unexpectedly succeeded"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["effect"], "recorded", "{value}");
        assert!(
            value["result"]["signal"].is_number()
                || value["result"]["code"]
                    .as_i64()
                    .is_some_and(|code| code != 0),
            "recorded check did not report failure: {value}"
        );
        assert_eq!(value["next"]["action"], "lead_decision", "{value}");
        assert_eq!(value["next"]["role"], "lead", "{value}");
        value
    }

    fn presentation(&self, work: &str) -> Value {
        self.value(&["work", "next", work], None)["presentation"].clone()
    }

    /// Drive the façade until the given action is pending, answering scripted
    /// results on the way.
    fn drive_until(&self, work: &str, action: &str) -> Value {
        loop {
            let next = self.value(&["work", "next", work], None)["next"].clone();
            // The scope decision shares the lead_decision action; only the
            // acceptance decision ends this drive.
            let scope_stage = next["outcomes"][0] == "scoped";
            if next["action"] == action && !(action == "lead_decision" && scope_stage) {
                return next;
            }
            match next["action"].as_str().unwrap() {
                "check" => {
                    self.value(&["work", "check", work], None);
                }
                "spawn" => {
                    let role = next["role"].as_str().unwrap();
                    let outcome = if role == "reviewer" {
                        "approved"
                    } else {
                        "completed"
                    };
                    self.value(
                        &[
                            "work",
                            "return",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--outcome",
                            outcome,
                        ],
                        Some(b"scripted"),
                    );
                }
                "lead_decision" if next["outcomes"][0] == "scoped" => {
                    self.value(
                        &[
                            "work",
                            "return",
                            work,
                            next["assignment"].as_str().unwrap(),
                            "--outcome",
                            "scoped",
                        ],
                        Some(b"scope"),
                    );
                }
                other => panic!("unexpected action {other}"),
            }
        }
    }
}

fn begin(project: &Project, preservation: bool) -> String {
    fs::write(project.root.join("source.txt"), b"env-first\n").unwrap();
    fs::write(
        project.root.join("check.sh"),
        b"#!/bin/sh\ngrep -q env-first source.txt\n",
    )
    .unwrap();
    fs::set_permissions(
        project.root.join("check.sh"),
        <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let mut args = vec![
        "work",
        "begin",
        "change",
        "--goal",
        "make it hold",
        "--check-command",
        "sh check.sh",
    ];
    if preservation {
        args.extend([
            "--preserve-requirement",
            "precedence:environment wins",
            "--preservation-check-command",
            "sh check.sh",
        ]);
    }
    project.value(&args, None)["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The presentation cache is derived, replaceable memory. These cases hold the
/// line that losing it, or finding something hostile in its place, costs at
/// most a repeated phrase and never touches authority, evidence, or another
/// file.
mod cache {
    use super::*;

    fn cache_file(project: &Project, work: &str) -> std::path::PathBuf {
        project
            .root
            .join(".exitbind/presentation")
            .join(format!("{work}.json"))
    }

    /// Drive a run to a passing check so the cache holds a real document.
    fn primed(label: &str) -> (Project, String) {
        let project = Project::new(label);
        let work = begin(&project, false);
        project.drive_until(&work, "check");
        project.value(&["work", "check", &work], None);
        project.presentation(&work);
        (project, work)
    }

    #[test]
    fn a_symlink_at_the_cache_file_overwrites_nothing() {
        let (project, work) = primed("cache-symlink");
        let sentinel = project.root.join("sentinel.txt");
        fs::write(&sentinel, b"do not touch\n").unwrap();
        let cache = cache_file(&project, &work);
        fs::remove_file(&cache).unwrap();
        std::os::unix::fs::symlink(&sentinel, &cache).unwrap();

        // The read refuses to follow it and the write replaces the name, not
        // its target, so the work state still projects normally.
        let seen = project.presentation(&work);
        assert!(seen["neuro"].as_str().unwrap().starts_with("[Neuro] "));
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "do not touch\n");
        fs::remove_dir_all(project.root).unwrap();
    }

    #[test]
    fn an_unsafe_parent_is_refused_without_disturbing_the_run() {
        let (project, work) = primed("cache-parent");
        let elsewhere = project.root.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let directory = project.root.join(".exitbind/presentation");
        fs::remove_dir_all(&directory).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &directory).unwrap();

        let seen = project.presentation(&work);
        assert_eq!(seen["state"]["check"], "current");
        assert!(!elsewhere.join(format!("{work}.json")).exists());
        fs::remove_dir_all(project.root).unwrap();
    }

    #[test]
    fn a_cache_it_cannot_read_is_left_exactly_as_found() {
        for (label, bytes) in [
            ("cache-corrupt", b"{ not json".to_vec()),
            ("cache-oversized", vec![b'x'; 128 * 1024]),
        ] {
            let (project, work) = primed(label);
            let cache = cache_file(&project, &work);
            fs::write(&cache, &bytes).unwrap();

            // Unreadable memory reads as no memory: the state is still
            // classified from the run, and the file the process could not read
            // is not replaced by one it invented.
            let seen = project.presentation(&work);
            assert_eq!(seen["state"]["check"], "current");
            assert_eq!(seen["exitState"], "IN_PROGRESS");
            assert_eq!(fs::read(&cache).unwrap(), bytes, "{label}");
            fs::remove_dir_all(project.root).unwrap();
        }
    }

    #[test]
    fn an_absent_or_unwritable_cache_changes_no_decision() {
        let (project, work) = primed("cache-unwritable");
        let directory = project.root.join(".exitbind/presentation");
        fs::remove_dir_all(&directory).unwrap();
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(
            &directory,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o500),
        )
        .unwrap();

        let seen = project.presentation(&work);
        assert_eq!(seen["state"]["check"], "current");
        // Reuse is still offered from the run state, not from the cache.
        let packet = project.value(&["work", "next", &work], None)["residual"].clone();
        assert!(packet["doNotRepeat"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "passed_check"));

        fs::set_permissions(
            &directory,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        )
        .unwrap();
        fs::remove_dir_all(project.root).unwrap();
    }
}

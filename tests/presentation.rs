//! The conversational surface: what Exitbind says, when it says it, and when it
//! stays quiet. Every line here is derived from canonical run state.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

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
        command
            .current_dir(&self.root)
            .args(args)
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

#[test]
fn an_unchanged_state_is_read_without_repeating_the_line() {
    let project = Project::new("presentation-unchanged");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);

    let first = project.presentation(&work);
    assert_eq!(first["transition"], "evidence_became_current");
    assert_eq!(first["phrase"], "It fits now.");
    for _ in 0..3 {
        let again = project.presentation(&work);
        assert!(again["transition"].is_null(), "{again}");
        assert!(again["phrase"].is_null());
        // The compact status surface stays available.
        assert!(again["neuro"]
            .as_str()
            .unwrap()
            .starts_with("[Neuro] Exitbind progress: "));
    }
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_check_that_goes_stale_speaks_once_and_recovers_once() {
    let project = Project::new("presentation-stale-check");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);
    project.presentation(&work);

    // A covered input changes: the passing check no longer belongs here.
    fs::write(project.root.join("source.txt"), b"env-first\nmore\n").unwrap();
    let stale = project.presentation(&work);
    assert_eq!(stale["transition"], "check_became_stale");
    assert_eq!(stale["phrase"], "That key fit the earlier result.");
    assert_eq!(stale["state"]["check"], "stale");
    assert!(project.presentation(&work)["transition"].is_null());

    project.value(&["work", "check", &work], None);
    let current = project.presentation(&work);
    assert_eq!(current["transition"], "evidence_became_current");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_review_that_goes_stale_speaks_once() {
    let project = Project::new("presentation-stale-review");
    let work = begin(&project, false);
    project.drive_until(&work, "lead_decision");
    project.presentation(&work);

    // A covered input changes after the review: the checks must run again, and
    // the approval that belongs to the earlier files cannot carry the result.
    fs::write(project.root.join("source.txt"), b"env-first\nchanged\n").unwrap();
    project.presentation(&work);
    project.value(&["work", "check", &work], None);
    let stale = project.presentation(&work);
    assert_eq!(stale["state"]["review"], "stale");
    assert_eq!(stale["transition"], "review_became_stale");
    assert_eq!(stale["phrase"], "The review is one step behind.");
    assert!(project.presentation(&work)["transition"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn preservation_speaks_when_it_becomes_active_and_then_stays_quiet() {
    let project = Project::new("presentation-preservation");
    let work = begin(&project, true);
    project.drive_until(&work, "check");

    let active = project.presentation(&work);
    assert_eq!(active["state"]["preservation"], "active");
    let holytail = active["holytail"].as_str().unwrap();
    // The accepted requirements resolve the route, and the accepted policy
    // assigns FULL to the formal route. The evidence axis reports how the
    // evidence was acquired, never that anything was independently verified.
    // No requirement check has run yet, so the evidence axis says exactly that.
    assert_eq!(
        holytail, "Holytail :FULL · FORMAL · evidence=none",
        "{holytail}"
    );

    let again = project.presentation(&work);
    assert!(again["transition"].is_null(), "{again}");
    assert_eq!(again["holytail"], active["holytail"]);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn terminal_readiness_says_exit_ready_and_nothing_else() {
    let project = Project::new("presentation-ready");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    let accepted = project.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    assert_eq!(accepted["next"]["progress"]["percent"], 100);

    let ready = project.presentation(&work);
    assert_eq!(ready["exitState"], "READY");
    assert!(
        ready["transition"].is_null(),
        "READY takes no flavour line: {ready}"
    );
    assert!(ready["phrase"].is_null());
    assert_eq!(ready["neuro"], "[Neuro] Exitbind progress: 100%.");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn resuming_with_valid_evidence_can_say_so_once() {
    let project = Project::new("presentation-resume");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);
    // A fresh consumer has no memory of what was presented before.
    fs::remove_dir_all(project.root.join(".exitbind/presentation")).unwrap();

    let resumed = project.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    let presentation = &resumed["presentation"];
    assert_eq!(presentation["transition"], "resumed_with_valid_evidence");
    assert_eq!(presentation["phrase"], "That door is already open.");
    assert!(project.presentation(&work)["transition"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn without_governed_work_there_is_no_progress_or_holytail_line() {
    let project = Project::new("presentation-none");
    let idle = project.value(&["work", "resume"], None);
    assert_eq!(idle["status"], "none");
    assert!(idle["presentation"].is_null(), "{idle}");
    assert!(
        !String::from_utf8_lossy(&project.call(&["work", "resume"], None).stdout)
            .contains("[Neuro]")
    );
    fs::remove_dir_all(project.root).unwrap();
}

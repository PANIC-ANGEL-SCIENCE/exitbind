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
    assert!(first["terminal"].is_null());
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
    assert!(active["terminal"].is_null());
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
    // The decision itself hands back the block to print, so nothing has to be
    // assembled from status and progress.
    assert_eq!(accepted["presentation"]["terminal"], "EXIT READY");
    assert!(accepted["presentation"]["phrase"].is_null());

    let ready = project.presentation(&work);
    assert_eq!(ready["exitState"], "READY");
    assert_eq!(ready["terminal"], "EXIT READY");
    assert!(
        ready["transition"].is_null(),
        "READY takes no flavour line: {ready}"
    );
    assert!(ready["phrase"].is_null());
    assert_eq!(ready["neuro"], "[Neuro] Exitbind progress: 100%.");
    let reread = project.presentation(&work);
    assert_eq!(reread["terminal"], "EXIT READY");
    assert!(reread["transition"].is_null());
    assert!(reread["phrase"].is_null());
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

/// `EXIT READY` is the canonical acceptance decision speaking, not a formatting
/// choice. Nothing short of that decision may reach the terminal block, and
/// suppressing the decoration changes no machine value.
#[test]
fn only_the_accepted_decision_reaches_the_terminal_block() {
    // Checks pass, review approves, acceptance is still pending.
    let pending = Project::new("terminal-pending");
    let work = begin(&pending, false);
    let decision = pending.drive_until(&work, "lead_decision");
    let waiting = pending.presentation(&work);
    assert!(waiting["terminal"].is_null(), "{waiting}");
    assert_eq!(waiting["state"]["check"], "current");
    assert_eq!(waiting["state"]["review"], "approved");
    assert_ne!(waiting["progress"], 100);

    // The lead blocks instead. The run is terminal, and still not ready.
    let refused_decision = pending.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"blocked"),
    );
    assert!(refused_decision["presentation"]["terminal"].is_null());
    let blocked = pending.presentation(&work);
    assert!(blocked["terminal"].is_null(), "{blocked}");
    assert_ne!(blocked["exitState"], "READY");
    fs::remove_dir_all(pending.root).unwrap();

    // A failing check is evidence against readiness, never a route to it.
    let failing = Project::new("terminal-failing");
    let broken = begin(&failing, false);
    failing.drive_until(&broken, "check");
    fs::write(failing.root.join("source.txt"), b"nothing it looks for\n").unwrap();
    failing.value(&["work", "check", &broken], None);
    let refused = failing.presentation(&broken);
    assert!(refused["terminal"].is_null(), "{refused}");
    assert_eq!(refused["state"]["check"], "failed");
    fs::remove_dir_all(failing.root).unwrap();

    // Work that was never governed has no terminal block to copy.
    let idle = Project::new("terminal-idle");
    let none = idle.value(&["work", "resume"], None);
    assert_eq!(none["status"], "none");
    assert!(none["presentation"].is_null());
    fs::remove_dir_all(idle.root).unwrap();
}

/// The terminal block is display only: the machine record beneath it keeps its
/// decision, evidence, and reuse values exactly as the run recorded them.
#[test]
fn the_terminal_block_changes_no_machine_value() {
    let project = Project::new("terminal-machine");
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
    let seen = project.value(&["work", "next", &work], None);

    // Valid JSON with the recorded decision intact beside the display value.
    assert_eq!(seen["presentation"]["terminal"], "EXIT READY");
    assert_eq!(seen["next"]["progress"]["state"], "READY");
    assert_eq!(seen["next"]["progress"]["percent"], 100);
    assert_eq!(
        seen["next"]["progress"]["state"],
        accepted["next"]["progress"]["state"]
    );
    assert_eq!(seen["residual"]["historical"], true);
    // Acceptance is history: it authorizes no further work.
    let help = seen["residual"]["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap();
    assert!(help.contains("history"), "{help}");
    fs::remove_dir_all(project.root).unwrap();
}

/// A finished run is where "what is the state here?" actually lands, and an
/// acceptance speaks for the present only while the tree it was bound to is
/// still the one on disk.
#[test]
fn a_finished_run_is_reported_and_stops_speaking_when_the_tree_moves() {
    let project = Project::new("terminal-resume");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    project.value(
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

    // Nothing is active, and the product still supplies the block to print.
    let settled = project.value(&["work", "resume"], None);
    assert_eq!(settled["status"], "none");
    assert_eq!(settled["recent"]["work"], work.as_str());
    assert_eq!(settled["recent"]["exitState"], "READY");
    assert_eq!(settled["presentation"]["terminal"], "EXIT READY");

    // One covered file changes: the acceptance is history again, so the
    // terminal block goes quiet while the recorded decision stays readable.
    fs::write(project.root.join("source.txt"), b"env-first\nlater edit\n").unwrap();
    let moved = project.value(&["work", "resume"], None);
    assert_eq!(moved["recent"]["work"], work.as_str());
    assert_eq!(moved["recent"]["exitState"], "READY");
    assert!(
        moved["presentation"]["terminal"].is_null(),
        "history must not speak for a tree that changed under it: {moved}"
    );
    let direct = project.value(&["work", "next", &work], None);
    assert!(direct["presentation"]["terminal"].is_null());
    let help = direct["residual"]["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap();
    assert!(help.contains("history"), "{help}");

    // Restoring the exact tested inputs restores the decision's reach.
    fs::write(project.root.join("source.txt"), b"env-first\n").unwrap();
    let restored = project.value(&["work", "resume"], None);
    assert_eq!(restored["presentation"]["terminal"], "EXIT READY");
    fs::remove_dir_all(project.root).unwrap();
}

/// The display memo is not the run record. It says so in its own bytes,
/// because a live host was observed reading it off disk and reporting it as
/// the status instead of asking the CLI.
#[test]
fn the_display_memo_names_itself_and_claims_no_authority() {
    let project = Project::new("memo-self-describing");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);
    project.presentation(&work);

    let memo: Value = serde_json::from_str(
        &fs::read_to_string(
            project
                .root
                .join(".exitbind/presentation")
                .join(format!("{work}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(memo["kind"], "derived_display_memo");
    assert_eq!(memo["authority"], "none");
    // It carries no decision of its own: no terminal block, no evidence.
    assert!(memo["terminal"].is_null());
    assert!(memo["classification"]["exitState"].is_string());

    // A memo that does not say what it is reads as nothing, and is left alone.
    let path = project
        .root
        .join(".exitbind/presentation")
        .join(format!("{work}.json"));
    let foreign = "{\"classification\":{\"exitState\":\"READY\"},\"signature\":\"x\"}\n";
    fs::write(&path, foreign).unwrap();
    let seen = project.presentation(&work);
    assert_ne!(seen["exitState"], "READY");
    assert_eq!(fs::read_to_string(&path).unwrap(), foreign);
    fs::remove_dir_all(project.root).unwrap();
}

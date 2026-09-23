//! The conversational surface: what Exitbind says, when it says it, and when it
//! stays quiet. Every line here is derived from canonical run state.
#![cfg(unix)]

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
    "+------------------------------+\n| Nothing remains here.        |\n+------------------------------+\n\nEXIT READY";

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
fn preservation_state_keeps_retired_line_null_and_stays_quiet() {
    let project = Project::new("presentation-preservation");
    let work = begin(&project, true);
    project.drive_until(&work, "check");

    let active = project.presentation(&work);
    assert_eq!(active["state"]["preservation"], "active");
    assert!(active["terminal"].is_null());
    assert!(active["holytail"].is_null());

    let again = project.presentation(&work);
    assert!(again["transition"].is_null(), "{again}");
    assert!(again["holytail"].is_null());
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
fn without_governed_work_there_is_no_progress_or_preservation_line() {
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

/// A reader who drills down to the low-level run surface must not lose the
/// product's wording: a lead was observed doing exactly that and composing its
/// own closing sentence because this surface offered none.
#[test]
fn the_run_surface_offers_the_same_terminal_block() {
    let project = Project::new("terminal-run-surface");
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
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);

    let printed = project.call(&["run", "status", &ledger], None);
    assert!(printed.status.success(), "{printed:?}");
    let text = String::from_utf8_lossy(&printed.stdout);
    assert_eq!(
        text.lines().last(),
        Some("EXIT READY"),
        "the run surface does not end with the terminal block: {text}"
    );
    assert_eq!(text.matches("EXIT READY").count(), 1);

    let machine = project.value(&["run", "status", &ledger, "--json"], None);
    assert_eq!(machine["terminal"], "EXIT READY");

    // The same guard applies here: a changed tree ends the block.
    fs::write(project.root.join("source.txt"), b"env-first\nlater\n").unwrap();
    let after = String::from_utf8_lossy(&project.call(&["run", "status", &ledger], None).stdout)
        .into_owned();
    assert!(!after.contains("EXIT READY"), "{after}");
    let machine = project.value(&["run", "status", &ledger, "--json"], None);
    assert!(machine["terminal"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn canonical_goal_updates_obligations_and_invalidates_prior_closure() {
    let project = Project::new("goal-revision");
    let evidence_work = begin(&project, false);
    let evidence_decision = project.drive_until(&evidence_work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &evidence_work,
            evidence_decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    let first = project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-a",
            "--goal",
            "ship",
            "--obligation",
            "first",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ],
        None,
    );
    assert_eq!(first["revision"], 1);
    assert_eq!(first["obligations"][0]["disposition"], "open");

    let accepted = project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-a",
            "--goal",
            "ship",
            "--obligation",
            "first",
            "--disposition",
            "accepted",
            "--result-ref",
            &evidence_work,
        ],
        None,
    );
    assert_eq!(accepted["revision"], 2);
    assert_eq!(accepted["obligations"].as_array().unwrap().len(), 1);
    assert_eq!(accepted["obligations"][0]["disposition"], "accepted");
    project.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-a",
            "--result-ref",
            &evidence_work,
        ],
        None,
    );

    let successor = project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-a",
            "--goal",
            "ship the follow-up",
        ],
        None,
    );
    assert_eq!(successor["revision"], 4);
    assert_eq!(successor["closure"]["closed"], false);
    assert_eq!(project.value(&["goal", "status"], None)["revision"], 4);
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn unresolved_goal_facts_block_close_but_outside_scope_action_does_not() {
    let finding = Project::new("goal-finding");
    finding.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-f",
            "--goal",
            "ship",
            "--finding",
            "open review finding",
        ],
        None,
    );
    let blocked = finding.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-f",
            "--result-ref",
            "run-f",
        ],
        None,
    );
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("unresolved"));
    fs::remove_dir_all(finding.root).unwrap();

    let in_scope = Project::new("goal-external-in-scope");
    in_scope.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-i",
            "--goal",
            "ship",
            "--external-action",
            "publish release",
            "--scope",
            "in_scope",
        ],
        None,
    );
    let blocked = in_scope.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-i",
            "--result-ref",
            "run-i",
        ],
        None,
    );
    assert!(!blocked.status.success());
    fs::remove_dir_all(in_scope.root).unwrap();

    let external = Project::new("goal-external");
    let evidence_work = begin(&external, false);
    let evidence_decision = external.drive_until(&evidence_work, "lead_decision");
    external.value(
        &[
            "work",
            "return",
            &evidence_work,
            evidence_decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    external.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "goal-e",
            "--goal",
            "ship",
            "--external-action",
            "publish release",
            "--scope",
            "outside_scope",
            "--result-ref",
            &evidence_work,
            "--none-applicable",
            "obligations,findings,blockers,decisions",
        ],
        None,
    );
    let closed = external.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "goal-e",
            "--result-ref",
            &evidence_work,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    fs::remove_dir_all(external.root).unwrap();
}

#[test]
fn goal_close_requires_explicit_categories_and_current_governed_result() {
    let project = Project::new("goal-close-guards");
    project.value(
        &["goal", "incorporate", "--goal-id", "g", "--goal", "ship"],
        None,
    );
    let omitted = project.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "g",
            "--result-ref",
            "arbitrary",
        ],
        None,
    );
    assert!(!omitted.status.success());
    assert!(String::from_utf8_lossy(&omitted.stderr).contains("unresolved"));

    let explicit = Project::new("goal-result-guard");
    explicit.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let invalid = explicit.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "g",
            "--result-ref",
            "does-not-exist",
        ],
        None,
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("not a governed result"));
    let history = fs::read_to_string(explicit.root.join(".exitbind/session-goal.jsonl")).unwrap();
    assert_eq!(
        history.lines().count(),
        1,
        "rejected closure must not append"
    );
    fs::remove_dir_all(project.root).unwrap();
    fs::remove_dir_all(explicit.root).unwrap();
}

#[test]
fn goal_close_keeps_accepted_result_after_config_drift_and_warns() {
    let project = Project::new("goal-close-config-drift");
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
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );

    let config = project.root.join("exitbind.json");
    let mut config_bytes = fs::read(&config).unwrap();
    config_bytes.push(b'\n');
    fs::write(config, config_bytes).unwrap();
    let closed = project.call(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    assert!(
        closed.status.success(),
        "{}{}",
        String::from_utf8_lossy(&closed.stdout),
        String::from_utf8_lossy(&closed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&closed.stderr).contains("config_drift"),
        "{}",
        String::from_utf8_lossy(&closed.stderr)
    );
    let value: Value = serde_json::from_slice(&closed.stdout).unwrap();
    assert_eq!(value["closure"]["closed"], true);
    assert_eq!(value["closure"]["resultRefs"][0], work);

    let history = fs::read_to_string(project.root.join(".exitbind/session-goal.jsonl")).unwrap();
    assert!(!history.contains("testedInputsSha256"));
    assert!(!history.contains("\"warnings\""));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn session_goal_history_is_hash_bound_and_legacy_is_refused() {
    let project = Project::new("goal-history-integrity");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let history_path = project.root.join(".exitbind/session-goal.jsonl");
    let original = fs::read_to_string(&history_path).unwrap();
    let tampered = original.replace("\"goal\":\"ship\"", "\"goal\":\"tampered\"");
    fs::write(&history_path, &tampered).unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("event hash mismatch"));
    assert_eq!(fs::read_to_string(&history_path).unwrap(), tampered);

    fs::remove_file(&history_path).unwrap();
    fs::write(project.root.join(".exitbind/session-goal.json"), original).unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("legacy session goal format"));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn hash_valid_but_broken_goal_chain_links_are_refused() {
    let project = Project::new("goal-chain-integrity");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &["goal", "incorporate", "--goal-id", "g", "--goal", "ship"],
        None,
    );
    let path = project.root.join(".exitbind/session-goal.jsonl");
    let valid = fs::read_to_string(&path).unwrap();

    let mut lines = valid
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    lines[1]["revision"] = Value::from(99);
    reseal(&mut lines[1]);
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("revision is not continuous"));

    fs::write(&path, &valid).unwrap();
    let mut lines = valid
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    lines[1]["predecessor"]["revision"] = Value::from(99);
    reseal(&mut lines[1]);
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("predecessor linkage mismatch"));

    fs::write(&path, &valid).unwrap();
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "successor",
            "--goal",
            "follow-up",
        ],
        None,
    );
    let valid_successor = fs::read_to_string(&path).unwrap();
    let mut lines = valid_successor
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    lines[2]["successorOf"] = Value::Null;
    reseal(&mut lines[2]);
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let refused = project.call(&["goal", "status"], None);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("successor linkage mismatch"));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn concurrent_goal_incorporation_preserves_each_successful_revision() {
    let project = Project::new("goal-concurrent");
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    let handles = (0..8)
        .map(|index| {
            let root = project.root.clone();
            thread::spawn(move || {
                let finding = format!("finding-{index}");
                let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
                    .current_dir(&root)
                    .args([
                        "goal",
                        "incorporate",
                        "--goal-id",
                        "g",
                        "--goal",
                        "ship",
                        "--finding",
                    ])
                    .arg(&finding)
                    .arg("--config")
                    .arg(root.join("exitbind.json"))
                    .output()
                    .unwrap();
                (finding, output)
            })
        })
        .collect::<Vec<_>>();
    let mut successful = Vec::new();
    for handle in handles {
        let (finding, output) = handle.join().unwrap();
        if output.status.success() {
            successful.push(finding);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("run ledger is busy"), "{stderr}");
        }
    }
    assert!(
        !successful.is_empty(),
        "all competing mutations were refused"
    );
    let history = fs::read_to_string(project.root.join(".exitbind/session-goal.jsonl")).unwrap();
    let records = history
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), successful.len() + 1);
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record["revision"], index as u64 + 1);
    }
    let final_record = records.last().unwrap();
    for finding in successful {
        assert!(final_record["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == finding));
    }
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn resolved_goal_facts_are_not_projected_as_pending_card_items() {
    let project = Project::new("goal-resolved-card");
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
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--finding",
            "review finding",
            "--none-applicable",
            "obligations,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--finding",
            "review finding",
            "--disposition",
            "accepted",
            "--result-ref",
            &work,
        ],
        None,
    );
    project.value(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert_eq!(text.matches(SESSION_GOAL_CARD_TEST).count(), 1, "{text}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn all_five_resolved_categories_emit_then_new_open_item_suppresses_card() {
    let project = Project::new("goal-five-categories");
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
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    for (category, item) in [
        ("obligation", "obligation-one"),
        ("finding", "finding-one"),
        ("blocker", "blocker-one"),
        ("decision", "decision-one"),
        ("external-action", "external-one"),
    ] {
        let flag = match category {
            "obligation" => "--obligation",
            "finding" => "--finding",
            "blocker" => "--blocker",
            "decision" => "--decision",
            "external-action" => "--external-action",
            _ => unreachable!(),
        };
        let mut open = vec![
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            flag,
            item,
        ];
        if category == "external-action" {
            open.extend(["--scope", "in_scope"]);
        }
        project.value(&open, None);
        let mut resolve = vec![
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            flag,
            item,
            "--disposition",
            "accepted",
            "--result-ref",
            &work,
        ];
        if category == "external-action" {
            resolve.extend(["--scope", "in_scope"]);
        }
        project.value(&resolve, None);
    }
    project.value(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert_eq!(text.matches(SESSION_GOAL_CARD_TEST).count(), 1, "{text}");

    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--blocker",
            "new-blocker",
        ],
        None,
    );
    let output = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert_eq!(text.matches(SESSION_GOAL_CARD_TEST).count(), 0, "{text}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn stale_closure_is_suppressed_before_and_after_display_cache_loss() {
    let project = Project::new("goal-stale-card");
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
        Some(b"accepted"),
    );
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "g",
            "--goal",
            "ship",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &["goal", "close", "--goal-id", "g", "--result-ref", &work],
        None,
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    fs::write(project.root.join("source.txt"), b"drifted\n").unwrap();
    let first = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let first_text = String::from_utf8_lossy(&first.stdout).replace('\r', "");
    assert_eq!(
        first_text.matches(SESSION_GOAL_CARD_TEST).count(),
        1,
        "{first_text}"
    );
    let cache = project
        .root
        .join(".exitbind/presentation/session-goal.json");
    fs::write(&cache, b"corrupt cache").unwrap();
    let corrupt_stale = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let corrupt_stale_text = String::from_utf8_lossy(&corrupt_stale.stdout).replace('\r', "");
    assert_eq!(
        corrupt_stale_text.matches(SESSION_GOAL_CARD_TEST).count(),
        1,
        "{corrupt_stale_text}"
    );
    fs::write(project.root.join("source.txt"), b"env-first\n").unwrap();
    let current = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let current_text = String::from_utf8_lossy(&current.stdout).replace('\r', "");
    assert_eq!(
        current_text.matches(SESSION_GOAL_CARD_TEST).count(),
        0,
        "{current_text}"
    );
    fs::write(project.root.join("source.txt"), b"drifted\n").unwrap();
    fs::remove_file(&cache).unwrap();
    let second = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let second_text = String::from_utf8_lossy(&second.stdout).replace('\r', "");
    assert_eq!(
        second_text.matches(SESSION_GOAL_CARD_TEST).count(),
        1,
        "{second_text}"
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn governed_begin_refuses_missing_exitbind_namespace_before_ledger_write() {
    let project = Project::new("activation-namespace");
    let namespace = project.root.join(".exitbind");
    let saved = project.root.join("saved-exitbind");
    fs::rename(&namespace, saved).unwrap();
    let refused = project.call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "material",
            "--check-command",
            "true",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("governed activation is unavailable"));
    assert!(!namespace.join("runs").exists());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn interactive_session_closure_card_is_final_once_only_and_machine_silent() {
    let project = Project::new("session-card-pty");
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
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    // The display-only flag cannot manufacture whole-session closure, even
    // when the run itself is READY.
    let before_goal = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let before_goal_text = String::from_utf8_lossy(&before_goal.stdout).replace('\r', "");
    let card = "+------------------------------+\n| Nothing remains here.        |\n+------------------------------+\n\nEXIT READY";
    assert_eq!(
        before_goal_text.matches(card).count(),
        0,
        "{before_goal_text}"
    );

    // The card is eligible only after the Lead explicitly incorporates and
    // closes the canonical session goal.
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "session-a",
            "--goal",
            "make it hold",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "session-a",
            "--result-ref",
            work.as_str(),
        ],
        None,
    );
    let first = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let first_text = String::from_utf8_lossy(&first.stdout).replace('\r', "");
    assert_eq!(first_text.matches(card).count(), 1, "{first_text}");
    assert_eq!(first_text.lines().last(), Some("EXIT READY"));
    let second = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let second_text = String::from_utf8_lossy(&second.stdout).replace('\r', "");
    assert_eq!(second_text.matches(card).count(), 0, "{second_text}");
    fs::remove_file(
        project
            .root
            .join(".exitbind/presentation/session-goal.json"),
    )
    .unwrap();
    let after_loss = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&after_loss.stdout)
            .replace('\r', "")
            .matches(card)
            .count(),
        1
    );
    let machine = project.value(
        &["run", "status", &ledger, "--json", "--session-closed"],
        None,
    );
    assert!(machine["sessionGoal"].is_null());
    assert_eq!(machine["terminal"], "EXIT READY");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn interactive_failure_joke_is_transition_scoped_and_machine_silent() {
    let project = Project::new("failure-joke-pty");
    let work = begin(&project, false);
    let next = project.value(&["work", "next", &work], None)["next"].clone();
    project.value(
        &[
            "work",
            "return",
            &work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"blocked"),
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    let first = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let first_text = String::from_utf8_lossy(&first.stdout).replace('\r', "");
    let joke = "Developer special: the bug has requested a second opinion.";
    assert_eq!(first_text.matches(joke).count(), 1, "{first_text}");
    assert!(first_text.find("Lead decision: blocked").unwrap() < first_text.find(joke).unwrap());
    let second = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&second.stdout)
            .replace('\r', "")
            .matches(joke)
            .count(),
        0
    );
    let machine = project.value(&["run", "status", &ledger, "--json"], None);
    assert!(machine["joke"].is_null());
    let headless = project.call(&["run", "status", &ledger], None);
    assert!(!String::from_utf8_lossy(&headless.stdout).contains(joke));
    fs::remove_dir_all(project.root).unwrap();
}

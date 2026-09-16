#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::new_with_workers(true)
    }

    fn new_single() -> Self {
        Self::new_with_workers(false)
    }

    fn new_with_workers(two_workers: bool) -> Self {
        let root = support::temp("subject-progress");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config_path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        if two_workers {
            let base = config["agents"]["worker"].clone();
            let mut second = base;
            second["profile"] = serde_json::json!("exitbind/agents/worker_two.md");
            second["purpose"] = serde_json::json!("Complete bounded work for worker_two.");
            config["agents"]["worker_two"] = second;
            fs::write(
                root.join("exitbind/agents/worker_two.md"),
                b"# worker_two\n\nComplete bounded work.\n",
            )
            .unwrap();
            config["workflows"]["change"]["workers"] = serde_json::json!(["worker", "worker_two"]);
        }
        fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
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
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
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

    fn ledger(&self, work: &str) -> String {
        format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        )
    }

    fn submit_low_level(&self, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Output {
        self.call(
            &[
                "run",
                "submit",
                agent,
                ledger,
                "--outcome",
                outcome,
                "--artifact",
                artifact,
                "--artifact-root",
                "state",
                "--json",
            ],
            None,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn assert_progress(action: &Value) {
    assert_eq!(action["progress"]["applicable"], true);
    assert!(
        action["progress"]["percent"].is_number(),
        "missing progress: {action}"
    );
    assert!(action["progress"]["weights"]["worker"].is_number());
}

#[test]
fn skill_presentation_is_exact_and_packaged_copy_matches() {
    const ROUTINE: &str = "Neuro\nExitbind progress: N%.";
    let canonical = include_bytes!("../skills/exitbind/SKILL.md");
    let packaged = include_bytes!("../plugins/exitbind/skills/exitbind/SKILL.md");
    assert_eq!(canonical, packaged);
    let text = std::str::from_utf8(canonical).unwrap();
    assert!(text.contains(ROUTINE));
    assert!(text.contains("Exitbind computes that progress and owns the terminal state."));
    assert!(!text.contains("Holytail\nExitbind progress"));
}

#[test]
fn skill_keeps_readiness_lead_owned_and_holytail_preservation_narrow() {
    let text = std::str::from_utf8(include_bytes!("../skills/exitbind/SKILL.md"))
        .unwrap()
        .to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for contract in [
        "complex but has no material semantic-preservation risk",
        "keeps holytail off",
        "tool and agent selection remains the lead's responsibility",
        "clarify the accepted behavior, invariants, allowed changes",
        "freeze that accepted meaning",
        "preserve the frozen meaning",
        "read back the same accepted meaning",
        "current implementation subject",
        "trivial work has no holytail ceremony",
        "minimizer may reduce mechanism",
        "does not choose the goal, tools, or agents",
        "does not own final acceptance",
        "does not create a second ledger",
        "standalone holytail install",
        "preservation_missing",
        "preservation_failed",
        "do not install standalone holytail for this path",
    ] {
        assert!(
            text.contains(contract),
            "missing Exitbind skill contract: {contract}"
        );
    }
    assert!(!text.contains("coffee"));
    assert!(!text.contains("holytail chooses tools"));
}

#[test]
fn work_facade_surfaces_stale_worker_and_recovers_to_ready() {
    let fixture = Fixture::new();
    let check = "test -f marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "subject recovery",
            "--check-command",
            check,
        ],
        None,
    );
    assert_progress(&begin["next"]);
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);

    let scope = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    assert_progress(&scope["next"]);
    let worker_a = fixture.value(&["work", "next", &work], None);
    assert_progress(&worker_a["next"]);
    let worker_a_return = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_a["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker A"),
    );
    assert_progress(&worker_a_return["next"]);
    assert_eq!(worker_a_return["next"]["action"], "check");

    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    let checked_a = fixture.value(&["work", "check", &work], None);
    assert_progress(&checked_a["next"]);
    assert_eq!(checked_a["next"]["action"], "spawn");
    let worker_b = fixture.value(&["work", "next", &work], None);
    let worker_b_return = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_b["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker B"),
    );
    assert_progress(&worker_b_return["next"]);
    assert_eq!(worker_b_return["next"]["action"], "check");

    let events: Vec<Value> = fs::read_to_string(fixture.root.join(&ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let worker_b_event = events
        .iter()
        .find(|event| event["action"] == "submit" && event["agent"] == "worker_two")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let checked_b = fixture.call(
        &[
            "run",
            "record-check",
            &ledger,
            "--target",
            &worker_b_event,
            "--check-command",
            check,
            "--exit-code",
            "0",
            "--json",
        ],
        None,
    );
    assert!(checked_b.status.success(), "{checked_b:?}");

    let stale = fixture.value(&["work", "next", &work], None);
    assert_progress(&stale["next"]);
    assert_eq!(stale["next"]["action"], "check");

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    let reviewer = fixture.submit_low_level(
        "reviewer",
        &ledger,
        "approved",
        ".exitbind/artifacts/reviewer.md",
    );
    assert!(reviewer.status.success(), "{reviewer:?}");
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let premature =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(
        !premature.status.success(),
        "stale A check was accepted: {premature:?}"
    );

    let recovered_check = fixture.value(&["work", "check", &work], None);
    assert_progress(&recovered_check["next"]);
    assert_eq!(recovered_check["next"]["action"], "lead_decision");
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            recovered_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(done["next"]["action"], "done");
    assert_eq!(done["next"]["progress"]["percent"], 100);
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

#[test]
fn work_resume_projects_residual_packet_without_repeating_valid_work() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "residual resume",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["action"], "spawn");
    assert_eq!(checked["next"]["role"], "reviewer");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], work);
    assert_eq!(resumed["next"]["role"], "reviewer");
    assert_eq!(resumed["residual"]["next"], "spawn");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check"));
    assert!(resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "current_check" && item["status"] == "passed"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "review"));
    assert_eq!(
        resumed["residual"]["invalidation"]["sessionRestartInvalidates"],
        false
    );
    assert_eq!(
        resumed["residual"]["invalidation"]["subjectChangeInvalidates"],
        true
    );

    let reviewed = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"review"),
    );
    assert_eq!(reviewed["next"]["action"], "lead_decision");
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "lead_acceptance"));

    assert_progress(&scoped["next"]);
}

#[test]
fn preservation_requirement_is_separate_from_functional_check_and_resume_reuses_it() {
    let fixture = Fixture::new_single();
    let functional = "n=$(cat functional.count 2>/dev/null || echo 0); n=$((n+1)); printf '%s\\n' \"$n\" > functional.count";
    let preservation = "n=$(cat preservation.count 2>/dev/null || echo 0); n=$((n+1)); printf '%s\\n' \"$n\" > preservation.count";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve accepted precedence",
            "--check-command",
            functional,
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    assert_eq!(completed["next"]["action"], "check");
    assert_eq!(completed["next"]["check"]["kind"], "check");

    let functional_checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(functional_checked["next"]["action"], "check");
    assert_eq!(functional_checked["next"]["check"]["kind"], "preservation");
    assert_eq!(
        functional_checked["next"]["check"]["requirementId"],
        "precedence"
    );

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    assert!(fixture
        .submit_low_level(
            "reviewer",
            &ledger,
            "approved",
            ".exitbind/artifacts/reviewer.md",
        )
        .status
        .success());
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let premature =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(
        !premature.status.success(),
        "accepted without preservation check: {premature:?}"
    );
    let premature_text = format!(
        "{}{}",
        String::from_utf8_lossy(&premature.stdout),
        String::from_utf8_lossy(&premature.stderr)
    );
    assert!(
        premature_text.contains("preservation_missing"),
        "{premature:?}"
    );

    let preserved = fixture.value(&["work", "check", &work], None);
    assert_eq!(
        fs::read_to_string(fixture.root.join("functional.count")).unwrap(),
        "1\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("preservation.count")).unwrap(),
        "1\n"
    );
    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(
        fs::read_to_string(fixture.root.join("functional.count")).unwrap(),
        "1\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("preservation.count")).unwrap(),
        "1\n"
    );
    assert!(resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "preservation" && item["requirementId"] == "precedence"));
    assert!(resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "preservation:precedence"));
    assert_eq!(preserved["next"]["action"], "lead_decision");
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            preserved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

#[test]
fn failed_preservation_blocks_acceptance_without_claiming_functional_failure() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "detect weakened requirement",
            "--check-command",
            "true",
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            "test ! -f preservation-fail",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"implementation"),
    );
    fixture.value(&["work", "check", &work], None);
    fs::write(fixture.root.join("preservation-fail"), b"weakened").unwrap();
    let failed = fixture.value(&["work", "check", &work], None);
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][1]["status"],
        "failed"
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][1]["requirementId"],
        "precedence"
    );

    fs::write(
        fixture.root.join(".exitbind/artifacts/reviewer.md"),
        b"review\n",
    )
    .unwrap();
    assert!(fixture
        .submit_low_level(
            "reviewer",
            &ledger,
            "approved",
            ".exitbind/artifacts/reviewer.md",
        )
        .status
        .success());
    fs::write(
        fixture.root.join(".exitbind/artifacts/lead.md"),
        b"accept\n",
    )
    .unwrap();
    let refused =
        fixture.submit_low_level("lead", &ledger, "accepted", ".exitbind/artifacts/lead.md");
    assert!(!refused.status.success(), "{refused:?}");
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("preservation_failed"), "{refused:?}");
}

#[test]
fn residual_packet_does_not_invent_review_before_initial_scope() {
    let fixture = Fixture::new_single();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "initial lead resume",
            "--check-command",
            "test -f pass-marker",
        ],
        None,
    );
    assert_eq!(begin["next"]["action"], "lead_decision");

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], begin["work"]);
    assert_eq!(resumed["next"]["action"], "lead_decision");
    assert!(resumed["next"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "scoped"));

    assert!(!resumed["residual"]["stillValid"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["evidence"] == "current_review" && item["status"] == "approved"));
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "review"));
    assert!(!resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "lead_acceptance"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "scope"));
}

#[test]
fn residual_packet_keeps_stale_subject_evidence_out_of_reuse() {
    let fixture = Fixture::new();
    let check = "test -f marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "subject invalidation",
            "--check-command",
            check,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker_a = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_a["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker A"),
    );
    fs::write(fixture.root.join("marker"), b"ok").unwrap();
    fixture.value(&["work", "check", &work], None);
    let worker_b = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker_b["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker B"),
    );

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "check");
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "check"));
}

#[test]
fn preservation_checks_rerun_after_rework_changes_subject() {
    let fixture = Fixture::new_single();
    let functional = "n=$(cat functional.count 2>/dev/null || echo 0); n=$((n+1)); printf '%s\\n' \"$n\" > functional.count";
    let preservation = "n=$(cat preservation.count 2>/dev/null || echo 0); n=$((n+1)); printf '%s\\n' \"$n\" > preservation.count";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preservation recheck after rework",
            "--check-command",
            functional,
            "--preserve-requirement",
            "precedence:accepted precedence remains binding",
            "--preservation-check-command",
            preservation,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first implementation"),
    );
    fixture.value(&["work", "check", &work], None);
    let checked = fixture.value(&["work", "check", &work], None);
    assert_eq!(checked["next"]["role"], "reviewer");
    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            checked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"needs another implementation"),
    );
    assert_eq!(reworked["next"]["role"], "worker");
    let worker = fixture.value(&["work", "next", &work], None);
    fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"second implementation"),
    );

    let resumed = fixture.value(&["work", "resume"], None);
    assert_eq!(resumed["next"]["action"], "check");
    assert!(!resumed["residual"]["doNotRepeat"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "passed_check" || item == "preservation:precedence"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "check"));
    assert!(resumed["residual"]["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["obligation"] == "preservation" && item["requirementId"] == "precedence"));

    fixture.value(&["work", "check", &work], None);
    fixture.value(&["work", "check", &work], None);
    assert_eq!(
        fs::read_to_string(fixture.root.join("functional.count")).unwrap(),
        "2\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("preservation.count")).unwrap(),
        "2\n"
    );
}

#[test]
fn failed_check_surfaces_rework_and_requires_fresh_acceptance_path() {
    let fixture = Fixture::new_single();
    let check = "test -f pass-marker";
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "failed check rework",
            "--check-command",
            check,
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let ledger = fixture.ledger(&work);
    let scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            worker["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"first attempt"),
    );
    assert_progress(&scoped["next"]);
    assert_eq!(completed["next"]["action"], "check");

    let failed = fixture.value(&["work", "check", &work], None);
    assert_eq!(failed["next"]["action"], "spawn");
    assert_eq!(failed["next"]["role"], "reviewer");
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"][0]["status"],
        "failed"
    );
    assert_eq!(
        failed["next"]["packet"]["checkEvidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let failed_ledger = fs::read(fixture.root.join(&ledger)).unwrap();

    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            failed["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        Some(b"repair requested"),
    );
    assert_eq!(reworked["next"]["action"], "spawn");
    assert_eq!(reworked["next"]["role"], "worker");
    assert_eq!(reworked["next"]["packet"]["attempt"], 2);
    let after_rework = fs::read(fixture.root.join(&ledger)).unwrap();
    assert!(after_rework.starts_with(&failed_ledger));

    fs::write(fixture.root.join("pass-marker"), b"ok").unwrap();
    let fresh_completed = fixture.value(
        &[
            "work",
            "return",
            &work,
            reworked["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"fresh worker result"),
    );
    assert_eq!(fresh_completed["next"]["action"], "check");
    let fresh_check = fixture.value(&["work", "check", &work], None);
    assert_eq!(fresh_check["next"]["action"], "spawn");
    assert_eq!(fresh_check["next"]["role"], "reviewer");
    assert_eq!(
        fresh_check["next"]["packet"]["checkEvidence"][0]["status"],
        "passed"
    );

    let approved = fixture.value(
        &[
            "work",
            "return",
            &work,
            fresh_check["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"fresh review"),
    );
    assert_eq!(approved["next"]["action"], "lead_decision");
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            approved["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"fresh acceptance"),
    );
    assert_eq!(done["next"]["action"], "done");
    assert_eq!(done["next"]["progress"]["percent"], 100);
    assert_eq!(done["next"]["progress"]["state"], "READY");
}

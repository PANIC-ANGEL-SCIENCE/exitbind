//! Owner-selectable review policy remains explicit, current, and orthogonal to
//! basis, preservation, findings, and receipt integrity.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

const CHECK: &str = "true";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("invalid JSON for {args:?}: {error}; {}", text(&output)))
    }

    fn ledger_event(&self, ledger: &str) -> Value {
        fs::read_to_string(self.root.join(ledger))
            .unwrap()
            .lines()
            .last()
            .map(|line| serde_json::from_str(line).unwrap())
            .unwrap()
    }

    fn artifact(&self, name: &str) -> String {
        let relative = format!(".exitbind/artifacts/{name}.md");
        fs::write(self.root.join(&relative), format!("{name}\n")).unwrap();
        relative
    }

    fn start(&self, ledger: &str, policy: &str) -> Value {
        self.json(&[
            "run",
            "start",
            "change",
            "--goal",
            "review policy control",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--review-policy",
            policy,
            "--config",
            "exitbind.json",
        ])
    }

    fn start_with_preservation(&self, ledger: &str, policy: &str) -> Value {
        self.json(&[
            "run",
            "start",
            "change",
            "--goal",
            "review policy preservation control",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--preserve-requirement",
            "canonical:preserved behavior",
            "--preservation-check-command",
            CHECK,
            "--preservation-proof-origin",
            "synthetic",
            "--review-policy",
            policy,
            "--config",
            "exitbind.json",
        ])
    }

    fn configure_roles(&self, workers: &[&str], reviewers: &[&str]) {
        let path = self.root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["workflows"]["change"]["workers"] = json!(workers);
        config["workflows"]["change"]["reviewers"] = json!(reviewers);
        fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    }

    fn submit(&self, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Output {
        self.call(&[
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
            "--config",
            "exitbind.json",
        ])
    }

    fn submit_ok(&self, agent: &str, ledger: &str, outcome: &str, name: &str) -> Value {
        let artifact = self.artifact(name);
        let output = self.submit(agent, ledger, outcome, &artifact);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn worker_and_check(&self, ledger: &str) -> Value {
        self.submit_ok("lead", ledger, "scoped", "scope");
        let worker = self.submit_ok("worker", ledger, "completed", "worker");
        let target = worker["event"]["eventSha256"].as_str().unwrap();
        let check = self.call(&[
            "run",
            "record-check",
            ledger,
            "--target",
            target,
            "--check-command",
            CHECK,
            "--exit-code",
            "0",
            "--json",
            "--config",
            "exitbind.json",
        ]);
        assert!(check.status.success(), "{}", text(&check));
        self.json(&[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "exitbind.json",
        ])
    }

    fn work_begin(&self, policy: &str) -> (String, Value) {
        let value = self.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            "review policy work",
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--review-policy",
            policy,
            "--config",
            "exitbind.json",
        ]);
        (
            value["work"].as_str().unwrap().to_owned(),
            value["next"].clone(),
        )
    }

    fn work_return(&self, work: &str, action: &Value, outcome: &str, body: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                work,
                action["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
                "--config",
                "exitbind.json",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn work_return_ok(&self, work: &str, action: &Value, outcome: &str, body: &str) -> Value {
        let output = self.work_return(work, action, outcome, body);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn work_check(&self, work: &str) -> Value {
        let output = self.call(&["work", "check", work, "--config", "exitbind.json"]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ledger_for(work: &str) -> String {
        format!(".exitbind/runs/work-{}.jsonl", &work[4..])
    }

    fn review_policy(&self, ledger: &str, decision: &str) -> Output {
        self.call(&[
            "run",
            "review-policy",
            "lead",
            ledger,
            "--decision",
            decision,
            "--reason",
            "owner revised review policy for this run",
            "--config",
            "exitbind.json",
        ])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_refused(output: &Output, needle: &str) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        text(output)
    );
    assert!(
        text(output).contains(needle),
        "missing {needle:?}: {}",
        text(output)
    );
}

#[test]
fn required_review_refuses_without_approval_but_omitted_policy_accepts_without_basis() {
    let required = Fixture::new("review-policy-required");
    required.configure_roles(&["worker"], &[]);
    let ledger = ".exitbind/runs/required.jsonl";
    required.start(ledger, "required");
    let start_event = required.ledger_event(ledger);
    assert!(start_event["basisProtocol"].is_number());
    assert!(start_event["basis"].is_null());
    assert_eq!(start_event["reviewPolicy"]["decision"], "required");
    required.worker_and_check(ledger);
    let missing = required.submit("lead", ledger, "accepted", &required.artifact("missing"));
    assert_refused(&missing, "canonical acceptance requires reviewer approval");

    let omitted = Fixture::new("review-policy-omitted");
    omitted.configure_roles(&["worker"], &[]);
    let omitted_ledger = ".exitbind/runs/omitted.jsonl";
    omitted.start(omitted_ledger, "omitted");
    let start_event = omitted.ledger_event(omitted_ledger);
    assert!(start_event["basisProtocol"].is_number());
    assert!(start_event["basis"].is_null());
    omitted.worker_and_check(omitted_ledger);
    let accepted = omitted.submit_ok("lead", omitted_ledger, "accepted", "accept");
    assert_eq!(accepted["status"], "accepted");
    let events = fs::read_to_string(omitted.root.join(omitted_ledger)).unwrap();
    assert!(!events.contains("\"role\":\"reviewer\""));
    assert!(events.contains("\"decision\":\"omitted\""));
}

#[test]
fn required_to_omitted_retires_stale_reviewer_handle_and_accepts_truthfully() {
    let fixture = Fixture::new("review-policy-required-to-omitted");
    let (work, mut action) = fixture.work_begin("required");
    action = fixture.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    fs::write(fixture.root.join("actual-product-check"), b"pass\n").unwrap();
    fixture.work_return_ok(&work, &action, "completed", "worker");
    let checked = fixture.work_check(&work);
    let reviewer = checked["next"].clone();
    assert_eq!(reviewer["role"], "reviewer");
    let ledger = Fixture::ledger_for(&work);
    let policy = fixture.review_policy(&ledger, "omitted");
    assert!(policy.status.success(), "{}", text(&policy));
    let stale = fixture.work_return(&work, &reviewer, "approved", "stale-review");
    assert_refused(&stale, "assignment is not the current pending work action");
    let next = fixture.json(&["work", "next", &work, "--config", "exitbind.json"])["next"].clone();
    assert_eq!(next["role"], "lead");
    let accepted = fixture.work_return_ok(&work, &next, "accepted", "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn omitted_to_required_during_initial_scoping_keeps_lead_owner() {
    let fixture = Fixture::new("review-policy-initial-revision");
    fixture.configure_roles(&["worker"], &["reviewer"]);
    let ledger = ".exitbind/runs/initial-revision.jsonl";
    fixture.start(ledger, "omitted");
    let initial = fixture.json(&["run", "next", ledger, "--config", "exitbind.json"]);
    assert_eq!(initial["assignments"][0]["role"], "lead");
    assert_eq!(initial["assignments"][0]["stage"], 1);
    let required = fixture.review_policy(ledger, "required");
    assert!(required.status.success(), "{}", text(&required));
    let after = fixture.json(&["run", "next", ledger, "--config", "exitbind.json"]);
    assert_eq!(after["assignments"][0]["role"], "lead");
    assert_eq!(after["assignments"][0]["stage"], 1);
    fixture.worker_and_check(ledger);
    let reviewer = fixture.json(&["run", "next", ledger, "--config", "exitbind.json"]);
    assert_eq!(reviewer["assignments"][0]["role"], "reviewer");
    fixture.submit_ok("reviewer", ledger, "approved", "initial-review");
    fixture.submit_ok("lead", ledger, "accepted", "initial-accept");
    let status = fixture.json(&[
        "run",
        "status",
        ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(status["status"], "accepted");
}

#[test]
fn omitted_to_required_requires_fresh_review_and_retires_old_lead_handle() {
    let fixture = Fixture::new("review-policy-omitted-to-required");
    let (work, mut action) = fixture.work_begin("omitted");
    action = fixture.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    fs::write(fixture.root.join("actual-product-check"), b"pass\n").unwrap();
    fixture.work_return_ok(&work, &action, "completed", "worker");
    let checked = fixture.work_check(&work);
    let lead = checked["next"].clone();
    assert_eq!(lead["role"], "lead");
    let ledger = Fixture::ledger_for(&work);
    let policy = fixture.review_policy(&ledger, "required");
    assert!(policy.status.success(), "{}", text(&policy));
    let stale = fixture.work_return(&work, &lead, "accepted", "stale-acceptance");
    assert_refused(&stale, "assignment is not the current pending work action");
    let reviewer =
        fixture.json(&["work", "next", &work, "--config", "exitbind.json"])["next"].clone();
    assert_eq!(reviewer["role"], "reviewer");
    let accepted_review = fixture.work_return_ok(&work, &reviewer, "approved", "review");
    let accepted = fixture.work_return_ok(&work, &accepted_review["next"], "accepted", "accepted");
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
}

#[test]
fn omission_does_not_bypass_preservation_missing_or_stale_subject() {
    let preservation = Fixture::new("review-policy-preservation");
    let ledger = ".exitbind/runs/preservation.jsonl";
    preservation.start_with_preservation(ledger, "omitted");
    preservation.submit_ok("lead", ledger, "scoped", "scope");
    let worker = preservation.submit_ok("worker", ledger, "completed", "worker");
    let target = worker["event"]["eventSha256"].as_str().unwrap();
    let check = preservation.call(&[
        "run",
        "record-check",
        ledger,
        "--target",
        target,
        "--check-command",
        CHECK,
        "--exit-code",
        "0",
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert!(check.status.success(), "{}", text(&check));
    let missing = preservation.submit("lead", ledger, "accepted", &preservation.artifact("accept"));
    assert_refused(&missing, "preservation_missing");

    let stale = Fixture::new("review-policy-stale-subject");
    let stale_ledger = ".exitbind/runs/stale.jsonl";
    stale.start(stale_ledger, "omitted");
    stale.worker_and_check(stale_ledger);
    fs::write(stale.root.join("changed-after-check"), b"drift\n").unwrap();
    let refused = stale.submit("lead", stale_ledger, "accepted", &stale.artifact("accept"));
    assert_refused(&refused, "check_missing");
}

#[test]
fn omission_after_rework_retains_lead_disposition_duty() {
    let fixture = Fixture::new("review-policy-rework");
    let (work, mut action) = fixture.work_begin("required");
    action = fixture.work_return_ok(&work, &action, "scoped", "scope")["next"].clone();
    fs::write(fixture.root.join("actual-product-check"), b"pass\n").unwrap();
    fixture.work_return_ok(&work, &action, "completed", "worker");
    let checked = fixture.work_check(&work);
    let rework = fixture.work_return_ok(&work, &checked["next"], "rework", "finding");
    assert_eq!(rework["next"]["role"], "lead");
    let ledger = Fixture::ledger_for(&work);
    let policy = fixture.review_policy(&ledger, "omitted");
    assert!(policy.status.success(), "{}", text(&policy));
    let required = fixture.review_policy(&ledger, "required");
    assert!(required.status.success(), "{}", text(&required));
    let lead = fixture.json(&["work", "next", &work, "--config", "exitbind.json"])["next"].clone();
    assert_eq!(lead["role"], "lead");
    let refused = fixture.work_return(&work, &lead, "accepted", "must-dispose");
    assert_refused(&refused, "disposition");
}

#[test]
fn receipt_tamper_is_rejected_and_historical_run_has_no_new_policy_marker() {
    let fixture = Fixture::new("review-policy-receipt");
    let ledger = ".exitbind/runs/receipt.jsonl";
    fixture.start(ledger, "omitted");
    fixture.worker_and_check(ledger);
    fixture.submit_ok("lead", ledger, "accepted", "accept");
    let receipt = ".exitbind/receipts/policy.json";
    let written = fixture.call(&[
        "receipt",
        ledger,
        "--output",
        receipt,
        "--config",
        "exitbind.json",
    ]);
    assert!(written.status.success(), "{}", text(&written));
    let valid = fixture.call(&["verify", receipt, "--config", "exitbind.json"]);
    assert!(valid.status.success(), "{}", text(&valid));
    let path = fixture.root.join(receipt);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["subject"]["sha256"] = json!("0".repeat(64));
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let tampered = fixture.call(&["verify", receipt, "--config", "exitbind.json"]);
    assert_refused(&tampered, "receipt");

    let root = support::temp("review-policy-historical");
    let init = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", text(&init));
    let start = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(&root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "historical policy meaning",
            "--ledger",
            ".soulmate/runs/historical.jsonl",
            "--config",
            "soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(start.status.success(), "{}", text(&start));
    let historical = fs::read_to_string(root.join(".soulmate/runs/historical.jsonl"))
        .unwrap()
        .lines()
        .last()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .unwrap();
    assert!(historical["reviewPolicy"].is_null());
    assert!(historical["basisProtocol"].is_null());
    let _ = fs::remove_dir_all(root);
}

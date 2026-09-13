#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CHECK: &str = "test -f actual-product-check";
const GOAL: &str = "Complete the bounded host workflow without losing evidence.";

struct Fixture {
    root: PathBuf,
    ledger: String,
    calls: Vec<Vec<String>>,
    actor_calls: Vec<(String, u64, u64)>,
}

impl Fixture {
    fn new(label: &str, ledger: &str) -> Self {
        let root = support::temp(label);
        let mut fixture = Self {
            root,
            ledger: ledger.to_owned(),
            calls: Vec::new(),
            actor_calls: Vec::new(),
        };
        let root = fixture
            .root
            .to_str()
            .expect("test fixture root must be valid UTF-8")
            .to_owned();
        let initialized = fixture.run_owned(vec![
            "init".into(),
            "--mode".into(),
            "portable".into(),
            "--root".into(),
            root,
        ]);
        assert!(initialized.status.success(), "{}", text(&initialized));
        fixture
    }

    fn run_owned(&mut self, arguments: Vec<String>) -> Output {
        self.calls.push(arguments.clone());
        Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .current_dir(&self.root)
            .args(arguments)
            .output()
            .unwrap()
    }

    fn value(&mut self, arguments: Vec<String>) -> Value {
        let output = self.run_owned(arguments);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn artifact(&self, packet: &Value, body: &str) -> String {
        assert_eq!(packet["artifactRootHint"], "state");
        let relative = packet["artifactPathHint"].as_str().unwrap();
        assert!(!Path::new(relative).is_absolute());
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        relative.to_owned()
    }

    fn execute_actor(&mut self, packet: &Value, body: &str) -> String {
        self.actor_calls.push((
            packet["agent"].as_str().unwrap().to_owned(),
            packet["stage"].as_u64().unwrap(),
            packet["attempt"].as_u64().unwrap(),
        ));
        self.artifact(packet, body)
    }

    fn submit_output_with_body(
        &mut self,
        packet: &Value,
        outcome: &str,
        body: &str,
    ) -> (Output, String) {
        let artifact = self.execute_actor(packet, body);
        let output = self.run_owned(vec![
            "run".into(),
            "submit".into(),
            packet["agent"].as_str().unwrap().to_owned(),
            self.ledger.clone(),
            "--outcome".into(),
            outcome.to_owned(),
            "--artifact".into(),
            artifact.clone(),
            "--artifact-root".into(),
            packet["artifactRootHint"].as_str().unwrap().to_owned(),
            "--config".into(),
            "soulmate.json".into(),
        ]);
        (output, artifact)
    }

    fn submit_with_body(&mut self, packet: &Value, outcome: &str, body: &str) -> (Value, String) {
        let (output, artifact) = self.submit_output_with_body(packet, outcome, body);
        assert!(output.status.success(), "{}", text(&output));
        (serde_json::from_slice(&output.stdout).unwrap(), artifact)
    }

    fn submit(&mut self, packet: &Value, outcome: &str, body: &str) -> Value {
        self.submit_with_body(packet, outcome, body).0
    }

    fn record_check(&mut self, target: &str, check_command: &str, exit_code: i32) -> Value {
        self.value(vec![
            "run".into(),
            "record-check".into(),
            self.ledger.clone(),
            "--target".into(),
            target.to_owned(),
            "--check-command".into(),
            check_command.to_owned(),
            "--exit-code".into(),
            exit_code.to_string(),
            "--config".into(),
            "soulmate.json".into(),
        ])
    }

    fn execute_check(&self, check_command: &str) -> i32 {
        Command::new("sh")
            .args(["-c", check_command])
            .current_dir(&self.root)
            .status()
            .unwrap()
            .code()
            .unwrap_or(125)
    }

    fn events(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join(&self.ledger))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn fresh_next(&mut self) -> Value {
        self.value(vec![
            "run".into(),
            "next".into(),
            self.ledger.clone(),
            "--json".into(),
            "--config".into(),
            "soulmate.json".into(),
        ])
    }

    fn fresh_status(&mut self) -> Value {
        self.value(vec![
            "run".into(),
            "status".into(),
            self.ledger.clone(),
            "--json".into(),
            "--config".into(),
            "soulmate.json".into(),
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
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assignment(value: &Value) -> Value {
    value["assignments"]
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or_else(|| panic!("response did not return an assignment: {value}"))
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn packet_check_command(packet: &Value) -> String {
    let command = packet["checkPolicy"]["command"]
        .as_str()
        .expect("assignment must carry the frozen check command")
        .to_owned();
    assert_eq!(command, CHECK);
    command
}

fn verify_packet(
    fixture: &Fixture,
    packet: &Value,
    agent: &str,
    role: &str,
    stage: u64,
    attempt: u64,
) {
    assert_eq!(packet["agent"], agent);
    assert_eq!(packet["nativeTaskName"], agent);
    assert_eq!(packet["role"], role);
    assert_eq!(packet["stage"], stage);
    assert_eq!(packet["attempt"], attempt);
    assert_eq!(packet["goal"], GOAL);
    assert_eq!(packet["checkPolicy"]["command"], CHECK);
    assert_eq!(packet["checkPolicy"]["origin"], "local_report");

    let profile = packet["profile"]["path"].as_str().unwrap();
    let profile_bytes = fs::read(fixture.root.join(profile)).unwrap();
    assert_eq!(sha256(&profile_bytes), packet["profile"]["sha256"]);

    for artifact in packet["upstreamArtifacts"].as_array().unwrap() {
        let root = artifact["root"].as_str().unwrap();
        assert_eq!(root, "state");
        let relative = artifact["path"].as_str().unwrap();
        let bytes = fs::read(fixture.root.join(relative)).unwrap();
        assert_eq!(sha256(&bytes), artifact["sha256"]);
    }
}

fn run_actions(calls: &[Vec<String>]) -> Vec<String> {
    calls
        .iter()
        .filter(|arguments| arguments.first().map(String::as_str) == Some("run"))
        .map(|arguments| arguments[1].clone())
        .collect()
}

#[test]
fn clean_response_driven_checked_run_uses_nine_soulmate_calls() {
    let mut fixture = Fixture::new("host-workflow-clean", ".soulmate/runs/clean.jsonl");
    let config = fixture.value(vec![
        "check".into(),
        "--json".into(),
        "--config".into(),
        "soulmate.json".into(),
    ]);
    assert_eq!(config["valid"], true);

    let started = fixture.value(vec![
        "run".into(),
        "start".into(),
        "change".into(),
        "--goal".into(),
        GOAL.into(),
        "--check-command".into(),
        CHECK.into(),
        "--ledger".into(),
        fixture.ledger.clone(),
        "--config".into(),
        "soulmate.json".into(),
    ]);
    assert_eq!(started["status"], "running");
    let lead = assignment(&started);
    verify_packet(&fixture, &lead, "lead", "lead", 1, 1);

    let scoped = fixture.submit(&lead, "scoped", "lead scope\n");
    let worker = assignment(&scoped);
    verify_packet(&fixture, &worker, "worker", "worker", 2, 1);
    let check_command = packet_check_command(&worker);

    let (worker_response, worker_artifact) =
        fixture.submit_with_body(&worker, "completed", "worker completion\n");
    let worker_event = worker_response["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let reviewer = assignment(&worker_response);
    verify_packet(&fixture, &reviewer, "reviewer", "reviewer", 3, 1);
    fs::write(fixture.root.join("actual-product-check"), b"ready\n").unwrap();
    assert_eq!(fixture.execute_check(&check_command), 0);
    let check = fixture.record_check(&worker_event, &check_command, 0);
    assert_eq!(check["event"]["targetEventSha256"], worker_event);
    assert_eq!(check["event"]["checkCommand"], check_command);

    let reviewed = fixture.submit(&reviewer, "approved", "review finding\n");
    let accepting_lead = assignment(&reviewed);
    verify_packet(&fixture, &accepting_lead, "lead", "lead", 4, 1);
    let accepted = fixture.submit(&accepting_lead, "accepted", "lead acceptance\n");
    assert_eq!(accepted["status"], "accepted");
    assert!(accepted["assignments"].as_array().unwrap().is_empty());

    let status = fixture.fresh_status();
    assert_eq!(status["status"], "accepted");
    assert_eq!(status["checks"]["status"], "passed");
    assert_eq!(status["checks"]["targetCount"], 1);
    assert_eq!(status["checks"]["observedCount"], 1);
    assert_eq!(
        status["claim"]["artifactSha256"],
        sha256(&fs::read(fixture.root.join(worker_artifact)).unwrap())
    );
    assert_eq!(status["review"]["status"], "approved");
    assert_eq!(status["acceptance"]["status"], "accepted");

    let events = fixture.events();
    let actions: Vec<_> = events
        .iter()
        .map(|event| event["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        ["start", "submit", "submit", "check", "submit", "submit"]
    );
    let submissions: Vec<_> = events
        .iter()
        .filter(|event| event["action"] == "submit")
        .map(|event| {
            (
                event["role"].as_str().unwrap(),
                event["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        submissions,
        [
            ("lead", "scoped"),
            ("worker", "completed"),
            ("reviewer", "approved"),
            ("lead", "accepted")
        ]
    );
    assert_eq!(fixture.calls.len(), 9);
    assert_eq!(
        run_actions(&fixture.calls),
        [
            "start",
            "submit",
            "submit",
            "record-check",
            "submit",
            "submit",
            "status"
        ]
    );
    assert!(!fixture
        .calls
        .iter()
        .any(|arguments| arguments.get(1).map(String::as_str) == Some("next")));
    assert_eq!(
        fixture.actor_calls,
        [
            ("lead".to_owned(), 1, 1),
            ("worker".to_owned(), 2, 1),
            ("reviewer".to_owned(), 3, 1),
            ("lead".to_owned(), 4, 1),
        ]
    );
}

#[test]
fn missing_check_report_is_recovered_without_worker_rework() {
    let mut fixture = Fixture::new("host-workflow-missing", ".soulmate/runs/missing.jsonl");
    let config = fixture.value(vec![
        "check".into(),
        "--json".into(),
        "--config".into(),
        "soulmate.json".into(),
    ]);
    assert_eq!(config["valid"], true);
    let started = fixture.value(vec![
        "run".into(),
        "start".into(),
        "change".into(),
        "--goal".into(),
        GOAL.into(),
        "--check-command".into(),
        CHECK.into(),
        "--ledger".into(),
        fixture.ledger.clone(),
        "--config".into(),
        "soulmate.json".into(),
    ]);
    let lead = assignment(&started);
    verify_packet(&fixture, &lead, "lead", "lead", 1, 1);
    let scoped = fixture.submit(&lead, "scoped", "scope\n");
    let worker = assignment(&scoped);
    verify_packet(&fixture, &worker, "worker", "worker", 2, 1);
    let worker_check_command = packet_check_command(&worker);
    let (worker_response, _) = fixture.submit_with_body(&worker, "completed", "completion\n");
    let worker_event = worker_response["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let reviewer = assignment(&worker_response);
    verify_packet(&fixture, &reviewer, "reviewer", "reviewer", 3, 1);
    let reviewed = fixture.submit(&reviewer, "approved", "review before report\n");
    let lead_after_review = assignment(&reviewed);
    verify_packet(&fixture, &lead_after_review, "lead", "lead", 4, 1);

    let before_reads = fs::read(fixture.root.join(&fixture.ledger)).unwrap();
    let fresh_next = fixture.fresh_next();
    let fresh_status = fixture.fresh_status();
    assert_eq!(
        fs::read(fixture.root.join(&fixture.ledger)).unwrap(),
        before_reads
    );
    let fresh_lead = assignment(&fresh_next);
    verify_packet(&fixture, &fresh_lead, "lead", "lead", 4, 1);
    let fresh_check_command = packet_check_command(&fresh_lead);
    assert_eq!(fresh_check_command, worker_check_command);
    assert_eq!(fresh_status["checks"]["status"], "not_observed");
    assert_eq!(fresh_status["checks"]["missingCount"], 1);
    assert_eq!(fresh_status["evidence"]["protectionCount"], 0);
    let fresh_worker_event = fresh_status["claim"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(fresh_worker_event, worker_event);

    fs::write(fixture.root.join("actual-product-check"), b"ready\n").unwrap();
    assert_eq!(fixture.execute_check(&fresh_check_command), 0);
    let check = fixture.record_check(&fresh_worker_event, &fresh_check_command, 0);
    assert_eq!(check["event"]["targetEventSha256"], fresh_worker_event);
    assert_eq!(check["event"]["checkCommand"], fresh_check_command);
    assert_eq!(
        fixture.submit(&fresh_lead, "accepted", "accept after report\n")["status"],
        "accepted"
    );

    let final_status = fixture.fresh_status();
    assert_eq!(final_status["status"], "accepted");
    assert_eq!(final_status["checks"]["status"], "passed");
    assert_eq!(final_status["evidence"]["protectionCount"], 0);
    let events = fixture.events();
    let actions: Vec<_> = events
        .iter()
        .map(|event| event["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        ["start", "submit", "submit", "submit", "check", "submit"]
    );
    assert_eq!(fixture.calls.len(), 11);
    assert_eq!(
        run_actions(&fixture.calls),
        [
            "start",
            "submit",
            "submit",
            "submit",
            "next",
            "status",
            "record-check",
            "submit",
            "status",
        ]
    );
    assert_eq!(
        fixture.actor_calls,
        [
            ("lead".to_owned(), 1, 1),
            ("worker".to_owned(), 2, 1),
            ("reviewer".to_owned(), 3, 1),
            ("lead".to_owned(), 4, 1),
        ]
    );
}

#[test]
fn rework_reads_fresh_state_and_keeps_failed_attempt_evidence() {
    let mut fixture = Fixture::new("host-workflow-rework", ".soulmate/runs/rework.jsonl");
    let config = fixture.value(vec![
        "check".into(),
        "--json".into(),
        "--config".into(),
        "soulmate.json".into(),
    ]);
    assert_eq!(config["valid"], true);

    let started = fixture.value(vec![
        "run".into(),
        "start".into(),
        "change".into(),
        "--goal".into(),
        GOAL.into(),
        "--check-command".into(),
        CHECK.into(),
        "--ledger".into(),
        fixture.ledger.clone(),
        "--config".into(),
        "soulmate.json".into(),
    ]);
    let lead = assignment(&started);
    verify_packet(&fixture, &lead, "lead", "lead", 1, 1);
    let scoped = fixture.submit(&lead, "scoped", "initial scope\n");
    let worker = assignment(&scoped);
    verify_packet(&fixture, &worker, "worker", "worker", 2, 1);
    let first_check_command = packet_check_command(&worker);
    let (first_worker_response, first_worker_artifact) =
        fixture.submit_with_body(&worker, "completed", "first worker completion\n");
    let first_worker_event = first_worker_response["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let reviewer = assignment(&first_worker_response);
    verify_packet(&fixture, &reviewer, "reviewer", "reviewer", 3, 1);
    assert_eq!(fixture.execute_check(&first_check_command), 1);
    let failed_check = fixture.record_check(&first_worker_event, &first_check_command, 1);
    assert_eq!(
        failed_check["event"]["targetEventSha256"],
        first_worker_event
    );
    assert_eq!(failed_check["event"]["checkCommand"], first_check_command);
    let reviewed = fixture.submit(&reviewer, "approved", "first review\n");
    let accepting_lead = assignment(&reviewed);
    verify_packet(&fixture, &accepting_lead, "lead", "lead", 4, 1);
    let (failed_accept, _) =
        fixture.submit_output_with_body(&accepting_lead, "accepted", "premature acceptance\n");
    assert!(!failed_accept.status.success());
    assert!(text(&failed_accept).contains("acceptance refused"));

    let before_reads = fs::read(fixture.root.join(&fixture.ledger)).unwrap();
    let fresh_next = fixture.fresh_next();
    let fresh_status = fixture.fresh_status();
    assert_eq!(
        fs::read(fixture.root.join(&fixture.ledger)).unwrap(),
        before_reads
    );
    assert_eq!(fresh_next["status"], "running");
    let rework_lead = assignment(&fresh_next);
    verify_packet(&fixture, &rework_lead, "lead", "lead", 4, 1);
    assert_eq!(fresh_status["status"], "running");
    assert_eq!(fresh_status["checks"]["status"], "blocked");
    assert_eq!(fresh_status["checks"]["failedCount"], 1);
    assert_eq!(fresh_status["evidence"]["protectionCount"], 1);

    let (rework_response, _) = fixture.submit_with_body(&rework_lead, "rework", "repair request\n");
    let worker_two = assignment(&rework_response);
    verify_packet(&fixture, &worker_two, "worker", "worker", 2, 2);
    let second_check_command = packet_check_command(&worker_two);
    assert!(worker_two["upstreamArtifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|artifact| {
            artifact["path"] == first_worker_artifact && artifact["attemptStatus"] == "prior"
        }));
    fs::write(fixture.root.join("actual-product-check"), b"repaired\n").unwrap();
    let (second_worker_response, _) =
        fixture.submit_with_body(&worker_two, "completed", "second worker completion\n");
    let second_worker_event = second_worker_response["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first_worker_event, second_worker_event);
    let reviewer_two = assignment(&second_worker_response);
    verify_packet(&fixture, &reviewer_two, "reviewer", "reviewer", 3, 2);
    assert_eq!(fixture.execute_check(&second_check_command), 0);
    assert_eq!(second_check_command, first_check_command);
    let passing_check = fixture.record_check(&second_worker_event, &second_check_command, 0);
    assert_eq!(
        passing_check["event"]["targetEventSha256"],
        second_worker_event
    );
    assert_eq!(passing_check["event"]["checkCommand"], second_check_command);
    let reviewed_two = fixture.submit(&reviewer_two, "approved", "fresh review\n");
    let accepting_lead_two = assignment(&reviewed_two);
    verify_packet(&fixture, &accepting_lead_two, "lead", "lead", 4, 2);
    assert_eq!(
        fixture.submit(&accepting_lead_two, "accepted", "fresh acceptance\n")["status"],
        "accepted"
    );

    let final_status = fixture.fresh_status();
    assert_eq!(final_status["status"], "accepted");
    assert_eq!(final_status["attempt"], 2);
    assert_eq!(final_status["checks"]["status"], "passed");
    assert_eq!(final_status["checks"]["targetCount"], 1);
    assert_eq!(final_status["evidence"]["protectionCount"], 1);
    assert_eq!(final_status["evidence"]["checkCount"], 2);
    assert_eq!(final_status["review"]["status"], "approved");
    assert_eq!(final_status["acceptance"]["status"], "accepted");
    assert_eq!(
        fs::read(fixture.root.join(&first_worker_artifact)).unwrap(),
        b"first worker completion\n"
    );

    let events = fixture.events();
    let actions: Vec<_> = events
        .iter()
        .map(|event| event["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "start", "submit", "submit", "check", "submit", "protect", "submit", "submit", "check",
            "submit", "submit"
        ]
    );
    let submissions: Vec<_> = events
        .iter()
        .filter(|event| event["action"] == "submit")
        .map(|event| {
            (
                event["attempt"].as_u64().unwrap(),
                event["role"].as_str().unwrap(),
                event["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        submissions,
        [
            (1, "lead", "scoped"),
            (1, "worker", "completed"),
            (1, "reviewer", "approved"),
            (1, "lead", "rework"),
            (2, "worker", "completed"),
            (2, "reviewer", "approved"),
            (2, "lead", "accepted"),
        ]
    );
    assert_eq!(fixture.calls.len(), 16);
    assert_eq!(
        run_actions(&fixture.calls),
        [
            "start",
            "submit",
            "submit",
            "record-check",
            "submit",
            "submit",
            "next",
            "status",
            "submit",
            "submit",
            "record-check",
            "submit",
            "submit",
            "status",
        ]
    );
    assert_eq!(
        fixture.actor_calls,
        [
            ("lead".to_owned(), 1, 1),
            ("worker".to_owned(), 2, 1),
            ("reviewer".to_owned(), 3, 1),
            ("lead".to_owned(), 4, 1),
            ("lead".to_owned(), 4, 1),
            ("worker".to_owned(), 2, 2),
            ("reviewer".to_owned(), 3, 2),
            ("lead".to_owned(), 4, 2),
        ]
    );
}

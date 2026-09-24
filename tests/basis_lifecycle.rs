#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{Arc, Barrier},
    thread,
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
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
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }

    fn value(&self, args: &[&str]) -> Value {
        self.value_with_input(args, b"")
    }

    fn value_with_input(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, Some(input));
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{args:?}: invalid JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    fn artifact(&self, name: &str, body: &[u8]) -> String {
        let path = self.root.join(".exitbind/artifacts").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        format!(".exitbind/artifacts/{name}")
    }

    fn ledger(&self) -> &'static str {
        ".exitbind/runs/basis.jsonl"
    }

    fn artifact_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = fs::read_dir(self.root.join(".exitbind/artifacts"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| format!("{key:?}:{}", canonical(&object[key])))
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

fn hash_value(value: &Value) -> String {
    let digest = Sha256::digest(canonical(value).as_bytes());
    format!("{digest:x}")
}

fn basis_with_hash(mut basis: Value) -> Value {
    basis["sha256"] = json!(hash_value(&basis));
    basis
}

fn disposition_with_hash(mut disposition: Value) -> Value {
    disposition["sha256"] = json!(hash_value(&disposition));
    disposition
}

fn ledger_events(fixture: &Fixture) -> Vec<Value> {
    ledger_events_at(fixture, fixture.ledger())
}

fn ledger_events_at(fixture: &Fixture, ledger: &str) -> Vec<Value> {
    fs::read_to_string(fixture.root.join(ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn supersede_preserves_explicit_basis_and_review_policy_options() {
    let fixture = Fixture::new("supersede-policy-propagation");
    let original_basis = json!({
        "version": 1,
        "constraints": ["preserve predecessor"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["successor records owner policy"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
        "--basis",
        &serde_json::to_string(&original_basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let _lead = fixture.value(&["run", "next", fixture.ledger()]);
    let lead_artifact = fixture.artifact("supersede-lead.md", b"scope");
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &lead_artifact,
            "--artifact-root",
            "state",
        ],
        b"scope",
    );

    let successor_basis = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve predecessor", "require currentness"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["successor records owner policy"]
    }));
    let successor = fixture.value(&[
        "run",
        "supersede",
        fixture.ledger(),
        "--workflow",
        "change",
        "--goal",
        "successor goal",
        "--ledger",
        ".exitbind/runs/successor.jsonl",
        "--basis",
        &serde_json::to_string(&successor_basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let event = ledger_events_at(&fixture, ".exitbind/runs/successor.jsonl")[0].clone();
    assert_eq!(event["basis"], successor_basis);
    assert_eq!(event["reviewPolicy"]["decision"], "omitted");
    assert_eq!(event["subject"]["basisSha256"], successor_basis["sha256"]);
    let next = fixture.value(&["run", "next", ".exitbind/runs/successor.jsonl"]);
    assert_eq!(
        next["assignments"][0]["basisSha256"],
        successor_basis["sha256"]
    );
    assert_eq!(
        next["assignments"][0]["reviewDecisionSha256"],
        event["reviewPolicy"]["sha256"]
    );
    assert!(successor["runId"].is_string());
}

#[test]
fn supersede_claim_is_atomic_and_idempotent_under_concurrency() {
    let fixture = Fixture::new("supersede-concurrent-claim");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "concurrent predecessor",
        "--ledger",
        fixture.ledger(),
    ]);

    let barrier = Arc::new(Barrier::new(2));
    let root = fixture.root.clone();
    let workers = (0..2)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let root = root.clone();
            thread::spawn(move || {
                barrier.wait();
                Command::new(env!("CARGO_BIN_EXE_exitbind"))
                    .current_dir(&root)
                    .args([
                        "run",
                        "supersede",
                        ".exitbind/runs/basis.jsonl",
                        "--workflow",
                        "change",
                        "--goal",
                        "concurrent successor",
                        "--ledger",
                        ".exitbind/runs/concurrent-successor.jsonl",
                        "--review-policy",
                        "omitted",
                        "--config",
                    ])
                    .arg(root.join("exitbind.json"))
                    .output()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let outputs = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1,
        "concurrent supersede did not choose one winner: {:?}",
        outputs
            .iter()
            .map(|output| String::from_utf8_lossy(&output.stderr).into_owned())
            .collect::<Vec<_>>()
    );
    assert!(outputs.iter().any(|output| {
        !output.status.success()
            && (String::from_utf8_lossy(&output.stderr).contains("busy")
                || String::from_utf8_lossy(&output.stderr).contains("already claimed"))
    }));

    let claim = fixture.root.join(".exitbind/runs/basis.jsonl.supersede");
    let successor = fixture
        .root
        .join(".exitbind/runs/concurrent-successor.jsonl");
    assert!(claim.is_file());
    assert!(successor.is_file());
    assert_eq!(
        ledger_events_at(&fixture, ".exitbind/runs/concurrent-successor.jsonl").len(),
        1
    );
    let claim_bytes = fs::read(&claim).unwrap();
    let successor_bytes = fs::read(&successor).unwrap();

    let rerun = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "concurrent successor",
            "--ledger",
            ".exitbind/runs/concurrent-successor.jsonl",
            "--review-policy",
            "omitted",
        ],
        None,
    );
    assert!(
        rerun.status.success(),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(fs::read(&claim).unwrap(), claim_bytes);
    assert_eq!(fs::read(&successor).unwrap(), successor_bytes);

    let conflicting = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "different concurrent successor",
            "--ledger",
            ".exitbind/runs/conflicting-successor.jsonl",
        ],
        None,
    );
    assert!(!conflicting.status.success());
    assert!(String::from_utf8_lossy(&conflicting.stderr).contains("already claimed"));
    assert!(!fixture
        .root
        .join(".exitbind/runs/conflicting-successor.jsonl")
        .exists());
}

#[test]
fn supersede_rejects_basis_only_or_invalid_review_without_durable_mutation() {
    let fixture = Fixture::new("supersede-invalid-extension");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
    ]);
    let original = fs::read(fixture.root.join(fixture.ledger())).unwrap();
    let basis = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve predecessor"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["invalid extension is inert"]
    }));

    let basis_only = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "basis-only successor",
            "--ledger",
            ".exitbind/runs/basis-only.jsonl",
            "--basis",
            &serde_json::to_string(&basis).unwrap(),
        ],
        None,
    );
    assert!(!basis_only.status.success());
    assert!(String::from_utf8_lossy(&basis_only.stderr)
        .contains("--basis requires an explicit --review-policy"));
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        original
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/basis-only.jsonl")
        .exists());
    assert!(!fixture
        .root
        .join(format!("{}.supersede", fixture.ledger()))
        .exists());

    let invalid_review = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "invalid-review successor",
            "--ledger",
            ".exitbind/runs/invalid-review.jsonl",
            "--review-policy",
            "unsupported",
        ],
        None,
    );
    assert!(!invalid_review.status.success());
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        original
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/invalid-review.jsonl")
        .exists());
}

#[test]
fn supersede_review_policy_only_creates_marked_basisless_successor_assignment() {
    let fixture = Fixture::new("supersede-review-policy-only");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
    ]);
    fixture.value(&[
        "run",
        "supersede",
        fixture.ledger(),
        "--workflow",
        "change",
        "--goal",
        "review-only successor",
        "--ledger",
        ".exitbind/runs/review-only.jsonl",
        "--review-policy",
        "omitted",
    ]);

    let event = ledger_events_at(&fixture, ".exitbind/runs/review-only.jsonl")[0].clone();
    assert_eq!(event["reviewPolicy"]["decision"], "omitted");
    assert!(event["basisProtocol"].is_number());
    assert!(event.get("basis").is_none());
    assert!(event["subject"].get("basisSha256").is_none());
    let next = fixture.value(&["run", "next", ".exitbind/runs/review-only.jsonl"]);
    assert!(next["assignments"][0]["basisSha256"].is_null());
    assert_eq!(
        next["assignments"][0]["reviewDecisionSha256"],
        event["reviewPolicy"]["sha256"]
    );
}

#[test]
fn supersede_without_options_keeps_historical_unmarked_compatibility() {
    let fixture = Fixture::new("supersede-historical-no-options");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "original goal",
        "--ledger",
        fixture.ledger(),
    ]);
    fixture.value(&[
        "run",
        "supersede",
        fixture.ledger(),
        "--workflow",
        "change",
        "--goal",
        "historical successor",
        "--ledger",
        ".exitbind/runs/historical-successor.jsonl",
    ]);

    let event = ledger_events_at(&fixture, ".exitbind/runs/historical-successor.jsonl")[0].clone();
    assert!(event.get("basisProtocol").is_none());
    assert!(event.get("basis").is_none());
    assert!(event.get("reviewPolicy").is_none());
    assert!(event["subject"].get("basisSha256").is_none());
    let next = fixture.value(&["run", "next", ".exitbind/runs/historical-successor.jsonl"]);
    assert!(next["assignments"][0]["basisSha256"].is_null());
    assert!(next["assignments"][0]["reviewDecisionSha256"].is_null());
}

#[test]
fn marked_basis_contradiction_successor_preserves_governor_and_currentness() {
    let fixture = Fixture::new("basis-lifecycle");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["contradiction routes to Lead"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "basis lifecycle",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let start_event = ledger_events(&fixture).pop().unwrap();
    assert_eq!(start_event["version"], 8);
    assert_eq!(start_event["basisProtocol"], 1);
    let original_basis_sha = start_event["basis"]["sha256"].as_str().unwrap().to_owned();
    let initial_governor = fixture.value(&["run", "inspect", fixture.ledger()])["governor"].clone();

    let duplicate_policy = fixture.call(
        &[
            "run",
            "review-policy",
            "lead",
            fixture.ledger(),
            "--decision",
            "required",
        ],
        None,
    );
    assert!(!duplicate_policy.status.success());
    assert!(
        String::from_utf8_lossy(&duplicate_policy.stderr).contains("duplicate")
            || String::from_utf8_lossy(&duplicate_policy.stdout).contains("duplicate")
    );
    assert_eq!(ledger_events(&fixture).len(), 1);

    let lead = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(lead["assignments"][0]["role"], "lead");
    let lead_artifact = fixture.artifact("lead.md", b"scoped basis work");
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &lead_artifact,
            "--artifact-root",
            "state",
        ],
        b"scoped basis work",
    );

    let worker = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(worker["assignments"][0]["role"], "worker");
    let worker_artifact = fixture.artifact("worker.md", b"contradiction finding");
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &worker_artifact,
            "--artifact-root",
            "state",
            "--reason",
            "basis assumption failed",
        ],
        b"contradiction finding",
    );
    let finding_sha = contradiction["event"]["eventSha256"].as_str().unwrap();
    let pending = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(pending["assignments"][0]["role"], "lead");
    assert_eq!(pending["assignments"][0]["stage"], 4);

    let successor = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve history", "require currentness"],
        "openZones": ["worker implementation"],
        "decisiveCases": [
            "contradiction routes to Lead",
            "successor rechecks contradiction"
        ]
    }));
    let disposition = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding_sha],
        "basisSha256": original_basis_sha,
        "causalAssumption": "the original basis omitted a required currentness constraint",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "successor basis and fresh worker evidence",
        "decisiveRegression": "contradiction routes through Lead disposition",
        "invalidatedEvidence": [finding_sha],
        "successorBasis": successor,
    }));
    let disposition_text = serde_json::to_string(&disposition).unwrap();
    let disposition_artifact = fixture.artifact("disposition.md", b"Lead disposition");
    let events_before_invalid = ledger_events(&fixture).len();
    let mut wrong_finding = disposition.clone();
    wrong_finding["findingSha256s"] = json!(["0".repeat(64)]);
    let wrong_finding = disposition_with_hash(wrong_finding);
    let mut wrong_basis = disposition.clone();
    wrong_basis["basisSha256"] = json!("0".repeat(64));
    let wrong_basis = disposition_with_hash(wrong_basis);
    let mut wrong_category = disposition.clone();
    wrong_category["category"] = json!("unknown_category");
    let wrong_category = disposition_with_hash(wrong_category);
    for (agent, raw) in [
        ("lead", serde_json::to_string(&wrong_finding).unwrap()),
        ("lead", serde_json::to_string(&wrong_basis).unwrap()),
        ("lead", serde_json::to_string(&wrong_category).unwrap()),
        ("worker", disposition_text.clone()),
    ] {
        let refused = fixture.call(
            &[
                "run",
                "submit",
                agent,
                fixture.ledger(),
                "--outcome",
                "disposition",
                "--artifact",
                &disposition_artifact,
                "--artifact-root",
                "state",
                "--disposition",
                &raw,
            ],
            None,
        );
        assert!(
            !refused.status.success(),
            "invalid disposition unexpectedly accepted"
        );
        assert_eq!(ledger_events(&fixture).len(), events_before_invalid);
    }
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &disposition_artifact,
            "--artifact-root",
            "state",
            "--disposition",
            &disposition_text,
        ],
        b"Lead disposition",
    );

    let fresh_worker = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(fresh_worker["assignments"][0]["role"], "worker");
    assert_ne!(
        fresh_worker["assignments"][0]["basisSha256"],
        original_basis_sha
    );
    let state = fixture.value(&["run", "status", fixture.ledger(), "--json"]);
    let inspect = fixture.value(&["run", "inspect", fixture.ledger()]);
    assert_eq!(state["status"], "running");
    assert_eq!(
        inspect["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["action"] == "submit" && event["outcome"] == "disposition")
            .count(),
        1
    );
    assert_eq!(inspect["governor"]["enabled"], true);
    assert!(inspect["governor"]["spent"].is_number());
    assert!(inspect["governor"]["consumedGrants"].is_array());
    assert_eq!(inspect["governor"]["spent"], initial_governor["spent"]);
    assert_eq!(
        inspect["governor"]["consumedGrants"],
        initial_governor["consumedGrants"]
    );
    assert_eq!(
        inspect["governor"]["lineageSha256"],
        initial_governor["lineageSha256"]
    );

    let fresh_artifact = fixture.artifact("fresh-worker.md", b"fresh successor evidence");
    let completed = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "completed",
            "--artifact",
            &fresh_artifact,
            "--artifact-root",
            "state",
        ],
        b"fresh successor evidence",
    );
    fixture.value(&[
        "run",
        "record-check",
        fixture.ledger(),
        "--target",
        completed["event"]["eventSha256"].as_str().unwrap(),
        "--check-command",
        "true",
        "--exit-code",
        "0",
        "--duration-ms",
        "1",
    ]);
    fs::write(
        fixture.root.join(&fresh_artifact),
        b"tampered successor evidence",
    )
    .unwrap();
    let stale = fixture.value(&["run", "status", fixture.ledger(), "--json"]);
    assert_eq!(
        stale["artifact"]["status"], "drifted",
        "tampered artifact unexpectedly remained current: {stale}"
    );

    let events = ledger_events(&fixture);
    assert!(events
        .iter()
        .any(|event| event["action"] == "submit" && event["outcome"] == "contradiction"));
    assert!(events
        .iter()
        .any(|event| event["action"] == "submit" && event["outcome"] == "disposition"));
    assert!(events
        .iter()
        .any(|event| event["action"] == "start" && event["version"] == 8));
    let events_after_disposition = events.len();
    let duplicate = fixture.call(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &disposition_artifact,
            "--artifact-root",
            "state",
            "--disposition",
            &disposition_text,
        ],
        None,
    );
    assert!(!duplicate.status.success());
    assert_eq!(ledger_events(&fixture).len(), events_after_disposition);
}

#[test]
fn implementation_correction_is_a_basis_noop_and_restarts_worker() {
    let fixture = Fixture::new("basis-noop-correction");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["correction keeps the basis"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "basis no-op correction",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let original_basis = ledger_events(&fixture)[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &fixture.artifact("noop-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &fixture.artifact("noop-finding.md", b"finding"),
            "--artifact-root",
            "state",
        ],
        b"finding",
    );
    let finding = contradiction["event"]["eventSha256"].as_str().unwrap();
    let disposition = disposition_with_hash(json!({
        "category": "implementation_defect",
        "findingSha256s": [finding],
        "basisSha256": original_basis,
        "causalAssumption": "the implementation changed without changing the basis",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "fresh worker evidence",
        "decisiveRegression": "the original basis remains authoritative",
        "invalidatedEvidence": [finding],
        "successorBasis": null
    }));
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &fixture.artifact("noop-disposition.md", b"disposition"),
            "--artifact-root",
            "state",
            "--disposition",
            &serde_json::to_string(&disposition).unwrap(),
        ],
        b"disposition",
    );
    let next = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(next["assignments"][0]["role"], "worker");
    assert_eq!(next["assignments"][0]["basisSha256"], original_basis);
}

#[test]
fn design_correction_narrows_open_zone_and_preserves_bound_claims() {
    let fixture = Fixture::new("basis-narrowing-correction");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation", "lead clarification"],
        "decisiveCases": ["the original finding is recorded"],
        "boundarySha256": "a".repeat(64),
        "preservationSha256": "b".repeat(64)
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "narrow basis correction",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let original_basis = ledger_events(&fixture)[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &fixture.artifact("narrow-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &fixture.artifact("narrow-finding.md", b"finding"),
            "--artifact-root",
            "state",
        ],
        b"finding",
    );
    let finding = contradiction["event"]["eventSha256"].as_str().unwrap();
    let successor = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve history", "require currentness"],
        "openZones": ["worker implementation"],
        "decisiveCases": [
            "the original finding is recorded",
            "successor regression"
        ],
        "boundarySha256": "a".repeat(64),
        "preservationSha256": "b".repeat(64)
    }));
    let disposition = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding],
        "basisSha256": original_basis,
        "causalAssumption": "the open zone was too broad",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "narrowed successor basis and fresh evidence",
        "decisiveRegression": "the successor retains the bound claims",
        "invalidatedEvidence": [finding],
        "successorBasis": successor
    }));
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &fixture.artifact("narrow-disposition.md", b"disposition"),
            "--artifact-root",
            "state",
            "--disposition",
            &serde_json::to_string(&disposition).unwrap(),
        ],
        b"disposition",
    );
    let disposition_event = ledger_events(&fixture)
        .into_iter()
        .find(|event| event["outcome"] == "disposition")
        .unwrap();
    assert_eq!(
        disposition_event["disposition"]["successorBasis"]["openZones"],
        json!(["worker implementation"])
    );
    assert_eq!(
        disposition_event["disposition"]["successorBasis"]["boundarySha256"],
        "a".repeat(64)
    );
    assert_eq!(
        disposition_event["disposition"]["successorBasis"]["preservationSha256"],
        "b".repeat(64)
    );
    let next = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(next["assignments"][0]["role"], "worker");
    assert_eq!(
        next["assignments"][0]["basisSha256"],
        disposition_event["disposition"]["successorBasis"]["sha256"]
    );
}

#[test]
fn blocked_or_superseded_correction_refuses_without_mutation() {
    let blocked = Fixture::new("blocked-correction-refusal");
    blocked.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "blocked correction",
        "--ledger",
        blocked.ledger(),
        "--review-policy",
        "omitted",
    ]);
    blocked.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            blocked.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &blocked.artifact("blocked-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    blocked.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            blocked.ledger(),
            "--outcome",
            "blocked",
            "--artifact",
            &blocked.artifact("blocked-worker.md", b"blocked"),
            "--artifact-root",
            "state",
        ],
        b"blocked",
    );
    let before = fs::read(blocked.root.join(blocked.ledger())).unwrap();
    let artifacts = blocked.artifact_snapshot();
    let refused = blocked.call(
        &[
            "run",
            "submit",
            "lead",
            blocked.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            ".exitbind/artifacts/blocked-worker.md",
            "--artifact-root",
            "state",
            "--disposition",
            "{}",
        ],
        Some(b"should not persist"),
    );
    assert!(!refused.status.success());
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("terminal"), "{refused_text}");
    assert_eq!(
        fs::read(blocked.root.join(blocked.ledger())).unwrap(),
        before
    );
    assert_eq!(blocked.artifact_snapshot(), artifacts);

    let superseded = Fixture::new("superseded-correction-refusal");
    superseded.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "superseded correction",
        "--ledger",
        superseded.ledger(),
    ]);
    superseded.value(&[
        "run",
        "supersede",
        superseded.ledger(),
        "--workflow",
        "change",
        "--goal",
        "successor",
        "--ledger",
        ".exitbind/runs/successor.jsonl",
    ]);
    let stale_artifact = superseded.artifact("stale-correction.md", b"stale");
    let before = fs::read(superseded.root.join(superseded.ledger())).unwrap();
    let artifacts = superseded.artifact_snapshot();
    let refused = superseded.call(
        &[
            "run",
            "submit",
            "lead",
            superseded.ledger(),
            "--outcome",
            "blocked",
            "--artifact",
            &stale_artifact,
            "--artifact-root",
            "state",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("superseded"));
    assert_eq!(
        fs::read(superseded.root.join(superseded.ledger())).unwrap(),
        before
    );
    assert_eq!(superseded.artifact_snapshot(), artifacts);
}

#[test]
fn invalid_successor_or_claim_is_inert() {
    let fixture = Fixture::new("invalid-correction-noop");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["invalid correction is inert"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "invalid correction",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "omitted",
    ]);
    let original_basis = ledger_events(&fixture)[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &fixture.artifact("invalid-scope.md", b"scope"),
            "--artifact-root",
            "state",
        ],
        b"scope",
    );
    let contradiction = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "contradiction",
            "--artifact",
            &fixture.artifact("invalid-finding.md", b"finding"),
            "--artifact-root",
            "state",
        ],
        b"finding",
    );
    let finding = contradiction["event"]["eventSha256"].as_str().unwrap();
    let invalid = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding],
        "basisSha256": original_basis,
        "causalAssumption": "invalid widening",
        "affectedPaths": ["worker implementation"],
        "repairBoundary": "none",
        "decisiveRegression": "rejected before artifact creation",
        "invalidatedEvidence": [finding],
        "successorBasis": basis_with_hash(json!({
            "version": 1,
            "constraints": ["preserve history"],
            "openZones": ["worker implementation"],
            "decisiveCases": ["dropped decisive case"]
        }))
    }));
    let invalid_artifact = fixture.artifact("invalid-disposition.md", b"must not persist");
    let before = fs::read(fixture.root.join(fixture.ledger())).unwrap();
    let artifacts = fixture.artifact_snapshot();
    let refused = fixture.call(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "disposition",
            "--artifact",
            &invalid_artifact,
            "--artifact-root",
            "state",
            "--disposition",
            &serde_json::to_string(&invalid).unwrap(),
        ],
        None,
    );
    assert!(!refused.status.success());
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(refused_text.contains("decisive case"), "{refused_text}");
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        before
    );
    assert_eq!(fixture.artifact_snapshot(), artifacts);

    let claim = fixture.root.join(format!("{}.supersede", fixture.ledger()));
    fs::write(&claim, b"not a valid supersession claim\n").unwrap();
    let before = fs::read(fixture.root.join(fixture.ledger())).unwrap();
    let refused = fixture.call(
        &[
            "run",
            "supersede",
            fixture.ledger(),
            "--workflow",
            "change",
            "--goal",
            "invalid claim successor",
            "--ledger",
            ".exitbind/runs/invalid-claim.jsonl",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("claim"));
    assert_eq!(
        fs::read(fixture.root.join(fixture.ledger())).unwrap(),
        before
    );
    assert!(!fixture
        .root
        .join(".exitbind/runs/invalid-claim.jsonl")
        .exists());
    assert_eq!(
        fs::read(&claim).unwrap(),
        b"not a valid supersession claim\n"
    );
}

#[test]
fn successor_retains_a_real_governor_grant_and_owner_lineage() {
    let fixture = Fixture::new("basis-governor-successor");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve governor lineage"],
        "openZones": ["worker implementation"],
        "decisiveCases": ["review rework gets a Lead disposition"]
    });
    let begin = fixture.value(&[
        "work",
        "begin",
        "change",
        "--goal",
        "governor successor",
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let work = begin["work"].as_str().unwrap().to_owned();
    let lead = begin["next"].clone();
    let lead_handle = lead["assignment"].as_str().unwrap();
    let scoped = fixture.value_with_input(
        &["work", "return", &work, lead_handle, "--outcome", "scoped"],
        b"scope",
    );
    let worker = scoped["next"].clone();
    let worker_handle = worker["assignment"].as_str().unwrap();
    let permit = fixture.value(&[
        "work",
        "permit",
        &work,
        worker_handle,
        "--operation",
        "successor worker mutation",
    ]);
    assert_eq!(permit["governor"]["enabled"], true);
    assert!(
        permit["governor"].is_object(),
        "invalid governor projection: {permit}"
    );
    let granted = fixture.value_with_input(
        &[
            "work",
            "return",
            &work,
            worker_handle,
            "--outcome",
            "completed",
        ],
        b"worker evidence",
    );
    fixture.value(&["work", "check", &work]);
    let reviewer = fixture.value(&["work", "next", &work])["next"].clone();
    assert_eq!(reviewer["role"], "reviewer");
    let rework = fixture.value_with_input(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "rework",
        ],
        b"review finding",
    );
    assert_eq!(rework["next"]["role"], "lead");
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let before = fixture.value(&["run", "inspect", &ledger]);
    let events_before = ledger_events_at(&fixture, &ledger);
    let finding = events_before
        .iter()
        .find(|event| event["outcome"] == "rework")
        .unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let current_basis = events_before[0]["basis"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let successor = basis_with_hash(json!({
        "version": 1,
        "constraints": ["preserve governor lineage", "preserve review finding"],
        "openZones": ["worker implementation"],
        "decisiveCases": [
            "review rework gets a Lead disposition",
            "fresh worker evidence"
        ]
    }));
    let disposition = disposition_with_hash(json!({
        "category": "contract_or_design_defect",
        "findingSha256s": [finding],
        "basisSha256": current_basis,
        "causalAssumption": "review rework identifies a design correction",
        "affectedPaths": ["reviewed worker path"],
        "repairBoundary": "successor basis and fresh evidence",
        "decisiveRegression": "the next worker is bound to the successor",
        "invalidatedEvidence": [finding],
        "successorBasis": successor,
    }));
    let lead_after_rework = rework["next"]["assignment"].as_str().unwrap();
    let after = fixture.value_with_input(
        &[
            "work",
            "return",
            &work,
            lead_after_rework,
            "--outcome",
            "disposition",
            "--disposition",
            &serde_json::to_string(&disposition).unwrap(),
        ],
        b"Lead disposition",
    );
    assert_eq!(after["next"]["role"], "worker");
    assert_eq!(after["next"]["resolvedActor"], "worker");
    assert_eq!(after["next"]["requiresExpansion"], true);
    assert!(after["next"]["packet"].is_null());
    let successor_packet = fixture.value(&["work", "next", &work, "--full"]);
    assert!(successor_packet["next"]["packet"]["context"].is_object());
    let after_inspect = fixture.value(&["run", "inspect", &ledger]);
    assert_eq!(before["governor"]["enabled"], true);
    assert!(before["governor"]["spent"].as_u64().unwrap_or(0) > 0);
    assert!(!before["governor"]["consumedGrants"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(after_inspect["governor"]["enabled"], true);
    assert_eq!(
        after_inspect["governor"]["spent"],
        before["governor"]["spent"]
    );
    assert_eq!(
        after_inspect["governor"]["consumedGrants"],
        before["governor"]["consumedGrants"]
    );
    assert_eq!(
        after_inspect["governor"]["lineageSha256"],
        before["governor"]["lineageSha256"]
    );
    let _ = granted;
}

#[test]
fn marked_required_to_omitted_accepts_and_exit_receipt_detects_tamper() {
    let fixture = Fixture::new("review-omission");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve findings"],
        "openZones": [],
        "decisiveCases": ["omission remains explicit"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "review omission",
        "--ledger",
        fixture.ledger(),
        "--check-command",
        "true",
        "--proof-origin",
        "local_report",
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let lead = fixture.value(&["run", "next", fixture.ledger()]);
    let lead_artifact = fixture.artifact("lead.md", b"lead scope");
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "scoped",
            "--artifact",
            &lead_artifact,
            "--artifact-root",
            "state",
        ],
        b"lead scope",
    );
    let worker = fixture.value(&["run", "next", fixture.ledger()]);
    let worker_artifact = fixture.artifact("worker.md", b"worker finding");
    let completed = fixture.value_with_input(
        &[
            "run",
            "submit",
            "worker",
            fixture.ledger(),
            "--outcome",
            "completed",
            "--artifact",
            &worker_artifact,
            "--artifact-root",
            "state",
        ],
        b"worker finding",
    );
    fixture.value(&[
        "run",
        "record-check",
        fixture.ledger(),
        "--target",
        completed["event"]["eventSha256"].as_str().unwrap(),
        "--check-command",
        "true",
        "--exit-code",
        "0",
        "--duration-ms",
        "1",
    ]);
    let reviewer = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(
        reviewer["assignments"][0]["role"], "reviewer",
        "unexpected reviewer-stage view: {reviewer}"
    );
    fixture.value(&[
        "run",
        "review-policy",
        "lead",
        fixture.ledger(),
        "--decision",
        "omitted",
        "--reason",
        "owner accepted the explicit omission after the worker finding",
    ]);
    let final_lead = fixture.value(&["run", "next", fixture.ledger()]);
    assert_eq!(final_lead["assignments"][0]["role"], "lead");
    assert_eq!(final_lead["assignments"][0]["stage"], 4);
    let acceptance_artifact = fixture.artifact("acceptance.md", b"accepted with finding preserved");
    fixture.value_with_input(
        &[
            "run",
            "submit",
            "lead",
            fixture.ledger(),
            "--outcome",
            "accepted",
            "--artifact",
            &acceptance_artifact,
            "--artifact-root",
            "state",
        ],
        b"accepted with finding preserved",
    );
    let status = fixture.value(&["run", "status", fixture.ledger(), "--json"]);
    assert_eq!(status["status"], "accepted");
    assert_eq!(status["review"]["status"], "omitted");
    assert_eq!(status["review"]["source"], "owner-reported");
    assert_eq!(status["evidence"]["submissionCount"], 3);

    let receipt = fixture.value(&[
        "receipt",
        fixture.ledger(),
        "--output",
        ".exitbind/receipts/omitted.json",
        "--json",
    ]);
    assert_eq!(receipt["review"]["status"], "omitted");
    let receipt_path = fixture.root.join(".exitbind/receipts/omitted.json");
    let verified = fixture.value(&["verify", ".exitbind/receipts/omitted.json"]);
    assert_eq!(verified["valid"], true);
    let mut tampered: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    tampered["review"]["status"] = json!("approved");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
    let invalid = fixture.call(&["verify", ".exitbind/receipts/omitted.json"], None);
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8_lossy(&invalid.stdout).contains("receipt_mismatch")
            || String::from_utf8_lossy(&invalid.stderr).contains("receipt_mismatch")
    );

    let events = ledger_events(&fixture);
    assert!(events.iter().any(|event| event["action"] == "review_policy"
        && event["reviewPolicy"]["decision"] == "omitted"));
    assert!(!events.iter().any(|event| event["role"] == "reviewer"));
    let finding = events
        .iter()
        .find(|event| event["role"] == "worker" && event["outcome"] == "completed")
        .unwrap();
    let finding_path = finding["artifact"]["path"].as_str().unwrap();
    assert!(fs::read(fixture.root.join(finding_path))
        .unwrap()
        .windows(b"worker finding".len())
        .any(|window| window == b"worker finding"));
    let _ = lead;
    let _ = worker;
}

#[test]
fn unmarked_historic_start_remains_unmarked() {
    let fixture = Fixture::new("historic-unmarked");
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "historic",
        "--ledger",
        fixture.ledger(),
    ]);
    let start_event = ledger_events(&fixture).pop().unwrap();
    assert_eq!(start_event["version"], 8);
    assert!(start_event.get("basisProtocol").is_none());
    assert!(start_event.get("reviewPolicy").is_none());
    let events = ledger_events(&fixture);
    assert_eq!(events.len(), 1);
    assert!(events[0].get("basis").is_none());
}

#[test]
fn marked_extension_is_rejected_by_the_installed_older_reader() {
    let Ok(old_reader) = std::env::var("EXITBIND_V022_BIN") else {
        eprintln!("skipped: set EXITBIND_V022_BIN to the installed v0.22 reader");
        return;
    };
    let version = Command::new(&old_reader).arg("--version").output().unwrap();
    assert!(version.status.success(), "older reader --version failed");
    let version_text = format!(
        "{}{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(
        version_text.contains("0.22"),
        "unexpected reader version: {version_text}"
    );
    let fixture = Fixture::new("old-reader-marked");
    let basis = json!({
        "version": 1,
        "constraints": ["preserve history"],
        "openZones": [],
        "decisiveCases": ["old readers refuse the marker"]
    });
    fixture.value(&[
        "run",
        "start",
        "change",
        "--goal",
        "old reader",
        "--ledger",
        fixture.ledger(),
        "--basis",
        &serde_json::to_string(&basis).unwrap(),
        "--review-policy",
        "required",
    ]);
    let output = Command::new(&old_reader)
        .current_dir(&fixture.root)
        .args([
            "run",
            "status",
            fixture.ledger(),
            "--config",
            "exitbind.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("unknown field") || text.contains("unsupported"),
        "{text}"
    );
}

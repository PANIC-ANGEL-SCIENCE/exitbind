use super::*;

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
    let version = super::support::git_topology::command(&old_reader)
        .arg("--version")
        .output()
        .unwrap();
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
    let output = super::support::git_topology::command(&old_reader)
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

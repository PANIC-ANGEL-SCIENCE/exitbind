use super::*;

#[test]
fn sequential_preservation_requirement_uses_its_own_deadline_and_replays_exactly() {
    let f = Fixture::new("printf primary >> .exitbind/attempt-count; exit 0\n");
    let start = f.ok(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "two applicable checks",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "identity:keep the record",
            "--preservation-check-command",
            "printf preservation >> .exitbind/attempt-count; exit 0",
        ],
        b"",
    );
    let work = start["work"].as_str().unwrap();
    let worker = f.give(work, &start["next"], "scoped")["next"].clone();
    f.give(work, &worker, "completed");
    let primary = f.ok(&["work", "check", work, "--timeout-ms", "1000"], b"");
    assert_eq!(primary["result"]["code"], 0);
    let preservation = f.ok(&["work", "check", work, "--timeout-ms", "2000"], b"");
    assert_eq!(preservation["result"]["code"], 0);
    assert!(preservation["eventSha256"] != primary["eventSha256"]);
    let replay = f.ok(&["work", "check", work, "--timeout-ms", "2000"], b"");
    assert_eq!(replay["eventSha256"], preservation["eventSha256"]);
    assert_eq!(replay["recovered"], true);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"primarypreservation"
    );
}

#[test]
fn complete_failure_and_lost_response_replay_never_execute_twice() {
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; exit 7\n");
    let work = f.begin();
    let failed = f.call(&["work", "check", &work], b"");
    assert!(!failed.status.success());
    let first = value(&failed);
    assert_eq!(first["result"]["code"], 7);
    let replay = f.call(&["work", "check", &work], b"");
    assert!(!replay.status.success());
    let replay = value(&replay);
    assert_eq!(first["eventSha256"], replay["eventSha256"]);
    assert_eq!(replay["recovered"], true);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        1
    );
}

#[test]
fn successful_lost_response_replays_and_changed_limits_and_inputs_refuse() {
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; exit 0\n");
    let work = f.begin();
    let first = f.ok(&["work", "check", &work], b"");
    let replay = f.ok(&["work", "check", &work], b"");
    assert_eq!(replay["eventSha256"], first["eventSha256"]);
    assert_eq!(replay["recovered"], true);
    assert!(!f
        .call(&["work", "check", &work, "--timeout-ms", "1000"], b"")
        .status
        .success());
    fs::write(f.root.join("checker.sh"), "exit 0 # drift\n").unwrap();
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
}

#[test]
fn simultaneous_requests_claim_once_and_unfinished_admission_blocks_supersession() {
    let f = Fixture::new("printf started > .exitbind/started; while [ ! -f .exitbind/release ]; do sleep 0.01; done; exit 0\n");
    let work = f.begin();
    let mut first = f
        .command(&["work", "check", &work, "--timeout-ms", "5000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.root.join(".exitbind/started").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(f.root.join(".exitbind/started").exists());
    let second = f.call(&["work", "check", &work, "--timeout-ms", "5000"], b"");
    assert!(!second.status.success());
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    let relative = f
        .ledger(&work)
        .strip_prefix(&f.root)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let refused = f.call(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "retry",
            "--ledger",
            ".exitbind/runs/successor.jsonl",
        ],
        b"",
    );
    assert!(!refused.status.success());
    assert!(!f.root.join(".exitbind/runs/successor.jsonl").exists());
    fs::write(f.root.join(".exitbind/release"), b"release").unwrap();
    assert!(first.wait().unwrap().success());
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check_observation")
            .count(),
        1
    );
}

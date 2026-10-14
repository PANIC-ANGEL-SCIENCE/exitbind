use super::*;

#[test]
fn request_id_replays_exact_permit_without_new_event_or_spent() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "replay a mutation request",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let args = [
        "work",
        "permit",
        work.as_str(),
        assignment.as_str(),
        "--operation",
        "edit",
        "--request-id",
        "request-1",
    ];
    let first = fixture.call(&args);
    assert!(first.status.success(), "{first:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["compact"], true);
    assert_eq!(first["allowed"], true);
    assert_eq!(first["effect"], "recorded");
    assert_eq!(first["event"]["action"], "govern");
    assert!(first["governor"]["spent"].is_null());
    let event_sha = first["event"]["eventSha256"].as_str().unwrap().to_owned();
    let detail = fixture.json(&["run", "inspect", &ledger, "--event", &event_sha]);
    assert_eq!(detail["eventSha256"], event_sha);
    assert_eq!(detail["event"]["requestId"], "request-1");
    assert!(detail["event"]["requestDigest"].is_string());
    assert_eq!(
        detail["event"]["requestDigest"],
        detail["event"]["governorEvent"]["requestDigest"]
    );
    assert_eq!(
        detail["event"]["requestId"],
        detail["event"]["governorEvent"]["requestId"]
    );
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let replay = fixture.call(&args);
    assert!(replay.status.success(), "{replay:?}");
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["idempotent"], true);
    assert_eq!(
        replay["event"]["eventSha256"],
        first["event"]["eventSha256"]
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        1
    );
}

#[test]
fn request_id_conflicting_operation_refuses_without_ledger_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject a conflicting mutation request",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let conflict = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "different-edit",
        "--request-id",
        "request-1",
    ]);
    assert!(!conflict.status.success(), "{conflict:?}");
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("request-id conflict"));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        1
    );
}

#[test]
fn stale_request_id_is_not_replayed_after_assignment_advances() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "refuse a stale mutation request",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());
    fs::write(fixture.root.join("product.txt"), b"advanced\n").unwrap();
    assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    assert!(fixture
        .return_body(&work, &assignment, "completed", b"worker\n")
        .status
        .success());
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let stale = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-1",
    ]);
    assert!(
        !stale.status.success(),
        "stale request was replayed: {stale:?}"
    );
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn invalid_request_id_is_refused_before_ledger_append() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "validate mutation request ids",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let before = fs::read(fixture.root.join(&ledger)).unwrap();
    let empty = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "",
    ]);
    assert!(!empty.status.success());
    let oversized = "x".repeat(121);
    let oversized_call = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        oversized.as_str(),
    ]);
    assert!(!oversized_call.status.success());
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before);
}

#[test]
fn blocked_request_id_replay_remains_refused_without_a_second_event() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve a durable blocked refusal",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let mut assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    for request_id in ["request-1", "request-2"] {
        let permit = fixture.call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            request_id,
        ]);
        assert!(permit.status.success(), "{permit:?}");
        fs::write(fixture.root.join("product.txt"), request_id).unwrap();
        assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
        if request_id == "request-2" {
            let replanned = fixture.call(&[
                "work",
                "replan",
                &work,
                &assignment,
                "--hypothesis",
                "bounded blocked replay",
            ]);
            assert!(replanned.status.success(), "{replanned:?}");
            assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned();
        }
    }
    let third = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-3",
    ]);
    assert!(third.status.success(), "{third:?}");
    fs::write(fixture.root.join("product.txt"), b"request-3").unwrap();
    assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let blocked = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-4",
    ]);
    assert!(
        !blocked.status.success(),
        "blocked permit unexpectedly succeeded"
    );
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("durably blocked"));
    let after_block = fs::read(fixture.root.join(&ledger)).unwrap();
    let replay = fixture.call(&[
        "work",
        "permit",
        &work,
        &assignment,
        "--operation",
        "edit",
        "--request-id",
        "request-4",
    ]);
    assert!(
        !replay.status.success(),
        "blocked request replay escaped refusal"
    );
    assert!(String::from_utf8_lossy(&replay.stderr).contains("durably blocked"));
    assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), after_block);
}

#[test]
fn reducer_rejects_rehashed_request_digest_and_duplicate_request_scope() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject forged request identity history",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());

    let path = fixture.root.join(&ledger);
    let mut lines: Vec<Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let last = lines.last_mut().unwrap();
    last["requestDigest"] = json!("f".repeat(64));
    last["governorEvent"]["requestDigest"] = last["requestDigest"].clone();
    refresh_event_hash(&mut last["governorEvent"]);
    refresh_event_hash(last);
    fs::write(
        &path,
        lines
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let forged = fixture.call(&["run", "inspect", &ledger]);
    assert!(
        !forged.status.success(),
        "forged request digest was accepted"
    );
    assert!(String::from_utf8_lossy(&forged.stderr).contains("request digest"));

    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject duplicate request identity history",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(fixture
        .return_body(
            &work,
            started["next"]["assignment"].as_str().unwrap(),
            "scoped",
            b"scope\n",
        )
        .status
        .success());
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    assert!(fixture
        .call(&[
            "work",
            "permit",
            &work,
            &assignment,
            "--operation",
            "edit",
            "--request-id",
            "request-1",
        ])
        .status
        .success());
    let path = fixture.root.join(&ledger);
    let mut lines: Vec<Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut duplicate = lines.last().unwrap().clone();
    duplicate["previousEventSha256"] = lines.last().unwrap()["eventSha256"].clone();
    refresh_event_hash(&mut duplicate);
    lines.push(duplicate);
    fs::write(
        &path,
        lines
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let duplicate = fixture.call(&["run", "inspect", &ledger]);
    assert!(
        !duplicate.status.success(),
        "duplicate request identity was accepted"
    );
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate governor request"));
}

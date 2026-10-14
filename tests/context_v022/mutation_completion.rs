use super::*;

#[test]
fn legacy_permit_without_request_id_keeps_historical_event_shape() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve legacy mutation permits",
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
    let worker = fixture.json(&["work", "next", &work]);
    let permit = fixture.call(&[
        "work",
        "permit",
        &work,
        worker["next"]["assignment"].as_str().unwrap(),
        "--operation",
        "edit",
    ]);
    assert!(permit.status.success(), "{permit:?}");
    let value: Value = serde_json::from_slice(&permit.stdout).unwrap();
    assert_eq!(value["compact"], true);
    assert_eq!(value["allowed"], true);
    assert_eq!(value["effect"], "recorded");
    assert_eq!(value["event"]["action"], "govern");
    assert!(value["event"]["eventSha256"].is_string());
    assert!(value["governor"]["spent"].is_null());
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let event_sha = value["event"]["eventSha256"].as_str().unwrap();
    let detail = fixture.json(&["run", "inspect", &ledger, "--event", event_sha]);
    assert_eq!(detail["eventSha256"], event_sha);
    assert!(detail["event"]["requestId"].is_null());
    assert!(detail["event"]["requestDigest"].is_null());
    assert!(detail["event"]["governorEvent"]["requestId"].is_null());
    assert!(detail["event"]["governorEvent"]["requestDigest"].is_null());
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        1
    );
}

#[test]
fn public_pre_mutation_permit_persists_before_product_edit_and_refuses_fourth() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "consume pre-mutation units atomically",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let worker = fixture.json(&["work", "next", &work]);
    let mut assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let mut tampered_assignment = assignment.clone();
    tampered_assignment.pop();
    tampered_assignment.push(if assignment.ends_with('0') { '1' } else { '0' });
    assert_ne!(tampered_assignment, assignment);
    let tampered = fixture.call(&[
        "work",
        "permit",
        &work,
        &tampered_assignment,
        "--operation",
        "edit",
    ]);
    assert!(
        !tampered.status.success(),
        "tampered assignment was accepted"
    );
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        0
    );
    for unit in 1..=4 {
        if unit == 3 {
            let replanned = fixture.call(&[
                "work",
                "replan",
                &work,
                &assignment,
                "--hypothesis",
                "new bounded hypothesis",
            ]);
            assert!(replanned.status.success(), "{replanned:?}");
            assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned();
        }
        let before_permit = fs::read(fixture.root.join(&ledger)).unwrap();
        let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
        if unit < 4 {
            assert!(permit.status.success(), "{permit:?}");
            let value: Value = serde_json::from_slice(&permit.stdout).unwrap();
            assert_eq!(value["allowed"], true);
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], unit);
            fs::write(
                fixture.root.join("product.txt"),
                format!("permitted product edit {unit}\n"),
            )
            .unwrap();
            let resumed = fixture.json(&["work", "resume"]);
            assert_eq!(resumed["status"], "resumed");
        } else {
            assert!(!permit.status.success(), "fourth permit bypassed the bound");
            assert_eq!(
                fs::read(fixture.root.join("product.txt")).unwrap(),
                b"permitted product edit 3\n"
            );
            assert_ne!(fs::read(fixture.root.join(&ledger)).unwrap(), before_permit);
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], 3);
            assert_eq!(inspected["governor"]["state"], "blocked");
        }
    }

    // The owning action refuses cross-run and malformed operation inputs
    // before it reaches the ledger append boundary.
    let other = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "cross-run permit refusal",
        "--check-command",
        "true",
    ]);
    let cross = fixture.call(&[
        "work",
        "permit",
        other["work"].as_str().unwrap(),
        &assignment,
        "--operation",
        "edit",
    ]);
    assert!(!cross.status.success());
    let malformed = fixture.call(&["work", "permit", &work, &assignment, "--operation", ""]);
    assert!(!malformed.status.success());
}

#[test]
fn pre_mutation_permits_are_not_double_counted_by_worker_completion() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "complete after cooperative mutation permits",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = fixture.json(&["work", "next", &work]);
    let mut assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );

    // Three independent CLI calls authorize product edits before any worker
    // completion. The third leaves the canonical governor at its hard bound.
    for unit in 1..=4 {
        if unit == 3 {
            let replanned = fixture.call(&[
                "work",
                "replan",
                &work,
                &assignment,
                "--hypothesis",
                "new bounded hypothesis",
            ]);
            assert!(replanned.status.success(), "{replanned:?}");
            assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
                .as_str()
                .unwrap()
                .to_owned();
        }
        let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
        if unit == 4 {
            assert!(!permit.status.success(), "fourth permit bypassed the bound");
            assert_eq!(
                fixture.json(&["run", "inspect", &ledger])["governor"]["state"],
                "blocked"
            );
            break;
        }
        assert!(permit.status.success(), "{permit:?}");
        let inspected = fixture.json(&["run", "inspect", &ledger]);
        assert_eq!(inspected["governor"]["spent"], unit);
        fs::write(
            fixture.root.join("product.txt"),
            format!("permitted completion edit {unit}\n"),
        )
        .unwrap();
        let resumed = fixture.json(&["work", "resume"]);
        assert_eq!(resumed["status"], "resumed");
    }

    // Once the fourth permit durably blocks the governor, the pending
    // completion cannot reopen or append another mutation.
    let artifacts_before = fixture.artifact_snapshot();
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker completion\n");
    assert!(
        !completed.status.success(),
        "blocked completion was accepted"
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 3);
    assert_eq!(inspected["governor"]["state"], "blocked");
    assert_eq!(fixture.artifact_snapshot(), artifacts_before);
    let stale = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
    assert!(!stale.status.success(), "stale assignment was accepted");
}

#[test]
fn explicit_grant_binds_source_and_result_across_restart() {
    let (fixture, _work, ledger) = granted_worker_completion();
    let first = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(first["governor"]["spent"], 1);
    assert_eq!(
        first["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let replay = fixture.call(&["run", "inspect", &ledger]);
    assert!(replay.status.success(), "{replay:?}");
    let second: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(second["governor"]["spent"], 1);
    assert_eq!(
        second["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn discriminated_completion_rejects_missing_and_tampered_authorization() {
    for variant in [
        "payload",
        "ack",
        "mode",
        "nested-input",
        "unknown",
        "duplicate",
    ] {
        let (fixture, _work, ledger) = granted_worker_completion();
        fixture.rewrite_last(&ledger, |event| {
            if variant == "payload" {
                event.as_object_mut().unwrap().remove("governorEvent");
                return;
            }
            let governor = event["governorEvent"].as_object_mut().unwrap();
            match variant {
                "ack" => {
                    governor.remove("grantEventSha256s");
                    governor.remove("sourceInputsSha256s");
                }
                "mode" => {
                    governor.remove("authorizationMode");
                }
                "nested-input" => {
                    governor.insert("inputSha256".into(), json!("0".repeat(64)));
                }
                "unknown" => {
                    governor["grantEventSha256s"] = json!(["0".repeat(64)]);
                }
                "duplicate" => {
                    let grant = governor["grantEventSha256s"][0].clone();
                    let source = governor["sourceInputsSha256s"][0].clone();
                    governor["grantEventSha256s"] = json!([grant.clone(), grant]);
                    governor["sourceInputsSha256s"] = json!([source.clone(), source]);
                }
                _ => unreachable!(),
            }
        });
        let refused = fixture.call(&["run", "inspect", &ledger]);
        assert!(!refused.status.success(), "tampered {variant} replayed");
    }
}

#[test]
fn nonmutation_evidence_then_implicit_completion_is_typed() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "implicit after evidence",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let artifact = ".exitbind/artifacts/explicit-evidence.md";
    fs::write(fixture.root.join(artifact), b"evidence\n").unwrap();
    let evidenced = fixture.call(&[
        "work",
        "evidence",
        &work,
        &assignment,
        "--artifact",
        artifact,
        "--artifact-root",
        "state",
    ]);
    assert!(evidenced.status.success(), "{evidenced:?}");
    let evidenced: Value = serde_json::from_slice(&evidenced.stdout).unwrap();
    assert_no_transport_paths(&evidenced);
    assert_resolver_backed_references(&evidenced);
    let next = fixture.json(&["work", "next", &work]);
    assert_no_transport_paths(&next);
    let packet_path = fixture.root.join(".exitbind/evidence-residual.json");
    fs::write(&packet_path, serde_json::to_vec(&next["residual"]).unwrap()).unwrap();
    let validated = fixture.json(&[
        "work",
        "validate",
        &work,
        "--packet",
        packet_path.to_str().unwrap(),
    ]);
    assert_eq!(validated["result"], "usable");
    assert_no_transport_paths(&validated);
    let completed = fixture.return_body(&work, &assignment, "completed", b"implicit result\n");
    assert!(completed.status.success(), "{completed:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let lines = fs::read_to_string(fixture.root.join(ledger)).unwrap();
    let event: Value = serde_json::from_str(lines.lines().last().unwrap()).unwrap();
    assert_eq!(event["governorEvent"]["authorizationMode"], "implicit");
    assert_eq!(event["governorEvent"]["grantEventSha256s"], json!([]));
}

#[test]
fn pre_discriminator_v8_marker_remains_readable() {
    let fixture = Fixture::new();
    let ledger = ".exitbind/runs/work-historical-v8.jsonl";
    let started = fixture.call(&[
        "run",
        "start",
        "change",
        "--goal",
        "historical v8 marker",
        "--ledger",
        ledger,
        "--check-command",
        "true",
    ]);
    assert!(started.status.success(), "{started:?}");
    let path = fixture.root.join(ledger);
    let mut event: Value =
        serde_json::from_str(fs::read_to_string(&path).unwrap().lines().next().unwrap()).unwrap();
    event["governor"]
        .as_object_mut()
        .unwrap()
        .remove("noInformationLimit");
    event["governor"]
        .as_object_mut()
        .unwrap()
        .remove("postReplanLimit");
    event["governor"]
        .as_object_mut()
        .unwrap()
        .remove("grantProtocol");
    event.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(serde_json::to_vec(&event).unwrap());
    let mut digest_hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut digest_hex, "{byte:02x}").unwrap();
    }
    event["eventSha256"] = json!(digest_hex);
    fs::write(
        path,
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();
    let lead_artifact = ".exitbind/artifacts/historical-lead.md";
    fs::write(fixture.root.join(lead_artifact), b"historical lead\n").unwrap();
    let scoped = fixture.call(&[
        "run",
        "submit",
        "lead",
        ledger,
        "--outcome",
        "scoped",
        "--artifact",
        lead_artifact,
        "--artifact-root",
        "state",
    ]);
    assert!(scoped.status.success(), "{scoped:?}");
    let next = fixture.json(&["run", "next", ledger]);
    assert_eq!(next["assignments"][0]["role"], "worker");
    let worker_artifact = ".exitbind/artifacts/historical-worker.md";
    fs::write(fixture.root.join(worker_artifact), b"historical worker\n").unwrap();
    let completed = fixture.call(&[
        "run",
        "submit",
        "worker",
        ledger,
        "--outcome",
        "completed",
        "--artifact",
        worker_artifact,
        "--artifact-root",
        "state",
    ]);
    assert!(completed.status.success(), "{completed:?}");
    let inspected = fixture.call(&["run", "inspect", ledger]);
    assert!(inspected.status.success(), "{inspected:?}");
}

#[test]
fn lead_permit_is_refused_without_a_ledger_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "lead permit is not a worker grant",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap();
    let assignment = started["next"]["assignment"].as_str().unwrap();
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    let refused = fixture.call(&["work", "permit", work, assignment, "--operation", "edit"]);
    assert!(!refused.status.success());
    assert_eq!(fs::read(&ledger).unwrap(), before);
}

#[test]
fn stale_assignment_and_unknown_outcome_refuse_before_artifact_or_ledger_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "reject invalid worker returns",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let assignment = fixture.json(&["work", "next", &work])["next"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut stale = assignment.clone();
    stale.push('x');
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    let artifacts = fixture.artifact_snapshot();
    let refused = fixture.return_body(&work, &stale, "completed", b"must not persist\n");
    assert!(!refused.status.success(), "stale assignment was accepted");
    assert_eq!(fs::read(&ledger).unwrap(), before);
    assert_eq!(fixture.artifact_snapshot(), artifacts);

    let refused = fixture.return_body(&work, &assignment, "unknown", b"must not persist\n");
    assert!(!refused.status.success(), "unknown outcome was accepted");
    assert_eq!(fs::read(&ledger).unwrap(), before);
    assert_eq!(fixture.artifact_snapshot(), artifacts);
}

#[test]
fn conservative_completion_holds_before_artifact_or_append() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "conservative completion keeps worker bytes without appending",
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
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    for unit in 1..=2 {
        assert!(fixture
            .call(&["work", "permit", &work, &assignment, "--operation", "edit"])
            .status
            .success());
        fs::write(
            fixture.root.join("product.txt"),
            format!("grant edit {unit}\n"),
        )
        .unwrap();
        assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    }
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    let artifacts_before = fixture.artifact_snapshot();
    let response = fixture.return_body(&work, &assignment, "completed", b"held worker result\n");
    assert!(response.status.success(), "{response:?}");
    let held = fixture.json(&["work", "next", &work]);
    assert!(held["next"]["held"]["reference"].is_string());
    assert_eq!(fs::read(&ledger).unwrap(), before);
    assert_eq!(fixture.artifact_snapshot(), artifacts_before);
}

#[test]
fn multiple_exact_grants_are_acknowledged_once_and_survive_replay() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "acknowledge exact grants",
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
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    for unit in 1..=2 {
        assert!(fixture
            .call(&["work", "permit", &work, &assignment, "--operation", "edit"])
            .status
            .success());
        fs::write(
            fixture.root.join("product.txt"),
            format!("grant edit {unit}\n"),
        )
        .unwrap();
        assert_eq!(fixture.json(&["work", "resume"])["status"], "resumed");
    }
    let replanned = fixture.call(&[
        "work",
        "replan",
        &work,
        &assignment,
        "--hypothesis",
        "same worker, new plan",
    ]);
    assert!(replanned.status.success(), "{replanned:?}");
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker\n");
    assert!(completed.status.success(), "{completed:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 2);
    assert_eq!(
        inspected["governor"]["consumedGrants"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let resumed = fixture.json(&["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    let replayed = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(
        replayed["governor"]["consumedGrants"],
        inspected["governor"]["consumedGrants"]
    );
}

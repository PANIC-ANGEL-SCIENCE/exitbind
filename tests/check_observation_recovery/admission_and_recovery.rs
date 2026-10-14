use super::*;

#[test]
fn ended_storage_failure_recovers_once_without_check_credit_or_accounting_reset() {
    let (f, work, d, prefix) = storage_interruption();
    let preview = f.ok(&["work", "recover-check", &work, "--json"], b"");
    assert_eq!(preview["ownerDecision"]["approved"], false);
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    assert!(preview["apply"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "--apply"));
    let before = f.next(&work)["packet"]["context"]["loop"].clone();
    let recovered = recover(&f, &work, &d);
    assert!(recovered.status.success(), "{recovered:?}");
    let result = value(&recovered);
    assert_eq!(result["checkRecorded"], false);
    assert!(result["result"].is_null());
    assert_eq!(result["provenance"], "configured_lead_reported");
    // Even the real exit0 above supplies no passing check after capture loss.
    assert_eq!(result["observationFailure"]["facts"]["process"]["code"], 0);
    let bytes = fs::read(f.ledger(&work)).unwrap();
    assert!(bytes.starts_with(&prefix));
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check_observation_recovered")
            .count(),
        1
    );
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    assert_eq!(f.next(&work)["packet"]["context"]["loop"], before);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
    let readback = f.emitted(&preview["readback"]["command"], b"");
    assert!(readback.status.success(), "{readback:?}");
    let readback: Value = serde_json::from_slice(&readback.stdout).unwrap();
    assert_eq!(readback["work"], work);
    let replay = recover_with_preview(&f, &preview, &d);
    assert!(replay.status.success(), "{replay:?}");
    assert_eq!(value(&replay)["effect"], "no-change");
    assert_eq!(value(&replay)["eventSha256"], result["eventSha256"]);
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), bytes);
    let mut changed = d.clone();
    changed["reason"] = json!("different request");
    assert!(!recover_with_preview(&f, &preview, &changed)
        .status
        .success());
    let worker = f.repair(&work);
    assert_eq!(worker["role"], "worker");
    assert!(!recover_with_preview(&f, &preview, &d).status.success());
    let permit = f.ok(
        &[
            "work",
            "permit",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--operation",
            "repair checker",
            "--request-id",
            "recovery-repair",
        ],
        b"",
    );
    assert_eq!(permit["allowed"], true);
    fs::write(f.root.join("checker.sh"), "exit 0\n").unwrap();
    f.give(&work, &worker, "completed");
    let check = f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert_eq!(check["result"]["code"], 0);
    let reviewer = f.next(&work);
    assert_eq!(reviewer["role"], "reviewer");
    f.give(&work, &reviewer, "approved");
    let lead = f.next(&work);
    f.give(&work, &lead, "accepted");
}

#[test]
fn recovery_refuses_unapproved_foreign_stale_unknown_and_changed_evidence() {
    let (f, work, d, prefix) = storage_interruption();
    let preview = f.ok(&["work", "recover-check", &work, "--json"], b"");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    let mut cases = Vec::new();
    for (path, value) in [
        ("/approved", json!(false)),
        ("/currentBinding", json!("b".repeat(64))),
        ("/agent", json!("worker")),
        ("/work", json!(format!("smw_{}", "a".repeat(64)))),
        ("/snapshot/admissionEventSha256", json!("a".repeat(64))),
        ("/snapshot/ledgerSourceSha256", json!("a".repeat(64))),
        ("/snapshot/timeoutMs", json!(1)),
        ("/snapshot/binding/subjectSha256", json!("a".repeat(64))),
        ("/snapshot/binding/inputsSha256", json!("a".repeat(64))),
        ("/snapshot/binding/configSha256", json!("a".repeat(64))),
        (
            "/snapshot/binding/checkCommandSha256",
            json!("a".repeat(64)),
        ),
        (
            "/snapshot/binding/observerExecutableSha256",
            json!("a".repeat(64)),
        ),
        ("/snapshot/binding/requirementId", json!("foreign")),
        ("/responseSha256", json!("a".repeat(64))),
    ] {
        let mut c = d.clone();
        *c.pointer_mut(path).unwrap() = value;
        cases.push(c);
    }
    for (path, value) in [
        ("/observation/termination", json!("unknown")),
        ("/observation/groupEnded", json!(false)),
        ("/observation/captureReadersEnded", json!(false)),
        ("/observation/cleanupErrorCount", json!(1)),
        ("/observation/remainingOwnedPathCount", json!(1)),
        ("/observation/partialCaptures", json!([])),
        ("/checkRecorded", json!(true)),
    ] {
        let mut c = d.clone();
        let mut r: Value = serde_json::from_str(c["response"].as_str().unwrap()).unwrap();
        *r.pointer_mut(path).unwrap() = value;
        c["response"] = json!(r.to_string());
        response_hash(&mut c);
        cases.push(c);
    }
    for c in cases {
        let refused = recover_with_preview(&f, &preview, &c);
        assert!(!refused.status.success(), "{c}: {refused:?}");
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    }
    let mut general = d.clone();
    general["currentBinding"] = f.ok(&["work", "detail", &work, "--json"], b"")["binding"].clone();
    assert!(!recover_with_preview(&f, &preview, &general)
        .status
        .success());
    let disagree = f.call(
        &[
            "work",
            "recover-check",
            &work,
            "--apply",
            "--current-binding",
            &"c".repeat(64),
            "--json",
        ],
        &serde_json::to_vec(&d).unwrap(),
    );
    assert!(!disagree.status.success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    let r: Value = serde_json::from_str(d["response"].as_str().unwrap()).unwrap();
    let partial = f.root.join(
        r["observation"]["partialCaptures"][0]["path"]
            .as_str()
            .unwrap(),
    );
    let saved = fs::read(&partial).unwrap();
    fs::write(&partial, b"changed").unwrap();
    assert!(!recover_with_preview(&f, &preview, &d).status.success());
    fs::write(partial, saved).unwrap();
    let checker = fs::read(f.root.join("checker.sh")).unwrap();
    fs::write(f.root.join("checker.sh"), b"exit 0 # changed input\n").unwrap();
    assert!(!recover_with_preview(&f, &preview, &d).status.success());
    fs::write(f.root.join("checker.sh"), checker).unwrap();
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
}

#[test]
fn canonical_recovery_rejects_rehashed_changed_binding_facts_and_prefix() {
    let (f, work, d, prefix) = storage_interruption();
    assert!(recover(&f, &work, &d).status.success());
    let original = f.events(&work).last().unwrap().clone();
    for (path, v) in [
        (
            "/observation/recovery/currentBinding",
            json!("b".repeat(64)),
        ),
        (
            "/observation/recovery/snapshot/binding/observerExecutableSha256",
            json!("b".repeat(64)),
        ),
        (
            "/observation/recovery/snapshot/admissionEventSha256",
            json!("b".repeat(64)),
        ),
        ("/observation/recovery/approved", json!(false)),
        ("/observation/recovery/agent", json!("worker")),
        ("/observation/facts/groupEnded", json!(false)),
    ] {
        let mut e = original.clone();
        *e.pointer_mut(path).unwrap() = v;
        rehash(&mut e);
        let mut bytes = prefix.clone();
        bytes.extend(serde_json::to_vec(&e).unwrap());
        bytes.push(b'\n');
        fs::write(f.ledger(&work), bytes).unwrap();
        assert!(
            !f.call(&["work", "next", &work], b"").status.success(),
            "accepted corrupt recovery {path}"
        );
    }
    // Shape-valid but wrong raw-prefix identity must fail the ledger owner too.
    let mut e = original;
    let c = &mut e["observation"]["recovery"];
    c["snapshot"]["ledgerSourceSha256"] = json!("b".repeat(64));
    use sha2::{Digest, Sha256};
    let operation=format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"identity":c["snapshot"]["binding"],"ledgerSourceSha256":c["snapshot"]["ledgerSourceSha256"]})).unwrap()));
    let mut r: Value = serde_json::from_str(c["response"].as_str().unwrap()).unwrap();
    for (i, p) in r["observation"]["partialCaptures"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        p["path"] = json!(format!(
            ".exitbind/artifacts/check-partial-{operation}-1-{i}.raw"
        ));
    }
    c["response"] = json!(r.to_string());
    response_hash(c);
    e["observation"]["facts"] = r["observation"].clone();
    rehash(&mut e);
    let mut bytes = prefix;
    bytes.extend(serde_json::to_vec(&e).unwrap());
    bytes.push(b'\n');
    fs::write(f.ledger(&work), bytes).unwrap();
    assert!(!f.call(&["work", "next", &work], b"").status.success());
}

#[test]
fn timeout_records_failure_then_lead_repair_recheck_required_review_and_acceptance() {
    let f = Fixture::new("printf before-timeout; exec sleep 3\n");
    let work = f.begin();
    let started = Instant::now();
    let failure = f.call(&["work", "check", &work, "--timeout-ms", "50"], b"");
    assert!(!failure.status.success(), "{failure:?}");
    // This includes debug executable hashing and concurrent fixture load.
    assert!(started.elapsed() < Duration::from_secs(10));
    let result = value(&failure);
    assert_eq!(result["checkRecorded"], false);
    let facts = &result["observationFailure"]["facts"];
    assert_eq!(facts["deadlineExceeded"], true);
    assert!(facts["durationMs"].as_u64().unwrap() < 1000);
    assert_eq!(facts["termination"], "ended");
    assert_eq!(facts["capture"], "incomplete");
    assert!(!facts["process"].is_null());
    assert_eq!(facts["partialCaptures"].as_array().unwrap().len(), 2);
    for partial in facts["partialCaptures"].as_array().unwrap() {
        use sha2::{Digest, Sha256};
        let bytes = fs::read(f.root.join(partial["path"].as_str().unwrap())).unwrap();
        assert_eq!(partial["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(partial["completeness"], "partial");
    }
    assert_eq!(f.next(&work)["action"], "lead_decision");
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    let repeated = f.call(&["work", "check", &work, "--timeout-ms", "50"], b"");
    assert!(!repeated.status.success());
    let worker = f.repair(&work);
    let permit = f.ok(
        &[
            "work",
            "permit",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--operation",
            "repair-checker",
        ],
        b"",
    );
    assert_eq!(permit["allowed"], true);
    let spent = f.next(&work)["packet"]["context"]["loop"]["spent"].clone();
    fs::write(f.root.join("checker.sh"), "printf repaired; exit 0\n").unwrap();
    f.give(&work, &worker, "completed");
    let check = f.ok(&["work", "check", &work], b"");
    assert_eq!(check["result"]["code"], 0);
    assert_eq!(check["next"]["role"], "reviewer");
    let review = f.next(&work);
    let lead = f.give(&work, &review, "approved")["next"].clone();
    f.give(&work, &lead, "accepted");
    assert_eq!(f.next(&work)["status"], "accepted");
    let events = f.events(&work);
    let recovery = events
        .iter()
        .find(|e| e["operation"] == "authorized_repair_recovery_v1")
        .unwrap();
    assert_eq!(recovery["governorEvent"]["checkpoint"], spent);
    // Rehashed but unauthorised/stale/mismatched proposals still fail canonical validation.
    let index = events.iter().position(|e| e == recovery).unwrap();
    for field in ["dispositionSha256", "assignmentPacketSha256", "inputSha256"] {
        let mut proposal = recovery.clone();
        proposal["governorEvent"][field] = json!("0".repeat(64));
        rehash(&mut proposal["governorEvent"]);
        rehash(&mut proposal);
        let mut prefix = events[..index].to_vec();
        prefix.push(proposal);
        let ledger = format!(".exitbind/runs/invalid-{field}.jsonl");
        fs::write(
            f.root.join(&ledger),
            prefix
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let refused = f.call(&["run", "inspect", &ledger, "--json"], b"");
        assert!(!refused.status.success(), "{field}: {refused:?}");
    }

    assert!(recovery["inputsSha256"] != events[0]["inputsSha256"]);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["action"] == "check_observation_failed")
            .count(),
        1
    );
    assert_eq!(events.iter().filter(|e| e["action"] == "check").count(), 1);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["role"] == "worker" && e["outcome"] == "completed")
            .count(),
        2
    );
}

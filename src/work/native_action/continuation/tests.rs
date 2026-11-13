// Tests native continuation retirement and active-owner facts.
// Held results earn no credit in retirement cases; see `src/work/native_action/continuation.rs`.
use super::*;

#[test]
fn retirement_requires_observed_end_and_never_credits_a_held_result() {
    let mut journal = json!({"status":"running"});
    assert!(blocks_retirement(&journal, true));
    journal["status"] = json!("started");
    assert!(
        blocks_retirement(&journal, true),
        "missing PID is not an end observation"
    );
    journal["observation"] = json!({"interrupted":false,
            "process":{"success":false,"code":1,"signal":null,"timedOut":false}});
    assert!(!blocks_retirement(&journal, true));
    journal["provisionalFinalResult"] = json!({"summary":"retained"});
    assert!(blocks_retirement(&journal, true));
    journal["status"] = json!("completed");
    assert!(blocks_retirement(&journal, true));
    assert!(!blocks_retirement(&journal, false));
    journal["status"] = json!("not_started");
    journal.as_object_mut().unwrap().remove("observation");
    assert!(blocks_retirement(&journal, true));
    journal["controlledInterruption"] = json!({"spawned":false});
    assert!(!blocks_retirement(&journal, true));
}

fn pending() -> Value {
    json!({"stage":2,"attempt":1,"agent":"worker","role":"worker"})
}

fn journal(packet: &Value) -> Value {
    json!({"version":1,"work":"fixture-work","status":"running",
            "assignment":crate::run::assignment::handle("fixture-work",packet).unwrap(),
            "role":"worker","agent":"worker","stage":2,"attempt":1,
            "operationId":"native-fixture-operation",
            "request":{"executable":"/fixture/native","persistSession":true},
            "continuationOperation":"untrusted-until-account-verified"})
}

fn empty() -> NativeControlFacts {
    NativeControlFacts {
        all_ended: true,
        retirement_blocked: false,
        active: Vec::new(),
        saved: Vec::new(),
    }
}

fn observe(
    journal: &Value,
    pending: &[Value],
    liveness: ProcessLiveness,
) -> Result<NativeControlFacts, String> {
    let mut facts = empty();
    collect(
        journal,
        journal["assignment"].as_str().unwrap(),
        "fixture-work",
        pending,
        liveness,
        &mut facts,
    )?;
    Ok(facts)
}

#[test]
fn active_facts_require_current_pending_owner_and_observed_alive_identity() {
    let packet = pending();
    let journal = journal(&packet);
    let active = observe(&journal, &[packet.clone()], ProcessLiveness::Alive).unwrap();
    assert!(!active.all_ended);
    assert_eq!(active.active.len(), 1);
    assert_eq!(active.active[0].key, "untrusted-until-account-verified");
    assert_eq!(
        active.active[0].request_sha256,
        hash::value(&journal["request"])
    );
    assert_eq!(active.active[0].work, "fixture-work");
    for field in ["stage", "attempt", "agent", "role"] {
        let mut other = packet.clone();
        other[field] = match field {
            "stage" | "attempt" => json!(3),
            _ => json!("another"),
        };
        let stale = observe(&journal, &[other], ProcessLiveness::Alive).unwrap();
        assert!(!stale.all_ended);
        assert!(stale.active.is_empty(), "{field}");
    }
    let uncertain = observe(&journal, &[packet.clone()], ProcessLiveness::Uncertain).unwrap();
    assert!(!uncertain.all_ended);
    assert!(uncertain.active.is_empty());
    let ended = observe(&journal, &[packet], ProcessLiveness::Ended).unwrap();
    assert!(ended.all_ended);
    assert!(ended.active.is_empty());
}

fn completed(packet: &Value) -> (Value, Vec<u8>) {
    let mut journal = journal(packet);
    let bytes =
        serde_json::to_vec(&json!({"outcome":"completed","summary":"bounded fixture","reason":""}))
            .unwrap();
    journal["status"] = json!("completed");
    journal["result"] = json!(hex_encode(&bytes));
    journal["observation"] = json!({"process":{"success":true,"code":0,"signal":null,"timedOut":false},
            "turn":"completed","interrupted":false,"coverageGaps":[]});
    (journal, bytes)
}

#[test]
fn saved_facts_bind_exact_bytes_outcome_and_current_assignment_without_reexecution() {
    let packet = pending();
    let (journal, bytes) = completed(&packet);
    let facts = observe(&journal, &[packet.clone()], ProcessLiveness::Ended).unwrap();
    assert!(facts.all_ended);
    assert!(facts.active.is_empty());
    assert_eq!(facts.saved.len(), 1);
    let saved = &facts.saved[0];
    assert_eq!(saved.result_sha256, hash::bytes(&bytes));
    assert_eq!(saved.request_sha256, hash::value(&journal["request"]));
    assert_eq!(saved.outcome, "completed");
    assert_eq!(saved.agent, "worker");
    assert_eq!(saved.assignment, journal["assignment"].as_str().unwrap());
    let mut next = packet;
    next["attempt"] = json!(2);
    assert!(observe(&journal, &[next], ProcessLiveness::Ended)
        .unwrap()
        .saved
        .is_empty());
}

#[test]
fn saved_debit_recovery_requires_exact_completed_result_and_known_group_end() {
    let (journal, _) = completed(&pending());
    let proof = saved_recovery_proof(&journal, "fixture-work", ProcessLiveness::Ended)
        .unwrap()
        .unwrap();
    assert_eq!(proof.0, "untrusted-until-account-verified");
    assert_eq!(proof.1, journal["request"]);
    for liveness in [ProcessLiveness::Alive, ProcessLiveness::Uncertain] {
        assert!(saved_recovery_proof(&journal, "fixture-work", liveness)
            .unwrap()
            .is_none());
    }
    for (field, value) in [
        ("version", json!(2)),
        ("work", json!("other-work")),
        ("status", json!("running")),
        ("operationId", Value::Null),
        ("role", json!("lead")),
        ("request", Value::Null),
        ("result", json!("not-hex")),
        ("observation", Value::Null),
    ] {
        let mut invalid = journal.clone();
        invalid[field] = value;
        assert!(
            saved_recovery_proof(&invalid, "fixture-work", ProcessLiveness::Ended).is_err(),
            "{field}"
        );
    }
    let mut ordinary = journal;
    ordinary
        .as_object_mut()
        .unwrap()
        .remove("continuationOperation");
    assert!(
        saved_recovery_proof(&ordinary, "fixture-work", ProcessLiveness::Uncertain)
            .unwrap()
            .is_none()
    );
}

#[test]
fn live_agent_a_does_not_create_native_facts_for_pending_agent_b_of_same_role() {
    let packet_a = pending();
    let mut packet_b = packet_a.clone();
    packet_b["agent"] = json!("worker-b");
    let journal_a = journal(&packet_a);
    let facts = observe(
        &journal_a,
        &[packet_a.clone(), packet_b.clone()],
        ProcessLiveness::Alive,
    )
    .unwrap();
    assert_eq!(facts.active.len(), 1);
    assert_eq!(facts.active[0].agent, "worker");
    assert_eq!(facts.active[0].role, "worker");
    assert_eq!(
        facts.active[0].assignment,
        crate::run::assignment::handle("fixture-work", &packet_a).unwrap()
    );
    assert_ne!(
        facts.active[0].assignment,
        crate::run::assignment::handle("fixture-work", &packet_b).unwrap()
    );
    let only_b = observe(&journal_a, &[packet_b], ProcessLiveness::Alive).unwrap();
    assert!(only_b.active.is_empty());
    assert!(!only_b.all_ended);
}

#[test]
fn aggregate_failure_preserves_exact_owner_result_and_pending_recovery() {
    let directory = crate::test_root::temp("native-account-fault");
    let path = directory.join("native.json");
    let (journal, bytes) = completed(&pending());
    write_journal(&path, &journal, true).unwrap();
    let error = observe_account(&path, "actual-reservation", true, true, || {
        let retained = read_journal(&path).unwrap().unwrap();
        assert_eq!(retained["status"], "completed");
        assert_eq!(journal_result(&retained).unwrap(), bytes);
        assert_eq!(retained["observation"], journal["observation"]);
        assert_eq!(retained["accountingRecovery"]["status"], "pending");
        Err("injected aggregate append failure".into())
    })
    .unwrap_err();
    assert_eq!(error, "injected aggregate append failure");
    let retained = read_journal(&path).unwrap().unwrap();
    assert_eq!(journal_result(&retained).unwrap(), bytes);
    assert_eq!(retained["observation"], journal["observation"]);
    assert_eq!(retained["accountingRecovery"]["status"], "pending");
    assert_eq!(retained["accountingRecovery"]["knownEnded"], true);

    observe_account(&path, "actual-reservation", true, true, || Ok(())).unwrap();
    assert_eq!(
        read_journal(&path).unwrap().unwrap()["accountingRecovery"]["status"],
        "observed"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn aggregate_failure_keeps_owned_interruption_and_unknown_end() {
    let directory = crate::test_root::temp("native-interrupt-fault");
    let path = directory.join("native.json");
    let mut journal = journal(&pending());
    journal["error"] = json!("controlled eligibility hold");
    journal["controlledInterruption"] = json!({"version":1,"rootReaped":true,
            "wholeGroupEnded":null,"ownedRemainingLiveness":"uncertain"});
    write_journal(&path, &journal, true).unwrap();
    assert!(
        observe_account(&path, "actual-reservation", false, false, || {
            Err("injected aggregate append failure".into())
        })
        .is_err()
    );
    let retained = read_journal(&path).unwrap().unwrap();
    assert_eq!(retained["error"], journal["error"]);
    assert_eq!(
        retained["controlledInterruption"],
        journal["controlledInterruption"]
    );
    assert_eq!(retained["accountingRecovery"]["status"], "pending");
    assert_eq!(retained["accountingRecovery"]["knownEnded"], false);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_or_unverified_completed_journals_are_holds() {
    let packet = pending();
    let (journal, _) = completed(&packet);
    for (field, value) in [
        ("version", json!(2)),
        ("work", json!("other-work")),
        ("status", json!("assumed_done")),
        ("request", Value::Null),
        ("result", json!("not-hex")),
    ] {
        let mut invalid = journal.clone();
        invalid[field] = value;
        assert!(
            observe(&invalid, &[packet.clone()], ProcessLiveness::Ended).is_err(),
            "{field}"
        );
    }
    let mut failed = journal;
    failed["observation"]["process"]["success"] = json!(false);
    assert!(observe(&failed, &[packet], ProcessLiveness::Ended).is_err());
}

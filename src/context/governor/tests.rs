use super::*;

fn mutation(previous: Option<&Value>, n: u64, evidence: Option<&str>) -> Value {
    let exact_evidence = evidence.map(|label| {
        json!({
            "root": "state",
            "path": format!("evidence/{label}"),
            "sha256": format!("{:064x}", n),
            "subjectSha256": "subject",
            "attempt": 1,
            "inputSha256": "input",
        })
    });
    event(
        previous,
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":n, "inputSha256":"input",
            "lineageSha256":"lineage", "unit":"unit", "operation":"replan",
            "newEvidenceSha256":evidence,
            "newEvidence":exact_evidence,
            "timestamp":n, "comment":format!("wrapper-{n}"),
        }),
    )
}

#[test]
fn equivalent_replans_and_wrappers_consume_one_shared_budget() {
    let first = mutation(None, 1, None);
    let second = mutation(Some(&first), 2, None);
    let state = reduce_governor(&[first.clone(), second.clone()]).unwrap();
    assert_eq!(state["spent"], 2);
    assert_eq!(state["state"], "replan_required");
    let replan = event(
        Some(&second),
        json!({
            "action":"replan", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "inputSha256":"input", "hypothesis":"new hypothesis"
        }),
    );
    let third = mutation(Some(&replan), 3, None);
    let blocked = event(
        Some(&third),
        json!({
            "action":"blocked", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "inputSha256":"input", "reason":"evidence required"
        }),
    );
    let bounded = reduce_governor(&[first, second, replan, third, blocked]).unwrap();
    assert_eq!(bounded["spent"], 3);
    assert_eq!(bounded["state"], "blocked");
    let refused = checkpoint(&bounded, &json!({"operation":"replan"}));
    assert_eq!(refused["allowed"], false);
}

#[test]
fn seeded_carry_preserves_phase_and_counters_without_old_authority() {
    let seed = json!({
        "spent": 2,
        "noInformationStreak": 0,
        "postReplanSpent": 0,
        "replanCount": 1,
        "afterReplan": true,
        "state": "ready",
    });
    let carried = reduce_governor_seeded(&[], Some(&seed)).unwrap();
    assert_eq!(carried["spent"], 2);
    assert_eq!(carried["replanCount"], 1);
    assert_eq!(carried["afterReplan"], true);
    assert_eq!(carried["state"], "ready");
    assert!(carried["currentMutation"].is_null());
    assert!(carried["currentGrantEventSha256"].is_null());
    assert!(carried["currentReplan"].is_null());
    assert!(carried["currentSensorRequest"].is_null());
    assert!(carried["seenMutations"].as_array().unwrap().is_empty());
    assert!(carried["consumedGrants"].as_array().unwrap().is_empty());

    let fresh = mutation(None, 3, None);
    let spent = reduce_governor_seeded(&[fresh], Some(&seed)).unwrap();
    assert_eq!(spent["spent"], 3);
    assert_eq!(spent["postReplanSpent"], 1);
    assert_eq!(spent["state"], "evidence_required");
}

#[test]
fn seeded_carry_preserves_only_valid_noncontent_observation_keys() {
    let observation_keys = json!(["a".repeat(64)]);
    let seed = json!({
        "spent": 2,
        "noInformationStreak": 1,
        "postReplanSpent": 0,
        "replanCount": 1,
        "afterReplan": true,
        "state": "ready",
        "observationKeys": observation_keys,
    });
    let carried = reduce_governor_seeded(&[], Some(&seed)).unwrap();
    assert_eq!(carried["observations"], json!([{"key": "a".repeat(64)}]));

    for invalid in [
        json!(["A".repeat(64)]),
        json!([{"key": "a".repeat(64), "stdout": "must not be carried"}]),
        json!(["a".repeat(64), "a".repeat(64)]),
    ] {
        let mut invalid_seed = seed.clone();
        invalid_seed["observationKeys"] = invalid;
        assert!(reduce_governor_seeded(&[], Some(&invalid_seed)).is_err());
    }
}

#[test]
fn seeded_carry_never_reopens_a_blocked_phase() {
    let seed = json!({
        "spent": 3,
        "noInformationStreak": 2,
        "postReplanSpent": 1,
        "replanCount": 1,
        "afterReplan": true,
        "state": "blocked",
    });
    let carried = reduce_governor_seeded(&[], Some(&seed)).unwrap();
    assert_eq!(carried["state"], "blocked");
    let fresh = mutation(None, 4, None);
    assert!(reduce_governor_seeded(&[fresh], Some(&seed)).is_err());
}

#[test]
fn seeded_duplicate_evidence_cannot_reset_post_replan_streak() {
    let repeated = format!("{:064x}", 0);
    let seed = json!({
        "spent": 1,
        "noInformationStreak": 0,
        "postReplanSpent": 0,
        "replanCount": 1,
        "afterReplan": true,
        "state": "ready",
        "seenEvidenceSha256": [repeated],
    });
    let state =
        reduce_governor_seeded(&[mutation(None, 0, Some("repeated"))], Some(&seed)).unwrap();
    assert_eq!(state["spent"], 2);
    assert_eq!(state["noInformationStreak"], 0);
    assert_eq!(state["postReplanSpent"], 1);
    assert_eq!(state["state"], "evidence_required");
}

#[test]
fn seeded_duplicate_evidence_cannot_reset_pre_replan_streak() {
    let repeated = format!("{:064x}", 0);
    let seed = json!({
        "spent": 1,
        "noInformationStreak": 1,
        "postReplanSpent": 0,
        "replanCount": 0,
        "afterReplan": false,
        "state": "ready",
        "seenEvidenceSha256": [repeated],
    });
    let state =
        reduce_governor_seeded(&[mutation(None, 0, Some("repeated"))], Some(&seed)).unwrap();
    assert_eq!(state["spent"], 2);
    assert_eq!(state["noInformationStreak"], 2);
    assert_eq!(state["state"], "replan_required");
}

#[test]
fn replan_follows_a_carried_lineage_subject_transition() {
    let first = mutation(None, 1, None);
    let second = event(
        Some(&first),
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"next-subject",
            "attempt":2, "checkpoint":2, "inputSha256":"input",
            "lineageSha256":"lineage", "carryLineage":true, "unit":"unit",
            "operation":"completed_submission", "newEvidenceSha256":null,
        }),
    );
    let replan = event(
        Some(&second),
        json!({
            "action":"replan", "runId":"run", "subjectSha256":"next-subject",
            "attempt":2, "inputSha256":"input", "hypothesis":"repair the fresh attempt"
        }),
    );
    let state = reduce_governor(&[first, second, replan]).unwrap();
    assert_eq!(state["state"], "ready");
    assert_eq!(state["attempt"], 2);
    assert_eq!(state["subjectSha256"], "next-subject");
}

#[test]
fn marked_replan_requires_a_carried_current_mutation() {
    let first = mutation(None, 1, None);
    let second = mutation(Some(&first), 2, None);
    let replan = event(
        Some(&second),
        json!({
            "action":"replan", "identityTransition":"carried_mutation_v1",
            "runId":"run", "subjectSha256":"subject", "attempt":1,
            "inputSha256":"input", "hypothesis":"ordinary mutation replay"
        }),
    );
    let error = reduce_governor(&[first, second, replan]).unwrap_err();
    assert!(error.contains("carry lineage"), "{error}");
}

#[test]
fn markerless_replan_preserves_legacy_identity_transition() {
    let first = mutation(None, 1, None);
    let second = mutation(Some(&first), 2, None);
    let replan = event(
        Some(&second),
        json!({
            "action":"replan", "runId":"run",
            "subjectSha256":"new-subject", "attempt":2,
            "inputSha256":"input", "hypothesis":"legacy identity bypass"
        }),
    );
    let state = reduce_governor(&[first, second, replan]).unwrap();
    assert_eq!(state["subjectSha256"], "new-subject");
    assert_eq!(state["attempt"], 2);
}

#[test]
fn identity_changing_replan_requires_the_current_carried_mutation() {
    let mut state = json!({
        "runId": "run",
        "subjectSha256": "old-subject",
        "attempt": 2,
        "state": "replan_required",
        "currentMutation": null,
        "currentReplan": null,
        "replanCount": 0,
        "spent": 2,
        "noInformationStreak": 2,
        "postReplanSpent": 0,
        "afterReplan": false,
    });
    let event = event(
        None,
        json!({
            "action":"replan", "identityTransition":"carried_mutation_v1",
            "runId":"run", "subjectSha256":"new-subject", "attempt":3,
            "inputSha256":"input", "hypothesis":"forged transition"
        }),
    );
    assert!(apply_replan(&mut state, &event).is_err());
}

#[test]
fn genuine_evidence_is_recoverable_but_cannot_extend_budget() {
    let first = mutation(None, 1, Some("evidence-a"));
    let second = mutation(Some(&first), 2, Some("evidence-b"));
    let third = mutation(Some(&second), 3, Some("evidence-c"));
    let state = reduce_governor(&[first, second, third]).unwrap();
    assert_eq!(state["evidence"].as_array().unwrap().len(), 3);
    assert_eq!(state["spent"], 3);
    assert_eq!(state["budget"], HARD_ITERATION_BUDGET);
}

#[test]
fn stale_tampered_replay_and_sensor_duplicates_fail_closed() {
    let first = mutation(None, 1, None);
    let mut tampered = first.clone();
    tampered["comment"] = json!("tampered");
    assert!(reduce_governor(&[tampered]).is_err());

    let current = reduce_governor(std::slice::from_ref(&first)).unwrap();
    let request = event(Some(&first), sensor_request(&current).unwrap());
    let sensor = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
            "checkpoint":1, "inputSha256":"input", "inputDigest":"d",
            "sensorVersion":1,
            "requestDigest":request["requestDigest"], "identitySource":"host-reported",
            "assessment":"block", "confidence":0.95,
        }),
    );
    let duplicate = event(
        Some(&sensor),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
            "checkpoint":1, "inputSha256":"input", "inputDigest":"d",
            "sensorVersion":1,
            "requestDigest":request["requestDigest"], "identitySource":"host-reported",
            "assessment":"block", "confidence":0.95,
        }),
    );
    let state = reduce_governor(&[first.clone(), request, sensor, duplicate]).unwrap();
    assert_eq!(state["state"], "blocked");
    assert_eq!(state["seenSensors"].as_array().unwrap().len(), 1);

    let stale = mutation(Some(&first), 2, None);
    let mut stale = stale;
    stale["subjectSha256"] = json!("other-subject");
    stale["eventSha256"] = json!(hash::value(&{
        let mut without = stale.clone();
        without.as_object_mut().unwrap().remove("eventSha256");
        without
    }));
    assert!(reduce_governor(&[first, stale]).is_err());
}

#[test]
fn deepseek_shaped_worker_and_cooperative_host_fixture_share_the_bound() {
    let mut previous = None;
    let mut events = Vec::new();
    for checkpoint in 1..=2 {
        let event = event(
            previous.as_ref(),
            json!({
                "action":"mutation", "runId":"run", "subjectSha256":"subject",
                "attempt":1, "checkpoint":checkpoint, "inputSha256":"input",
                "lineageSha256":"lineage", "unit":"worker-mutation",
                "operation":"replan", "runtime":{"model":"deepseek"},
                "comment":format!("host wrapper {checkpoint}"), "newEvidenceSha256":null,
            }),
        );
        previous = Some(event.clone());
        events.push(event);
    }
    let replan = event(
        events.last(),
        json!({
            "action":"replan", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "inputSha256":"input", "hypothesis":"deepseek hypothesis"
        }),
    );
    let third = event(
        Some(&replan),
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":3, "inputSha256":"input",
            "lineageSha256":"lineage", "unit":"worker-mutation",
            "operation":"replan", "runtime":{"model":"deepseek"},
            "comment":"host wrapper 3", "newEvidenceSha256":null,
        }),
    );
    let blocked = event(
        Some(&third),
        json!({
            "action":"blocked", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "inputSha256":"input", "reason":"evidence required"
        }),
    );
    events.extend([replan, third, blocked]);
    let state = reduce_governor(&events).unwrap();
    assert_eq!(state["state"], "blocked");
    let decision = checkpoint(&state, &json!({"completedSubmission":false}));
    assert_eq!(decision["allowed"], false);
    assert_eq!(decision["reason"], "bounded_iteration_refused");
}

#[test]
fn sensor_must_match_the_exact_current_mutation() {
    let input_b = "b".repeat(64);
    let input_d = "d".repeat(64);
    let no_current_sensor = event(
        None,
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":1, "inputSha256":"d".repeat(64),
            "inputDigest":"d".repeat(64), "sensorVersion":1,
            "requestDigest":"request", "identitySource":"host-reported",
            "assessment":"low_information", "confidence":0.95,
        }),
    );
    assert!(reduce_governor(&[no_current_sensor]).is_err());
    let mutation = event(
        None,
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":2, "inputSha256":input_b,
            "lineageSha256":"lineage", "unit":"worker-mutation",
            "operation":"replan", "newEvidenceSha256":null,
        }),
    );
    let current = reduce_governor(std::slice::from_ref(&mutation)).unwrap();
    let request = event(Some(&mutation), sensor_request(&current).unwrap());
    let stale_sensor = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":1, "inputSha256":input_d.clone(),
            "inputDigest":input_d, "sensorVersion":1,
            "requestDigest":request["requestDigest"], "identitySource":"host-reported",
            "assessment":"low_information", "confidence":0.95,
        }),
    );
    assert!(reduce_governor(&[mutation.clone(), request.clone(), stale_sensor]).is_err());
    let current = reduce_governor(&[mutation.clone()]).unwrap();
    assert_eq!(current["state"], "ready");
    assert!(current["seenSensors"].as_array().unwrap().is_empty());

    let valid_sensor = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":2, "inputSha256":"b".repeat(64),
            "inputDigest":"d".repeat(64), "sensorVersion":1,
            "requestDigest":request["requestDigest"], "identitySource":"host-reported",
            "assessment":"block", "confidence":0.95,
        }),
    );
    let blocked = reduce_governor(&[mutation, request, valid_sensor]).unwrap();
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["seenSensors"].as_array().unwrap().len(), 1);
}

#[test]
fn current_worker_grant_survives_only_sensor_observation_without_authority_change() {
    let grant = event(
        None,
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":1, "inputSha256":"input",
            "lineageSha256":"lineage", "unit":"worker-mutation",
            "operation":"edit", "newEvidenceSha256":null,
        }),
    );
    let grant_state = reduce_governor(std::slice::from_ref(&grant)).unwrap();
    let request = event(Some(&grant), sensor_request(&grant_state).unwrap());
    let requested = reduce_governor(&[grant.clone(), request.clone()]).unwrap();
    assert_eq!(requested["currentGrantEventSha256"], grant["eventSha256"]);
    let inert = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":1, "inputSha256":"input", "inputDigest":"digest",
            "sensorVersion":1, "requestDigest":request["requestDigest"],
            "identitySource":"host-reported", "assessment":"uncertain", "confidence":0.95,
        }),
    );
    let observed = reduce_governor(&[grant.clone(), request.clone(), inert.clone()]).unwrap();
    assert_eq!(observed["currentGrantEventSha256"], grant["eventSha256"]);
    assert_eq!(observed["headSha256"], inert["eventSha256"]);
    assert_eq!(observed["state"], "ready");

    let conservative = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":1, "inputSha256":"input", "inputDigest":"digest",
            "sensorVersion":1, "requestDigest":request["requestDigest"],
            "identitySource":"host-reported", "assessment":"evidence", "confidence":0.95,
        }),
    );
    let stopped = reduce_governor(&[grant.clone(), request, conservative]).unwrap();
    assert!(stopped["currentGrantEventSha256"].is_null());
    assert_eq!(stopped["state"], "evidence_required");

    let second = event(
        Some(&grant),
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":2, "inputSha256":"input",
            "lineageSha256":"lineage", "unit":"worker-mutation",
            "operation":"edit", "newEvidenceSha256":null,
        }),
    );
    let replan = event(
        Some(&second),
        json!({
            "action":"replan", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "inputSha256":"input", "hypothesis":"revise the approach"
        }),
    );
    let replanned = reduce_governor(&[grant.clone(), second, replan]).unwrap();
    assert!(replanned["currentGrantEventSha256"].is_null());

    let held = event(
        Some(&grant),
        json!({
            "action":"mutation", "runId":"run", "subjectSha256":"subject",
            "attempt":1, "checkpoint":1, "inputSha256":"input",
            "lineageSha256":"lineage", "unit":"worker",
            "operation":"completed_submission", "newEvidenceSha256":null,
        }),
    );
    let completed = reduce_governor(&[grant, held]).unwrap();
    assert!(completed["currentGrantEventSha256"].is_null());
    assert_eq!(completed["currentMutation"]["unit"], "worker");
}

#[test]
fn absent_sensor_and_optimistic_output_do_not_unlock_or_retry() {
    let mutation_event = mutation(None, 1, None);
    let current = reduce_governor(std::slice::from_ref(&mutation_event)).unwrap();
    let request = event(Some(&mutation_event), sensor_request(&current).unwrap());
    let unavailable = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
            "checkpoint":1, "inputSha256":"input", "inputDigest":"d",
            "sensorVersion":1,
            "requestDigest":request["requestDigest"], "identitySource":"host-reported",
            "assessment":"unavailable", "confidence":null,
        }),
    );
    let state = reduce_governor(&[mutation_event, request, unavailable]).unwrap();
    assert_eq!(state["state"], "ready");
    assert_eq!(state["spent"], 1);

    let mutation_event = mutation(None, 1, None);
    let current = reduce_governor(std::slice::from_ref(&mutation_event)).unwrap();
    let request = event(Some(&mutation_event), sensor_request(&current).unwrap());
    let optimistic = event(
        Some(&request),
        json!({
            "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
            "checkpoint":1, "inputSha256":"input", "inputDigest":"e",
            "sensorVersion":1,
            "requestDigest":request["requestDigest"], "identitySource":"host-reported",
            "assessment":"ready", "confidence":1.0,
        }),
    );
    let state = reduce_governor(&[mutation_event, request, optimistic]).unwrap();
    assert_eq!(state["state"], "ready");
    assert_eq!(state["spent"], 1);
}

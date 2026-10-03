//! Numeric admission and projection counterexamples.

use super::*;
use crate::session_goal::usage::numeric_event;
use std::collections::BTreeSet;

fn event() -> Value {
    json!({
        "id": "host-1",
        "source": "host_reported",
        "scope": "direct",
        "phase": "closeout",
        "status": "observed",
        "semantics": "delta",
        "lifetime": "invocation",
        "goalId": "goal-1",
        "taskId": "task-1",
        "role": "lead",
        "assignment": "assignment-1",
        "attempt": 2,
        "invocation": "invocation-1",
        "turn": "turn-1",
        "sessionMode": "persistent",
        "adapterVersion": "codex-1",
        "values": {"inputTokens": 0, "cachedInputTokens": 0, "outputTokens": 3},
    })
}

#[test]
fn cumulative_decrease_requires_explicit_reset() {
    let mut first = event();
    first["id"] = json!("counter-1");
    first["semantics"] = json!("cumulative");
    first["counterId"] = json!("provider-counter");
    first["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
    let first = normalize_event("smw_123", &first).unwrap();
    let mut second = first.clone();
    second["id"] = json!("counter-2");
    second["values"]["inputTokens"] = json!(3);
    assert!(validate_monotonic(&[first], &second).is_err());
    second["reset"] = json!(true);
    let prior = normalize_event("smw_123", &event()).unwrap();
    let mut prior = prior;
    prior["semantics"] = json!("cumulative");
    prior["counterId"] = json!("provider-counter");
    prior["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
    assert!(validate_monotonic(&[prior], &second).is_ok());
}

#[test]
fn parent_and_native_snapshots_never_become_additive_values() {
    let mut parent = event();
    parent["id"] = json!("parent");
    let mut child = event();
    child["id"] = json!("child");
    child["parentEventId"] = json!("parent");
    let mut ids = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut cumulative = std::collections::BTreeMap::new();
    let mut representations = std::collections::BTreeMap::new();
    let (_, parent_values) = numeric_event(
        &normalize_event("smw_123", &parent).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut cumulative,
        &mut representations,
    )
    .unwrap();
    let (_, child_values) = numeric_event(
        &normalize_event("smw_123", &child).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut cumulative,
        &mut representations,
    )
    .unwrap();
    assert!(parent_values.is_some());
    assert!(child_values.is_none());

    let mut native = event();
    native["id"] = json!("native");
    native["source"] = json!("native_observation");
    let (_, native_values) = numeric_event(
        &normalize_event("smw_123", &native).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut cumulative,
        &mut representations,
    )
    .unwrap();
    assert!(native_values.is_none());
}

#[test]
fn failed_execution_with_known_delta_remains_additive() {
    let mut failed = event();
    failed["id"] = json!("failed");
    failed["status"] = json!("failed");
    let mut ids = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut cumulative = std::collections::BTreeMap::new();
    let mut representations = std::collections::BTreeMap::new();
    let (_, values) = numeric_event(
        &normalize_event("smw_123", &failed).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut cumulative,
        &mut representations,
    )
    .unwrap();
    assert_eq!(values, Some([0, 0, 3]));

    let mut missing = failed;
    missing["id"] = json!("failed-missing");
    missing["values"] = Value::Null;
    let (_, values) = numeric_event(
        &normalize_event("smw_123", &missing).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut cumulative,
        &mut representations,
    )
    .unwrap();
    assert!(values.is_none());
}

#[test]
fn cumulative_and_per_turn_contracts_derive_only_known_deltas() {
    let mut ids = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut counters = std::collections::BTreeMap::new();
    let mut representations = std::collections::BTreeMap::new();

    let mut cumulative = event();
    cumulative["semantics"] = json!("cumulative");
    cumulative["lifetime"] = json!("session");
    cumulative["sessionId"] = json!("session-1");
    cumulative["counterId"] = json!("counter-1");
    cumulative["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
    let first = numeric_event(
        &normalize_event("smw_123", &cumulative).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut counters,
        &mut representations,
    )
    .unwrap();
    assert_eq!(first.1, None);

    cumulative["id"] = json!("counter-2");
    cumulative["values"] = json!({"inputTokens": 12, "cachedInputTokens": 3, "outputTokens": 5});
    let second = numeric_event(
        &normalize_event("smw_123", &cumulative).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut counters,
        &mut representations,
    )
    .unwrap();
    assert_eq!(second.1, Some([2, 1, 1]));

    let mut per_turn = event();
    per_turn["id"] = json!("turn-1");
    per_turn["semantics"] = json!("per_turn");
    per_turn["lifetime"] = json!("session");
    per_turn["sessionId"] = json!("session-2");
    per_turn["counterId"] = json!("turn-counter");
    let (_, values) = numeric_event(
        &normalize_event("smw_123", &per_turn).unwrap(),
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut counters,
        &mut representations,
    )
    .unwrap();
    assert_eq!(values, Some([0, 0, 3]));
}

#[test]
fn mixed_counter_semantics_are_refused_for_one_stream() {
    let mut cumulative = event();
    cumulative["id"] = json!("cumulative-100");
    cumulative["semantics"] = json!("cumulative");
    cumulative["lifetime"] = json!("session");
    cumulative["sessionId"] = json!("session-1");
    cumulative["counterId"] = json!("counter-1");
    cumulative["values"] = json!({"inputTokens": 100, "cachedInputTokens": 20, "outputTokens": 10});
    let cumulative = normalize_event("smw_123", &cumulative).unwrap();
    let mut per_turn = cumulative.clone();
    per_turn["id"] = json!("per-turn-10");
    per_turn["semantics"] = json!("per_turn");
    per_turn["turn"] = json!("turn-1");
    per_turn["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 1});
    assert!(validate_monotonic(&[cumulative.clone()], &per_turn).is_err());
    let mut delta = cumulative.clone();
    delta["id"] = json!("delta-10");
    delta["semantics"] = json!("delta");
    assert!(validate_monotonic(&[cumulative.clone()], &delta).is_err());
    delta["counterId"] = json!("counter-2");
    assert!(validate_monotonic(&[cumulative.clone()], &delta).is_ok());

    let mut ids = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut counters = std::collections::BTreeMap::new();
    let mut representations = std::collections::BTreeMap::new();
    numeric_event(
        &cumulative,
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut counters,
        &mut representations,
    )
    .unwrap();
    assert_eq!(
        numeric_event(
            &per_turn,
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut counters,
            &mut representations,
        )
        .unwrap_err(),
        "mixed_counter_semantics"
    );
    let mut delta = cumulative.clone();
    delta["id"] = json!("delta-10");
    delta["semantics"] = json!("delta");
    delta["counterId"] = json!("counter-2");
    assert!(numeric_event(
        &delta,
        &mut ids,
        &mut overlaps,
        &mut turns,
        &mut counters,
        &mut representations,
    )
    .is_ok());
}

#[test]
fn stream_identity_partitions_sessions() {
    let mut first = event();
    first["id"] = json!("session-1-a");
    first["semantics"] = json!("cumulative");
    first["lifetime"] = json!("session");
    first["sessionId"] = json!("session-1");
    first["counterId"] = json!("counter");
    first["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
    let first = normalize_event("smw_123", &first).unwrap();

    let mut second = first.clone();
    second["id"] = json!("session-2-a");
    second["sessionId"] = json!("session-2");
    second["values"] = json!({"inputTokens": 20, "cachedInputTokens": 4, "outputTokens": 8});
    assert!(validate_monotonic(&[first.clone()], &second).is_ok());

    let mut next = first.clone();
    next["id"] = json!("session-1-b");
    next["values"] = json!({"inputTokens": 12, "cachedInputTokens": 3, "outputTokens": 5});
    assert!(validate_monotonic(&[first], &next).is_ok());
    next["adapterVersion"] = json!("adapter-2");
    assert!(validate_monotonic(&[next.clone()], &second).is_ok());

    let mut missing = second.clone();
    missing["sessionId"] = Value::Null;
    assert!(validate_contract(&missing).is_err());
    missing["sessionId"] = json!("session-2");
    missing["counterId"] = Value::Null;
    assert!(validate_contract(&missing).is_err());
}

#[test]
fn per_turn_streams_count_same_turn_once_per_session() {
    let mut first = event();
    first["id"] = json!("turn-session-1");
    first["semantics"] = json!("per_turn");
    first["lifetime"] = json!("session");
    first["sessionId"] = json!("session-1");
    first["counterId"] = json!("turn-counter");
    let mut second = first.clone();
    second["id"] = json!("turn-session-2");
    second["sessionId"] = json!("session-2");
    let mut ids = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut counters = std::collections::BTreeMap::new();
    let mut representations = std::collections::BTreeMap::new();
    assert_eq!(
        numeric_event(
            &normalize_event("smw_123", &first).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut counters,
            &mut representations,
        )
        .unwrap()
        .1,
        Some([0, 0, 3])
    );
    assert_eq!(
        numeric_event(
            &normalize_event("smw_123", &second).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut counters,
            &mut representations,
        )
        .unwrap()
        .1,
        Some([0, 0, 3])
    );
}

#[test]
fn rejects_invalid_cache_and_total() {
    let mut invalid_cache = event();
    invalid_cache["values"] = json!({"inputTokens": 1, "cachedInputTokens": 2, "outputTokens": 0});
    assert!(normalize_event("smw_123", &invalid_cache).is_err());

    let mut overflow = event();
    overflow["values"] =
        json!({"inputTokens": u64::MAX, "cachedInputTokens": 0, "outputTokens": 1});
    assert!(normalize_event("smw_123", &overflow).is_err());
}

#[test]
fn native_account_redacts_unknown_scalars_and_gaps() {
    let account = json!({
        "status":"observed", "source":"provider-secret", "scope":"secret",
        "counterSemantics":"secret", "additive":true,
        "usage":{"inputTokens":1,"cachedInputTokens":2,"outputTokens":1},
        "coverage":{"gaps":["provider-secret"],"rootSetup":"secret","turn":"provider-secret"}
    });
    let bounded = bounded_native_account(&account);
    assert_eq!(bounded["status"], "missing");
    assert_eq!(bounded["source"], "unavailable");
    assert_eq!(bounded["additive"], false);
    assert_eq!(bounded["coverage"]["gaps"][0], "unknown_gap");
    assert_eq!(bounded["coverage"]["turn"], Value::Null);
    assert_eq!(bounded["coverage"]["commands"], Value::Null);
    let missing = bounded_native_account(&json!({
        "status":"observed", "usage":{"inputTokens":1,"cachedInputTokens":0,"outputTokens":1}
    }));
    assert_eq!(missing["status"], "missing");
}

#[test]
fn native_record_redacts_nested_siblings_and_keeps_valid_scalars() {
    let account = json!({
        "status":"observed", "source":"native_observation", "scope":"native_turn",
        "counterSemantics":"provider_turn_snapshot", "usage":{"inputTokens":1,"cachedInputTokens":0,"outputTokens":1},
        "coverage":{"turn":"completed","gaps":[],"unobservedItems":0}
    });
    let malformed = json!({
        "attempt":{"rawSentinel":"secret"},
        "sessionMode":{"rawSentinel":"secret"},
        "adapterVersion":{"rawSentinel":"secret"},
        "goalId":{"rawSentinel":"secret"},
        "taskId":{"rawSentinel":"secret"},
        "role":{"rawSentinel":"secret"},
        "observation":{"turn":{"rawSentinel":"secret"}}
    });
    let bounded = native_record(
        &malformed,
        "assignment-1",
        "operation-1",
        "implementation",
        true,
        &account,
    );
    assert!(bounded["attempt"].is_null());
    assert!(bounded["sessionMode"].is_null());
    assert!(bounded["adapterVersion"].is_null());
    assert!(bounded["goalId"].is_null());
    assert!(bounded["taskId"].is_null());
    assert!(bounded["role"].is_null());
    assert!(bounded["turn"].is_null());
    assert!(!serde_json::to_string(&bounded)
        .unwrap()
        .contains("rawSentinel"));

    let valid = json!({
        "attempt":2, "sessionMode":"persistent", "adapterVersion":"codex-1",
        "goalId":"goal-1", "taskId":"task-1", "role":"worker",
        "observation":{"turn":"completed"}
    });
    let bounded = native_record(
        &valid,
        "assignment-1",
        "operation-1",
        "implementation",
        true,
        &account,
    );
    assert_eq!(bounded["attempt"], 2);
    assert_eq!(bounded["sessionMode"], "persistent");
    assert_eq!(bounded["adapterVersion"], "codex-1");
    assert_eq!(bounded["turn"], "completed");
}

#[test]
fn prior_failure_accepts_operation_alias() {
    let value = json!({"operation":"op-1","status":"failed"});
    let projected = super::super::bounded_prior_failure(&value);
    assert_eq!(projected["operation"], "op-1");
}

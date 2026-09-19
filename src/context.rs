//! Thin, replayable context projections and the bounded low-information loop.
//!
//! This module is intentionally provider-neutral.  The run ledger remains the
//! authority; these values are projections that carry exact references back to
//! it.  The governor is a small append-only reducer so a fresh process can
//! replay the same decision without conversation history.

use crate::hash;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;

pub(crate) const CONTEXT_VERSION: u64 = 1;
pub(crate) const GOVERNOR_VERSION: u64 = 1;
pub(crate) const SENSOR_VERSION: u64 = 1;
pub(crate) const HARD_ITERATION_BUDGET: u64 = 3;

/// Marker persisted on a v0.22 start event.  It deliberately carries only
/// the validated defaults; the append-only governor events remain the source
/// of spent budget and are replayed by `reduce_governor`.
pub(crate) fn validate_marker(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == 2
            && value["version"].as_u64() == Some(GOVERNOR_VERSION)
            && value["budget"].as_u64() == Some(HARD_ITERATION_BUDGET)
    })
}

const PROJECTION_FIELDS: &[&str] = &[
    "version",
    "role",
    "run",
    "subject",
    "goal",
    "scope",
    "obligations",
    "evidence",
    "loop",
    "next",
    "expansions",
];

/// Project only the current decision surface.  Every item that is omitted is
/// reachable through an exact hash/path expansion reference.
pub(crate) fn project(
    work: &str,
    view: &Value,
    status: &Value,
    next: &Value,
) -> Result<Value, String> {
    let role = if view["status"] != "running" {
        "recovery"
    } else {
        next["role"].as_str().unwrap_or("recovery")
    };
    let run_id = view["runId"]
        .as_str()
        .ok_or("context projection has no run identity")?;
    let workflow = view["workflow"]
        .as_str()
        .ok_or("context projection has no workflow")?;
    let goal = view["events"][0]["goal"]
        .as_str()
        .ok_or("context projection has no goal")?;
    let subject = view.get("subject").cloned().unwrap_or(Value::Null);
    let obligations = obligations(view, status, next);
    let evidence = evidence(view, status);
    let expansions = expansions(view, status, work);
    let mut value = json!({
        "version": CONTEXT_VERSION,
        "role": role,
        "run": {"id": run_id, "workflow": workflow},
        "subject": subject,
        "goal": goal,
        "scope": scope(view),
        "obligations": obligations,
        "evidence": evidence,
        "loop": governor_projection(view),
        "next": next_surface(next),
        "expansions": expansions,
    });
    value["digest"] = json!(hash::value(&value));
    Ok(value)
}

fn scope(view: &Value) -> Value {
    json!({
        "planSha256": view["subject"]["planSha256"],
        "configSha256": view["subject"]["configSha256"],
        "attempt": view["attempt"],
        "currentStage": view["currentStage"],
    })
}

fn next_surface(next: &Value) -> Value {
    let mut value = json!({
        "action": next["action"],
        "role": next["role"],
        "agent": next["agent"],
        "assignment": next["assignment"],
    });
    if let Some(outcomes) = next.get("outcomes") {
        value["outcomes"] = outcomes.clone();
    }
    if let Some(check) = next.get("check") {
        value["check"] = check.clone();
    }
    value
}

fn obligations(view: &Value, status: &Value, next: &Value) -> Value {
    let mut result = Vec::new();
    if view["status"] != "running" {
        return Value::Array(result);
    }
    if let Some(targets) = status["checks"]["targets"].as_array() {
        for target in targets {
            let mut item = json!({
                "kind": target["kind"],
                "status": target["status"],
                "targetId": target["targetEventSha256"],
                "evidenceId": target["checkEventSha256"],
                "requirementId": target["requirementId"],
                "requirementText": target["requirementText"],
                "checker": target["checkCommand"],
                "checkerSha256": target["checkCommandSha256"],
                "origin": target["origin"],
                "acquisition": target["acquisition"],
            });
            if let Some(requirement_id) = target["requirementId"].as_str() {
                if let Some(requirement) = view["events"][0]["preservation"]["requirements"]
                    .as_array()
                    .and_then(|items| items.iter().find(|item| item["id"] == requirement_id))
                {
                    item["requirementText"] = requirement["text"].clone();
                    item["checker"] = requirement["command"].clone();
                    item["checkerSha256"] = requirement["commandSha256"].clone();
                }
            } else {
                item["checker"] = view["events"][0]["checkPolicy"]["command"].clone();
                item["checkerSha256"] = view["events"][0]["checkPolicy"]["commandSha256"].clone();
            }
            if target["status"] == "missing" {
                item["state"] = json!("required");
            } else if target["status"] == "failed" {
                item["state"] = json!("repair_required");
            } else {
                item["state"] = json!("satisfied");
            }
            result.push(item);
        }
    }
    if next["action"] == "lead_decision" {
        result.push(json!({"kind":"lead_acceptance", "state":"required"}));
    } else if next["role"].is_string() {
        result.push(json!({
            "kind": next["role"],
            "agent": next["agent"],
            "state": "required",
        }));
    }
    Value::Array(result)
}

fn evidence(view: &Value, status: &Value) -> Value {
    let mut result = Vec::new();
    if let Some(targets) = status["checks"]["targets"].as_array() {
        for target in targets {
            let mut item = Map::new();
            for key in [
                "kind",
                "status",
                "targetId",
                "evidenceId",
                "requirementId",
                "requirementText",
                "checker",
                "checkerSha256",
                "origin",
                "acquisition",
                "result",
            ] {
                if let Some(value) = target.get(key) {
                    item.insert(key.to_owned(), value.clone());
                }
            }
            if let Some(requirement_id) = item.get("requirementId").and_then(Value::as_str) {
                if let Some(requirement) = view["events"][0]["preservation"]["requirements"]
                    .as_array()
                    .and_then(|items| items.iter().find(|item| item["id"] == requirement_id))
                {
                    item.insert("requirementText".into(), requirement["text"].clone());
                    item.insert("checker".into(), requirement["command"].clone());
                    item.insert("checkerSha256".into(), requirement["commandSha256"].clone());
                }
            } else {
                item.insert(
                    "checker".into(),
                    view["events"][0]["checkPolicy"]["command"].clone(),
                );
                item.insert(
                    "checkerSha256".into(),
                    view["events"][0]["checkPolicy"]["commandSha256"].clone(),
                );
            }
            if let Some(target_sha) = target["targetEventSha256"].as_str() {
                item.insert(
                    "rawEventRef".into(),
                    json!({"kind":"ledger_event", "sha256":target_sha, "exact":true}),
                );
            }
            result.push(Value::Object(item));
        }
    }
    if let Some(events) = view["events"].as_array() {
        for event in events {
            if event["action"] == "submit" {
                if let Some(sha) = event["eventSha256"].as_str() {
                    result.push(json!({
                        "kind": "submission",
                        "evidenceId": sha,
                        "artifact": event["artifact"],
                        "rawEventRef": {"kind":"ledger_event", "sha256":sha, "exact":true},
                    }));
                }
            }
        }
    }
    Value::Array(result)
}

fn expansions(view: &Value, status: &Value, work: &str) -> Value {
    let mut refs = Vec::new();
    if let Some(events) = view["events"].as_array() {
        for event in events {
            if let Some(sha) = event["eventSha256"].as_str() {
                refs.push(json!({
                    "id": sha,
                    "kind": "ledger_event",
                    "path": format!("events/{sha}"),
                    "sha256": sha,
                    "exact": true,
                }));
            }
        }
    }
    if let Some(targets) = status["checks"]["targets"].as_array() {
        for target in targets {
            if let Some(sha) = target["targetEventSha256"].as_str() {
                refs.push(json!({
                    "id": format!("check:{sha}"),
                    "kind": "check_evidence",
                    "path": format!("checks/{sha}"),
                    "sha256": sha,
                    "exact": true,
                }));
            }
        }
    }
    refs.push(json!({
        "id": "residual_packet",
        "kind": "projection",
        "path": format!("work/{work}"),
        "sha256": hash::value(&json!({"work":work,"run":view["runId"]})),
        "exact": true,
    }));
    dedupe_refs(refs)
}

fn dedupe_refs(refs: Vec<Value>) -> Value {
    let mut seen = BTreeSet::new();
    Value::Array(
        refs.into_iter()
            .filter(|value| seen.insert(value["id"].as_str().unwrap_or_default().to_owned()))
            .collect(),
    )
}

fn governor_projection(view: &Value) -> Value {
    view.get("governor").cloned().unwrap_or_else(|| {
        json!({
            "version": GOVERNOR_VERSION,
            "budget": HARD_ITERATION_BUDGET,
            "spent": 0,
            "state": "ready",
            "lineageSha256": view["subject"]["sha256"],
            "evidence": [],
            "sensor": {"available": false, "accepted": 0},
        })
    })
}

/// A role packet can be replayed only while this exact projection is current.
/// Older packets without `context` remain accepted by the v0.21 façade.
pub(crate) fn validate_projection(packet: &Value, fresh: &Value) -> Result<(), String> {
    let Some(packet_context) = packet.get("context") else {
        return Ok(());
    };
    let Some(fresh_context) = fresh.get("context") else {
        return Err("context projection is missing from current state".into());
    };
    if !valid_projection(packet_context) {
        return Err("context projection is malformed".into());
    }
    if packet_context != fresh_context {
        return Err("context projection is stale or tampered".into());
    }
    Ok(())
}

fn valid_projection(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    PROJECTION_FIELDS
        .iter()
        .all(|field| object.contains_key(*field))
        && value["version"].as_u64() == Some(CONTEXT_VERSION)
        && value["digest"].as_str().is_some_and(|digest| {
            let mut copy = value.clone();
            copy.as_object_mut().unwrap().remove("digest");
            hash::value(&copy) == digest
        })
}

fn semantic_mutation(value: &Value) -> Result<Value, String> {
    let object = value.as_object().ok_or("mutation must be an object")?;
    let mut key = Map::new();
    for field in ["unit", "operation", "lineageSha256", "inputSha256"] {
        let value = object
            .get(field)
            .ok_or_else(|| format!("mutation is missing {field}"))?;
        key.insert(field.to_owned(), value.clone());
    }
    if let Some(evidence) = object.get("newEvidenceSha256") {
        key.insert("newEvidenceSha256".into(), evidence.clone());
    }
    Ok(Value::Object(key))
}

/// Reduce a governor event stream.  All accepted mutations consume one unit;
/// wrappers, timestamps, comments, and equivalent replans cannot reset it.
pub(crate) fn reduce_governor(events: &[Value]) -> Result<Value, String> {
    let mut state = json!({
        "version": GOVERNOR_VERSION,
        "budget": HARD_ITERATION_BUDGET,
        "spent": 0,
        "state": "ready",
        "lineageSha256": Value::Null,
        "evidence": [],
        "seenMutations": [],
        "seenSensors": [],
        "currentMutation": Value::Null,
    });
    let mut expected_previous = Value::Null;
    for (index, event) in events.iter().enumerate() {
        let object = event
            .as_object()
            .ok_or_else(|| format!("governor event {} is not an object", index + 1))?;
        if event["version"].as_u64() != Some(GOVERNOR_VERSION)
            || event["previousSha256"] != expected_previous
        {
            return Err(format!("governor event {} is not replayable", index + 1));
        }
        let action = event["action"].as_str().unwrap_or_default();
        match action {
            "mutation" => apply_mutation(&mut state, event)?,
            "checkpoint" => apply_checkpoint(&mut state, event)?,
            "sensor" => apply_sensor(&mut state, event)?,
            _ => return Err(format!("governor event {} has unknown action", index + 1)),
        }
        if !object.contains_key("eventSha256") {
            return Err(format!("governor event {} has no identity", index + 1));
        }
        let mut without_hash = event.clone();
        without_hash.as_object_mut().unwrap().remove("eventSha256");
        let sha = hash::value(&without_hash);
        if event["eventSha256"] != sha {
            return Err(format!("governor event {} is tampered", index + 1));
        }
        expected_previous = event["eventSha256"].clone();
    }
    state["headSha256"] = expected_previous;
    Ok(state)
}

fn apply_mutation(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "checkpoint",
        "inputSha256",
    ] {
        if event.get(field).is_none() {
            return Err(format!("mutation is missing {field}"));
        }
    }
    if state["state"] == "blocked" {
        return Err("governor is blocked; mutation refused".into());
    }
    let carry_lineage = event["carryLineage"] == true;
    if carry_lineage {
        // A worker completion may deliberately advance the subject and
        // attempt while remaining in one bounded mutation lineage.  The
        // lineage identity is still immutable; this explicit field prevents
        // an ordinary stale replay from masquerading as a continuation.
        bind_run(state, event)?;
        if state["lineageSha256"].is_null() {
            state["lineageSha256"] = event["lineageSha256"].clone();
        } else if state["lineageSha256"] != event["lineageSha256"] {
            return Err("mutation lineage changed; hard budget cannot be reset".into());
        }
        state["subjectSha256"] = event["subjectSha256"].clone();
        state["attempt"] = event["attempt"].clone();
    } else {
        bind_identity(state, event)?;
    }
    if state["lineageSha256"].is_null() {
        state["lineageSha256"] = event["lineageSha256"].clone();
    } else if state["lineageSha256"] != event["lineageSha256"] {
        return Err("mutation lineage changed; hard budget cannot be reset".into());
    }
    let key = semantic_mutation(event)?;
    let spent = state["spent"].as_u64().unwrap_or(0);
    if spent >= HARD_ITERATION_BUDGET {
        state["state"] = json!("blocked");
        return Err("bounded iteration budget exhausted".into());
    }
    state["spent"] = json!(spent + 1);
    let digest = hash::value(&key);
    let equivalent = state["seenMutations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["digest"] == digest));
    state["seenMutations"].as_array_mut().unwrap().push(json!({
        "digest": digest,
        "equivalent": equivalent,
        "evidence": event["newEvidenceSha256"],
    }));
    if let Some(evidence) = event["newEvidenceSha256"].as_str() {
        if !evidence.trim().is_empty()
            && !state["evidence"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item == evidence))
        {
            state["evidence"]
                .as_array_mut()
                .unwrap()
                .push(json!(evidence));
        }
    }
    if state["spent"] == state["budget"] {
        state["state"] = json!("checkpoint_required");
    }
    state["currentMutation"] = json!({
        "runId": event["runId"],
        "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"],
        "checkpoint": event["checkpoint"],
        "inputSha256": event["inputSha256"],
    });
    Ok(())
}

fn apply_checkpoint(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "checkpoint",
        "inputSha256",
    ] {
        if event.get(field).is_none() {
            return Err(format!("checkpoint is missing {field}"));
        }
    }
    bind_identity(state, event)?;
    if event["allowed"] != true {
        state["state"] = json!("blocked");
        return Ok(());
    }
    if state["state"] == "blocked" {
        return Err("blocked governor cannot be reopened by a checkpoint".into());
    }
    state["state"] = if state["spent"].as_u64().unwrap_or(0) >= HARD_ITERATION_BUDGET {
        json!("blocked")
    } else {
        json!("ready")
    };
    Ok(())
}

fn apply_sensor(state: &mut Value, event: &Value) -> Result<(), String> {
    if event["sensorVersion"].as_u64() != Some(SENSOR_VERSION) {
        return Err("sensor event has an unsupported version".into());
    }
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "checkpoint",
        "inputSha256",
        "inputDigest",
    ] {
        if event.get(field).is_none() {
            return Err(format!("sensor event is missing {field}"));
        }
    }
    let current = state["currentMutation"]
        .as_object()
        .ok_or("sensor has no current mutation to bind")?;
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "checkpoint",
        "inputSha256",
    ] {
        if current.get(field) != event.get(field) {
            return Err(format!("sensor is stale or mismatched for {field}"));
        }
    }
    bind_identity(state, event)?;
    let key = hash::value(&json!({
        "runId": event["runId"], "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"], "checkpoint": event["checkpoint"],
        "inputSha256": event["inputSha256"], "inputDigest": event["inputDigest"],
    }));
    let seen = state["seenSensors"].as_array().unwrap();
    if seen.iter().any(|item| item == &key) {
        return Ok(());
    }
    state["seenSensors"]
        .as_array_mut()
        .unwrap()
        .push(json!(key));
    // Sensor output may only stop earlier.  It can never grant READY, reset the
    // budget, or request a retry.  Missing/malformed/optimistic output is inert.
    let confidence = event["confidence"].as_f64().unwrap_or(-1.0);
    if event["assessment"] == "low_information"
        && (0.0..=1.0).contains(&confidence)
        && confidence >= 0.9
    {
        state["state"] = json!("blocked");
        state["sensorStop"] = json!("conservative_low_information");
    }
    Ok(())
}

fn bind_identity(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in ["runId", "subjectSha256", "attempt"] {
        if state[field].is_null() {
            state[field] = event[field].clone();
        } else if state[field] != event[field] {
            return Err(format!("governor identity changed: {field}"));
        }
    }
    Ok(())
}

fn bind_run(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["runId"].is_null() {
        state["runId"] = event["runId"].clone();
    } else if state["runId"] != event["runId"] {
        return Err("governor identity changed: runId".into());
    }
    Ok(())
}

/// Build an event with its hash and chain link.  Callers can persist these
/// bytes in their existing append-only state without a provider dependency.
pub(crate) fn event(previous: Option<&Value>, mut value: Value) -> Value {
    value["version"] = json!(GOVERNOR_VERSION);
    value["previousSha256"] = previous
        .and_then(|item| item["eventSha256"].as_str())
        .map_or(Value::Null, |value| json!(value));
    value["eventSha256"] = json!(hash::value(&value));
    value
}

pub(crate) fn checkpoint(state: &Value, proposed: &Value) -> Value {
    let allowed = state["state"] != "blocked"
        && state["spent"].as_u64().unwrap_or(HARD_ITERATION_BUDGET) < HARD_ITERATION_BUDGET;
    json!({
        "allowed": allowed,
        "reason": if allowed { "governed_mutation_unit_allowed" } else { "bounded_iteration_refused" },
        "next": if allowed { proposed.clone() } else { Value::Null },
        "governor": state,
    })
}

pub(crate) fn read_json(path: &str) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("context input cannot be read: {error}"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("context input exceeds the supported size".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("context input is not valid JSON: {error}"))
}

pub(crate) fn reduce_file(path: &str) -> Result<Value, String> {
    let value = read_json(path)?;
    let events = value
        .as_array()
        .ok_or("governor event input must be a JSON array")?;
    reduce_governor(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation(previous: Option<&Value>, n: u64, evidence: Option<&str>) -> Value {
        event(
            previous,
            json!({
                "action":"mutation", "runId":"run", "subjectSha256":"subject",
                "attempt":1, "checkpoint":n, "inputSha256":"input",
                "lineageSha256":"lineage", "unit":"unit", "operation":"replan",
                "newEvidenceSha256":evidence,
                "timestamp":n, "comment":format!("wrapper-{n}"),
            }),
        )
    }

    #[test]
    fn equivalent_replans_and_wrappers_consume_one_shared_budget() {
        let first = mutation(None, 1, None);
        let second = mutation(Some(&first), 2, None);
        let third = mutation(Some(&second), 3, None);
        let state = reduce_governor(&[first, second, third]).unwrap();
        assert_eq!(state["spent"], 3);
        assert_eq!(state["state"], "checkpoint_required");
        let refused = checkpoint(&state, &json!({"operation":"replan"}));
        assert_eq!(refused["allowed"], false);
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

        let sensor = event(
            Some(&first),
            json!({
                "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
                "checkpoint":1, "inputSha256":"input", "inputDigest":"d",
                "sensorVersion":1,
                "assessment":"low_information", "confidence":0.95,
            }),
        );
        let duplicate = event(
            Some(&sensor),
            json!({
                "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
                "checkpoint":1, "inputSha256":"input", "inputDigest":"d",
                "sensorVersion":1,
                "assessment":"ready", "confidence":1.0,
            }),
        );
        let state = reduce_governor(&[first.clone(), sensor, duplicate]).unwrap();
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
        for checkpoint in 1..=3 {
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
        let state = reduce_governor(&events).unwrap();
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
        let stale_sensor = event(
            Some(&mutation),
            json!({
                "action":"sensor", "runId":"run", "subjectSha256":"subject",
                "attempt":1, "checkpoint":1, "inputSha256":input_d.clone(),
                "inputDigest":input_d, "sensorVersion":1,
                "assessment":"low_information", "confidence":0.95,
            }),
        );
        assert!(reduce_governor(&[mutation.clone(), stale_sensor]).is_err());
        let current = reduce_governor(&[mutation.clone()]).unwrap();
        assert_eq!(current["state"], "ready");
        assert!(current["seenSensors"].as_array().unwrap().is_empty());

        let valid_sensor = event(
            Some(&mutation),
            json!({
                "action":"sensor", "runId":"run", "subjectSha256":"subject",
                "attempt":1, "checkpoint":2, "inputSha256":"b".repeat(64),
                "inputDigest":"d".repeat(64), "sensorVersion":1,
                "assessment":"low_information", "confidence":0.95,
            }),
        );
        let blocked = reduce_governor(&[mutation, valid_sensor]).unwrap();
        assert_eq!(blocked["state"], "blocked");
        assert_eq!(blocked["seenSensors"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn absent_sensor_and_optimistic_output_do_not_unlock_or_retry() {
        let mutation = mutation(None, 1, None);
        let unavailable = event(
            Some(&mutation),
            json!({
                "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
                "checkpoint":1, "inputSha256":"input", "inputDigest":"d",
                "sensorVersion":1,
                "assessment":"unavailable", "confidence":null,
            }),
        );
        let optimistic = event(
            Some(&unavailable),
            json!({
                "action":"sensor", "runId":"run", "subjectSha256":"subject", "attempt":1,
                "checkpoint":1, "inputSha256":"input", "inputDigest":"e",
                "sensorVersion":1,
                "assessment":"ready", "confidence":1.0,
            }),
        );
        let state = reduce_governor(&[mutation, unavailable, optimistic]).unwrap();
        assert_eq!(state["state"], "ready");
        assert_eq!(state["spent"], 1);
    }
}

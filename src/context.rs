//! Thin, replayable context projections and the bounded low-information loop.
//!
//! This module is intentionally provider-neutral.  The run ledger remains the
//! authority; these values are projections that carry exact references back to
//! it.  The governor is a small append-only reducer so a fresh process can
//! replay the same decision without conversation history.

use crate::evidence::hash;
pub(crate) use crate::kernel::governor::{
    HARD_ITERATION_BUDGET, NO_INFORMATION_LIMIT, POST_REPLAN_LIMIT,
};
use serde_json::{json, Map, Value};
use std::fs;

mod governor;
mod recovery_observation;

pub(crate) use governor::{reduce_governor, reduce_governor_seeded, sensor_request};

pub(crate) const CONTEXT_VERSION: u64 = 2;
pub(crate) const OPAQUE_CONTEXT_VERSION: u64 = 3;
pub(crate) const GOVERNOR_VERSION: u64 = crate::kernel::governor::VERSION;
pub(crate) const SENSOR_VERSION: u64 = 1;
pub(crate) const GRANT_PROTOCOL_VERSION: u64 = 1;

/// Marker persisted on a v0.22 start event.  It deliberately carries only
/// the validated defaults; the append-only governor events remain the source
/// of spent budget and are replayed by `reduce_governor`.
pub(crate) fn validate_marker(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        let legacy = object.len() == 2
            || (object.len() == 3 && value["replanBinding"] == "assignment_packet_v1");
        let current = (object.len() == 4 || object.len() == 5 || object.len() == 6)
            && value["noInformationLimit"].as_u64() == Some(NO_INFORMATION_LIMIT)
            && value["postReplanLimit"].as_u64() == Some(POST_REPLAN_LIMIT);
        (legacy || current)
            && value["version"].as_u64() == Some(GOVERNOR_VERSION)
            && value["budget"].as_u64() == Some(HARD_ITERATION_BUDGET)
            && (object.len() == 2
                || object.len() == 4
                || (object.len() == 3 && value["replanBinding"] == "assignment_packet_v1")
                || (object.len() == 6 && value["replanBinding"] == "assignment_packet_v1")
                || value["grantProtocol"].as_u64() == Some(GRANT_PROTOCOL_VERSION))
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
    let evidence = evidence(work, view, status);
    let stale = stale_evidence(work, view);
    let loop_state = governor_projection(view);
    let mut next_action = next_surface(next);
    match loop_state["state"].as_str() {
        Some("replan_required") => next_action["requiredAction"] = json!("work replan"),
        Some("evidence_required") => next_action["requiredAction"] = json!("work evidence"),
        Some("blocked") => next_action["requiredAction"] = json!("terminal refusal"),
        _ => {}
    }
    let recovery = json!({
        "goal": goal,
        "scope": scope(view),
        "subject": subject.clone(),
        "current": evidence.clone(),
        "stale": stale.clone(),
        "missing": obligations.clone(),
        "conflicts": [],
        "loop": loop_state.clone(),
        "next": next_action.clone(),
    });
    let expansions = expansions(view, status, work);
    let mut value = json!({
        "version": OPAQUE_CONTEXT_VERSION,
        "role": role,
        "run": {"id": run_id, "workflow": workflow},
        "subject": subject,
        "goal": goal,
        "scope": scope(view),
        "obligations": obligations,
        "evidence": evidence,
        "loop": loop_state,
        "next": next_action,
        "expansions": expansions,
        "recovery": recovery,
    });
    crate::work::packet::sanitize_assignment(&mut value);
    value["digest"] = json!(hash::value(&value));
    Ok(value)
}

fn scope(view: &Value) -> Value {
    json!({
        "source": "run_ledger",
        "owner": "lead",
        "planSha256": view["subject"]["planSha256"],
        "basisSha256": view["basis"].get("sha256").cloned().unwrap_or_else(|| view["subject"]["basisSha256"].clone()),
        "reviewDecisionSha256": view["reviewPolicy"].get("sha256").cloned().unwrap_or(Value::Null),
        "configSha256": view["subject"]["configSha256"],
        "freshness": if view["status"] == "running" { "current" } else { "historical" },
        "attempt": view["attempt"],
        "currentStage": view["currentStage"],
    })
}

fn next_surface(next: &Value) -> Value {
    let mut value = json!({
        "source": "run_state",
        "owner": next_owner(next),
        "action": next["action"],
        "role": next["role"],
        "agent": next["agent"],
        "assignment": next["assignment"],
        "freshness": "current",
    });
    if let Some(outcomes) = next.get("outcomes") {
        value["outcomes"] = outcomes.clone();
    }
    if let Some(check) = next.get("check") {
        value["check"] = check.clone();
    }
    value
}

fn next_owner(next: &Value) -> &'static str {
    if let Some(actor) = next["resolvedActor"].as_str() {
        return match actor {
            "worker" => "worker",
            "reviewer" => "reviewer",
            "adviser" => "adviser",
            "exitbind" => "exitbind",
            _ => "lead",
        };
    }
    if next["action"] == "check" {
        "exitbind"
    } else if next["action"] == "lead_decision" {
        "lead"
    } else {
        match next["role"].as_str() {
            Some("worker") => "worker",
            Some("reviewer") => "reviewer",
            Some("adviser") => "adviser",
            _ => "lead",
        }
    }
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
                "freshness": if target["status"] == "passed" { "current" } else { "unresolved" },
                "strength": evidence_strength(target["acquisition"].as_str()),
                "targetEventSha256": target["targetEventSha256"],
                "checkEventSha256": target["checkEventSha256"],
                "requirementId": target["requirementId"],
                "requirementText": target["requirementText"],
                "checker": target["checkCommand"],
                "checkerSha256": target["checkCommandSha256"],
                "origin": target["origin"],
                "acquisition": target["acquisition"],
            });
            if let Some(check_sha) = target["checkEventSha256"].as_str() {
                item["logStatus"] = log_status(view, check_sha);
            }
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
            if !crate::producer::exitbind_surface() {
                item.as_object_mut()
                    .expect("obligation projection is an object")
                    .remove("targetEventSha256");
                item.as_object_mut()
                    .expect("obligation projection is an object")
                    .remove("checkEventSha256");
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

fn evidence(work: &str, view: &Value, status: &Value) -> Value {
    let mut result = Vec::new();
    if let Some(targets) = status["checks"]["targets"].as_array() {
        for target in targets {
            let mut item = Map::new();
            for key in [
                "kind",
                "status",
                "targetEventSha256",
                "checkEventSha256",
                "requirementId",
                "requirementText",
                "checker",
                "checkerSha256",
                "origin",
                "acquisition",
                "logStatus",
                "result",
            ] {
                if let Some(value) = target.get(key) {
                    item.insert(key.to_owned(), value.clone());
                }
            }
            item.insert(
                "freshness".into(),
                json!(if target["status"] == "passed" {
                    "current"
                } else {
                    "unresolved"
                }),
            );
            item.insert(
                "strength".into(),
                json!(evidence_strength(target["acquisition"].as_str())),
            );
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
                    event_ref(work, view, target_sha, "ledger_event"),
                );
            }
            if let Some(check_sha) = target["checkEventSha256"].as_str() {
                item.insert("logStatus".into(), log_status(view, check_sha));
                item.insert(
                    "checkEventRef".into(),
                    event_ref(work, view, check_sha, "ledger_event"),
                );
                if let Some(log) = check_log_ref(work, view, check_sha) {
                    item.insert("checkLogRef".into(), log);
                }
            }
            if !crate::producer::exitbind_surface() {
                item.remove("targetEventSha256");
                item.remove("checkEventSha256");
            }
            result.push(Value::Object(item));
        }
    }
    let current_attempt = view["attempt"].as_u64();
    if let Some(events) = view["events"].as_array() {
        for event in events {
            if event["action"] == "submit"
                && current_attempt.is_some_and(|attempt| event["attempt"] == attempt)
            {
                if let Some(sha) = event["eventSha256"].as_str() {
                    result.push(json!({
                        "kind": "submission",
                        "evidenceId": sha,
                        "artifact": artifact_reference(&event["artifact"], "evidence"),
                        "rawEventRef": event_ref(work, view, sha, "ledger_event"),
                    }));
                }
            }
        }
    }
    Value::Array(result)
}

fn evidence_strength(acquisition: Option<&str>) -> &'static str {
    match acquisition {
        Some("observed") => "observed",
        Some("reported") => "reported",
        _ => "unknown",
    }
}

/// Keep the immediately prior attempt visible for recovery without replaying
/// every historical submission. Older evidence remains reachable through the
/// exact ledger snapshot reference in `expansions`.
fn stale_evidence(work: &str, view: &Value) -> Value {
    let Some(current_attempt) = view["attempt"].as_u64() else {
        return Value::Array(Vec::new());
    };
    let previous_attempt = current_attempt.saturating_sub(1);
    let mut result = Vec::new();
    if let Some(events) = view["events"].as_array() {
        for event in events {
            if event["attempt"] != previous_attempt
                || !matches!(event["action"].as_str(), Some("submit" | "check"))
            {
                continue;
            }
            let Some(sha) = event["eventSha256"].as_str() else {
                continue;
            };
            result.push(json!({
                "kind": event["action"],
                "status": "stale",
                "evidenceId": sha,
                "rawEventRef": event_ref(work, view, sha, "ledger_event"),
            }));
        }
    }
    Value::Array(result)
}

fn expansions(view: &Value, _status: &Value, work: &str) -> Value {
    let events = view["events"].as_array().cloned().unwrap_or_default();
    let head = events
        .last()
        .and_then(|event| event["eventSha256"].as_str())
        .unwrap_or_default();
    let mut reference = json!({
        "kind": "ledger_history",
        "work": work,
        "sha256": view["ledgerSha256"],
        "headEventSha256": head,
        "eventCount": events.len(),
        "exact": true,
    });
    set_opaque_id(&mut reference);
    Value::Array(vec![reference])
}

fn event_ref(work: &str, view: &Value, event_sha: &str, kind: &str) -> Value {
    let events = view["events"].as_array().cloned().unwrap_or_default();
    let head = events
        .last()
        .and_then(|event| event["eventSha256"].as_str())
        .unwrap_or_default();
    let mut reference = json!({
        "kind": kind,
        "work": work,
        "sha256": view["ledgerSha256"],
        "headEventSha256": head,
        "eventCount": events.len(),
        "selector": event_sha,
        "exact": true,
    });
    set_opaque_id_with_selector(&mut reference, event_sha);
    reference
}

fn check_log_ref(work: &str, view: &Value, check_sha: &str) -> Option<Value> {
    let event = view["events"].as_array()?.iter().find(|event| {
        event["action"] == "check" && event["eventSha256"].as_str() == Some(check_sha)
    })?;
    if event["acquisition"] != "observed"
        || event["stdout"].as_object().is_none()
        || event["stderr"].as_object().is_none()
    {
        return None;
    }
    let events = view["events"].as_array()?;
    let head = events.last()?.get("eventSha256")?;
    let mut reference = json!({
        "kind": "check_log",
        "work": work,
        "sha256": view["ledgerSha256"],
        "headEventSha256": head,
        "eventCount": events.len(),
        "checkEventSha256": check_sha,
        "stdout": artifact_reference(&event["stdout"], "stdout"),
        "stderr": artifact_reference(&event["stderr"], "stderr"),
        "exact": true,
    });
    set_opaque_id(&mut reference);
    Some(reference)
}

/// Public evidence references identify canonical bytes without carrying the
/// state-root transport needed to resolve them.
pub(crate) fn artifact_reference(artifact: &Value, kind: &str) -> Value {
    let reference = json!({
        "kind": kind,
        "sha256": artifact["sha256"],
        "bytes": artifact["bytes"],
        "exact": true,
    });
    reference
}

fn set_opaque_id(reference: &mut Value) {
    let digest = hash::value(reference);
    reference["id"] = json!(format!("ref:{digest}"));
}

fn set_opaque_id_with_selector(reference: &mut Value, selector: &str) {
    reference["selector"] = json!(selector);
    set_opaque_id(reference);
}

fn log_status(view: &Value, check_sha: &str) -> Value {
    let Some(event) = view["events"].as_array().and_then(|events| {
        events.iter().find(|event| {
            event["action"] == "check" && event["eventSha256"].as_str() == Some(check_sha)
        })
    }) else {
        return json!("unavailable_missing");
    };
    if event["version"].as_u64().is_some_and(|version| version < 8) {
        return json!("unavailable_historical");
    }
    if event["acquisition"] == "observed"
        && event["stdout"].is_object()
        && event["stderr"].is_object()
    {
        json!("available")
    } else {
        json!("unavailable_reported")
    }
}

fn governor_projection(view: &Value) -> Value {
    view.get("governor").cloned().unwrap_or_else(|| {
        json!({
            "enabled": false,
            "state": "not_applicable",
            "reason": "governor_not_applicable",
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
        && matches!(
            value["version"].as_u64(),
            Some(OPAQUE_CONTEXT_VERSION | CONTEXT_VERSION)
        )
        && value["digest"].as_str().is_some_and(|digest| {
            let mut copy = value.clone();
            copy.as_object_mut().unwrap().remove("digest");
            hash::value(&copy) == digest
        })
}

pub(crate) fn event(previous: Option<&Value>, mut value: Value) -> Value {
    value["version"] = json!(GOVERNOR_VERSION);
    value["previousSha256"] = previous
        .and_then(|item| item["eventSha256"].as_str())
        .map_or(Value::Null, |value| json!(value));
    value["eventSha256"] = json!(hash::value(&value));
    value
}

pub(crate) fn checkpoint(state: &Value, proposed: &Value) -> Value {
    let allowed = state["state"] == "ready";
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

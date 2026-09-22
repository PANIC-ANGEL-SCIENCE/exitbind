//! Thin, replayable context projections and the bounded low-information loop.
//!
//! This module is intentionally provider-neutral.  The run ledger remains the
//! authority; these values are projections that carry exact references back to
//! it.  The governor is a small append-only reducer so a fresh process can
//! replay the same decision without conversation history.

use crate::evidence::hash;
use crate::kernel::governor::{LoopState, Phase, Transition};
pub(crate) use crate::kernel::governor::{
    HARD_ITERATION_BUDGET, NO_INFORMATION_LIMIT, POST_REPLAN_LIMIT,
};
use serde_json::{json, Map, Value};
use std::fs;

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
    if let Some(evidence) = object.get("newEvidence") {
        key.insert("newEvidence".into(), evidence.clone());
    }
    Ok(Value::Object(key))
}

/// Reduce a governor event stream.  All accepted mutations consume one unit;
/// wrappers, timestamps, comments, and equivalent replans cannot reset it.
pub(crate) fn reduce_governor(events: &[Value]) -> Result<Value, String> {
    let mut state = json!({
        "version": GOVERNOR_VERSION,
        "budget": HARD_ITERATION_BUDGET,
        "defaults": {
            "noInformationLimit": NO_INFORMATION_LIMIT,
            "postReplanLimit": POST_REPLAN_LIMIT,
        },
        "spent": 0,
        "noInformationStreak": 0,
        "postReplanSpent": 0,
        "replanCount": 0,
        "afterReplan": false,
        "state": "ready",
        "lineageSha256": Value::Null,
        "evidence": [],
        "seenMutations": [],
        "consumedGrants": [],
        "seenSensors": [],
        "currentMutation": Value::Null,
        "currentReplan": Value::Null,
        "currentSensorRequest": Value::Null,
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
            "replan" => apply_replan(&mut state, event)?,
            "evidence" => apply_evidence(&mut state, event)?,
            "sensor_request" => apply_sensor_request(&mut state, event)?,
            "sensor" => apply_sensor(&mut state, event)?,
            "blocked" => apply_blocked(&mut state, event)?,
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

fn apply_blocked(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in ["runId", "subjectSha256", "attempt", "inputSha256"] {
        if event.get(field).is_none() {
            return Err(format!("blocked event is missing {field}"));
        }
    }
    bind_identity(state, event)?;
    let mut loop_state = loop_state(state)?;
    loop_state.apply(Transition::MutationRequested)?;
    sync_loop(state, loop_state);
    state["blocker"] = event
        .get("reason")
        .cloned()
        .unwrap_or_else(|| json!("governor refusal"));
    Ok(())
}

fn loop_state(state: &Value) -> Result<LoopState, String> {
    Ok(LoopState {
        phase: Phase::parse(state["state"].as_str().unwrap_or("ready"))?,
        total_mutations: state["spent"].as_u64().unwrap_or_default(),
        no_information_streak: state["noInformationStreak"].as_u64().unwrap_or_default(),
        replanned: state["afterReplan"] == true,
        post_replan_no_information: state["postReplanSpent"].as_u64().unwrap_or_default(),
    })
}

fn sync_loop(state: &mut Value, loop_state: LoopState) {
    state["state"] = json!(loop_state.phase.as_str());
    state["spent"] = json!(loop_state.total_mutations);
    state["noInformationStreak"] = json!(loop_state.no_information_streak);
    state["postReplanSpent"] = json!(loop_state.post_replan_no_information);
    state["afterReplan"] = json!(loop_state.replanned);
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
    if let Some(grants) = event
        .get("grantEventSha256s")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
    {
        if grants.iter().any(|grant| {
            grant.as_str().map_or(true, |value| {
                value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        }) {
            return Err("mutation grant acknowledgement is malformed".into());
        }
        let consumed = state["consumedGrants"]
            .as_array()
            .ok_or("governor consumed grants are invalid")?;
        if grants
            .iter()
            .any(|grant| consumed.iter().any(|item| item == grant))
        {
            return Err("mutation grant was already consumed".into());
        }
        let mut unique = std::collections::BTreeSet::new();
        if grants
            .iter()
            .filter_map(Value::as_str)
            .any(|grant| !unique.insert(grant))
        {
            return Err("mutation grant acknowledgement is duplicated".into());
        }
        bind_identity(state, event)?;
        let carry_lineage = event["carryLineage"] == true;
        if !carry_lineage {
            return Err("mutation grant acknowledgement must carry lineage".into());
        }
        bind_run(state, event)?;
        if state["lineageSha256"] != event["lineageSha256"] {
            return Err("mutation grant lineage changed".into());
        }
        state["consumedGrants"]
            .as_array_mut()
            .unwrap()
            .extend(grants.iter().cloned());
        return Ok(());
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
    let digest = hash::value(&key);
    let seen = state["seenMutations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(previous) = seen
        .iter()
        .find(|item| item["identity"] == mutation_identity(event))
    {
        if previous["digest"] != digest {
            return Err("conflicting duplicate mutation refused".into());
        }
        return Ok(());
    }
    if state["state"] == "replan_required" {
        return Err("material re-plan is required before the next mutation".into());
    }
    let mut loop_state = loop_state(state)?;
    let evidence = mutation_evidence(event)?;
    let has_new_evidence = if let Some(evidence) = evidence {
        record_evidence(state, event, evidence)?
    } else {
        false
    };
    state["seenMutations"].as_array_mut().unwrap().push(json!({
        "digest": digest,
        "identity": mutation_identity(event),
        "evidence": event.get("newEvidence").cloned().unwrap_or_else(|| event["newEvidenceSha256"].clone()),
    }));
    loop_state.apply(Transition::Mutation {
        new_evidence: has_new_evidence,
    })?;
    sync_loop(state, loop_state);
    state["currentMutation"] = json!({
        "runId": event["runId"],
        "eventSha256": event["eventSha256"],
        "carryLineage": event["carryLineage"],
        "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"],
        "checkpoint": event["checkpoint"],
        "inputSha256": event["inputSha256"],
    });
    Ok(())
}

fn mutation_identity(event: &Value) -> Value {
    json!({
        "runId": event["runId"],
        "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"],
        "checkpoint": event["checkpoint"],
        "inputSha256": event["inputSha256"],
        "unit": event["unit"],
        "operation": event["operation"],
    })
}

fn mutation_evidence(event: &Value) -> Result<Option<Value>, String> {
    if let Some(value) = event.get("newEvidence") {
        if value.is_null() {
            return Ok(None);
        }
        return Ok(Some(value.clone()));
    }
    // `newEvidenceSha256` is retained as legacy telemetry only. Current v8
    // reset authority belongs to an exact, separately resolved evidence event.
    Ok(None)
}

fn record_evidence(state: &mut Value, event: &Value, evidence: Value) -> Result<bool, String> {
    validate_evidence(event, &evidence)?;
    if state["state"] == "blocked" {
        return Err("blocked governor cannot be reopened by evidence".into());
    }
    let evidence_key = evidence_id(&evidence);
    if state["evidence"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| evidence_id(item) == evidence_key))
    {
        return Ok(false);
    }
    state["evidence"].as_array_mut().unwrap().push(evidence);
    Ok(true)
}

fn evidence_id(value: &Value) -> String {
    value
        .get("sha256")
        .and_then(Value::as_str)
        .map_or_else(|| hash::value(value), str::to_owned)
}

fn validate_evidence(event: &Value, evidence: &Value) -> Result<(), String> {
    if let Some(object) = evidence.as_object() {
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let root = object
            .get("root")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sha = object
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(root, "state" | "product")
            || path.is_empty()
            || path.starts_with('/')
            || path.split('/').any(|part| part == ".." || part.is_empty())
            || sha.len() != 64
            || !sha
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("evidence reference is malformed".into());
        }
        for field in ["subjectSha256", "attempt", "inputSha256"] {
            if let Some(value) = object.get(field) {
                if value != &event[field] {
                    return Err(format!("evidence is stale or mismatched for {field}"));
                }
            }
        }
    }
    Ok(())
}

fn apply_evidence(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "inputSha256",
        "evidence",
    ] {
        if event.get(field).is_none() {
            return Err(format!("evidence event is missing {field}"));
        }
    }
    bind_identity(state, event)?;
    let evidence = event["evidence"].clone();
    if record_evidence(state, event, evidence)? {
        let mut loop_state = loop_state(state)?;
        loop_state.apply(Transition::NewEvidence)?;
        sync_loop(state, loop_state);
    }
    Ok(())
}

fn apply_replan(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in ["runId", "subjectSha256", "attempt", "inputSha256"] {
        if event.get(field).is_none() {
            return Err(format!("re-plan event is missing {field}"));
        }
    }
    if state["state"] == "blocked" {
        return Err("blocked governor cannot be reopened by re-plan".into());
    }
    if state["state"] != "replan_required" {
        return Err("re-plan is not currently required".into());
    }
    // A completed worker mutation may carry the governor lineage into a new
    // subject/attempt.  The following re-plan belongs to that fresh subject;
    // requiring the pre-mutation identity here would make the governor
    // impossible to recover after a legitimate reviewer/worker transition.
    bind_replan_identity(state, event)?;
    let semantic = replan_semantic(event)?;
    if state["currentReplan"] == semantic {
        return Err("duplicate re-plan does not extend the bound".into());
    }
    state["currentReplan"] = semantic;
    state["replanCount"] = json!(state["replanCount"].as_u64().unwrap_or(0) + 1);
    let mut loop_state = loop_state(state)?;
    loop_state.apply(Transition::MaterialReplan)?;
    sync_loop(state, loop_state);
    Ok(())
}

fn bind_replan_identity(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["runId"].is_null() {
        state["runId"] = event["runId"].clone();
    } else if state["runId"] != event["runId"] {
        return Err("governor identity changed: runId".into());
    }
    let current_mutation = state["currentMutation"].as_object();
    let identity_transition = event["identityTransition"].as_str();
    if identity_transition == Some("carried_mutation_v1") {
        let current_mutation =
            current_mutation.ok_or("re-plan requires a current carried mutation")?;
        if current_mutation["carryLineage"] != true {
            return Err("re-plan requires the current mutation to carry lineage".into());
        }
        if event["previousSha256"] != current_mutation["eventSha256"] {
            return Err("re-plan is not immediately after the current mutation".into());
        }
    }
    let prior_attempt = state["attempt"].as_u64();
    let next_attempt = event["attempt"].as_u64();
    if identity_transition == Some("carried_mutation_v1")
        && state["subjectSha256"] != event["subjectSha256"]
    {
        if prior_attempt
            .zip(next_attempt)
            .map_or(true, |(prior, next)| next != prior + 1)
        {
            return Err(format!(
                "governor identity changed: subjectSha256 (attempt {:?} -> {:?})",
                prior_attempt, next_attempt
            ));
        }
        state["subjectSha256"] = event["subjectSha256"].clone();
        state["attempt"] = event["attempt"].clone();
    } else if identity_transition != Some("carried_mutation_v1") {
        // Version-1 governor ledgers predate the explicit assignment binding.
        // Keep their replay readable; new run ledgers reject this transition
        // at the outer run reducer via replanBinding.
        state["subjectSha256"] = event["subjectSha256"].clone();
        state["attempt"] = event["attempt"].clone();
    } else if state["attempt"] != event["attempt"] {
        return Err("governor identity changed: attempt".into());
    }
    Ok(())
}

fn replan_semantic(event: &Value) -> Result<Value, String> {
    let mut semantic = Map::new();
    let mut present = false;
    for field in ["hypothesis", "evidenceRequest", "scopeDecision", "blocker"] {
        if let Some(value) = event.get(field) {
            if !value.is_null() {
                present = true;
            }
            semantic.insert(field.to_owned(), value.clone());
        }
    }
    if !present {
        return Err("material re-plan requires a semantic field".into());
    }
    Ok(Value::Object(semantic))
}

fn apply_sensor_request(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "inputSha256",
        "questions",
        "requestDigest",
        "sensorVersion",
    ] {
        if event.get(field).is_none() {
            return Err(format!("sensor request is missing {field}"));
        }
    }
    bind_identity(state, event)?;
    if event["sensorVersion"].as_u64() != Some(SENSOR_VERSION)
        || !event["questions"].is_array()
        || event["questions"].as_array().unwrap().is_empty()
        || event["questions"].as_array().unwrap().len() > 8
        || event["questions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|question| !question.is_string() || question.as_str().unwrap().len() > 120)
    {
        return Err("sensor request has no bounded questions".into());
    }
    let expected = hash::value(&json!({
        "version": event["sensorVersion"],
        "runId": event["runId"],
        "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"],
        "inputSha256": event["inputSha256"],
        "questions": event["questions"],
    }));
    if event["requestDigest"] != expected {
        return Err("sensor request digest does not match its question set".into());
    }
    state["currentSensorRequest"] = json!({
        "runId": event["runId"], "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"], "inputSha256": event["inputSha256"],
        "requestDigest": event["requestDigest"],
    });
    Ok(())
}

pub(crate) fn sensor_request(state: &Value) -> Result<Value, String> {
    let current = state["currentMutation"].as_object();
    let run_id = state["runId"]
        .as_str()
        .or_else(|| {
            current
                .and_then(|value| value.get("runId"))
                .and_then(Value::as_str)
        })
        .ok_or("sensor state has no runId")?;
    let subject = state["subjectSha256"]
        .as_str()
        .or_else(|| {
            current
                .and_then(|value| value.get("subjectSha256"))
                .and_then(Value::as_str)
        })
        .ok_or("sensor state has no subject")?;
    let attempt = state["attempt"]
        .as_u64()
        .or_else(|| {
            current
                .and_then(|value| value.get("attempt"))
                .and_then(Value::as_u64)
        })
        .ok_or("sensor state has no attempt")?;
    let input = state["inputSha256"]
        .as_str()
        .or_else(|| {
            current
                .and_then(|value| value.get("inputSha256"))
                .and_then(Value::as_str)
        })
        .ok_or("sensor state has no input digest")?;
    let questions = json!([
        "is the current mutation gaining exact new evidence?",
        "does the current trajectory require a material re-plan?",
        "is an earlier conservative block warranted?"
    ]);
    let request_digest = hash::value(&json!({
        "version": SENSOR_VERSION,
        "runId": run_id,
        "subjectSha256": subject,
        "attempt": attempt,
        "inputSha256": input,
        "questions": questions,
    }));
    Ok(json!({
        "action": "sensor_request",
        "sensorVersion": SENSOR_VERSION,
        "runId": run_id,
        "subjectSha256": subject,
        "attempt": attempt,
        "inputSha256": input,
        "questions": questions,
        "requestDigest": request_digest,
    }))
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
    if state["state"] == "replan_required" {
        return Err("material re-plan is required before checkpoint".into());
    }
    if state["state"] != "ready" {
        return Err("checkpoint cannot reopen a conservative governor phase".into());
    }
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
        "requestDigest",
        "assessment",
        "identitySource",
    ] {
        if event.get(field).is_none() {
            return Err(format!("sensor event is missing {field}"));
        }
    }
    if event["identitySource"] != "host-reported" {
        return Err("sensor identity must remain host-reported".into());
    }
    let request = state["currentSensorRequest"]
        .as_object()
        .ok_or("sensor result has no current request")?;
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "inputSha256",
        "requestDigest",
    ] {
        if request.get(field) != event.get(field) {
            return Err(format!("sensor result is stale or mismatched for {field}"));
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
        "inputSha256": event["inputSha256"], "requestDigest": event["requestDigest"],
    }));
    let seen = state["seenSensors"].as_array().unwrap();
    if let Some(previous) = seen.iter().find(|item| item["key"] == key) {
        if previous["assessment"] != event["assessment"]
            || previous["inputDigest"] != event["inputDigest"]
        {
            return Err("conflicting duplicate sensor result refused".into());
        }
        return Ok(());
    }
    state["seenSensors"].as_array_mut().unwrap().push(json!({
        "key": key,
        "inputDigest": event["inputDigest"],
        "assessment": event["assessment"],
    }));
    // Sensor output may only stop earlier. It can never grant READY, reset the
    // budget, or request a retry. Missing/malformed/optimistic output is inert.
    let confidence = event["confidence"].as_f64().unwrap_or(-1.0);
    let transition =
        if event["assessment"] == "block" && (0.0..=1.0).contains(&confidence) && confidence >= 0.9
        {
            state["sensorStop"] = json!("conservative_low_information");
            Transition::SensorBlock
        } else if matches!(
            event["assessment"].as_str(),
            Some("low_information" | "replan" | "evidence")
        ) && (0.0..=1.0).contains(&confidence)
            && confidence >= 0.9
        {
            state["sensorStop"] = json!("conservative_sensor_stop");
            if event["assessment"] == "evidence" {
                Transition::SensorRequireEvidence
            } else {
                Transition::SensorRequireReplan
            }
        } else {
            Transition::SensorInert
        };
    let mut loop_state = loop_state(state)?;
    loop_state.apply(transition)?;
    sync_loop(state, loop_state);
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

#[cfg(test)]
mod tests {
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
}

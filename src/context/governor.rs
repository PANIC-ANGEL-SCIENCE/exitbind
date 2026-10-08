//! Replay the bounded governor state independently from its context projection.

#[cfg(test)]
use super::{checkpoint, event};
use super::{
    recovery_observation, GOVERNOR_VERSION, HARD_ITERATION_BUDGET, NO_INFORMATION_LIMIT,
    POST_REPLAN_LIMIT, SENSOR_VERSION,
};
use crate::evidence::hash;
use crate::kernel::governor::{LoopState, Phase, Transition};
use serde_json::{json, Map, Value};

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
    reduce_governor_seeded(events, None)
}

/// Replay new-run events on a validated accounting seed. Identity, mutation
/// grants, and event-chain authority always begin fresh for the successor.
pub(crate) fn reduce_governor_seeded(
    events: &[Value],
    seed: Option<&Value>,
) -> Result<Value, String> {
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
        "seenEvidenceSha256": [],
        "seenMutations": [],
        "consumedGrants": [],
        "seenSensors": [],
        "currentMutation": Value::Null,
        "currentReplan": Value::Null,
        "currentSensorRequest": Value::Null,
    });
    if let Some(seed) = seed {
        for field in [
            "spent",
            "noInformationStreak",
            "postReplanSpent",
            "replanCount",
            "afterReplan",
            "state",
        ] {
            state[field] = seed
                .get(field)
                .cloned()
                .ok_or_else(|| format!("governor carry is missing {field}"))?;
        }
        if let Some(identities) = seed.get("seenEvidenceSha256") {
            if !identities.is_array()
                || identities.as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        item.as_str().map_or(true, |value| {
                            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                    })
                })
            {
                return Err("governor carry has invalid evidence identities".into());
            }
            state["seenEvidenceSha256"] = identities.clone();
        }
        state["lineageSha256"] = Value::Null;
        state["runId"] = Value::Null;
        state["subjectSha256"] = Value::Null;
        state["attempt"] = Value::Null;
        state["currentMutation"] = Value::Null;
        state["currentReplan"] = Value::Null;
        state["currentSensorRequest"] = Value::Null;
    }
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
            "observation" => recovery_observation::apply(&mut state, event)?,
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

pub(super) fn loop_state(state: &Value) -> Result<LoopState, String> {
    Ok(LoopState {
        phase: Phase::parse(state["state"].as_str().unwrap_or("ready"))?,
        total_mutations: state["spent"].as_u64().unwrap_or_default(),
        no_information_streak: state["noInformationStreak"].as_u64().unwrap_or_default(),
        replanned: state["afterReplan"] == true,
        post_replan_no_information: state["postReplanSpent"].as_u64().unwrap_or_default(),
    })
}

pub(super) fn sync_loop(state: &mut Value, loop_state: LoopState) {
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
        state["currentMutation"] = current_mutation(event);
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
    state["currentMutation"] = current_mutation(event);
    Ok(())
}

fn current_mutation(event: &Value) -> Value {
    json!({
        "runId": event["runId"],
        "eventSha256": event["eventSha256"],
        "carryLineage": event["carryLineage"],
        "subjectSha256": event["subjectSha256"],
        "attempt": event["attempt"],
        "checkpoint": event["checkpoint"],
        "inputSha256": event["inputSha256"],
    })
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

use super::recovery_observation::mutation_evidence;

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
    if state["seenEvidenceSha256"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == &evidence_key))
    {
        return Ok(false);
    }
    state["evidence"].as_array_mut().unwrap().push(evidence);
    state["seenEvidenceSha256"]
        .as_array_mut()
        .unwrap()
        .push(json!(evidence_key));
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
    match event["identityTransition"].as_str() {
        None => bind_identity(state, event)?,
        Some("carried_mutation_v1") => bind_carried_identity(state, event, true)?,
        Some(_) => return Err("evidence identity transition is unsupported".into()),
    }
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
    let identity_transition = event["identityTransition"].as_str();
    if identity_transition == Some("carried_mutation_v1") {
        bind_carried_identity(state, event, false)?;
    } else {
        state["subjectSha256"] = event["subjectSha256"].clone();
        state["attempt"] = event["attempt"].clone();
    }
    Ok(())
}

fn bind_carried_identity(
    state: &mut Value,
    event: &Value,
    require_change: bool,
) -> Result<(), String> {
    let current_mutation = state["currentMutation"]
        .as_object()
        .ok_or("identity transition requires a carried mutation")?;
    if current_mutation["carryLineage"] != true {
        return Err("identity transition requires current mutation to carry lineage".into());
    }
    if event["previousSha256"] != current_mutation["eventSha256"] {
        return Err("identity transition not after current mutation".into());
    }
    if let Some(lineage) = event["lineageSha256"].as_str() {
        if state["lineageSha256"] != lineage {
            return Err("identity transition changed lineage".into());
        }
    }
    let changed =
        state["subjectSha256"] != event["subjectSha256"] || state["attempt"] != event["attempt"];
    if require_change && !changed {
        return Err("evidence identity transition is unnecessary".into());
    }
    if changed {
        let prior_attempt = state["attempt"].as_u64();
        let next_attempt = event["attempt"].as_u64();
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

pub(super) fn bind_identity(state: &mut Value, event: &Value) -> Result<(), String> {
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
#[cfg(test)]
mod tests;

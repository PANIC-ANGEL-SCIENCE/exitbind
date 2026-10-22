//! Governor sensor exchange for the exact pending assignment: record the
//! sensor request the governor currently asks for, then bind the assignee's
//! bounded assessment to that request, both under the assignment's ledger lock.
use super::*;

pub(crate) fn sensor_request_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
) -> Result<Value, String> {
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or("assignment is not currently pending")?;
        if !expected.matches(&assignment) {
            return Err("assignment changed while sensor request was requested".into());
        }
        let request = crate::context::sensor_request(&state["governor"])?;
        if state["governor"]["currentSensorRequest"]["requestDigest"] == request["requestDigest"] {
            if let Some(event) = events.iter().rev().find(|event| {
                event["governorEvent"]["action"] == "sensor_request"
                    && event["governorEvent"]["requestDigest"] == request["requestDigest"]
            }) {
                return Ok(
                    json!({"valid": true, "idempotent": true, "event": event, "governor": state["governor"]}),
                );
            }
        }
        let last = events.last().ok_or("run has no event head")?;
        let previous = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let governor_event = crate::context::event(previous.as_ref(), request);
        let version = state["version"]
            .as_u64()
            .ok_or("run state has no version")?;
        let event = run_state::make_event(json!({
            "version": version, "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern", "runId": state["runId"],
            "stage": assignment["stage"], "attempt": assignment["attempt"],
            "agent": assignment["agent"], "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": live_inputs(&state)?,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": "sensor_request", "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({"valid": true, "event": event, "governor": next_state["governor"]}))
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sensor_result_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    assessment: &str,
    confidence: Option<&str>,
    input_digest: &str,
    identity_source: &str,
) -> Result<Value, String> {
    if !matches!(
        assessment,
        "unavailable" | "low_information" | "replan" | "evidence" | "block" | "ready"
    ) {
        return Err("sensor assessment is outside the bounded schema".into());
    }
    if input_digest.len() != 64
        || !input_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("sensor input-digest must be lowercase SHA-256".into());
    }
    if identity_source != "host-reported" {
        return Err("sensor identity must remain host-reported".into());
    }
    let confidence = confidence
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|_| "sensor confidence is invalid".to_owned())
        })
        .transpose()?;
    if confidence.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err("sensor confidence must be finite and within 0..=1".into());
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or("assignment is not currently pending")?;
        if !expected.matches(&assignment) {
            return Err("assignment changed while sensor result was requested".into());
        }
        let request = state["governor"]["currentSensorRequest"]
            .as_object()
            .ok_or("sensor result has no current request")?;
        let current = state["governor"]["currentMutation"]
            .as_object()
            .ok_or("sensor result has no current mutation")?;
        if let Some(previous) = events.iter().rev().find(|event| {
            event["governorEvent"]["action"] == "sensor"
                && event["governorEvent"]["requestDigest"] == request["requestDigest"]
        }) {
            let prior = &previous["governorEvent"];
            let expected_confidence = confidence.map_or(Value::Null, |value| json!(value));
            if prior["assessment"] == assessment
                && prior["inputDigest"] == input_digest
                && prior["confidence"] == expected_confidence
            {
                return Ok(
                    json!({"valid": true, "idempotent": true, "event": previous, "governor": state["governor"]}),
                );
            }
            return Err("conflicting duplicate sensor result refused".into());
        }
        let last = events.last().ok_or("run has no event head")?;
        let previous = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let mut payload = json!({
            "action": "sensor", "sensorVersion": crate::context::SENSOR_VERSION,
            "runId": state["runId"], "subjectSha256": state["subject"]["sha256"],
            "attempt": state["attempt"], "checkpoint": current["checkpoint"],
            "inputSha256": current["inputSha256"], "inputDigest": input_digest,
            "requestDigest": request["requestDigest"], "assessment": assessment,
            "identitySource": identity_source,
        });
        if let Some(confidence) = confidence {
            payload["confidence"] = json!(confidence);
        } else {
            payload["confidence"] = Value::Null;
        }
        let governor_event = crate::context::event(previous.as_ref(), payload);
        let version = state["version"]
            .as_u64()
            .ok_or("run state has no version")?;
        let event = run_state::make_event(json!({
            "version": version, "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern", "runId": state["runId"],
            "stage": assignment["stage"], "attempt": assignment["attempt"],
            "agent": assignment["agent"], "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": live_inputs(&state)?,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": "sensor", "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({"valid": true, "event": event, "governor": next_state["governor"]}))
    })
}

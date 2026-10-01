//! Recovery information admitted only from a validated observed check.
//!
//! The run reducer constructs these records from its current checked event.
//! A check's wrapper, target hash, log timestamp, and duration are excluded
//! from the novelty key; replay and repeated failures cannot buy more work.

use super::*;

pub(super) fn mutation_evidence(event: &Value) -> Result<Option<Value>, String> {
    if let Some(value) = event.get("newEvidence") {
        if value.is_null() {
            return Ok(None);
        }
        return Ok(Some(value.clone()));
    }
    // The old SHA field is telemetry, not authority to reset a loop.
    Ok(None)
}

pub(super) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    for field in [
        "runId",
        "subjectSha256",
        "attempt",
        "inputSha256",
        "observationKey",
        "checkEventSha256",
    ] {
        if event.get(field).is_none() {
            return Err(format!("recovery observation is missing {field}"));
        }
    }
    for field in ["observationKey", "checkEventSha256"] {
        if !event[field].as_str().is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        }) {
            return Err(format!("recovery observation has invalid {field}"));
        }
    }
    if state["state"] == "blocked" {
        return Err("blocked governor cannot be reopened by observation".into());
    }
    match event["identityTransition"].as_str() {
        None => bind_identity(state, event)?,
        Some("checked_submission_v1") => {
            let current = state["currentMutation"]
                .as_object()
                .ok_or("checked submission transition needs a current mutation")?;
            if current.get("carryLineage") != Some(&json!(true))
                || event["previousSha256"] != current["eventSha256"]
                || event["runId"] != state["runId"]
                || event["attempt"] != state["attempt"]
                || event["subjectSha256"] == state["subjectSha256"]
                || event["lineageSha256"] != state["lineageSha256"]
            {
                return Err("checked submission transition is not bound to its mutation".into());
            }
            state["subjectSha256"] = event["subjectSha256"].clone();
        }
        Some(_) => return Err("recovery observation identity transition is unsupported".into()),
    }
    let key = &event["observationKey"];
    if state["observations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["key"] == *key))
    {
        return Err("duplicate recovery observation does not extend the bound".into());
    }
    if !state["observations"].is_array() {
        state["observations"] = json!([]);
    }
    state["observations"].as_array_mut().unwrap().push(json!({
        "key": key,
        "checkEventSha256": event["checkEventSha256"],
    }));
    let mut loop_state = loop_state(state)?;
    loop_state.apply(Transition::NewEvidence)?;
    sync_loop(state, loop_state);
    Ok(())
}

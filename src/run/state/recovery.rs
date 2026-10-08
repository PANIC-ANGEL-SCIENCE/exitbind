//! Derived R18 recovery information on explicitly marked new starts.
//!
//! Source events remain the canonical facts. These governor projections are
//! recomputed during replay, so a Lead decision and its recovery effect cannot
//! be half committed. Historical starts without `recoveryProtocol` replay as
//! they did before this extension.

use super::*;

pub(super) fn append_governor(state: &mut Value, governor_event: Value) -> Result<(), String> {
    let mut events = state["governorEvents"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    events.push(governor_event);
    let grant_protocol = state["governor"]["grantProtocol"].clone();
    let defaults = state["governor"]["defaults"].clone();
    let seed = state.get("governorCarry").map(|carry| &carry["accounting"]);
    let mut next = crate::context::reduce_governor_seeded(&events, seed)?;
    next["enabled"] = json!(true);
    if !grant_protocol.is_null() {
        next["grantProtocol"] = grant_protocol;
    }
    if state["recoveryProtocol"] == RECOVERY_PROTOCOL_VERSION
        || state.get("governorCarry").is_some()
    {
        next["defaults"] = defaults;
    }
    state["governor"] = next;
    state["governorEvents"] = Value::Array(events);
    Ok(())
}

/// A Lead repair recorded before the permit that exhausted the streak can
/// supply one forward re-plan. The original disposition is never replayed as
/// a new decision: only the still-current assignment may consume it once.
pub(crate) fn forward_repair_candidate(
    state: &Value,
    prior: &[Value],
    assignment: &Value,
) -> Option<Value> {
    let governor = &state["governor"];
    let repair = state.get("authorizedRepair")?.as_object()?;
    let sha = repair.get("dispositionSha256")?.as_str()?;
    if state["status"] != "running"
        || governor["enabled"] != true
        || governor["state"] != "replan_required"
        || governor["grantProtocol"] != crate::context::GRANT_PROTOCOL_VERSION
        || governor["headSha256"] != governor["currentMutation"]["eventSha256"]
        || governor["currentMutation"]["carryLineage"] != true
        || governor["currentMutation"]["attempt"] != state["attempt"]
        || governor["currentMutation"]["subjectSha256"] != state["subject"]["sha256"]
        || !state["pendingDisposition"].is_null()
        || assignment["role"] != "worker"
        || assignment["stage"] != state["currentStage"]
        || assignment["attempt"] != state["attempt"]
        || repair.get("attempt") != Some(&state["attempt"])
        || repair.get("basisSha256") != Some(&state["basis"]["sha256"])
        || !matches!(
            repair.get("decision").and_then(Value::as_str),
            Some("repair" | "supersede")
        )
        || !repair.get("reason").is_some_and(Value::is_string)
        || !repair.get("repairBoundary").is_some_and(Value::is_string)
        || !repair
            .get("decisiveRegression")
            .is_some_and(Value::is_string)
        || governor["lineageSha256"].as_str().is_none()
    {
        return None;
    }
    let disposition = prior.iter().rposition(|event| {
        event["action"] == "submit"
            && event["role"] == "lead"
            && event["outcome"] == "disposition"
            && event["disposition"]["sha256"] == sha
    })?;
    if prior[disposition + 1..]
        .iter()
        .any(|event| event["governorEvent"]["action"] == "replan")
    {
        return None;
    }
    Some(Value::Object(repair.clone()))
}

/// Validate the new event against the pre-event state, before the governor
/// reducer applies its re-plan. Markerless historical events remain unchanged.
pub(super) fn validate_forward_repair(
    state: &Value,
    prior: &[Value],
    event: &Value,
) -> Result<(), String> {
    let nested = &event["governorEvent"];
    if event["operation"] != "authorized_repair_recovery_v1"
        && nested.get("repairRecoveryProtocol").is_none()
    {
        return Ok(());
    }
    let assignment = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| item["agent"] == event["agent"])
        .ok_or("forward repair has no current worker assignment")?;
    let repair = forward_repair_candidate(state, prior, &assignment)
        .ok_or("forward repair has no current unused Lead decision")?;
    if event["action"] != "govern"
        || event["role"] != "worker"
        || event["operation"] != "authorized_repair_recovery_v1"
        || event["stage"] != assignment["stage"]
        || event["attempt"] != state["attempt"]
        || event["subjectSha256"] != state["subject"]["sha256"]
        || event["inputsSha256"] != nested["inputSha256"]
        || nested["action"] != "replan"
        || nested["repairRecoveryProtocol"] != 1
        || nested["dispositionSha256"] != repair["dispositionSha256"]
        || nested["repairDecision"] != repair["decision"]
        || nested["hypothesis"] != repair["reason"]
        || nested["scopeDecision"] != repair["repairBoundary"]
        || nested["evidenceRequest"] != repair["decisiveRegression"]
        || nested.get("blocker").is_some()
        || nested["identityTransition"] != "carried_mutation_v1"
        || nested["assignmentPacketSha256"] != crate::evidence::hash::value(&assignment)
        || nested["checkpoint"] != state["governor"]["spent"]
        || nested["lineageSha256"] != state["governor"]["lineageSha256"]
        || nested["previousSha256"] != state["governor"]["headSha256"]
    {
        return Err("forward repair is stale or not bound to its Lead decision".into());
    }
    Ok(())
}

pub(super) fn after_event(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["recoveryProtocol"] != RECOVERY_PROTOCOL_VERSION
        || state["governor"]["enabled"] != true
    {
        return Ok(());
    }
    if event["action"] == "check" {
        return observed_check(state, event);
    }
    if event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "disposition" {
        return lead_repair(state, event);
    }
    Ok(())
}

fn observed_check(state: &mut Value, event: &Value) -> Result<(), String> {
    // Reported, synthetic, stale, or invalid checks cannot supply information.
    // apply_check has already validated the frozen command, target, and input.
    if event["acquisition"] != "observed"
        || event["origin"] != "local_report"
        || state["governor"]["state"] == "blocked"
    {
        return Ok(());
    }
    let result = &event["result"];
    let status = match result["kind"].as_str() {
        Some("exit") if result["code"].as_u64().is_some() => json!({"exit": result["code"]}),
        Some("signal") if result["signal"].as_u64().is_some() => {
            json!({"signal": result["signal"]})
        }
        _ => return Ok(()),
    };
    // Target names, artifacts, log contents, durations, timestamps and wrapper
    // text are deliberately absent. The same failure stays the same finding.
    let key = crate::evidence::hash::value(&json!({
        "commandSha256": event["checkCommandSha256"],
        "status": status,
    }));
    if state["governor"]["observations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["key"] == key))
    {
        return Ok(());
    }
    let previous = state["governor"]["headSha256"]
        .as_str()
        .map(|sha| json!({"eventSha256": sha}));
    let mut payload = json!({
            "action": "observation",
            "runId": state["runId"],
            "subjectSha256": event["subjectSha256"],
            "attempt": state["attempt"],
            "inputSha256": event["inputsSha256"],
            "observationKey": key,
            "checkEventSha256": event["eventSha256"],
    });
    if state["governor"]["subjectSha256"] != event["subjectSha256"]
        || state["governor"]["attempt"] != state["attempt"]
    {
        payload["identityTransition"] = json!("checked_submission_v1");
        payload["lineageSha256"] = state["governor"]["lineageSha256"].clone();
        payload["targetEventSha256"] = event["targetEventSha256"].clone();
    }
    let derived = crate::context::event(previous.as_ref(), payload);
    append_governor(state, derived)
}

fn lead_repair(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["governor"]["state"] != "replan_required"
        || !matches!(
            event["disposition"]["decision"].as_str(),
            Some("repair" | "supersede")
        )
        || !state["governor"]["currentMutation"].is_object()
        || state["governor"]["headSha256"] != state["governor"]["currentMutation"]["eventSha256"]
    {
        return Ok(());
    }
    let previous = json!({"eventSha256": state["governor"]["headSha256"]});
    let derived = crate::context::event(
        Some(&previous),
        json!({
            "action": "replan",
            "identityTransition": "carried_mutation_v1",
            "runId": state["runId"],
            "subjectSha256": state["subject"]["sha256"],
            "attempt": state["attempt"],
            "checkpoint": state["governor"]["spent"],
            "inputSha256": state["inputsSha256"],
            "lineageSha256": state["governor"]["lineageSha256"],
            "hypothesis": event["disposition"]["reason"],
            "scopeDecision": event["disposition"]["repairBoundary"],
            "evidenceRequest": event["disposition"]["decisiveRegression"],
            "dispositionSha256": event["disposition"]["sha256"],
        }),
    );
    append_governor(state, derived)
}

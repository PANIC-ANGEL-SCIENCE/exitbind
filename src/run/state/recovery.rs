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
    let mut next = crate::context::reduce_governor(&events)?;
    next["enabled"] = json!(true);
    if !grant_protocol.is_null() {
        next["grantProtocol"] = grant_protocol;
    }
    if state["recoveryProtocol"] == RECOVERY_PROTOCOL_VERSION {
        next["defaults"] = defaults;
    }
    state["governor"] = next;
    state["governorEvents"] = Value::Array(events);
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

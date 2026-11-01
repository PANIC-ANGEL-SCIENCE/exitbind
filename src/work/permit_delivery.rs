//! Verify the single governor transition following an actual host delivery.

use crate::{config::Loaded, evidence::hash, run, session_goal};
use serde_json::Value;

pub(crate) struct VerifiedPermit {
    pub before: Value,
    pub current: Value,
    pub facts: Value,
    pub form: Value,
    pub event: Value,
}

/// Find the unique currently authorized worker grant using the existing
/// validated ledger and grant owner. This does not itself admit a result.
pub(crate) fn current_worker_grant(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    current: &Value,
) -> Result<Option<Value>, String> {
    if current["role"] != "worker" || current["assignment"] != assignment {
        return Ok(None);
    }
    super::details::ensure_current_config(loaded)?;
    let ledger = super::resolve(loaded, work)?;
    let (_, events, _) = run::ledger::load(loaded, &ledger)?;
    let grants = events
        .iter()
        .filter(|event| super::permit_response::is_current_grant_event(event, current, assignment))
        .collect::<Vec<_>>();
    let [event] = grants[..] else {
        return Ok(None);
    };
    if !event["requestId"].is_string()
        || event["requestId"] != event["governorEvent"]["requestId"]
        || !event["eventSha256"].is_string()
        || !event["governorEvent"]["eventSha256"].is_string()
    {
        return Ok(None);
    }
    Ok(Some((*event).clone()))
}

/// A permit is already recorded when this runs. Failure never changes its
/// authority; it leaves the host capture ineligible for automatic return.
pub(super) fn carry(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    ledger: &str,
    permission: &Value,
) -> Result<Option<bool>, String> {
    if !session_goal::has_presented_child(loaded, work, assignment)? {
        return Ok(None);
    }
    if permission["allowed"] != true {
        return Ok(Some(false));
    }
    let Some(verified) = verified_transition(
        loaded,
        work,
        assignment,
        ledger,
        Some(&permission["event"]),
        false,
    )?
    else {
        return Ok(Some(false));
    };
    let form_sha256 = hash::value(&verified.form);
    let proof = session_goal::PermitTransition {
        work,
        assignment,
        presented_binding: verified.before["current"]["binding"].as_str().unwrap(),
        form_sha256: &form_sha256,
        request_id: super::action_forms::permit_request_id(&verified.form).unwrap(),
        required_facts_sha256: verified.facts["requiredFactsSha256"].as_str().unwrap(),
        recipient_sha256: verified.facts["recipientSha256"].as_str().unwrap(),
        successor_binding: verified.current["current"]["binding"].as_str().unwrap(),
        run_event_sha256: verified.event["eventSha256"].as_str().unwrap(),
        governor_event_sha256: verified.event["governorEvent"]["eventSha256"]
            .as_str()
            .unwrap(),
    };
    session_goal::advance_child_delivery(loaded, &proof).map(Some)
}

/// Reconstruct and validate the one adjacent governor grant using the
/// existing ledger, snapshot, binding and action-form owners.
pub(crate) fn verified_transition(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    ledger: &str,
    expected_event: Option<&Value>,
    managed: bool,
) -> Result<Option<VerifiedPermit>, String> {
    super::details::ensure_current_config(loaded)?;
    let (_, events, source) = run::ledger::load(loaded, ledger)?;
    let Some((event, prefix)) = events.split_last() else {
        return Ok(None);
    };
    if expected_event.is_some_and(|expected| expected != event)
        || event["action"] != "govern"
        || event["governorEvent"]["action"] != "mutation"
    {
        return Ok(None);
    }
    let snapshot = run::RunSnapshot::from_events(loaded, &events, &source)?;
    let current = super::next_from(loaded, work, &snapshot)?;
    if !super::permit_response::is_current_grant_event(event, &current, assignment) {
        return Ok(None);
    }
    let Some(pre_edit_inputs) = event["inputsSha256"].as_str() else {
        return Ok(None);
    };
    let suffix = format!(
        "{}\n",
        serde_json::to_string(event).map_err(|e| e.to_string())?
    );
    let Some(previous_source) = source.strip_suffix(&suffix) else {
        return Ok(None);
    };
    let previous = run::RunSnapshot::from_events(loaded, prefix, previous_source)?;
    let before = super::next_from(loaded, work, &previous)?;
    let Some(presented_binding) = before["current"]["binding"].as_str() else {
        return Ok(None);
    };
    if before["action"] != "spawn"
        || before["role"] != "worker"
        || before["assignment"] != assignment
    {
        return Ok(None);
    }
    let facts = super::details::native_snapshot_facts(
        loaded,
        work,
        &previous,
        &before,
        pre_edit_inputs,
        managed,
    )?;
    if facts["binding"] != presented_binding {
        return Ok(None);
    }
    if !facts["requiredFactsSha256"].is_string() || !facts["recipientSha256"].is_string() {
        return Ok(None);
    }
    let forms = super::action_forms::full(loaded, work, &before, presented_binding);
    let form = &forms["beforeEditing"];
    let Some(request_id) = super::action_forms::permit_request_id(form) else {
        return Ok(None);
    };
    if event["requestId"] != request_id || event["governorEvent"]["requestId"] != request_id {
        return Ok(None);
    }
    let (Some(_successor), Some(_run_event), Some(_governor_event)) = (
        current["current"]["binding"].as_str(),
        event["eventSha256"].as_str(),
        event["governorEvent"]["eventSha256"].as_str(),
    ) else {
        return Ok(None);
    };
    let form = form.clone();
    Ok(Some(VerifiedPermit {
        before,
        current,
        facts,
        form,
        event: event.clone(),
    }))
}

/// At return, re-read the validated current ledger and exact grant rather than
/// treating a saved successor string as authority on its own.
pub(crate) fn validate_successor(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    current: &Value,
    delivery: &Value,
) -> Result<bool, String> {
    let successor = &delivery["authorizedSuccessor"];
    let form = &delivery["permitForm"];
    if !delivery["binding"].is_string()
        || !form["sha256"].is_string()
        || !form["requestId"].is_string()
        || successor["binding"] != current["current"]["binding"]
    {
        return Ok(false);
    }
    super::details::ensure_current_config(loaded)?;
    let ledger = super::resolve(loaded, work)?;
    let (_, events, _) = run::ledger::load(loaded, &ledger)?;
    let Some(event) = events.last() else {
        return Ok(false);
    };
    Ok(event["eventSha256"] == successor["runEventSha256"]
        && event["governorEvent"]["eventSha256"] == successor["governorEventSha256"]
        && event["requestId"] == form["requestId"]
        && event["governorEvent"]["requestId"] == form["requestId"]
        && super::permit_response::is_current_grant_event(event, current, assignment))
}

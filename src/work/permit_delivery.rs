// Verifies governor transitions after an actual host delivery.
// A current Worker grant must be unique; see `src/work/mod.rs`.
//! Verify the single governor transition following an actual host delivery.

use crate::{config::Loaded, evidence::hash, run, session_goal};
use serde_json::{json, Value};

pub(crate) struct VerifiedPermit {
    pub before: Value,
    pub current: Value,
    pub facts: Value,
    pub form: Value,
    pub event: Value,
}

/// A legacy grant may omit a retry identity. Preserve exact paired absence;
/// it is never a wildcard for a present, partial or malformed identity.
pub(crate) fn grant_request_identity(event: &Value) -> Option<Value> {
    let nested = &event["governorEvent"];
    match (
        event.get("requestId"),
        event.get("requestDigest"),
        nested.get("requestId"),
        nested.get("requestDigest"),
    ) {
        (None, None, None, None) => Some(json!({"state":"absent"})),
        (Some(Value::String(id)), Some(Value::String(digest)), Some(other), Some(other_digest))
            if !id.trim().is_empty()
                && id.len() <= crate::run::REQUEST_ID_MAX_BYTES
                && !id.contains('\0')
                && digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                && other.as_str() == Some(id.as_str())
                && other_digest.as_str() == Some(digest.as_str()) =>
        {
            Some(json!({"state":"present","id":id,"digest":digest}))
        }
        _ => None,
    }
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
    if grant_request_identity(event).is_none()
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
        actor_facts_sha256: verified.facts["actorFactsSha256"].as_str(),
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
    let exact_event = event["eventSha256"] == successor["runEventSha256"]
        && event["governorEvent"]["eventSha256"] == successor["governorEventSha256"]
        && event["requestId"] == form["requestId"]
        && event["governorEvent"]["requestId"] == form["requestId"]
        && super::permit_response::is_current_grant_event(event, current, assignment);
    if !exact_event {
        return Ok(false);
    }
    if let Some(expected) = delivery.get("actorFactsSha256") {
        let Some(expected) = expected.as_str() else {
            return Ok(false);
        };
        let Some(verified) =
            verified_transition(loaded, work, assignment, &ledger, Some(event), false)?
        else {
            return Ok(false);
        };
        let form_sha256 = hash::value(&verified.form);
        return Ok(verified.facts["actorFactsSha256"] == expected
            && verified.facts["requiredFactsSha256"] == delivery["requiredFactsSha256"]
            && verified.facts["recipientSha256"] == delivery["recipientSha256"]
            && verified.before["current"]["binding"] == delivery["binding"]
            && form["sha256"].as_str() == Some(form_sha256.as_str()));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_grant_request_identity_distinguishes_absence_from_malformed_or_mismatched() {
        let absent = json!({"governorEvent":{}});
        assert_eq!(
            grant_request_identity(&absent),
            Some(json!({"state":"absent"}))
        );
        let digest = "a".repeat(64);
        let present = json!({"requestId":"request-a","requestDigest":digest,
            "governorEvent":{"requestId":"request-a","requestDigest":digest}});
        assert_eq!(
            grant_request_identity(&present),
            Some(json!({"state":"present","id":"request-a","digest":digest}))
        );
        for broken in [
            json!({"requestId":null,"governorEvent":{}}),
            json!({"requestId":"request-a","governorEvent":{}}),
            json!({"governorEvent":{"requestId":"request-a"}}),
            json!({"requestId":"request-a","requestDigest":digest,
                "governorEvent":{"requestId":"request-b","requestDigest":digest}}),
            json!({"requestId":"request-a","requestDigest":digest,
                "governorEvent":{"requestId":"request-a","requestDigest":"b".repeat(64)}}),
            json!({"requestId":"request-a","requestDigest":"malformed",
                "governorEvent":{"requestId":"request-a","requestDigest":"malformed"}}),
        ] {
            assert!(grant_request_identity(&broken).is_none(), "{broken}");
        }
    }
}

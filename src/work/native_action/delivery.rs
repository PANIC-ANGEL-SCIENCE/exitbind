//! Recheck the exact delivered assignment before admitting a native result.

use crate::{config::Loaded, evidence::hash, work};
use serde_json::{json, Value};

fn goal_sha256(loaded: &Loaded, work_id: &str) -> String {
    hash::value(&json!(crate::session_goal::record_for_work(
        loaded, work_id
    )))
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn same_grant(event: &Value, delivered: &Value) -> bool {
    delivered["state"] == "current_for_this_worker_assignment"
        && delivered["governorEventSha256"] == event["governorEvent"]["eventSha256"]
}

/// Capture the actual complete packet and, when already present, the exact
/// authorized worker grant which was presented with it.
pub(super) fn proof(
    loaded: &Loaded,
    work_id: &str,
    current: &Value,
    selected: &Value,
) -> Result<Value, String> {
    let assignment = current["assignment"]
        .as_str()
        .ok_or("native assignment is unavailable")?;
    let mut result = json!({
        "binding": selected["binding"],
        "requiredFactsSha256": hash::value(&selected["sections"]),
        "recipientViewSha256": selected["recipientSha256"],
        "goalSha256": goal_sha256(loaded, work_id),
    });
    let original_input = selected["sections"]
        .pointer("/assignment/residual/snapshot/inputsSha256")
        .and_then(Value::as_str)
        .filter(|value| valid_sha(value));
    if let (Some(input), Some(event)) = (
        original_input,
        work::permit_delivery::current_worker_grant(loaded, work_id, assignment, current)?,
    ) {
        if same_grant(
            &event,
            &selected["sections"]["currentAction"]["currentGrant"],
        ) {
            result["currentGrant"] = json!({
                "runEventSha256":event["eventSha256"],
                "governorEventSha256":event["governorEvent"]["eventSha256"],
                "requestId":event["requestId"],
                "presentedInputsSha256":input,
            });
        }
    }
    Ok(result)
}

/// Re-observe the full delivered facts just before launching a new request.
pub(super) fn validate_before_launch(
    loaded: &Loaded,
    work_id: &str,
    expected: &Value,
) -> Result<(), String> {
    let current = work::current_action(loaded, work_id)?;
    let selected = work::details::managed_required_assignment(loaded, work_id, &current)?;
    if proof(loaded, work_id, &current, &selected)? != *expected {
        return Err("native assignment or delivered facts changed before launch".into());
    }
    Ok(())
}

fn facts_match_after_current_grant(
    loaded: &Loaded,
    work_id: &str,
    current: &Value,
    selected: &Value,
    expected: &Value,
) -> Result<bool, String> {
    let grant = &expected["currentGrant"];
    let Some(input) = grant["presentedInputsSha256"]
        .as_str()
        .filter(|v| valid_sha(v))
    else {
        return Ok(false);
    };
    let Some(assignment) = current["assignment"].as_str() else {
        return Ok(false);
    };
    let Some(event) =
        work::permit_delivery::current_worker_grant(loaded, work_id, assignment, current)?
    else {
        return Ok(false);
    };
    if grant["runEventSha256"] != event["eventSha256"]
        || grant["governorEventSha256"] != event["governorEvent"]["eventSha256"]
        || grant["requestId"] != event["requestId"]
        || !same_grant(
            &event,
            &selected["sections"]["currentAction"]["currentGrant"],
        )
    {
        return Ok(false);
    }
    let mut historical = selected["sections"].clone();
    let Some(observation) = historical
        .pointer_mut("/assignment/residual/snapshot/inputsSha256")
        .filter(|value| value.is_string())
    else {
        return Ok(false);
    };
    // Historical delivery comparison only. The current Work result, check,
    // and input observations are never rewritten or replaced.
    *observation = json!(input);
    Ok(expected["requiredFactsSha256"] == hash::value(&historical))
}

fn saved_identity_matches(
    loaded: &Loaded,
    work_id: &str,
    journal: &Value,
    assignment: &str,
    role: &str,
    agent: &str,
    permit_bridge: bool,
) -> Result<bool, String> {
    if let Some(identity) =
        super::journal::pre_return_replay_identity(loaded, work_id, journal, assignment)?
    {
        return Ok(super::verify_journal(journal, work_id, &identity, role, agent).is_ok());
    }
    if permit_bridge {
        if let Some(identity) =
            super::journal::permit_replay_identity(loaded, work_id, journal, assignment)?
        {
            return Ok(super::verify_journal(journal, work_id, &identity, role, agent).is_ok());
        }
    }
    Ok(false)
}

pub(super) fn submit_saved_checked(
    loaded: &Loaded,
    work_id: &str,
    assignment: &str,
    bytes: Vec<u8>,
    expected_binding: Option<&str>,
    journal: &Value,
) -> Result<Value, String> {
    // Already admitted exact bytes can be recovered without any new credit.
    if let Ok(replayed) = work::replay_recorded_native(loaded, work_id, assignment, journal, &bytes)
    {
        return Ok(replayed);
    }
    let current = work::current_action(loaded, work_id)?;
    let binding = current["current"]["binding"]
        .as_str()
        .ok_or("current native result binding is unavailable")?;
    let role = current["role"]
        .as_str()
        .ok_or("native role is unavailable")?;
    let agent = current["agent"]
        .as_str()
        .ok_or("native agent is unavailable")?;
    if current["assignment"] != assignment
        || expected_binding.is_some_and(|expected| expected != binding)
    {
        return Err(
            "native result assignment or binding changed; retained result has no automatic credit"
                .into(),
        );
    }
    let identity = super::assignment_identity(&current)?;
    if journal.get("recipientProofVersion").is_some() {
        if journal["recipientProofVersion"] != 1 {
            return Err("native recipient delivery proof version is unsupported".into());
        }
        let recipient = journal["recipientSha256"].as_str().ok_or(
            "native recipient delivery proof is unavailable; retained result has no automatic credit",
        )?;
        if work::details::managed_recipient_fingerprint(loaded, &current)? != recipient {
            return Err("native recipient rules, rights or guidance changed; retained result has no automatic credit".into());
        }
    }
    let delivery = journal.get("requiredDeliveryProof");
    let version = journal.get("deliveryProofVersion");
    let allowed =
        if version.and_then(Value::as_u64) == Some(1) && delivery.is_some_and(Value::is_object) {
            let delivery = delivery.expect("checked delivery proof");
            if !delivery["binding"].as_str().is_some_and(valid_sha)
                || !delivery["requiredFactsSha256"]
                    .as_str()
                    .is_some_and(valid_sha)
                || !delivery["recipientViewSha256"]
                    .as_str()
                    .is_some_and(valid_sha)
                || delivery["goalSha256"] != goal_sha256(loaded, work_id)
            {
                false
            } else if delivery["binding"] == binding {
                if super::verify_journal(journal, work_id, &identity, role, agent).is_err() {
                    false
                } else {
                    let selected =
                        work::details::managed_required_assignment(loaded, work_id, &current)?;
                    delivery["recipientViewSha256"] == selected["recipientSha256"]
                        && (delivery["requiredFactsSha256"] == hash::value(&selected["sections"])
                            || facts_match_after_current_grant(
                                loaded, work_id, &current, &selected, delivery,
                            )?)
                }
            } else {
                let permit = &journal["permitDeliveryProof"];
                let permit_bridge = permit.is_object()
                    && permit["binding"] == delivery["binding"]
                    && permit["requiredFactsSha256"] == delivery["requiredFactsSha256"]
                    && permit["recipientViewSha256"] == delivery["recipientViewSha256"];
                saved_identity_matches(
                    loaded,
                    work_id,
                    journal,
                    assignment,
                    role,
                    agent,
                    permit_bridge,
                )?
            }
        } else if version.is_none() && delivery.is_none() {
            // Old unadmitted journals keep only their existing exact packet or
            // already-owned pre-return recovery; no new grant bridge is inferred.
            super::verify_journal(journal, work_id, &identity, role, agent).is_ok()
                || saved_identity_matches(loaded, work_id, journal, assignment, role, agent, false)?
        } else {
            false
        };
    if !allowed {
        return Err("native delivered binding or complete facts changed; retained result has no automatic credit".into());
    }
    super::submit_saved(loaded, work_id, assignment, bytes, Some(binding))
}

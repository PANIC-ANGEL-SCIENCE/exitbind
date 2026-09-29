//! Validate governor records and acknowledgements against their bound events.

use super::*;

pub(super) fn validate_grant_acknowledgement(
    state: &Value,
    prior_events: &[Value],
    event: &Value,
    governor_event: &Value,
) -> Result<(), String> {
    let grants = governor_event["grantEventSha256s"]
        .as_array()
        .ok_or("grant acknowledgement is missing")?;
    let sources = governor_event["sourceInputsSha256s"]
        .as_array()
        .ok_or("grant source inputs are missing")?;
    let mode = governor_event["authorizationMode"]
        .as_str()
        .ok_or("grant authorization mode is missing")?;
    if governor_event["action"] != "mutation" || event["action"] != "submit" {
        return Err("grant acknowledgement is only valid on a worker mutation completion".into());
    }
    let assignment = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| item["agent"] == event["agent"])
        .ok_or("grant acknowledgement has no current assignment")?;
    if assignment["role"] != "worker"
        || event["role"] != "worker"
        || event["outcome"] != "completed"
        || event["stage"] != assignment["stage"]
        || event["attempt"] != assignment["attempt"]
    {
        return Err("grant acknowledgement is not bound to the current worker".into());
    }
    let assignment_sha256 = event["assignmentSha256"].as_str();
    let subject = state["subject"]["sha256"].clone();
    let result_input = event["inputsSha256"].clone();
    let lineage = state["governor"]["lineageSha256"].clone();
    let mut expected = Vec::new();
    let mut already_acknowledged = std::collections::BTreeSet::new();
    for prior in prior_events {
        if let Some(previous) = prior["governorEvent"]["grantEventSha256s"].as_array() {
            already_acknowledged.extend(previous.iter().filter_map(Value::as_str));
        }
    }
    for prior in prior_events {
        let nested = &prior["governorEvent"];
        if prior["action"] == "govern"
            && prior["runId"] == state["runId"]
            && prior["subjectSha256"] == subject
            && prior["stage"] == assignment["stage"]
            && prior["attempt"] == assignment["attempt"]
            && prior["agent"] == assignment["agent"]
            && prior["role"] == assignment["role"]
            && nested["action"] == "mutation"
            && nested["runId"] == state["runId"]
            && nested["subjectSha256"] == subject
            && nested["attempt"] == state["attempt"]
            && nested["inputSha256"] == prior["inputsSha256"]
            && nested["lineageSha256"] == lineage
            && nested["carryLineage"] == true
            && assignment_sha256.map_or(true, |value| prior["assignmentSha256"] == value)
        {
            let hash = prior["eventSha256"]
                .as_str()
                .ok_or("mutation grant has no outer event identity")?;
            if !already_acknowledged.contains(hash) {
                expected.push((
                    hash.to_owned(),
                    nested["inputSha256"]
                        .as_str()
                        .ok_or("mutation grant has no source input identity")?
                        .to_owned(),
                ));
            }
        }
    }
    let mut actual = grants
        .iter()
        .zip(sources)
        .map(|(grant, source)| {
            Ok((
                grant
                    .as_str()
                    .ok_or("grant reference is malformed")?
                    .to_owned(),
                source
                    .as_str()
                    .ok_or("grant source input is malformed")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if grants.len() != sources.len() {
        return Err("grant references and source inputs have different lengths".into());
    }
    expected.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    actual.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    match mode {
        "explicit" if assignment_sha256.is_none() || expected.is_empty() || expected != actual => {
            return Err("mutation completion does not acknowledge exact outstanding grants".into())
        }
        "implicit" if !expected.is_empty() || !actual.is_empty() => {
            return Err("implicit completion has outstanding explicit grants".into())
        }
        "explicit" | "implicit" => {}
        _ => return Err("grant authorization mode is invalid".into()),
    }
    if governor_event["inputSha256"] != result_input {
        return Err("completion governor input does not match result input".into());
    }
    if state["governor"]["state"] != "ready" {
        return Err("mutation grant completion requires a ready governor".into());
    }
    Ok(())
}

pub(super) fn validate_governor_event(event: &Value, line: usize) -> Result<(), String> {
    if event["version"].as_u64() < Some(6) {
        return Err(format!(
            "invalid run ledger line {line}: governor action requires v6"
        ));
    }
    for field in ["stage", "attempt"] {
        if event[field].as_u64().map_or(true, |value| value < 1) {
            return Err(format!(
                "invalid run ledger line {line}: invalid governor stage or attempt"
            ));
        }
    }
    if event["agent"]
        .as_str()
        .map_or(true, |value| value.trim().is_empty())
        || !ROLES.contains(&event["role"].as_str().unwrap_or(""))
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid governor actor"
        ));
    }
    for field in ["subjectSha256", "inputsSha256", "assignmentSha256"] {
        if !is_sha(event[field].as_str()) {
            return Err(format!(
                "invalid run ledger line {line}: governor binding is missing {field}"
            ));
        }
    }
    if event["operation"]
        .as_str()
        .map_or(true, |value| value.trim().is_empty() || value.len() > 120)
    {
        return Err(format!(
            "invalid run ledger line {line}: governor operation is invalid"
        ));
    }
    let request_id = event.get("requestId");
    let request_digest = event.get("requestDigest");
    if request_id.is_some() != request_digest.is_some()
        || request_id.is_some_and(|value| {
            value.as_str().map_or(true, |value| {
                value.trim().is_empty()
                    || value.len() > crate::run::REQUEST_ID_MAX_BYTES
                    || value.contains('\0')
            })
        })
        || request_digest.is_some_and(|value| !is_sha(value.as_str()))
    {
        return Err(format!(
            "invalid run ledger line {line}: governor request identity is malformed"
        ));
    }
    let Some(governor_event) = event.get("governorEvent") else {
        return Err(format!(
            "invalid run ledger line {line}: governor event is missing"
        ));
    };
    if governor_event.get("requestId") != request_id
        || governor_event.get("requestDigest") != request_digest
    {
        return Err(format!(
            "invalid run ledger line {line}: governor request identity does not bind to the action"
        ));
    }
    if let (Some(request_id), Some(request_digest)) = (
        request_id.and_then(Value::as_str),
        request_digest.and_then(Value::as_str),
    ) {
        let expected = crate::run::governor_request_digest(
            event["runId"].as_str().unwrap_or_default(),
            event["stage"].as_u64().unwrap_or_default(),
            event["attempt"].as_u64().unwrap_or_default(),
            event["agent"].as_str().unwrap_or_default(),
            event["role"].as_str().unwrap_or_default(),
            event["subjectSha256"].as_str().unwrap_or_default(),
            event["inputsSha256"].as_str().unwrap_or_default(),
            event["assignmentSha256"].as_str().unwrap_or_default(),
            event["operation"].as_str().unwrap_or_default(),
            request_id,
        );
        if request_digest != expected {
            return Err(format!(
                "invalid run ledger line {line}: governor request digest does not match its binding"
            ));
        }
    }
    let action = governor_event["action"].as_str().unwrap_or_default();
    if governor_event["runId"] != event["runId"]
        || governor_event["subjectSha256"] != event["subjectSha256"]
        || governor_event["attempt"] != event["attempt"]
        || governor_event["inputSha256"] != event["inputsSha256"]
    {
        return Err(format!(
            "invalid run ledger line {line}: governor event does not bind to the action"
        ));
    }
    match action {
        "mutation" => {
            if governor_event["operation"] != event["operation"]
                || governor_event["carryLineage"] != true
            {
                return Err(format!(
                    "invalid run ledger line {line}: governor mutation does not bind to the action"
                ));
            }
        }
        "replan" => {
            if ["hypothesis", "evidenceRequest", "scopeDecision", "blocker"]
                .iter()
                .all(|field| governor_event.get(*field).is_none())
            {
                return Err(format!(
                    "invalid run ledger line {line}: material re-plan is missing semantic fields"
                ));
            }
        }
        "evidence" => {
            if governor_event.get("evidence").is_none() {
                return Err(format!(
                    "invalid run ledger line {line}: governor evidence is missing"
                ));
            }
        }
        "sensor_request" => {
            if governor_event["requestDigest"].as_str().is_none()
                || governor_event["questions"].as_array().is_none()
            {
                return Err(format!(
                    "invalid run ledger line {line}: sensor request is malformed"
                ));
            }
        }
        "sensor" => {
            if governor_event["requestDigest"].as_str().is_none()
                || governor_event["assessment"].as_str().is_none()
            {
                return Err(format!(
                    "invalid run ledger line {line}: sensor result is malformed"
                ));
            }
        }
        "blocked" => {}
        _ => {
            return Err(format!(
                "invalid run ledger line {line}: unknown governor event action"
            ));
        }
    }
    Ok(())
}

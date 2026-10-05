//! Check and protection event validation and binding.

use super::*;

pub(crate) fn validate_check_event(event: &Value, line: usize) -> Result<(), String> {
    if matches!(event["version"].as_u64(), Some(4..=8)) {
        return validate_check_event_v4(event, line);
    }
    let record: CheckObservation = serde_json::from_value(event.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed check event"))?;
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: check event must be an object"))?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "targetEventSha256",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "exitCode",
        "durationMs",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "check")?;
    let required = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "targetEventSha256",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "exitCode",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    if !required.iter().all(|key| object.contains_key(*key))
        || record.version != 3
        || record.kind != "run"
        || record.action != CheckAction::Check
        || !crate::producer::valid(&record.producer)
        || !is_sha(Some(&record.run_id))
        || !is_sha(Some(&record.target_event_sha256))
        || record.check_command.trim().is_empty()
        || record.check_command.contains('\0')
        || !is_sha(Some(&record.check_command_sha256))
        || crate::evidence::hash::text(&record.check_command) != record.check_command_sha256
        || !valid_timestamp(&record.timestamp)
        || !is_sha(Some(&record.event_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed check event"
        ));
    }
    if record.origin != ProofOrigin::LocalReport && record.origin != ProofOrigin::Synthetic {
        return Err(format!(
            "invalid run ledger line {line}: proof origin is invalid"
        ));
    }
    if record.previous_event_sha256.is_none() && event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: previous event hash is malformed"
        ));
    }
    if let Some(duration) = event.get("durationMs") {
        if !duration.is_null() && record.duration_ms.is_none() {
            return Err(format!(
                "invalid run ledger line {line}: durationMs must be a non-negative integer or null"
            ));
        }
    }
    Ok(())
}

fn validate_check_event_v4(event: &Value, line: usize) -> Result<(), String> {
    let mut parsed = event.clone();
    if parsed["version"].as_u64() >= Some(5) {
        parsed
            .as_object_mut()
            .map(|object| object.remove("subjectSha256"));
    }
    parsed
        .as_object_mut()
        .map(|object| object.remove("inputsSha256"));
    parsed
        .as_object_mut()
        .map(|o| o.remove("observationEventSha256"));
    let record: CheckObservationV4 = serde_json::from_value(parsed)
        .map_err(|_| format!("invalid run ledger line {line}: malformed check event"))?;
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: check event must be an object"))?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "subjectSha256",
        "inputsSha256",
        "configSha256",
        "targetEventSha256",
        "requirementId",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "acquisition",
        "result",
        "durationMs",
        "observationEventSha256",
        "stdout",
        "stderr",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "check")?;
    if event.get("observationEventSha256").is_some()
        && (event["version"] != 8 || !is_sha(event["observationEventSha256"].as_str()))
    {
        return Err("check observation binding is invalid".into());
    }
    if object.contains_key("configSha256")
        && !record
            .config_sha256
            .as_deref()
            .is_some_and(|hash| is_sha(Some(hash)))
    {
        return Err(format!(
            "invalid run ledger line {line}: check configuration identity is invalid"
        ));
    }
    if event["version"].as_u64() >= Some(6) && !is_sha(event["inputsSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: check event requires tested input identity"
        ));
    }
    if event["version"].as_u64() < Some(6)
        && (object.contains_key("inputsSha256") || object.contains_key("requirementId"))
    {
        return Err(format!(
            "invalid run ledger line {line}: input and preservation bindings require v6"
        ));
    }
    if event["version"].as_u64() >= Some(5) && !is_sha(event["subjectSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: malformed subject binding"
        ));
    }
    if let Some(id) = record.requirement_id.as_deref() {
        validate_requirement_id(id).map_err(|_| {
            format!("invalid run ledger line {line}: malformed preservation requirement binding")
        })?;
    }
    let required = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "targetEventSha256",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "acquisition",
        "result",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    let result_valid = match &record.result {
        CheckResult::Exit { code } => event["result"].as_object().is_some_and(|result| {
            result.len() == 2
                && result.get("kind") == Some(&json!("exit"))
                && result["code"].as_u64() == Some(*code)
        }),
        CheckResult::Signal { signal } => event["result"].as_object().is_some_and(|result| {
            result.len() == 2 && result.get("kind") == Some(&json!("signal")) && *signal > 0
        }),
    };
    if !required.iter().all(|key| object.contains_key(*key))
        || (event["version"].as_u64() >= Some(5) && !object.contains_key("subjectSha256"))
        || !matches!(record.version, 4..=8)
        || record.kind != "run"
        || record.action != CheckAction::Check
        || !crate::producer::valid(&record.producer)
        || !is_sha(Some(&record.run_id))
        || !is_sha(Some(&record.target_event_sha256))
        || record.check_command.trim().is_empty()
        || record.check_command.contains('\0')
        || !is_sha(Some(&record.check_command_sha256))
        || crate::evidence::hash::text(&record.check_command) != record.check_command_sha256
        || record.origin.as_str() != event["origin"]
        || !matches!(record.acquisition.as_str(), "reported" | "observed")
        || !result_valid
        || !valid_timestamp(&record.timestamp)
        || !is_sha(Some(&record.event_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed check event"
        ));
    }
    if record.version == 8 {
        match record.acquisition.as_str() {
            "observed" => {
                validate_log_artifact(record.stdout.as_ref(), line, "stdout")?;
                validate_log_artifact(record.stderr.as_ref(), line, "stderr")?;
            }
            "reported" if record.stdout.is_some() || record.stderr.is_some() => {
                return Err(format!(
                    "invalid run ledger line {line}: reported checks cannot claim captured logs"
                ));
            }
            _ => {}
        }
    } else if record.stdout.is_some() || record.stderr.is_some() {
        return Err(format!(
            "invalid run ledger line {line}: captured logs require v8"
        ));
    }
    if record.previous_event_sha256.is_none() && event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: previous event hash is malformed"
        ));
    }
    if let Some(duration) = event.get("durationMs") {
        if !duration.is_null() && record.duration_ms.is_none() {
            return Err(format!(
                "invalid run ledger line {line}: durationMs must be a non-negative integer or null"
            ));
        }
    }
    Ok(())
}

pub(crate) const MAX_CAPTURE_BYTES: u64 = 8 * 1_048_576;

fn validate_log_artifact(value: Option<&Value>, line: usize, stream: &str) -> Result<(), String> {
    let Some(object) = value.and_then(Value::as_object) else {
        return Err(format!(
            "invalid run ledger line {line}: observed {stream} log artifact is missing"
        ));
    };
    let invalid_path = object["path"].as_str().map_or(true, |path| {
        let portable = path.replace('\\', "/");
        path.is_empty()
            || std::path::Path::new(path).is_absolute()
            || portable.starts_with('/')
            || portable == ".."
            || portable.starts_with("../")
            || portable.contains("/../")
    });
    if object.len() != 4
        || object["root"] != "state"
        || invalid_path
        || !is_sha(object["sha256"].as_str())
        || object["bytes"]
            .as_u64()
            .map_or(true, |bytes| bytes > MAX_CAPTURE_BYTES)
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed observed {stream} log artifact"
        ));
    }
    Ok(())
}

pub(crate) fn validate_protection_event(event: &Value, line: usize) -> Result<(), String> {
    if matches!(event["version"].as_u64(), Some(4..=8)) {
        return validate_protection_event_v4(event, line);
    }
    let record: ProtectionRecord = serde_json::from_value(event.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed protection event"))?;
    let object = event.as_object().ok_or_else(|| {
        format!("invalid run ledger line {line}: protection event must be an object")
    })?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "stage",
        "attempt",
        "actor",
        "role",
        "attemptedOutcome",
        "reason",
        "checkEvidence",
        "origin",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "protection")?;
    if object.len() != allowed.len()
        || record.version != 3
        || record.kind != "run"
        || record.action != ProtectionAction::Protect
        || !crate::producer::valid(&record.producer)
        || !is_sha(Some(&record.run_id))
        || record.stage < 1
        || record.attempt < 1
        || record.actor.trim().is_empty()
        || record.role != "lead"
        || record.attempted_outcome != "accepted"
        || record.check_evidence.is_empty()
        || !valid_timestamp(&record.timestamp)
        || !is_sha(Some(&record.event_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection event"
        ));
    }
    if record.previous_event_sha256.is_none() && event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: previous event hash is malformed"
        ));
    }
    let evidence = event["checkEvidence"]
        .as_array()
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    for item in evidence {
        validate_check_evidence(item, line)?;
    }
    let has_missing = record
        .check_evidence
        .iter()
        .any(|item| item.status == EvidenceStatus::Missing);
    let has_failed = record
        .check_evidence
        .iter()
        .any(|item| item.status == EvidenceStatus::Failed);
    let reason = record.reason.as_str();
    if (!has_missing && matches!(reason, "check_missing" | "preservation_missing"))
        || (!has_failed && matches!(reason, "check_failed" | "preservation_failed"))
    {
        return Err(format!(
            "invalid run ledger line {line}: protection reason does not match evidence"
        ));
    }
    Ok(())
}

fn validate_protection_event_v4(event: &Value, line: usize) -> Result<(), String> {
    let object = event.as_object().ok_or_else(|| {
        format!("invalid run ledger line {line}: protection event must be an object")
    })?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "subjectSha256",
        "inputsSha256",
        "stage",
        "attempt",
        "actor",
        "role",
        "attemptedOutcome",
        "reason",
        "checkEvidence",
        "origin",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "protection")
        .map_err(|_| format!("invalid run ledger line {line}: malformed protection event"))?;
    if (event["version"].as_u64() >= Some(6)) != is_sha(event["inputsSha256"].as_str())
        || (event["version"].as_u64() < Some(6) && object.contains_key("inputsSha256"))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed tested input binding"
        ));
    }
    if event["version"].as_u64() >= Some(5) && !is_sha(event["subjectSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: malformed subject binding"
        ));
    }
    let evidence = object
        .get("checkEvidence")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    if !matches!(event["version"].as_u64(), Some(4..=8))
        || event["kind"] != "run"
        || event["action"] != "protect"
        || !crate::producer::valid(&event["producer"])
        || !is_sha(event["runId"].as_str())
        || event["stage"].as_u64().map_or(true, |x| x < 1)
        || event["attempt"].as_u64().map_or(true, |x| x < 1)
        || event["actor"].as_str().map_or(true, str::is_empty)
        || event["role"] != "lead"
        || event["attemptedOutcome"] != "accepted"
        || evidence.is_empty()
        || !matches!(
            event["reason"].as_str(),
            Some("check_missing" | "check_failed" | "preservation_missing" | "preservation_failed")
        )
        || !matches!(event["origin"].as_str(), Some("local_report" | "synthetic"))
        || !event["timestamp"].as_str().is_some_and(valid_timestamp)
        || !is_sha(event["eventSha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection event"
        ));
    }
    for item in evidence {
        validate_check_evidence_v4(item, line)?;
    }
    let has_missing = evidence.iter().any(|item| item["status"] == "missing");
    let has_failed = evidence.iter().any(|item| item["status"] == "failed");
    if (!has_missing
        && matches!(
            event["reason"].as_str(),
            Some("check_missing" | "preservation_missing")
        ))
        || (!has_failed
            && matches!(
                event["reason"].as_str(),
                Some("check_failed" | "preservation_failed")
            ))
    {
        return Err(format!(
            "invalid run ledger line {line}: protection reason does not match evidence"
        ));
    }
    Ok(())
}

fn validate_check_evidence_v4(value: &Value, line: usize) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    if !is_sha(value["targetEventSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection evidence"
        ));
    }
    match value["status"].as_str() {
        Some("missing")
            if matches!(object.len(), 2 | 3) && object.contains_key("targetEventSha256") =>
        {
            if object.len() == 3 {
                value["requirementId"]
                    .as_str()
                    .filter(|id| validate_requirement_id(id).is_ok())
                    .map(|_| ())
                    .ok_or_else(|| {
                        format!("invalid run ledger line {line}: malformed protection evidence")
                    })
            } else {
                Ok(())
            }
        }
        Some("failed")
            if matches!(object.len(), 4 | 5)
                && object.contains_key("checkEventSha256")
                && is_sha(value["checkEventSha256"].as_str())
                && object.contains_key("result") =>
        {
            if object.len() == 5
                && value["requirementId"]
                    .as_str()
                    .filter(|id| validate_requirement_id(id).is_ok())
                    .is_none()
            {
                return Err(format!(
                    "invalid run ledger line {line}: malformed protection evidence"
                ));
            }
            let result = &value["result"];
            let valid = result.as_object().is_some_and(|result| {
                result.len() == 2
                    && matches!(result["kind"].as_str(), Some("exit" | "signal"))
                    && (result["kind"] == "exit" && result["code"].is_u64()
                        || result["kind"] == "signal"
                            && result["signal"].as_u64().is_some_and(|signal| signal > 0))
            });
            if valid {
                Ok(())
            } else {
                Err(format!(
                    "invalid run ledger line {line}: malformed protection evidence"
                ))
            }
        }
        _ => Err(format!(
            "invalid run ledger line {line}: malformed protection evidence"
        )),
    }
}

fn validate_check_evidence(value: &Value, line: usize) -> Result<(), String> {
    let record: CheckEvidence = serde_json::from_value(value.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    if !is_sha(Some(&record.target_event_sha256)) {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection evidence"
        ));
    }
    if record.status == EvidenceStatus::Missing {
        if !(object.len() == 2
            || (object.len() == 3
                && record
                    .requirement_id
                    .as_deref()
                    .is_some_and(|id| validate_requirement_id(id).is_ok())))
        {
            return Err(format!(
                "invalid run ledger line {line}: missing check evidence has extra fields"
            ));
        }
    } else {
        let has_valid_requirement = record
            .requirement_id
            .as_deref()
            .is_some_and(|id| validate_requirement_id(id).is_ok());
        if !((object.len() == 4 && record.requirement_id.is_none())
            || (object.len() == 5 && has_valid_requirement))
            || !is_sha(record.check_event_sha256.as_deref())
            || record.exit_code.is_none()
        {
            return Err(format!(
                "invalid run ledger line {line}: failed check evidence is malformed"
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_check_target(state: &Value, target: &str) -> Result<Value, String> {
    if !is_sha(Some(target)) {
        return Err("--target must be a lowercase SHA-256 event hash".into());
    }
    let attempt = state["attempt"].as_u64().ok_or("run attempt is invalid")?;
    state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?
        .iter()
        .find(|submission| {
            submission["eventSha256"].as_str() == Some(target)
                && submission["attempt"] == attempt
                && submission["role"] == "worker"
                && submission["outcome"] == "completed"
        })
        .cloned()
        .ok_or_else(|| "check target is not a current worker completion".into())
}

pub(crate) fn validate_check_against_state(
    state: &Value,
    event: &Value,
    line: usize,
) -> Result<(), String> {
    let target = event["targetEventSha256"]
        .as_str()
        .ok_or_else(|| format!("invalid run ledger line {line}: check target is malformed"))?;
    validate_check_target(state, target)
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    let version = state["version"].as_u64().unwrap_or(0);
    let requirement_id = event["requirementId"].as_str();
    let (command, command_sha256, origin) = if let Some(id) = requirement_id {
        if version < 6 {
            return Err(format!(
                "invalid run ledger line {line}: preservation checks require v6"
            ));
        }
        let preservation = state.get("preservation").ok_or_else(|| {
            format!("invalid run ledger line {line}: preservation is not configured")
        })?;
        let preservation = preservation_from_value(preservation, line)?;
        let requirement = preservation.requirement(id).ok_or_else(|| {
            format!("invalid run ledger line {line}: preservation requirement is not configured")
        })?;
        (
            requirement.command.clone(),
            requirement.command_sha256.clone(),
            requirement.origin,
        )
    } else {
        let policy_value = state.get("checkPolicy").ok_or_else(|| {
            format!("invalid run ledger line {line}: check policy is not configured")
        })?;
        let policy = policy_from_value(policy_value, line)?;
        (policy.command, policy.command_sha256, policy.origin)
    };
    let policy_matches = event["checkCommand"] == command
        && event["checkCommandSha256"] == command_sha256
        && event["origin"] == origin.as_str();
    let shape_matches = if matches!(version, 4..=8) {
        event["version"] == version
            && matches!(event["acquisition"].as_str(), Some("reported" | "observed"))
    } else {
        event["version"] == 3 && event.get("acquisition").is_none()
    };
    if !policy_matches || !shape_matches {
        return Err(format!(
            "invalid run ledger line {line}: check report does not match configured policy"
        ));
    }
    Ok(())
}

pub(crate) fn validate_protection_against_state(
    state: &Value,
    event: &Value,
    line: usize,
) -> Result<(), String> {
    if state["status"] != "running" {
        return Err(format!(
            "invalid run ledger line {line}: protection event requires a running run"
        ));
    }
    let assignments = crate::run::assignment::pending(state);
    let assignment = assignments
        .iter()
        .find(|assignment| assignment["agent"] == event["actor"])
        .ok_or_else(|| {
            format!("invalid run ledger line {line}: protection actor is not currently pending")
        })?;
    if assignment["stage"] != event["stage"]
        || assignment["attempt"] != event["attempt"]
        || assignment["role"] != event["role"]
    {
        return Err(format!(
            "invalid run ledger line {line}: protection target is out of order"
        ));
    }
    let assessment = crate::run_exit::assess(state)
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    if !assessment.has_refusal_evidence() {
        return Err(format!(
            "invalid run ledger line {line}: protection has no current complete check refusal"
        ));
    }
    let expected = assessment
        .targets
        .iter()
        .filter(|target| target.is_missing() || target.is_failed())
        .map(|target| {
            if matches!(state["version"].as_u64(), Some(4..=8)) {
                target.protection_value()
            } else {
                target.value()
            }
        })
        .collect::<Vec<_>>();
    if event["reason"] != assessment.reason().unwrap_or_default()
        || event["origin"]
            != assessment.policy.as_ref().map_or(Value::Null, |policy| {
                Value::String(policy.origin.as_str().to_owned())
            })
        || event["checkEvidence"] != Value::Array(expected)
    {
        return Err(format!(
            "invalid run ledger line {line}: protection evidence does not match current checks"
        ));
    }
    Ok(())
}

/// Build the factual refusal event.  It contains only the failing/missing
/// check references; it never claims avoided loss or invents human time.
pub(crate) fn protection_event(
    state: &Value,
    actor: &str,
    stage: &Value,
    attempt: &Value,
    previous_event: &Value,
    timestamp: &str,
) -> Result<Value, String> {
    let assessment = crate::run_exit::assess(state)?;
    let reason = assessment
        .reason()
        .ok_or("acceptance is not blocked by a configured check")?;
    let origin = assessment
        .policy
        .as_ref()
        .ok_or("acceptance is not blocked by a configured check")?
        .origin
        .as_str();
    let version = state["version"].as_u64().unwrap_or(3);
    let evidence = assessment
        .targets
        .iter()
        .filter(|target| target.is_missing() || target.is_failed())
        .map(|target| {
            if matches!(version, 4..=8) {
                target.protection_value()
            } else {
                target.value()
            }
        })
        .collect::<Vec<_>>();
    let mut value = json!({
        "version": version,
        "kind": "run",
        "producer": crate::producer::evidence_for_version(version),
        "action": "protect",
        "runId": state["runId"],
        "stage": stage,
        "attempt": attempt,
        "actor": actor,
        "role": "lead",
        "attemptedOutcome": "accepted",
        "reason": reason,
        "checkEvidence": evidence,
        "origin": origin,
        "previousEventSha256": previous_event["eventSha256"],
        "timestamp": timestamp,
    });
    if version >= 5 {
        value["subjectSha256"] = state["subject"]["sha256"].clone();
    }
    if version >= 6 {
        value["inputsSha256"] = state
            .get("inputsSha256")
            .filter(|inputs| inputs.is_string())
            .cloned()
            .ok_or("tested inputs cannot be established; no protection was recorded")?;
    }
    Ok(value)
}

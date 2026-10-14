use super::*;

pub(super) fn build_result(
    invoker: &Invoker,
    evidence: &ScenarioEvidence,
    elapsed_ms: u64,
) -> Result<Value, String> {
    let expected = expected_contract()?;
    if expected["scenarioId"] != SCENARIO_ID || expected["origin"] != SCENARIO_ORIGIN {
        return Err("benchmark expected contract has the wrong scenario identity".into());
    }
    let baseline_submissions = submissions(&evidence.baseline_events);
    let blocked_protections = protection_count(&evidence.blocked_events);
    let final_worker_attempts = evidence
        .final_events
        .iter()
        .filter(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
        })
        .count();
    let blocked_canonical = has_canonical_acceptance(&evidence.blocked_events);
    let baseline_status = blocked_status(&evidence.baseline_events);
    let blocked_status = blocked_status(&evidence.blocked_events);
    let final_status = final_status(&evidence.final_events)?;

    let assertions = vec![
        assertion(
            "baseline_nonzero_check_observed",
            evidence.baseline_check_exit_code == 1,
        ),
        assertion(
            "baseline_completed_with_four_distinct_artifacts",
            baseline_submissions.len() == 4 && distinct_artifact_count(&baseline_submissions) == 4,
        ),
        assertion(
            "protected_failed_check_observed",
            has_event_for_exit(&evidence.blocked_events, "check", 1),
        ),
        assertion(
            "failed_acceptance_refused",
            evidence.failed_acceptance_exit_code == 1
                && !blocked_canonical
                && blocked_status == "running",
        ),
        assertion(
            "no_canonical_acceptance_on_failed_check",
            !blocked_canonical,
        ),
        assertion("one_factual_protection_record", blocked_protections == 1),
        assertion(
            "blocked_ledger_snapshot_saved",
            !evidence.blocked_ledger.is_empty() && sha256(&evidence.blocked_ledger).len() == 64,
        ),
        assertion(
            "repair_check_observed_zero",
            evidence.repair_check_exit_code == 0
                && has_event_for_exit(&evidence.final_events, "check", 0),
        ),
        assertion(
            "passing_check_does_not_accept_alone",
            !evidence.passing_check_alone_accepted,
        ),
        assertion(
            "reviewer_approval_does_not_accept_alone",
            !evidence.reviewer_approval_alone_accepted,
        ),
        assertion("lead_acceptance_after_repair", final_status == "accepted"),
        assertion(
            "prior_attempt_bytes_preserved",
            evidence.prior_attempt_preserved,
        ),
        assertion(
            "report_redacted_and_origin_separated",
            report_has_separate_origins(&evidence.report)
                && report_forbidden_fields(&evidence.report, false).is_none(),
        ),
        assertion(
            "invocation_counts_observed",
            invoker.total > 0 && invoker.total == invoker.successful + invoker.failed,
        ),
    ];
    if assertions.iter().any(|item| item["passed"] != true) {
        let failed = assertions
            .iter()
            .filter(|item| item["passed"] != true)
            .filter_map(|item| item["id"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("benchmark assertion failed: {failed}"));
    }

    let expected_result = expected["expectedResult"].clone();
    let observed_result = json!({
        "baseline": {
            "checkExitCode": evidence.baseline_check_exit_code,
            "distinctArtifactCount": baseline_submissions.len(),
            "finalStatus": baseline_status,
        },
        "protectedFailedCheck": {
            "checkExitCode": evidence.baseline_check_exit_code,
            "acceptanceExitCodeNonzero": evidence.failed_acceptance_exit_code != 0,
            "canonicalAcceptedEvent": blocked_canonical,
            "protectionRecordCount": blocked_protections,
            "status": blocked_status,
        },
        "protectedRecovery": {
            "repairCheckExitCode": evidence.repair_check_exit_code,
            "passingCheckAloneAccepted": evidence.passing_check_alone_accepted,
            "reviewerApprovalAloneAccepted": evidence.reviewer_approval_alone_accepted,
            "finalStatus": final_status,
            "workerAttemptCount": final_worker_attempts,
            "priorAttemptPreserved": evidence.prior_attempt_preserved,
        },
    });
    if expected_result != observed_result {
        return Err("benchmark observed result differs from its stable expected result".into());
    }
    let expected_ids = expected["assertionIds"]
        .as_array()
        .ok_or("benchmark expected contract has no assertion IDs")?;
    let actual_ids = assertions
        .iter()
        .map(|item| item["id"].clone())
        .collect::<Vec<_>>();
    if expected_ids != &actual_ids {
        return Err("benchmark assertion IDs differ from the stable scenario contract".into());
    }

    Ok(json!({
        "version": 1,
        "scenarioId": SCENARIO_ID,
        "origin": SCENARIO_ORIGIN,
        "product": if exitbind_surface() { "Exitbind" } else { "Soulmate" },
        "producer": if exitbind_surface() { "exitbind" } else { "soulmate" },
        "formatVersion": if exitbind_surface() { 5 } else { 4 },
        "layout": {
            "config": config_name(),
            "state": state_namespace(),
        },
        "claim": claim(),
        "limitations": expected["limitations"],
        "assertions": assertions,
        "expectedResult": expected_result,
        "observedResult": observed_result,
        "cliInvocations": invoker.counts(),
        "manualJsonEdits": 0,
        "automatedElapsedMs": elapsed_ms,
        "humanInteractionMs": Value::Null,
        "report": evidence.report,
    }))
}

pub(super) fn assertion(id: &str, passed: bool) -> Value {
    json!({"id": id, "expected": true, "passed": passed})
}

pub(super) fn expected_contract() -> Result<Value, String> {
    serde_json::from_str(EXPECTED_JSON)
        .map_err(|error| format!("benchmark expected result is invalid JSON: {error}"))
}

pub(super) fn final_status(events: &[Value]) -> Result<&str, String> {
    if has_canonical_acceptance(events) {
        Ok("accepted")
    } else {
        Err("final protected ledger has no canonical acceptance".into())
    }
}

pub(super) fn blocked_status(events: &[Value]) -> &str {
    if has_canonical_acceptance(events) {
        "accepted"
    } else {
        "running"
    }
}

pub(super) fn submissions(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event["action"] == "submit")
        .collect()
}

pub(super) fn distinct_artifact_count(events: &[&Value]) -> usize {
    events
        .iter()
        .filter_map(|event| {
            Some((
                event["artifact"]["root"].as_str()?,
                event["artifact"]["path"].as_str()?,
                event["artifact"]["sha256"].as_str()?,
            ))
        })
        .collect::<BTreeSet<_>>()
        .len()
}

pub(super) fn has_submission(events: &[&Value], agent: &str, outcome: &str, attempt: u64) -> bool {
    events.iter().any(|event| {
        event["agent"] == agent && event["outcome"] == outcome && event["attempt"] == attempt
    })
}

pub(super) fn protection_count(events: &[Value]) -> usize {
    events
        .iter()
        .filter(|event| event["action"] == "protect")
        .count()
}

pub(super) fn has_protection_reason(events: &[Value], reason: &str) -> bool {
    events
        .iter()
        .any(|event| event["action"] == "protect" && event["reason"] == reason)
}

pub(super) fn has_canonical_acceptance(events: &[Value]) -> bool {
    events.iter().any(|event| {
        event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
    })
}

pub(super) fn has_event_for_exit(events: &[Value], action: &str, exit_code: u64) -> bool {
    events
        .iter()
        .any(|event| event["action"] == action && event_exit_code(event) == Some(exit_code))
}

pub(super) fn has_event_for_target(
    events: &[Value],
    action: &str,
    target: &str,
    exit_code: u64,
) -> bool {
    events.iter().any(|event| {
        event["action"] == action
            && event["targetEventSha256"] == target
            && event_exit_code(event) == Some(exit_code)
    })
}

pub(super) fn event_exit_code(event: &Value) -> Option<u64> {
    event["exitCode"].as_u64().or_else(|| {
        (event["result"]["kind"] == "exit")
            .then(|| event["result"]["code"].as_u64())
            .flatten()
    })
}

pub(super) fn event_sha<'a>(event: &'a Value, label: &str) -> Result<&'a str, String> {
    event["eventSha256"]
        .as_str()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| format!("{label} has no valid event hash"))
}

pub(super) fn artifact_sha_for_event(events: &[Value], target: &str) -> Result<String, String> {
    events
        .iter()
        .find(|event| event["action"] == "submit" && event["eventSha256"] == target)
        .and_then(|event| event["artifact"]["sha256"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("submission target {target} has no artifact hash"))
}

pub(super) fn string_field<'a>(
    value: &'a Value,
    field: &str,
    label: &str,
) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label} has no {field}"))
}

pub(super) fn report_has_separate_origins(report: &Value) -> bool {
    let groups = report.get("groups").and_then(Value::as_object);
    groups.is_some_and(|groups| {
        groups.get("synthetic").is_some_and(Value::is_object)
            && groups.get("local_report").is_some_and(Value::is_object)
    })
}

pub(super) fn report_forbidden_fields(value: &Value, in_redaction: bool) -> Option<String> {
    let forbidden = [
        "goal",
        "command",
        "commands",
        "prompt",
        "prompts",
        "profile",
        "profiles",
        "transcript",
        "transcripts",
        "path",
        "paths",
        "artifact",
        "artifacts",
        "artifactContent",
    ];
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if !in_redaction && forbidden.contains(&key.as_str()) {
                    return Some(key.clone());
                }
                if let Some(found) =
                    report_forbidden_fields(child, in_redaction || key == "redaction")
                {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| report_forbidden_fields(item, in_redaction)),
        _ => None,
    }
}

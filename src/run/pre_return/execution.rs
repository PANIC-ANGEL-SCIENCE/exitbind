//! Execute one admitted pre-return attempt and retain its actual outcome.
use super::*;
use std::time::{Duration, Instant};

pub(super) fn run_or_recover(
    loaded: &Loaded,
    attempt_id: &str,
    attempt: &Value,
    policy: &crate::run_value::PreReturnPolicy,
    state: &Value,
    replayed: bool,
    ticket: &crate::run::continuation_effect::Ticket,
) -> Result<Value, String> {
    if replayed {
        return Ok(attempt.clone());
    }
    let preparation =
        match crate::run_value::validate_checker_sources(&policy.checker_sources, "pre-return")
            .and_then(|()| {
                crate::project::continuation::runtime::guard(loaded, state)?;
                crate::run::verification_preparation::prepare(
                    loaded,
                    state,
                    attempt["binding"]["verificationOperationId"]
                        .as_str()
                        .unwrap_or(attempt_id),
                    &policy.checker_sources,
                )
            }) {
            Ok(preparation) => preparation,
            Err(error) => {
                return prelaunch_refusal(loaded, attempt_id, attempt, ticket, error);
            }
        };
    if let Some(preparation) = &preparation {
        if attempt["binding"]["verificationPreparationSha256"] != preparation.sha256()? {
            return prelaunch_refusal(loaded, attempt_id, attempt, ticket,
                "pre-return preparation recipe changed after attempt admission; no check was launched".into());
        }
    }
    let request = crate::run::capture::ObservationRequest {
        command: policy.command.clone(),
        working_dir: loaded.product_root.clone(),
        artifact_dir: loaded
            .state_root
            .join(crate::project::layout_types::state_namespace())
            .join("artifacts"),
        state_root: loaded.state_root.clone(),
        deadline: Instant::now()
            .checked_add(Duration::from_millis(policy.timeout_ms))
            .unwrap_or_else(Instant::now),
        timeout_ms: policy.timeout_ms,
        operation_id: attempt_id.to_owned(),
        capture: crate::run::capture::CapturePolicy {
            per_stream_bytes: policy.capture_limit_bytes,
            combined_bytes: policy.capture_limit_bytes.saturating_mul(2),
        },
        retain_partial: true,
        durable_proof: if cfg!(target_os = "linux") && preparation.is_some() {
            Some(journal::observer_path(loaded, attempt_id)?)
        } else {
            None
        },
        preparation,
    };
    let result_path = record_path(loaded, attempt_id, "result")?;
    if let Err(error) = crate::project::continuation::runtime::guard(loaded, state) {
        return prelaunch_refusal(loaded, attempt_id, attempt, ticket, error);
    }
    let mut known_ended = false;
    let mut process_started = true;
    let mut control = crate::run::continuation_effect::checker_control(loaded, state);
    let captured = if state.get("projectContinuation").is_some() {
        crate::run::capture::capture_observed_command_with_control(&request, &mut control)
    } else {
        crate::run::capture::capture_observed_command(&request)
    };
    let result = match captured {
        Ok(capture) if capture.group_ended => {
            known_ended = true;
            let code = capture.status.code();
            if let Err(error) = install_captured_logs(&capture) {
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain",
                    "failureKind":"storage_failure","diagnostic":{"diagnostic":error,"storageStage":"capture_install"},
                    "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                    "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            } else {
                let state = if code == Some(0) { "passed" } else { "failed" };
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":state,
                "failureKind":if code.is_some() { if code==Some(0) { Value::Null } else { json!("nonzero_exit") } } else { json!("terminated") },
                "exitCode":code,"durationMs":capture.duration_ms,"stdout":capture.stdout_artifact,"stderr":capture.stderr_artifact,
                "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            }
        }
        Ok(capture) => {
            if let Err(error) = install_captured_logs(&capture) {
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain",
                    "failureKind":"storage_failure","diagnostic":{"diagnostic":error,"storageStage":"capture_install"},
                    "exitCode":capture.status.code(),"durationMs":capture.duration_ms,
                    "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                    "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            } else {
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain","failureKind":"descendants_remain_or_unknown",
            "exitCode":capture.status.code(),"durationMs":capture.duration_ms,"stdout":capture.stdout_artifact,"stderr":capture.stderr_artifact,
            "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
            "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            }
        }
        Err(error) => {
            let facts = error.facts();
            known_ended = facts["termination"] == "ended";
            process_started = facts["processStarted"] == true;
            let diagnostic = facts["diagnostic"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase();
            let failure_kind = if facts["deadlineExceeded"] == true {
                "timeout"
            } else if diagnostic.contains("could not be launched") {
                "launch_failure"
            } else if diagnostic.contains("could not be created")
                || diagnostic.contains("artifact")
                || diagnostic.contains("directory")
                || diagnostic.contains("stored")
            {
                "storage_failure"
            } else if facts["capture"] == "incomplete" {
                "capture_failure"
            } else if facts["storageStage"] == "capture" {
                "storage_failure"
            } else {
                "execution_uncertain"
            };
            json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain",
                "failureKind":failure_kind,"diagnostic":facts,"producer":request_producer(),
                "bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
        }
    };
    let current = crate::project::continuation::runtime::guard(loaded, state);
    let mut result = result;
    if state.get("projectContinuation").is_some() {
        result["execution"] = json!({"started":process_started,"knownEnded":known_ended});
    }
    if let Err(error) = current {
        result["continuationHold"] = json!({"message":error,"storageStage":"continuation_guard"});
        if result["state"] == "passed" {
            result["state"] = json!("uncertain");
            result["failureKind"] = json!("continuation_hold");
        }
    }
    let bytes = record_bytes(&result)?;
    write_durable(&result_path, &bytes).map_err(|e| {
        format!("pre-return result storage failed; attempt remains unresolved: {e}")
    })?;
    // Preserve exact owning evidence before observing aggregate metadata.
    observe_result(loaded, ticket, &mut result, known_ended);
    if result["state"] != "passed" {
        return Err(failure_response(loaded, attempt_id, &result));
    }
    Ok(result)
}

fn request_producer() -> Value {
    json!({"executableSha256":crate::producer::build_identity()["executableSha256"],"producer":crate::producer::evidence_for_version(8)})
}

fn observe_result(
    loaded: &Loaded,
    ticket: &crate::run::continuation_effect::Ticket,
    result: &mut Value,
    known_ended: bool,
) {
    if let Err(error) = crate::run::continuation_effect::complete(
        loaded,
        ticket,
        result["state"] == "passed",
        known_ended,
    ) {
        result["continuationAccountingRecovery"] = json!({
            "message":error.chars().filter(|c| !c.is_control()).take(512).collect::<String>(),
            "action":"Inspect or replay this exact saved attempt; do not execute the command again."
        });
    }
}

fn prelaunch_refusal(
    loaded: &Loaded,
    attempt_id: &str,
    attempt: &Value,
    ticket: &crate::run::continuation_effect::Ticket,
    error: String,
) -> Result<Value, String> {
    let result_path = record_path(loaded, attempt_id, "result")?;
    let mut result = json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,
        "state":"uncertain","failureKind":"preparation_refused",
        "execution":{"started":false,"certainty":"known_not_started"},
        "diagnostic":{"message":error,"storageStage":"preparation"},
        "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
        "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()});
    write_durable(&result_path, &record_bytes(&result)?)
        .map_err(|store| format!("pre-return preparation refusal could not be recorded; attempt remains unresolved: {store}"))?;
    observe_result(loaded, ticket, &mut result, true);
    Err(failure_response(loaded, attempt_id, &result))
}

/// A saved known-ended outcome can reconcile its debit after interruption;
/// unknown execution remains held until the owner can prove it ended.
pub(super) fn recover_saved_attempt(
    loaded: &Loaded,
    state: &Value,
    attempt_id: &str,
    binding: &Value,
    policy: &crate::run_value::PreReturnPolicy,
    result: &Value,
) -> Result<(), String> {
    if state.get("projectContinuation").is_none() {
        return Ok(());
    }
    let successful = result["state"] == "passed";
    let known_ended = matches!(result["state"].as_str(), Some("passed" | "failed"))
        || result["execution"]["knownEnded"] == true
        || (result["execution"]["started"] == false
            && result["execution"]["certainty"] == "known_not_started")
        || (result["diagnostic"]["termination"] == "ended"
            && result["diagnostic"]["groupEnded"] == true
            && result["diagnostic"]["captureReadersEnded"] == true);
    if !known_ended {
        return Ok(());
    }
    if result["version"] != 1
        || result["kind"] != "pre_return_result"
        || result["producer"]["executableSha256"] != binding["checkerExecutableSha256"]
        || result["producer"]["producer"] != binding["producer"]
        || (successful && result["exitCode"] != 0)
    {
        return Err(
            "saved pre-return result producer or outcome differs from its exact binding".into(),
        );
    }
    for stream in ["stdout", "stderr"] {
        let Some(artifact) = result.get(stream) else {
            if matches!(result["state"].as_str(), Some("passed" | "failed")) {
                return Err("saved pre-return capture is missing".into());
            }
            continue;
        };
        crate::run::artifact::read_exact(
            loaded,
            artifact,
            "pre-return capture",
            crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
        )?;
    }
    crate::run::continuation_effect::recover_check(
        loaded,
        state,
        "pre-return",
        &json!({"attemptId":attempt_id,"binding":binding,"policy":policy.value()}),
        successful,
    )
}

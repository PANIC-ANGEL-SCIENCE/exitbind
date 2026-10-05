//! Execution, capture, and committed response for an observed check.

use super::capture::{
    capture_observed_command, cleanup_capture, install_capture, observed_result,
    rollback_installed, run_observed_command, CapturePolicy, CapturedCheck, ObservationRequest,
};
use super::*;
use std::time::{Duration, Instant};

#[cfg(test)]
thread_local! {
    static POST_APPEND_FAULT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn post_append_fault_is(code: u8) -> bool {
    POST_APPEND_FAULT.with(|fault| fault.get() & code != 0)
}

#[cfg(not(test))]
fn post_append_fault_is(_code: u8) -> bool {
    false
}

/// Execute the frozen policy command and bind its locally observed result to
/// the exact current worker completion.  No caller-supplied command/result
/// fields are accepted.
pub fn observe_check(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    timeout_ms: Option<&str>,
) -> Result<Value, String> {
    observe_check_for_requirement(loaded, ledger, target, None, timeout_ms)
}

pub fn observe_check_for_requirement(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    requirement_id: Option<&str>,
    timeout_ms: Option<&str>,
) -> Result<Value, String> {
    let timeout_ms = check_observation::timeout(timeout_ms)?;
    if let Some(replay) = check_observation::replay_for_operation(
        loaded,
        ledger,
        Some(&timeout_ms.to_string()),
        Some((target, requirement_id)),
    )? {
        if replay["event"]["targetEventSha256"] == target
            && replay["event"]["requirementId"].as_str() == requirement_id
        {
            return Ok(replay);
        }
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let (source, policy, inputs, identity, capture_logs, initial_warning, admission) = with_lock(
        &path,
        || {
            crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
            if claim_path(&path).exists() {
                return Err("run has been superseded; no mutation was made".into());
            }
            let (_, events, mut source) = load_at(loaded, &path)?;
            let state = reduce_live(loaded, &events)?;
            if state["version"].as_u64().unwrap_or(0) < 4 {
                return Err("observe-check requires a newly created checked run".into());
            }
            if state["status"] != "running" {
                return Err(
                    "run has already reached a terminal state; no mutation was made".into(),
                );
            }
            check_observation::assert_current(loaded, &state)?;
            let warning = action_drift_warning(loaded, &state)?;
            crate::run::artifact::assert_current(loaded, &state)?;
            predecessor(loaded, &events[0])?;
            let policy = active_check_policy(&state, requirement_id)?;
            crate::run_value::validate_check_target(&state, target)?;
            let inputs = if state["version"].as_u64() >= Some(6) {
                Some(live_inputs(&state)?)
            } else {
                None
            };
            let identity = json!({
                "runId": state["runId"], "targetEventSha256": target,
                "subjectSha256": state["subject"]["sha256"], "inputsSha256": inputs,
                "configSha256": hash::text(&loaded.source), "checkCommandSha256": policy.command_sha256,
                "requirementId": requirement_id,
                "observerExecutableSha256": if state["checkObservationProtocol"] == check_observation::PROTOCOL {
                    crate::producer::build_identity()["executableSha256"].clone()
                } else { Value::Null },
            });
            let admission = check_observation::admit(
                loaded,
                &path,
                &mut source,
                &state,
                &events,
                identity.clone(),
                timeout_ms,
            )?;
            Ok((
                source,
                policy.clone(),
                inputs.clone(),
                identity,
                state["version"] == 8,
                warning,
                admission,
            ))
        },
    )?;

    warn_drift(initial_warning.as_ref());
    let request = ObservationRequest {
        command: policy.command.clone(),
        working_dir: loaded.product_root.clone(),
        artifact_dir: loaded
            .state_root
            .join(crate::project::layout_types::state_namespace())
            .join("artifacts"),
        state_root: loaded.state_root.clone(),
        deadline: Instant::now()
            .checked_add(Duration::from_millis(timeout_ms))
            .unwrap_or_else(Instant::now),
        timeout_ms,
        operation_id: hash::value(&json!({
            "identity": identity,
            "ledgerSourceSha256": hash::text(&source),
        })),
        capture: CapturePolicy::FINITE,
        retain_partial: admission.is_some(),
    };
    let capture = match if capture_logs {
        capture_observed_command(&request)
    } else {
        run_observed_command(&request).map(CapturedCheck::without_logs)
    } {
        Ok(capture) => capture,
        Err(error) => {
            if admission.is_some() {
                return check_observation::record_failure(
                    loaded,
                    ledger,
                    &path,
                    admission.as_ref(),
                    error.facts(),
                );
            }
            return Err(error.to_string());
        }
    };

    let result = observed_result(&capture.status)?;
    let mut storage_partials = Vec::new();
    let committed = with_lock(&path, || {
        if admission.is_some() && !capture.group_ended {
            return Err("check leader ended but descendants remain or group termination is unknown; no check was recorded".into());
        }
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let (_, current_events, current_source) = load_at(loaded, &path)?;
        let current_state = reduce_live(loaded, &current_events)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; this check result was not recorded".into());
        }
        if current_source != source {
            return Err(
                "run ledger changed while observing; this check result was not recorded".into(),
            );
        }
        if current_state["version"].as_u64().unwrap_or(0) < 4
            || current_state["status"] != "running"
        {
            return Err("run changed while observing; this check result was not recorded".into());
        }
        check_observation::assert_current(loaded, &current_state)?;
        if admission.is_some()
            && (fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
                || current_state["inputsSha256"] != identity["inputsSha256"]
                || current_state["subject"]["sha256"] != identity["subjectSha256"]
                || crate::producer::build_identity()["executableSha256"]
                    != identity["observerExecutableSha256"])
        {
            return Err("observation binding changed before commit; no check was recorded".into());
        }
        let warning = action_drift_warning(loaded, &current_state)?;
        let warning = inputs
            .as_ref()
            .and_then(Value::as_str)
            .map(|expected| input_drift_warning_for(loaded, &current_state, Some(expected)))
            .transpose()?
            .flatten()
            .or(warning);
        crate::run::artifact::assert_current(loaded, &current_state)?;
        predecessor(loaded, &current_events[0])?;
        let current_policy = active_check_policy(&current_state, requirement_id)?;
        if current_policy != policy {
            return Err(
                "check policy changed while observing; this check result was not recorded".into(),
            );
        }
        crate::run_value::validate_check_target(&current_state, target)?;
        let last = current_events
            .last()
            .ok_or("run ledger has no event head")?;
        let mut event_value = json!({
            "version": current_state["version"],
            "kind": "run",
            "producer": crate::producer::evidence_for_version(current_state["version"].as_u64().unwrap_or(4)),
            "action": "check",
            "runId": current_state["runId"],
            "targetEventSha256": target,
            "checkCommand": policy.command,
            "checkCommandSha256": policy.command_sha256,
            "origin": policy.origin.as_str(),
            "acquisition": "observed",
            "result": result,
            "durationMs": capture.duration_ms,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        });
        if let Some(admission) = &admission {
            event_value["observationEventSha256"] = admission["eventSha256"].clone();
        }
        if let (Some(stdout), Some(stderr)) = (&capture.stdout_artifact, &capture.stderr_artifact) {
            event_value["stdout"] = stdout.clone();
            event_value["stderr"] = stderr.clone();
        }
        if current_state["version"].as_u64() >= Some(5) {
            event_value["subjectSha256"] = current_state["subject"]["sha256"].clone();
        }
        if current_state["version"] == 8 {
            event_value["configSha256"] = json!(hash::text(&loaded.source));
        }
        if let Some(inputs) = &inputs {
            event_value["inputsSha256"] = inputs.clone();
            if let Some(id) = requirement_id {
                event_value["requirementId"] = json!(id);
            }
        }
        let event = run_state::make_event(event_value);
        let mut all = current_events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        let installed_stdout = match (&capture.stdout_temp, &capture.stdout_final) {
            (Some(temp), Some(final_path)) => match install_capture(temp, final_path) {
                Ok(installed) => installed,
                Err(error) => {
                    let failure = match rollback_installed(&capture, false, false) {
                        Ok(()) => error,
                        Err(cleanup) => format!("{error}; cleanup failed: {cleanup}"),
                    };
                    return Err(observed_storage_failure(&capture, failure));
                }
            },
            _ => false,
        };
        let installed_stderr = match (&capture.stderr_temp, &capture.stderr_final) {
            (Some(temp), Some(final_path)) => match install_capture(temp, final_path) {
                Ok(installed) => installed,
                Err(error) => {
                    let failure = match rollback_installed(&capture, installed_stdout, false) {
                        Ok(()) => error,
                        Err(cleanup) => format!("{error}; cleanup failed: {cleanup}"),
                    };
                    return Err(observed_storage_failure(&capture, failure));
                }
            },
            _ => false,
        };
        let append_result = append(&path, &event, false, &current_source);
        if let Err(error) = append_result {
            if admission.is_some() {
                storage_partials = super::capture::retain_complete_failure(&request, &capture);
            }
            let failure = match rollback_installed(&capture, installed_stdout, installed_stderr) {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; cleanup failed: {cleanup}"),
            };
            return Err(observed_storage_failure(&capture, failure));
        }
        // Nothing fallible may erase the acknowledged append.  Keep the event
        // created in this lock scope even if the view cannot be projected.
        Ok(recorded_after_append(
            event,
            &next_state,
            warning
                .or_else(|| initial_warning.clone())
                .map_or_else(|| json!([]), |warning| json!([warning])),
            || {
                if post_append_fault_is(1) {
                    Err("injected post-append projection fault".into())
                } else {
                    crate::run_value::status(&next_state, Some(true))
                }
            },
        ))
    });
    let partial_captures = if !storage_partials.is_empty() {
        storage_partials
    } else if committed.is_err() && admission.is_some() {
        super::capture::retain_complete_failure(&request, &capture)
    } else {
        Vec::new()
    };
    let cleanup = cleanup_capture(&capture);
    let cleanup = if committed.is_ok() && post_append_fault_is(2) {
        Err("injected post-append cleanup fault".into())
    } else {
        cleanup
    };
    #[cfg(test)]
    POST_APPEND_FAULT.with(|fault| fault.set(0));
    if let Err(error) = &committed {
        if admission.is_some() {
            let facts = json!({"process": result, "processStarted": true, "groupEnded": capture.group_ended, "captureReadersEnded": true, "captureAvailability": "complete", "durationMs": capture.duration_ms, "capture": "complete", "storage": "not_committed", "storageStage": "commit", "deadlineExceeded": false, "partialCaptures": partial_captures,
                "termination": if capture.group_ended && cleanup.is_ok() { "ended" } else { "unknown" },
                "diagnostic": error.chars().filter(|c| !c.is_control()).take(1024).collect::<String>(),
                "cleanupErrorCount": if cleanup.is_ok() { 0 } else { 1 }, "remainingOwnedPathCount": 0});
            return check_observation::record_failure(
                loaded,
                ledger,
                &path,
                admission.as_ref(),
                facts,
            );
        }
    }
    match (committed, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(observed_storage_failure(&capture, error)),
        (Err(error), Err(cleanup)) => Err(observed_storage_failure(
            &capture,
            format!("{error}; cleanup failed: {cleanup}"),
        )),
        (Ok(value), Err(cleanup)) => Ok(with_cleanup_error(
            value,
            &cleanup,
            &observed_capture_summary(&capture),
        )),
    }
}

fn recorded_after_append(
    event: Value,
    next_state: &Value,
    warnings: Value,
    project: impl FnOnce() -> Result<Value, String>,
) -> Value {
    let mut recorded = json!({
        "valid": true,
        "event": event,
        "runId": next_state["runId"],
        "status": next_state["status"],
        "warnings": warnings,
    });
    match project() {
        Ok(view) => recorded["checks"] = view["checks"].clone(),
        Err(error) => recorded["projectionError"] = json!(error),
    }
    recorded
}

fn with_cleanup_error(mut recorded: Value, cleanup: &str, capture_summary: &str) -> Value {
    recorded["cleanupError"] = json!(format!("{cleanup}; {capture_summary}"));
    recorded
}

fn observed_capture_summary(capture: &CapturedCheck) -> String {
    let status = observed_result(&capture.status)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| format!("{:?}", capture.status));
    let stdout_bytes = capture
        .stdout_artifact
        .as_ref()
        .and_then(|artifact| artifact["bytes"].as_u64())
        .unwrap_or_default();
    let stderr_bytes = capture
        .stderr_artifact
        .as_ref()
        .and_then(|artifact| artifact["bytes"].as_u64())
        .unwrap_or_default();
    format!(
        "observed check outcome: status={status}, durationMs={}, capture=complete(stdout={stdout_bytes}, stderr={stderr_bytes})",
        capture.duration_ms
    )
}

fn observed_storage_failure(capture: &CapturedCheck, error: String) -> String {
    if error.contains("observed check outcome:") {
        return error;
    }
    format!("{error}; {}", observed_capture_summary(capture))
}

#[cfg(test)]
mod tests {
    use super::{recorded_after_append, with_cleanup_error, POST_APPEND_FAULT};
    use serde_json::json;

    #[test]
    fn acknowledged_event_survives_projection_and_cleanup_faults() {
        let event = json!({"eventSha256": "original", "result": {"kind": "exit", "code": 7}});
        let state = json!({"runId": "run", "status": "running"});
        let projected = recorded_after_append(event.clone(), &state, json!([]), || {
            Err("injected post-append projection fault".into())
        });
        let cleaned = with_cleanup_error(projected, "injected cleanup fault", "logs retained");
        assert_eq!(cleaned["event"], event);
        assert_eq!(cleaned["event"]["result"]["code"], 7);
        assert_eq!(
            cleaned["projectionError"],
            "injected post-append projection fault"
        );
        assert!(cleaned["cleanupError"]
            .as_str()
            .unwrap()
            .contains("logs retained"));
    }

    #[cfg(unix)]
    #[test]
    fn real_append_then_injected_secondary_fault_preserves_event() {
        use crate::project::onboarding::{init_with_options, InitOptions};
        use crate::{config, run};
        use std::fs;

        let marked = crate::producer::exitbind_surface();
        for fault in [1, 2, 3] {
            let root = std::env::temp_dir().join(format!(
                "exitbind-post-append-{}-{fault}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            let git_init = std::process::Command::new("git")
                .args(["-C", root.to_str().unwrap(), "init", "--quiet"])
                .status()
                .unwrap();
            assert!(
                git_init.success(),
                "temporary worktree initialization failed"
            );
            let config_path = init_with_options(InitOptions {
                product_root: root.to_str().unwrap(),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            })
            .unwrap();
            let loaded = config::load(Some(config_path.to_str().unwrap())).unwrap();
            let state = crate::project::layout_types::state_namespace();
            let ledger = format!("{state}/runs/post-append-{fault}.jsonl");
            fs::create_dir_all(root.join(format!("{state}/runs"))).unwrap();
            fs::create_dir_all(root.join(format!("{state}/locks"))).unwrap();
            run::start_with_policy(
                &loaded,
                "change",
                "post append fault",
                &ledger,
                None,
                None,
                Some("true"),
                None,
                None,
                None,
                None,
                None,
                marked.then_some("required"),
            )
            .unwrap();
            let artifact_dir = root.join(format!("{state}/artifacts"));
            fs::create_dir_all(&artifact_dir).unwrap();
            fs::write(artifact_dir.join("lead.md"), b"scope").unwrap();
            fs::write(artifact_dir.join("worker.md"), b"worker").unwrap();
            run::submit(
                &loaded,
                "lead",
                &ledger,
                "scoped",
                &format!("{state}/artifacts/lead.md"),
                Some("state"),
                None,
                None,
            )
            .unwrap();
            let worker = run::submit(
                &loaded,
                "worker",
                &ledger,
                "completed",
                &format!("{state}/artifacts/worker.md"),
                Some("state"),
                None,
                None,
            )
            .unwrap();
            let target = worker["event"]["eventSha256"].as_str().unwrap();
            POST_APPEND_FAULT.with(|injection| injection.set(fault));
            let observed = run::observe_check(&loaded, &ledger, target, None).unwrap();
            let event_sha = observed["event"]["eventSha256"].as_str().unwrap();
            let (_, events, _) = run::ledger::load(&loaded, &ledger).unwrap();
            assert_eq!(events.last().unwrap()["eventSha256"], event_sha);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event["action"] == "check")
                    .count(),
                1
            );
            assert_eq!(observed["event"]["result"]["code"], 0);
            assert_eq!(
                observed["event"]["observationEventSha256"].is_string(),
                marked
            );
            let replay = run::check_observation::committed_replay(&loaded, &ledger, None).unwrap();
            if marked {
                let replay = replay.unwrap();
                assert_eq!(replay["event"], observed["event"]);
                assert_eq!(replay["recovered"], true);
            } else {
                assert!(replay.is_none());
                assert!(!events
                    .iter()
                    .any(|event| event["action"] == "check_observation"));
            }
            if fault & 1 != 0 {
                assert!(observed["projectionError"]
                    .as_str()
                    .unwrap()
                    .contains("injected"));
            }
            if fault & 2 != 0 {
                assert!(observed["cleanupError"]
                    .as_str()
                    .unwrap()
                    .contains("injected"));
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}

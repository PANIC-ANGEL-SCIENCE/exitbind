//! Worker and role result delivery, held state, and post-record cleanup.

use super::*;

/// Return the already committed outcome of one native operation. This path
/// only reads a validated ledger event and its exact immutable artifact; it
/// never submits the old assignment again or advances the current action.
pub(crate) fn replay_recorded_native(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    journal: &Value,
    bytes: &[u8],
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let (_, events, _) = run::ledger::load(loaded, &ledger)?;
    let submitted: Value =
        serde_json::from_slice(bytes).map_err(|_| "completed native result is not valid JSON")?;
    let expected = hash::text(assignment);
    let matches = events
        .iter()
        .filter(|event| {
            event["action"] == "submit"
                && event["assignmentSha256"] == expected
                && event["role"] == journal["role"]
                && event["agent"] == journal["agent"]
                && event["stage"] == journal["stage"]
                && event["attempt"] == journal["attempt"]
        })
        .collect::<Vec<_>>();
    let [event] = matches.as_slice() else {
        return Err("native replay has no unique committed result for this assignment".into());
    };
    if submitted["outcome"] != event["outcome"] {
        return Err("native replay outcome conflicts with the committed result".into());
    }
    let artifact = run::artifact::read(loaded, &event["artifact"], "native replay")?;
    if artifact.bytes != bytes.len() as u64 || artifact.preview != bytes {
        return Err("native replay bytes differ from the committed result".into());
    }
    let (next, _, presentation) = next_and_residual(loaded, work, &ledger, false)?;
    let response = json!({
        "work": work,
        "event": event,
        "next": next,
        "presentation": presentation,
    });
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let mut result = bounded_mutation(&response, work, Some(assignment), &ledger, config_path);
    result["idempotent"] = json!(true);
    result["recoveryEvent"] = json!({
        "kind": "native_return_replay",
        "cue": "passing_black_cat",
        "work": work,
        "assignment": assignment,
        "eventSha256": event["eventSha256"],
        "resultSha256": artifact.sha256,
        "providerExecuted": false,
        "nextObligation": result["next"]["action"],
    });
    Ok(result)
}
use crate::kernel::result_contract::{self, Role};

#[allow(clippy::too_many_arguments)]
pub(crate) fn return_result(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    outcome: &str,
    reason: Option<&str>,
    disposition: Option<&str>,
    held_reference: Option<&str>,
    current_binding: Option<&str>,
) -> Result<Value, String> {
    return_result_with(
        loaded,
        work,
        assignment,
        outcome,
        reason,
        disposition,
        held_reference,
        None,
        current_binding,
    )
}

/// Deliver a result whose bytes Exitbind composed itself instead of reading
/// them from standard input.
#[allow(clippy::too_many_arguments)]
pub(crate) fn return_result_with(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    outcome: &str,
    reason: Option<&str>,
    disposition: Option<&str>,
    held_reference: Option<&str>,
    supplied: Option<Vec<u8>>,
    current_binding: Option<&str>,
) -> Result<Value, String> {
    details::ensure_action_binding(loaded, work, current_binding)?;
    if outcome.trim().is_empty() {
        return Err("work return requires --outcome OUTCOME".into());
    }
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" && action["action"] != "lead_decision" {
        return Err("an assignment is not the current work action".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let expected = run::AssignmentIdentity::from_action(&action)?;
    // `unavailable` is admissible only for a reviewer, and only to report that
    // the target could not execute — never to escape an adverse verdict, which
    // the reducer refuses independently.  The role vocabulary is shared with
    // native result validation; only Lead's current stage adds a restriction.
    let role = action["role"]
        .as_str()
        .ok_or("current assignment has no role")?;
    let role = Role::parse(role).ok_or("current assignment has an unsupported role")?;
    let requested = result_contract::validate(role.as_str(), outcome).ok();
    let allowed = requested.is_some_and(|requested| {
        role.permits(requested)
            && (role != Role::Lead
                || action["outcomes"].as_array().is_some_and(|outcomes| {
                    outcomes
                        .iter()
                        .any(|item| item.as_str() == Some(requested.as_str()))
                }))
    });
    if !allowed && action["packet"].get("pendingDisposition").is_some() {
        return Err(format!(
            "outcome '{outcome}' is not allowed while a finding waits for the Lead; permitted outcomes: {}. Use exitbind work disposition {work} {assignment} --decision repair|defer|reject|supersede --reason TEXT",
            return_outcomes(&action, role)
        ));
    }
    if !allowed {
        return Err(format!(
            "outcome '{outcome}' is not allowed for {} assignment; permitted outcomes: {}",
            role.as_str(),
            return_outcomes(&action, role)
        ));
    }
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    if held_reference.is_some() && (outcome != "completed" || action["role"] != "worker") {
        return Err("--result-ref requires a pending worker completion".into());
    }
    let bytes = match (held_reference, supplied) {
        (Some(reference), _) => held::read(loaded, reference, work, assignment)?,
        (None, Some(bytes)) => bytes,
        (None, None) => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|error| format!("work result could not be read: {error}"))?;
            bytes
        }
    };
    let held = if outcome == "completed" && action["role"] == "worker" {
        Some(match held_reference {
            Some(reference) => held::existing(loaded, reference, work, assignment)?,
            None => held::store(loaded, work, assignment, &bytes)?,
        })
    } else {
        None
    };
    let created_held = held.as_ref().is_some_and(|value| value.created);
    let held_reference = held_reference
        .map(str::to_owned)
        .or_else(|| held.as_ref().map(|value| value.reference.clone()));
    let created_artifact = std::cell::RefCell::new(None);
    let mut recorded_protection = None;
    let submitted = run::submit_for_assignment(
        loaded,
        &ledger,
        assignment,
        expected,
        outcome,
        reason,
        disposition,
        || {
            let path = write_artifact(loaded, work, assignment, &bytes)?;
            *created_artifact.borrow_mut() = Some(path.clone());
            Ok(path)
        },
        |events, source| {
            details::ensure_action_binding(loaded, work, current_binding)?;
            preflight_submission(loaded, work, events, source)
        },
        &mut recorded_protection,
    );
    let submitted = match submitted {
        Ok(value) => value,
        Err(submission_error) => {
            if run::submission_refusal(&submission_error)
                == Some(run::SubmissionRefusal::GovernorReplanOrEvidence)
            {
                let held = match held {
                    Some(held) => held,
                    None => held::store(loaded, work, assignment, &bytes)?,
                };
                let next = next_for(loaded, work, &ledger).unwrap_or_else(|error| {
                    json!({"action": "unavailable", "held": held::response(&held), "error": error})
                });
                let response = json!({
                    "work": work,
                    "held": held::response(&held),
                    "next": next,
                    "reason": {"code": "governor_replan_or_evidence_required"},
                    "effect": "held",
                    "nextAction": {"type": "replan_then_resubmit", "safe": true, "reference": held.reference}
                });
                return Ok(bounded_mutation(
                    &response,
                    work,
                    Some(assignment),
                    &ledger,
                    config_path,
                ));
            }
            if created_held {
                let _ = held_reference
                    .as_deref()
                    .map(|reference| held::remove(loaded, reference));
            }
            let cleanup_error = if let Some(path) = created_artifact.into_inner() {
                let artifact = loaded.state_root.join(&path);
                remove_created_artifact(&artifact)
                    .err()
                    .map(|error| format!("artifact cleanup failed at {path}: {error}"))
            } else {
                None
            };
            if let Some(recorded) = recorded_protection {
                let error = cleanup_error.as_deref().map_or_else(
                    || submission_error.clone(),
                    |cleanup| format!("{submission_error}; {cleanup}"),
                );
                let submitted = json!({"event": recorded.event});
                let reference = recorded_protection_reference(
                    &ledger,
                    &submitted["event"],
                    &recorded.ledger_sha256,
                );
                let mut response = recorded_projection_failure(
                    work,
                    &ledger,
                    &submitted,
                    &error,
                    reference,
                    config_path,
                );
                if let Some(cleanup_error) = cleanup_error {
                    response["cleanupError"] = json!(cleanup_error);
                }
                response["requestedOutcome"] = json!(outcome);
                return Ok(bounded_mutation(
                    &response,
                    work,
                    Some(assignment),
                    &ledger,
                    config_path,
                ));
            }
            if let Some(cleanup_error) = cleanup_error {
                return Err(format!("{submission_error}; {cleanup_error}"));
            }
            return Err(submission_error);
        }
    };
    let held_cleanup_warning = held_reference
        .as_deref()
        .and_then(|reference| held::remove(loaded, reference).err());
    // The append above is durable before this projection runs.  If projection
    // cannot read the new state, report that recorded fact with the exact
    // event/head reference.  Returning an ordinary error here would invite a
    // caller to retry a submission that is already in the ledger.
    let (next, _residual, presentation) = match next_and_residual(loaded, work, &ledger, false) {
        Ok(value) => value,
        Err(projection_error) => {
            let reference = recorded_reference(loaded, &ledger, &submitted);
            let mut response = recorded_projection_failure(
                work,
                &ledger,
                &submitted,
                &projection_error,
                reference,
                config_path,
            );
            if let Some(error) = held_cleanup_warning {
                response["heldCleanupWarning"] = json!(error);
            }
            return Ok(bounded_mutation(
                &response,
                work,
                Some(assignment),
                &ledger,
                config_path,
            ));
        }
    };
    let mut response = json!({"work": work, "event": submitted["event"], "next": next, "presentation": presentation});
    if let Some(error) = held_cleanup_warning {
        response["heldCleanupWarning"] = json!(error);
    }
    Ok(bounded_mutation(
        &response,
        work,
        Some(assignment),
        &ledger,
        config_path,
    ))
}

fn return_outcomes(action: &Value, role: Role) -> String {
    if role == Role::Lead {
        return action["outcomes"]
            .as_array()
            .map(|outcomes| {
                outcomes
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
    }
    result_contract::permitted_text(role)
}

fn remove_created_artifact(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod artifact_cleanup_tests {
    use super::remove_created_artifact;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn path(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("exitbind-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn created_artifact_cleanup_removes_the_file() {
        let file = path("cleanup-file");
        fs::write(&file, b"artifact").unwrap();
        remove_created_artifact(&file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn cleanup_failure_remains_visible_to_the_caller() {
        let directory = path("cleanup-directory");
        fs::create_dir(&directory).unwrap();
        let error = remove_created_artifact(&directory).unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(directory.is_dir());
        fs::remove_dir(&directory).unwrap();
    }
}

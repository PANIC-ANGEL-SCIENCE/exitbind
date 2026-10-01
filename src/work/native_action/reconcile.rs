//! Bounded, read-only inspection of the current native operation. The result
//! omits prompts, stderr, result text and machine-local journal paths.

use super::*;

pub(super) fn inspect(loaded: &Loaded, work: &str, current: &Value) -> Result<Value, String> {
    let identity = assignment_identity(current)?;
    let role = current["role"]
        .as_str()
        .ok_or("current native assignment has no role")?;
    let agent = current["agent"]
        .as_str()
        .ok_or("current native assignment has no agent")?;
    let paths = journal_paths_readonly(
        loaded,
        work,
        &identity.assignment,
        current["packet"]["substitution"].is_object(),
    )?;
    let journal = match paths {
        Some(paths) => read_journal(&paths.journal)?,
        None => None,
    };
    let Some(journal) = journal else {
        return Ok(json!({
            "work": work,
            "assignment": identity.assignment,
            "effect": "no-change",
            "status": "not_started",
            "reconciliation": "no_record",
            "nextAction": {"type": "execute_current", "safe": true},
        }));
    };
    verify_journal(&journal, work, &identity, role, agent)?;
    let status = journal["status"]
        .as_str()
        .ok_or("native journal has no status")?;
    let liveness = match recovery::liveness(&journal) {
        ProcessLiveness::Alive => "alive",
        ProcessLiveness::Ended => "ended",
        ProcessLiveness::Uncertain => "uncertain",
    };
    let observation = &journal["observation"];
    let commands = observation["commands"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .take(8)
                .map(|item| {
                    json!({
                        "status": item["status"],
                        "exitCode": item["exitCode"],
                        "invocationSha256": item["invocationSha256"],
                        "hostItemId": item["hostItemId"],
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let command_count = observation["commands"].as_array().map_or(0, Vec::len);
    let result_state = if status == "completed" {
        "retained_pending_submission"
    } else if journal
        .get("provisionalFinalResult")
        .is_some_and(|value| !value.is_null())
    {
        "provisional"
    } else {
        "none"
    };
    let result_bytes = if status == "completed" {
        journal_result(&journal).ok()
    } else {
        journal
            .get("provisionalFinalResult")
            .and_then(|value| serde_json::to_vec(value).ok())
            .filter(|bytes| {
                journal["provisionalFinalResultBytes"].as_str() == Some(hex_encode(bytes).as_str())
            })
    };
    let (reconciliation, next_action) = if matches!(status, "started" | "running")
        && journal
            .get("priorFailure")
            .is_some_and(|value| !value.is_null())
    {
        (
            "inspection_required",
            action(
                work,
                "inspect_effects",
                false,
                Some("bounded provider retry was already used"),
            ),
        )
    } else {
        match status {
            "completed" => match validate_completed_observation(&journal)
                .and_then(|_| journal_result(&journal).map(|_| ()))
            {
                Ok(()) => ("replay_ready", action(work, "replay_saved", true, None)),
                Err(error) => (
                    "inspection_required",
                    action(work, "inspect_effects", false, Some(&error)),
                ),
            },
            "started" | "running" if result_state == "provisional" => {
                if role == "worker" && liveness == "ended" {
                    match recovery::validate_provisional_observation(&journal) {
                        Ok(()) => ("resume_ready", action(work, "resume_current", true, None)),
                        Err(error) => (
                            "inspection_required",
                            action(work, "inspect_effects", false, Some(&error)),
                        ),
                    }
                } else {
                    (
                        "inspection_required",
                        action(
                            work,
                            "inspect_effects",
                            false,
                            Some("native execution is alive, uncertain, or reviewer-owned"),
                        ),
                    )
                }
            }
            "started" if role == "worker" && journal["threadId"].is_string() => {
                ("resume_ready", action(work, "resume_current", true, None))
            }
            "running"
                if role == "worker" && liveness == "ended" && journal["threadId"].is_string() =>
            {
                ("resume_ready", action(work, "resume_current", true, None))
            }
            "running" if liveness == "alive" => {
                ("wait", action(work, "wait_existing", false, None))
            }
            "started" | "running" => (
                "inspection_required",
                action(
                    work,
                    "inspect_effects",
                    false,
                    Some("native execution identity or resume thread is unavailable"),
                ),
            ),
            _ => return Err("native assignment journal has an invalid status".into()),
        }
    };
    Ok(json!({
        "work": work,
        "assignment": identity.assignment,
        "effect": "no-change",
        "status": status,
        "operationId": journal["operationId"],
        "process": {
            "liveness": liveness,
            "exitCode": observation["process"]["code"],
            "signal": observation["process"]["signal"],
            "timedOut": observation["process"]["timedOut"],
        },
        "observation": {
            "turn": observation["turn"],
            "coverageGaps": observation["coverageGaps"],
            "commandCount": command_count,
            "commands": commands,
            "unobservedItemCount": observation["unobservedItemCount"],
            "usage": observation["usage"],
        },
        "result": {
            "state": result_state,
            "sha256": result_bytes.as_ref().map(|bytes| hash::bytes(bytes)),
            "bytes": result_bytes.as_ref().map(Vec::len),
        },
        "reconciliation": reconciliation,
        "nextAction": next_action,
    }))
}

fn action(work: &str, kind: &str, safe: bool, reason: Option<&str>) -> Value {
    let mut value = json!({"type": kind, "safe": safe});
    if safe {
        value["commandSuffix"] = json!(["work", "act", work, "--resume"]);
        value["sameConfigRequired"] = json!(true);
        value["sameExecutableRequired"] = json!(true);
    }
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    value
}

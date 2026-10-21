//! State-specific, executable next-action descriptors for the Lead.

use crate::config::Loaded;
use serde_json::{json, Value};

pub(crate) mod fill;
mod quarantine;
mod repair;

pub(super) fn command(loaded: &Loaded, mut suffix: Vec<String>) -> Value {
    suffix.push("--config".into());
    let command = super::response_recovery::bounded_argv(suffix, loaded.path.to_str(), 16 * 1024);
    json!({
        "argv": command.argv,
        "sameConfigRequired": command.same_config,
        "sameExecutableRequired": command.same_executable,
    })
}

pub(crate) fn current_detail_command(loaded: &Loaded, work: &str) -> Value {
    command(
        loaded,
        vec!["work".into(), "detail".into(), work.into(), "--json".into()],
    )
}

/// Forward the held-result admission check to its existing owner. Recovery
/// must not create a successor while bytes for this exact assignment await
/// return, and an unreadable held directory is also a refusal.
pub(crate) fn has_held_result(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
) -> Result<bool, String> {
    Ok(!super::held::discover(loaded, work, assignment)?.is_empty())
}

fn choice(
    loaded: &Loaded,
    suffix: Vec<String>,
    label: &str,
    placeholders: &[&str],
    input: Value,
) -> Value {
    let mut form = json!({
        "label": label,
        "command": command(loaded, suffix),
        "placeholders": placeholders,
        "stdin": input,
        "sideEffect": "records only after the selected actor supplies the placeholders and executes the bound command",
    });
    if placeholders.contains(&"ARTIFACT_ROOT") {
        form["inputConstraints"] = json!({
            "ARTIFACT_ROOT": {
                "type": "string",
                "allowedValues": ["product", "state"],
                "example": "product"
            }
        });
    }
    form
}

fn return_input() -> Value {
    json!({
        "required": true,
        "kind": "result_artifact",
        "encoding": "raw bytes supplied on stdin (UTF-8 when readable)",
        "description": "Pass the complete result artifact on stdin; do not omit or truncate it. The ordinary return route determines its own artifact bounds.",
    })
}

fn no_input() -> Value {
    json!({
        "required": false,
        "kind": "none",
        "description": "This command records the supplied option values and reads no result artifact from stdin.",
    })
}

fn allowed_outcomes(next: &Value) -> Vec<&str> {
    if let Some(outcomes) = super::stale_check::reviewer_outcomes(next) {
        return outcomes;
    }
    if next["action"] == "spawn" && next["role"] != "lead" {
        return next["role"]
            .as_str()
            .and_then(crate::kernel::result_contract::Role::parse)
            .map(|role| {
                role.outcomes()
                    .iter()
                    .map(|outcome| outcome.as_str())
                    .collect()
            })
            .unwrap_or_default();
    }
    next["outcomes"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn current_grant(next: &Value) -> Option<&Value> {
    let loop_state = &next["packet"]["context"]["loop"];
    let grant = &loop_state["currentMutation"];
    (grant["eventSha256"].is_string()
        && loop_state["currentGrantEventSha256"] == grant["eventSha256"]
        && grant["unit"] == "worker-mutation"
        && grant["runId"] == next["packet"]["context"]["run"]["id"]
        && grant["attempt"] == next["packet"]["attempt"]
        && grant["subjectSha256"] == next["packet"]["context"]["subject"]["sha256"]
        && grant["carryLineage"] == true
        && matches!(
            loop_state["state"].as_str(),
            Some("ready" | "replan_required" | "evidence_required")
        ))
    .then_some(grant)
}

fn has_held_results(next: &Value) -> bool {
    next.get("held").is_some_and(|held| !held.is_null())
        || next["heldResults"]
            .as_array()
            .is_some_and(|held| !held.is_empty())
}

fn return_choice(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
    outcome: &str,
    label: &str,
) -> Value {
    choice(
        loaded,
        vec![
            "work".into(),
            "return".into(),
            work.into(),
            assignment.into(),
            "--current-binding".into(),
            binding.into(),
            "--outcome".into(),
            outcome.into(),
            "--reason".into(),
            crate::emitted::slot("REASON"),
        ],
        label,
        &["REASON"],
        return_input(),
    )
}

pub(crate) fn project(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    let mut result = full(loaded, work, next, binding);
    let replacement_choice = |choice: &Value| {
        choice["label"].as_str().is_some_and(|label| {
            label.contains("replacement of an explicitly defective frozen check")
                || label.contains("defective frozen pre-return check")
                || label == "replace an explicitly defective frozen check"
        })
    };
    if let Some(choices) = result.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            if !replacement_choice(choice) {
                if let Some(object) = choice.as_object_mut() {
                    object.remove("command");
                    object.remove("sideEffect");
                    object.remove("stdin");
                }
            }
        }
    }
    if let Some(object) = result.as_object_mut() {
        object.remove("fillRoute");
    }
    if let Some(native) = result
        .get_mut("productManaged")
        .and_then(Value::as_object_mut)
    {
        native.remove("command");
        native.remove("inspect");
        native.insert("requiresDetail".into(), json!(true));
    }
    repair::compact(&mut result);
    result["detail"] = json!({
        "route": "current.details.grouped",
        "binding": binding,
        "summary": "follow the current grouped detail route for complete executable forms"
    });
    result
}

/// The complete current forms: each Lead choice names its actor, and each
/// form with placeholders names the fill route that runs it filled.
pub(crate) fn full(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    let mut result = forms(loaded, work, next, binding);
    fill::attach(loaded, work, binding, &mut result);
    result
}

fn forms(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    if next["packet"]["quarantineVerification"].is_object() {
        return quarantine::project(loaded, work, next, binding);
    }
    let Some(assignment) = next["assignment"].as_str() else {
        if next["action"] == "done" && next["status"] == "blocked" {
            // A terminal Lead return does not resolve the observation. Keep
            // replacement advice consistent with the execution owner's guard.
            if super::check::unresolved(next) {
                return json!({
                    "version": 1, "state": "blocked_unresolved_check_observation",
                    "binding": binding, "leadChoiceRequired": false, "choices": [],
                    "mechanicalAction": Value::Null,
                    "meaning": "The check termination is unknown. Preserve this blocked Work; no retry or replacement is available.",
                    "diagnostic": {
                        "command": command(loaded, vec!["work".into(), "recover-check".into(), work.into(), "--json".into()]),
                        "readOnly": true, "inspectionOnly": true,
                        "meaning": "Inspect the retained unknown observation without execution or check credit."
                    }
                });
            }
            let suffix = vec![
                "work".into(),
                "replace-check".into(),
                work.into(),
                "--check-command".into(),
                crate::emitted::slot("NEW_CHECK_COMMAND"),
                "--reason".into(),
                crate::emitted::slot("REASON"),
                "--classification".into(),
                "defective-contract".into(),
                "--current-binding".into(),
                binding.into(),
                "--apply".into(),
            ];
            let mut choices = vec![json!({
                "label": "replace an explicitly defective frozen check",
                "command": command(loaded, suffix),
                "placeholders": ["NEW_CHECK_COMMAND", "REASON"],
                "stdin": {"required": false, "kind": "none"},
                "sideEffect": "creates one immutable successor only after the configured Lead chooses the new check and records a defective-contract reason; the blocked predecessor stays unchanged"
            })];
            let diagnostic_attempt = crate::run::pre_return::replacement_ready(loaded, work)
                .ok()
                .flatten();
            if let Some(attempt_id) = &diagnostic_attempt {
                choices.push(json!({
                    "label": "replace an explicitly defective frozen pre-return check",
                    "command": command(loaded, vec![
                        "work".into(), "replace-check".into(), work.into(),
                        "--pre-return-command".into(), crate::emitted::slot("NEW_PRE_RETURN_COMMAND"),
                        "--reason".into(), crate::emitted::slot("REASON"),
                        "--classification".into(), "defective-contract".into(),
                        "--current-binding".into(), binding.into(),
                    ]),
                    "placeholders": ["NEW_PRE_RETURN_COMMAND", "REASON"],
                    "stdin": {"required": false, "kind": "none"},
                    "source": {"kind":"blocked_worker","diagnosticAttemptId":attempt_id},
                    "sideEffect": "preview only; the blocked report must link this known-ended diagnostic; the predecessor and its grant remain unchanged"
                }));
            }
            return json!({
                "version": 1, "state": "blocked_lead_check_replacement",
                "binding": binding, "leadChoiceRequired": true,
                "choices": choices,
                "mechanicalAction": Value::Null,
                "constraints": {"classification": "defective-contract", "sourceKind": "blocked_worker", "diagnosticAttemptId":diagnostic_attempt,"binding": binding}
            });
        }
        if next["action"] == "lead_decision"
            && matches!(
                next["progress"]["reason"]["code"].as_str(),
                Some("check_failed" | "preservation_failed")
            )
        {
            let suffix = vec![
                "work".into(),
                "replace-check".into(),
                work.into(),
                "--check-command".into(),
                crate::emitted::slot("NEW_CHECK_COMMAND"),
                "--reason".into(),
                crate::emitted::slot("REASON"),
                "--classification".into(),
                "defective-contract".into(),
                "--current-binding".into(),
                binding.into(),
            ];
            return json!({"version":1,"state":"lead_check_replacement_preview",
                "binding":binding,"leadChoiceRequired":true,
                "choices":[{"label":"preview replacement of an explicitly defective frozen check",
                    "command":command(loaded,suffix),"placeholders":["NEW_CHECK_COMMAND","REASON"],
                    "stdin":{"required":false,"kind":"none"},
                    "sideEffect":"preview only; no predecessor or successor ledger is changed"}],
                "mechanicalAction":Value::Null});
        }
        return json!({"version": 1, "state": next["action"], "binding": binding,
            "leadChoiceRequired": false, "choices": [],
            "mechanicalAction": if next["action"] == "check" {
                json!({"command": command(loaded, vec!["work".into(), "check".into(), work.into()]),
                    "meaning": "execute only the current frozen applicable check; failure requires a Lead decision",
                    "executionOptions": {"timeoutMs": {"option": "--timeout-ms", "default": 1_800_000, "minimum": 1, "maximum": crate::run::check_observation::MAX_TIMEOUT_MS}, "repeat": "an admitted observation is never automatically retried"}})
            } else { Value::Null }});
    };
    let outcomes: Vec<_> = allowed_outcomes(next)
        .into_iter()
        .filter(|outcome| *outcome != "contradiction" || next["packet"]["basisSha256"].is_string())
        .collect();
    let state = if super::check::unresolved(next) {
        "unresolved_check_observation"
    } else if next["packet"]["pendingDisposition"]["kind"] == "check_observation_failed" {
        "failed_check_observation"
    } else if next["packet"].get("pendingDisposition").is_some() {
        "pending_review_finding"
    } else if next["action"] == "lead_decision"
        && (matches!(
            next["progress"]["reason"]["code"].as_str(),
            Some("check_failed" | "preservation_failed")
        ) || next["staleEndedCheck"]["status"] == "failed")
    {
        "failed_check"
    } else if next["action"] == "lead_decision" && next["staleEndedCheck"]["status"] == "passed" {
        "stale_successful_check"
    } else if next["action"] == "lead_decision" && outcomes.contains(&"scoped") {
        "initial_scope"
    } else if next["action"] == "lead_decision" {
        "lead_acceptance"
    } else {
        "pending_assignment"
    };
    let mut result = json!({
        "version": 1,
        "state": state,
        "binding": binding,
        "work": work,
        "assignment": assignment,
        "actor": next["resolvedActor"],
        "leadChoiceRequired": state != "pending_assignment",
        "choices": [],
    });
    let mut choices: Vec<Value> = if state == "unresolved_check_observation" {
        let mut choices: Vec<Value> = outcomes
            .iter()
            .filter(|v| matches!(**v, "blocked" | "rejected"))
            .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
            .collect();
        if next["packet"]["checkObservation"]["state"] == "running" {
            let mut preview = choice(
                loaded,
                vec![
                    "work".into(),
                    "recover-check".into(),
                    work.into(),
                    "--json".into(),
                ],
                "preview_ended_failure_recovery",
                &[],
                no_input(),
            );
            preview["readOnly"] = json!(true);
            preview["sideEffect"] = json!("no-change; previews an explicit Lead decision for a retained known-ended storage failure; unknown effects remain blocked");
            choices.push(preview);
        }
        choices
    } else if matches!(state, "pending_review_finding" | "failed_check_observation") {
        let available = next["packet"]["pendingDisposition"]["decisions"]
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut choices = Vec::new();
        if available.contains(&"repair") {
            choices.push(choice(
                loaded,
                vec![
                    "work".into(),
                    "disposition".into(),
                    work.into(),
                    assignment.into(),
                    "--current-binding".into(),
                    binding.into(),
                    "--decision".into(),
                    "repair".into(),
                    "--reason".into(),
                    crate::emitted::slot("REASON"),
                    "--repair-boundary".into(),
                    crate::emitted::slot("REPAIR_BOUNDARY"),
                    "--regression".into(),
                    crate::emitted::slot("REGRESSION"),
                ],
                "repair",
                &["REASON", "REPAIR_BOUNDARY", "REGRESSION"],
                no_input(),
            ));
        }
        if available.contains(&"defer") {
            choices.push(choice(
                loaded,
                vec![
                    "work".into(),
                    "disposition".into(),
                    work.into(),
                    assignment.into(),
                    "--current-binding".into(),
                    binding.into(),
                    "--decision".into(),
                    "defer".into(),
                    "--reason".into(),
                    crate::emitted::slot("REASON"),
                ],
                "defer",
                &["REASON"],
                no_input(),
            ));
        }
        if available.contains(&"reject") {
            choices.push(choice(
                loaded,
                vec![
                    "work".into(),
                    "disposition".into(),
                    work.into(),
                    assignment.into(),
                    "--current-binding".into(),
                    binding.into(),
                    "--decision".into(),
                    "reject".into(),
                    "--reason".into(),
                    crate::emitted::slot("REASON"),
                ],
                "reject",
                &["REASON"],
                no_input(),
            ));
        }
        if available.contains(&"supersede") {
            choices.push(choice(
                loaded,
                vec![
                    "work".into(),
                    "disposition".into(),
                    work.into(),
                    assignment.into(),
                    "--current-binding".into(),
                    binding.into(),
                    "--decision".into(),
                    "supersede".into(),
                    "--reason".into(),
                    crate::emitted::slot("REASON"),
                    "--repair-boundary".into(),
                    crate::emitted::slot("REPAIR_BOUNDARY"),
                    "--regression".into(),
                    crate::emitted::slot("REGRESSION"),
                    "--successor-basis".into(),
                    crate::emitted::slot("SUCCESSOR_BASIS_JSON"),
                ],
                "supersede",
                &[
                    "REASON",
                    "REPAIR_BOUNDARY",
                    "REGRESSION",
                    "SUCCESSOR_BASIS_JSON",
                ],
                no_input(),
            ));
        }
        if state == "failed_check_observation" {
            choices.extend(
                outcomes
                    .iter()
                    .filter(|outcome| matches!(**outcome, "blocked" | "rejected"))
                    .map(|outcome| {
                        return_choice(loaded, work, assignment, binding, outcome, outcome)
                    }),
            );
        }
        choices
    } else if state == "failed_check" {
        outcomes
            .iter()
            .filter(|outcome| matches!(**outcome, "rework" | "blocked"))
            .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
            .collect()
    } else if state == "stale_successful_check" {
        outcomes
            .iter()
            .filter(|outcome| matches!(**outcome, "rework" | "blocked"))
            .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
            .collect()
    } else if state == "initial_scope" {
        outcomes
            .iter()
            .filter(|outcome| matches!(**outcome, "scoped" | "blocked"))
            .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
            .collect()
    } else if state == "lead_acceptance" {
        outcomes
            .iter()
            .filter(|outcome| matches!(**outcome, "accepted" | "rework" | "blocked"))
            .map(|outcome| {
                let label = if *outcome == "accepted" {
                    "accepted"
                } else {
                    outcome
                };
                return_choice(loaded, work, assignment, binding, outcome, label)
            })
            .collect()
    } else {
        outcomes
            .iter()
            .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
            .collect()
    };
    if state == "failed_check" {
        choices.push(choice(
            loaded,
            vec![
                "work".into(),
                "replace-check".into(),
                work.into(),
                "--check-command".into(),
                crate::emitted::slot("NEW_CHECK_COMMAND"),
                "--reason".into(),
                crate::emitted::slot("REASON"),
                "--classification".into(),
                "defective-contract".into(),
                "--current-binding".into(),
                binding.into(),
            ],
            "preview replacement of an explicitly defective frozen check",
            &["NEW_CHECK_COMMAND", "REASON"],
            no_input(),
        ));
    }
    result["choices"] = json!(choices);
    if next["action"] == "spawn"
        && matches!(next["role"].as_str(), Some("worker" | "reviewer"))
        && next["agent"]
            .as_str()
            .and_then(|name| loaded.agent(name))
            .is_some_and(|agent| agent.runtime.host.as_deref() == Some("codex"))
    {
        let suffix = vec![
            "work".into(),
            "act".into(),
            work.into(),
            "--current-binding".into(),
            binding.into(),
        ];
        let mut inspect = suffix.clone();
        inspect.push("--inspect".into());
        result["productManaged"] = json!({"host": "codex", "command": command(loaded, suffix),
            "inspect": {"command": command(loaded, inspect), "readOnly": true},
            "profileSha256": next["packet"]["profileSha256"],
            "meaning": "when the Lead selects product-managed delivery, execute one current configured native role; the provider supplies its own verdict and the host owns model access/permissions"});
    }
    if next["action"] == "spawn" && next["role"] == "worker" {
        let grant = current_grant(next);
        let governor_phase = next["packet"]["context"]["loop"]["state"].as_str();
        let held = has_held_results(next);
        if let Some(grant) = grant.filter(|_| !held) {
            result["currentGrant"] = json!({
                "governorEventSha256": grant["eventSha256"],
                "operation": grant["operation"],
                "state": "current_for_this_worker_assignment",
                "use": "the exact current grant may support the present mutation; it grants no host permissions",
            });
        }
        let informed = next["packet"]["context"]["loop"]["completionInformation"].is_object();
        if let Some(next_request) = match governor_phase {
            Some("replan_required") if informed => None,
            Some("replan_required") => Some(json!({
                "requiredAction": "work replan",
                "meaning": "complete a material re-plan before another mutation request"
            })),
            Some("evidence_required") => Some(json!({
                "requiredAction": "work evidence",
                "meaning": "record new exact evidence before another mutation request"
            })),
            _ => None,
        } {
            result["nextRequest"] = next_request;
        }
        if grant.is_none() && governor_phase == Some("ready") && !held {
            result["beforeEditing"] = json!({
                "command": command(loaded, vec!["work".into(), "permit".into(), work.into(),
                    assignment.into(), "--operation".into(), crate::emitted::slot("OPERATION"),
                    "--request-id".into(), format!("permit-{binding}")]),
                "placeholders": ["OPERATION"],
                "inputConstraints": {
                    "OPERATION": {
                        "type": "string",
                        "unit": "UTF-8 bytes",
                        "minimumBytes": 1,
                        "maximumBytes": crate::run::OPERATION_MAX_BYTES,
                        "nonBlank": true,
                        "forbiddenCharacters": ["NUL"],
                        "example": "Update import parsing"
                    }
                },
                "required": true,
                "meaning": "obtain allowed:true before editing; repeat this exact request ID only to recover its response; this form grants no host permission"
            });
        }
    }
    repair::attach(loaded, work, assignment, binding, next, &mut result);
    result
}

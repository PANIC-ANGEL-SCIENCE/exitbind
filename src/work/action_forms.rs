//! State-specific, executable next-action descriptors for the Lead.

use crate::config::Loaded;
use crate::emitted::Route;
use serde_json::{json, Value};

mod environmental;
pub(crate) mod fill;
mod quarantine;
mod repair;

/// A printed action form's command: the route with this configuration,
/// bounded to the action view's budget.
pub(super) fn command(loaded: &Loaded, route: Route) -> Value {
    route.config(&loaded.path).within(16 * 1024).argv_json()
}

pub(crate) fn current_detail_command(loaded: &Loaded, work: &str) -> Value {
    command(loaded, Route::new(["work", "detail", work, "--json"]))
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
    suffix: Route,
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
        Route::new([
            "work",
            "return",
            work,
            assignment,
            "--current-binding",
            binding,
            "--outcome",
            outcome,
            "--reason",
            crate::emitted::slot("REASON").as_str(),
        ]),
        label,
        &["REASON"],
        return_input(),
    )
}

/// The bound Lead replacement forms a blocked Work keeps. Only the configured
/// Lead creates the successor, by running the form with its own check and
/// reason; nothing executes automatically. `never_ran` marks a Work whose
/// checker produced no result, so its label claims no defect.
fn blocked_replacement_choices(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    never_ran: bool,
) -> (Vec<Value>, Option<String>) {
    let suffix = Route::new([
        "work",
        "replace-check",
        work,
        "--check-command",
        crate::emitted::slot("NEW_CHECK_COMMAND").as_str(),
        "--reason",
        crate::emitted::slot("REASON").as_str(),
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
        "--apply",
    ]);
    let mut choice = json!({
        "label": "replace an explicitly defective frozen check",
        "command": command(loaded, suffix),
        "placeholders": ["NEW_CHECK_COMMAND", "REASON"],
        "stdin": {"required": false, "kind": "none"},
        "sideEffect": "creates one immutable successor only after the configured Lead chooses the new check and records a defective-contract reason; the blocked predecessor stays unchanged"
    });
    if never_ran {
        choice["label"] = json!(
            "replace the frozen check only on the Lead's explicit defective-contract decision"
        );
        choice["sideEffect"] = json!("creates one immutable successor only after the Lead chooses the new check and records a defective-contract reason; the checker produced no result, so this form asserts no defect by itself; the blocked predecessor stays unchanged");
    }
    let mut choices = vec![choice];
    let diagnostic_attempt = crate::run::pre_return::replacement_ready(loaded, work)
        .ok()
        .flatten();
    if let Some(attempt_id) = &diagnostic_attempt {
        choices.push(json!({
            "label": "replace an explicitly defective frozen pre-return check",
            "command": command(loaded, Route::new(["work", "replace-check", work, "--pre-return-command", crate::emitted::slot("NEW_PRE_RETURN_COMMAND").as_str(), "--reason", crate::emitted::slot("REASON").as_str(), "--classification", "defective-contract", "--current-binding", binding])),
            "placeholders": ["NEW_PRE_RETURN_COMMAND", "REASON"],
            "stdin": {"required": false, "kind": "none"},
            "source": {"kind":"blocked_worker","diagnosticAttemptId":attempt_id},
            "sideEffect": "preview only; the blocked report must link this known-ended diagnostic; the predecessor and its grant remain unchanged"
        }));
    }
    (choices, diagnostic_attempt)
}

pub(crate) fn project(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    let mut result = full(loaded, work, next, binding);
    let replacement_choice = |choice: &Value| {
        choice["label"].as_str().is_some_and(|label| {
            label.contains("replacement of an explicitly defective frozen check")
                || label.contains("defective frozen pre-return check")
                || label.contains("only on the Lead's explicit defective-contract decision")
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

/// The recorded owned group of a timed-out observation: its state now and, if
/// it still has processes, a few of them by pid and command. The form says so
/// plainly; the recorded group supplies no forward route while it is alive.
/// A running admission whose recorder ended is closed by a retry only when it
/// retains no capture or owned verification effect; the form says why.
fn attach_owned_group(loaded: &Loaded, work: &str, result: &mut Value, next: &Value) {
    let observation = &next["packet"]["checkObservation"];
    if observation["state"] == "running" {
        let retained = crate::run::check_observation::observer_ended(observation)
            && super::resolve(loaded, work)
                .and_then(|ledger| {
                    crate::run::check_observation::retained_effect(loaded, &ledger, observation)
                })
                .unwrap_or(false);
        if retained {
            result["meaning"] = json!(
                "The process observing this check ended without recording its result and the admission retains a partial capture or an owned verification effect. A retry neither closes nor appends over it: recover a known-ended storage failure with work recover-check, or block this Work."
            );
        }
        return;
    }
    let Some(view) = crate::run::owned_group::view(&observation["facts"]) else {
        return;
    };
    let group = view["ownedProcessGroup"].as_u64().unwrap_or_default();
    let meaning = match view["ownedProcessState"].as_str() {
        Some("ended") => format!(
            "The timed-out check's owned process group {group} has no remaining processes; the environmental re-run route is available."
        ),
        Some("alive") => format!(
            "The timed-out check's owned process group {group} still has processes: {}; wait for them to end, or stop this Work. No forward route is available while they run.",
            owned_process_names(&view)
        ),
        _ => format!(
            "The timed-out check's owned process group {group} could not be read on this host; wait and read this Work again. No forward route is available until it is observed ended."
        ),
    };
    if let Some(object) = view.as_object() {
        for (key, value) in object {
            result[key] = value.clone();
        }
    }
    result["meaning"] = json!(meaning);
}

fn owned_process_names(view: &Value) -> String {
    let named = view["ownedProcesses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|process| {
            Some(format!(
                "pid {} ({})",
                process["pid"].as_u64()?,
                process["command"].as_str()?
            ))
        })
        .collect::<Vec<_>>()
        .join(", ");
    if named.is_empty() {
        "unnamed processes".to_owned()
    } else {
        named
    }
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
                        "command": command(loaded, Route::new(["work", "recover-check", work, "--json"])),
                        "readOnly": true, "inspectionOnly": true,
                        "meaning": "Inspect the retained unknown observation without execution or check credit."
                    }
                });
            }
            let never_ran = super::check::blocked_check_never_ran(loaded, work, next);
            let (choices, diagnostic_attempt) =
                blocked_replacement_choices(loaded, work, binding, never_ran);
            if never_ran {
                // No check result exists, so nothing here proves a defective
                // contract. The forward route is still the Lead's bound
                // replacement: only the Lead's explicit defective-contract
                // decision creates a successor, and the form never runs it by
                // itself. The environmental attestation needs a current
                // assignment with a pending observation failure, which a
                // terminal blocked Work does not carry, so it is not offered
                // here; the failed-observation state offers it.
                return json!({
                    "version": 1, "state": "blocked_never_run_check",
                    "binding": binding, "leadChoiceRequired": true,
                    "choices": choices,
                    "mechanicalAction": Value::Null,
                    "meaning": "The frozen check never ran and this blocked Work holds no check result, so nothing here proves a defective contract. Replacing the check is the Lead's explicit decision; without that decision the Work stays blocked.",
                    "constraints": {"classification": "defective-contract", "sourceKind": "blocked_worker", "diagnosticAttemptId":diagnostic_attempt,"binding": binding},
                    "diagnostic": {
                        "command": command(loaded, Route::new(["work", "detail", work, "--json"])),
                        "readOnly": true, "inspectionOnly": true,
                        "meaning": "Read the blocked Work's evidence; this executes no check and records nothing."
                    }
                });
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
            let suffix = Route::new([
                "work",
                "replace-check",
                work,
                "--check-command",
                crate::emitted::slot("NEW_CHECK_COMMAND").as_str(),
                "--reason",
                crate::emitted::slot("REASON").as_str(),
                "--classification",
                "defective-contract",
                "--current-binding",
                binding,
            ]);
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
                json!({"command": command(loaded, Route::new(["work", "check", work])),
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
    let observation_never_ran =
        state == "failed_check_observation" && super::check::observation_never_ran(next);
    let mut choices: Vec<Value> = if state == "unresolved_check_observation" {
        let mut choices: Vec<Value> = outcomes
            .iter()
            .filter(|v| matches!(**v, "blocked" | "rejected"))
            .map(|outcome| return_choice(loaded, work, assignment, binding, outcome, outcome))
            .collect();
        if next["packet"]["checkObservation"]["state"] == "running" {
            let mut preview = choice(
                loaded,
                Route::new(["work", "recover-check", work, "--json"]),
                "preview_ended_failure_recovery",
                &[],
                no_input(),
            );
            preview["readOnly"] = json!(true);
            preview["sideEffect"] = json!("no-change; previews an explicit Lead decision for a retained known-ended storage failure; unknown effects remain blocked");
            choices.push(preview);
        }
        // A timed-out observation whose recorded owned group has ended owes a
        // check result exactly as a never-run checker does: the same
        // environmental route re-pends it. While any owned process is alive
        // this returns None and the form names the processes instead.
        if let Some(form) = environmental::choice(
            loaded,
            work,
            assignment,
            binding,
            next,
            environmental::Source::ClosedOwnedGroup,
        ) {
            choices.push(form);
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
                Route::new([
                    "work",
                    "disposition",
                    work,
                    assignment,
                    "--current-binding",
                    binding,
                    "--decision",
                    "repair",
                    "--reason",
                    crate::emitted::slot("REASON").as_str(),
                    "--repair-boundary",
                    crate::emitted::slot("REPAIR_BOUNDARY").as_str(),
                    "--regression",
                    crate::emitted::slot("REGRESSION").as_str(),
                ]),
                "repair",
                &["REASON", "REPAIR_BOUNDARY", "REGRESSION"],
                no_input(),
            ));
        }
        if available.contains(&"defer") {
            choices.push(choice(
                loaded,
                Route::new([
                    "work",
                    "disposition",
                    work,
                    assignment,
                    "--current-binding",
                    binding,
                    "--decision",
                    "defer",
                    "--reason",
                    crate::emitted::slot("REASON").as_str(),
                ]),
                "defer",
                &["REASON"],
                no_input(),
            ));
        }
        if available.contains(&"reject") {
            choices.push(choice(
                loaded,
                Route::new([
                    "work",
                    "disposition",
                    work,
                    assignment,
                    "--current-binding",
                    binding,
                    "--decision",
                    "reject",
                    "--reason",
                    crate::emitted::slot("REASON").as_str(),
                ]),
                "reject",
                &["REASON"],
                no_input(),
            ));
        }
        if available.contains(&"supersede") {
            choices.push(choice(
                loaded,
                Route::new([
                    "work",
                    "disposition",
                    work,
                    assignment,
                    "--current-binding",
                    binding,
                    "--decision",
                    "supersede",
                    "--reason",
                    crate::emitted::slot("REASON").as_str(),
                    "--repair-boundary",
                    crate::emitted::slot("REPAIR_BOUNDARY").as_str(),
                    "--regression",
                    crate::emitted::slot("REGRESSION").as_str(),
                    "--successor-basis",
                    crate::emitted::slot("SUCCESSOR_BASIS_JSON").as_str(),
                ]),
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
            if observation_never_ran {
                if let Some(form) = environmental::choice(
                    loaded,
                    work,
                    assignment,
                    binding,
                    next,
                    environmental::Source::NeverRan,
                ) {
                    choices.push(form);
                }
            }
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
    if state == "unresolved_check_observation" {
        attach_owned_group(loaded, work, &mut result, next);
    }
    if state == "failed_check" {
        choices.extend(environmental::choice(
            loaded,
            work,
            assignment,
            binding,
            next,
            environmental::Source::FailedCheck,
        ));
        choices.push(choice(
            loaded,
            Route::new([
                "work",
                "replace-check",
                work,
                "--check-command",
                crate::emitted::slot("NEW_CHECK_COMMAND").as_str(),
                "--reason",
                crate::emitted::slot("REASON").as_str(),
                "--classification",
                "defective-contract",
                "--current-binding",
                binding,
            ]),
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
        let suffix = Route::new(["work", "act", work, "--current-binding", binding]);
        let inspect = suffix.clone().words(["--inspect"]);
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
                "command": command(loaded, Route::new(["work", "permit", work, assignment, "--operation", crate::emitted::slot("OPERATION").as_str(), "--request-id", format!("permit-{binding}").as_str()])),
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

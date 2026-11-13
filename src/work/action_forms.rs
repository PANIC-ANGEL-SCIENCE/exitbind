// Projects state-specific action forms for the current binding.
// A displayed form is not a host permission; see `src/work/permit_response.rs`.
//! State-specific, executable next-action descriptors for the Lead.

use crate::config::Loaded;
use crate::emitted::Route;
use serde_json::{json, Value};

mod check_process_display;
mod environmental;
pub(crate) mod fill;
mod quarantine;
mod repair;

pub(crate) const PUBLIC_PERMIT_MEANING: &str = "advisory for ordinary host workspace writes; required:true applies to managed file effects and a selected controlled-effects launch, which require allowed:true. Only a successful controlled launch and protection preflight enforce that file effect. Repeat the exact request ID only to recover its response; this form grants no host permission";

/// Internal evidence of the exact offer selected by the canonical owner.
/// This is not serialized and grants no authority independently of the Work.
#[derive(Debug, Clone)]
pub(crate) struct GrantOffer {
    pub(crate) mutation: Value,
    pub(crate) form: Value,
}

/// A printed action form's command: the route with this configuration,
/// bounded to the action view's budget.
pub(super) fn command(loaded: &Loaded, route: Route) -> Value {
    route.config(&loaded.path).within(16 * 1024).argv_json()
}

pub(crate) fn current_detail_command(loaded: &Loaded, work: &str) -> Value {
    command(loaded, Route::new(["work", "detail", work, "--json"]))
}

/// The request ID from the actual emitted pre-edit form, never a reconstructed
/// request. A malformed or ambiguous form cannot establish delivery proof.
pub(crate) fn permit_request_id(form: &Value) -> Option<&str> {
    if form["required"] != true {
        return None;
    }
    let argv = form["command"]["argv"].as_array()?;
    let mut matches = argv.windows(2).filter(|pair| pair[0] == "--request-id");
    let id = matches.next()?[1].as_str()?;
    if id.is_empty() || matches.next().is_some() {
        None
    } else {
        Some(id)
    }
}

/// Explain the host boundary only in displayed forms. Canonical native forms
/// remain byte-identical for saved delivery proofs and permit recovery.
pub(crate) fn public_permit_guidance(value: &mut Value) {
    for pointer in [
        "/actionForms/beforeEditing",
        "/effectiveAction/beforeEditing",
        "/current/actionForm/beforeEditing",
        "/next/current/actionForm/beforeEditing",
        "/currentDelivery/actionForms/beforeEditing",
        "/currentDelivery/effectiveAction/beforeEditing",
    ] {
        let Some(form) = value.pointer_mut(pointer).filter(|form| form.is_object()) else {
            continue;
        };
        if permit_request_id(form).is_none() {
            continue;
        }
        form["enforcement"] = json!("advisory");
        form["meaning"] = json!(PUBLIC_PERMIT_MEANING);
    }
}

/// The generic fill route is a prefix until a choice and that choice's
/// semantic inputs are selected. Keep it available as an explicitly named
/// template, never as an argv advertised as executable.
pub(crate) fn public_fill_guidance(value: &mut Value) {
    for pointer in [
        "/actionForms/fillRoute",
        "/effectiveAction/fillRoute",
        "/current/actionForm/fillRoute",
        "/next/current/actionForm/fillRoute",
        "/currentDelivery/actionForms/fillRoute",
        "/currentDelivery/effectiveAction/fillRoute",
    ] {
        let Some(route) = value.pointer_mut(pointer).and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(prefix) = route.remove("argv") else {
            continue;
        };
        route.insert("templatePrefix".into(), prefix);
        route.insert("executable".into(), json!(false));
        route.insert("meaning".into(), json!("template prefix: choose a current form and supply its documented inputs; use its complete command for ordinary execution"));
    }
}

/// Fold only equivalent current return choices in a public step copy. The
/// canonical forms and explicit detail remain untouched.
pub(crate) fn public_return_templates(value: &mut Value, config: &std::path::Path) {
    let Some(binding) = value
        .pointer("/current/binding")
        .or_else(|| value.pointer("/next/current/binding"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    if value["effectiveAction"]["exists"] != true {
        return;
    }
    let (Ok(executable), Some(config)) = (crate::emitted::executable(), config.to_str()) else {
        return;
    };
    let embedded_current = value["currentDelivery"]["complete"] == true
        && value["currentDelivery"]["binding"] == binding;
    let reviewer = value
        .pointer("/current/recipient/role")
        .or_else(|| value.pointer("/next/current/recipient/role"))
        == Some(&json!("reviewer"));
    for pointer in [
        "/actionForms",
        "/effectiveAction",
        "/current/actionForm",
        "/next/current/actionForm",
        "/currentDelivery/actionForms",
        "/currentDelivery/effectiveAction",
    ] {
        if pointer.starts_with("/currentDelivery/") && !embedded_current {
            continue;
        }
        let Some(form) = value.pointer_mut(pointer) else {
            continue;
        };
        if form["binding"] != binding {
            continue;
        }
        if let Some(choices) = form.get_mut("choices").and_then(Value::as_array_mut) {
            fold_return_choices(choices, &executable, config);
            if reviewer {
                public_unavailable_reason(choices);
            }
        }
    }
}

/// The reducer's unavailable reasons apply only to that outcome, including
/// when a future public template presents it alongside other outcomes.
fn public_unavailable_reason(choices: &mut [Value]) {
    for choice in choices {
        let unavailable = choice["label"] == "unavailable"
            || (choice["kind"] == "outcome_template"
                && choice["outcomes"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == "unavailable")));
        if !unavailable
            || !choice["placeholders"]
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == "REASON"))
        {
            continue;
        }
        let Some(object) = choice.as_object_mut() else {
            continue;
        };
        let constraints = object
            .entry("inputConstraints")
            .or_insert_with(|| json!({}));
        let Some(constraints) = constraints.as_object_mut() else {
            continue;
        };
        let Some(reason) = constraints
            .entry("REASON")
            .or_insert_with(|| json!({}))
            .as_object_mut()
        else {
            continue;
        };
        let Some(outcomes) = reason
            .entry("whenOutcome")
            .or_insert_with(|| json!({}))
            .as_object_mut()
        else {
            continue;
        };
        outcomes.entry("unavailable").or_insert_with(|| {
            json!({"allowedValues":crate::run::state::FALLBACK_REASONS,
                "meaning":"These values apply only to outcome unavailable; other outcomes use their own reason rules."})
        });
    }
}

/// Normalize a current, complete return form while retaining every common
/// field. Any other command, missing input, or differing metadata stays alone.
fn return_template(choice: &Value, executable: &str, config: &str) -> Option<(Value, String)> {
    let outcome = choice["label"].as_str()?;
    let command = &choice["command"];
    let argv = command["argv"].as_array()?;
    if outcome.is_empty()
        || outcome.starts_with('<')
        // Reviewer unavailability has a distinct, bounded reason vocabulary
        // enforced by the reducer; do not merge it with ordinary returns.
        || outcome == "unavailable"
        || choice.get("kind").is_some()
        || choice.get("outcomes").is_some()
        || choice["fill"].get("choicePlaceholder").is_some()
        || command["sameExecutableRequired"] != false
        || command["sameConfigRequired"] != false
        || argv.len() < 8
        || argv[0] != executable
        || argv[1] != "work"
        || argv[2] != "return"
        || argv[argv.len() - 2] != "--config"
        || argv[argv.len() - 1] != config
        || choice["placeholders"] != json!(["REASON"])
        || choice["stdin"]["required"] != true
        || choice["fill"]["choice"] != outcome
        || choice["fill"]["options"] != json!({"REASON":"--reason"})
        || choice["fill"]["artifactOption"] != "--artifact-file"
    {
        return None;
    }
    let mut outcome_options = argv
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] == "--outcome");
    let (index, pair) = outcome_options.next()?;
    if pair[1] != outcome || outcome_options.next().is_some() {
        return None;
    }
    let mut reasons = argv.windows(2).filter(|pair| pair[0] == "--reason");
    if reasons.next()?[1] != crate::emitted::slot("REASON") || reasons.next().is_some() {
        return None;
    }
    let mut template = choice.clone();
    template["label"] = json!("return");
    template["kind"] = json!("outcome_template");
    template["command"]["argv"][index + 1] = json!(crate::emitted::slot("OUTCOME"));
    template["placeholders"] = json!(["OUTCOME", "REASON"]);
    template["fill"]["choice"] = json!(crate::emitted::slot("OUTCOME"));
    template["fill"]["choicePlaceholder"] = json!("OUTCOME");
    Some((template, outcome.to_owned()))
}

fn fold_return_choices(choices: &mut Vec<Value>, executable: &str, config: &str) {
    let mut groups: Vec<(Value, Vec<(usize, String)>)> = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let Some((template, outcome)) = return_template(choice, executable, config) else {
            continue;
        };
        if let Some((_, entries)) = groups
            .iter_mut()
            .find(|(candidate, _)| *candidate == template)
        {
            entries.push((index, outcome));
        } else {
            groups.push((template, vec![(index, outcome)]));
        }
    }
    groups.retain(|(_, entries)| {
        entries.len() > 1
            && entries.iter().enumerate().all(|(index, (_, outcome))| {
                entries[..index]
                    .iter()
                    .all(|(_, earlier)| earlier != outcome)
            })
    });
    if groups.is_empty() {
        return;
    }
    let mut folded = Vec::with_capacity(choices.len());
    for (index, choice) in choices.iter().enumerate() {
        if let Some((template, entries)) = groups
            .iter()
            .find(|(_, entries)| entries.iter().any(|(member, _)| *member == index))
        {
            if entries[0].0 == index {
                let mut template = template.clone();
                template["outcomes"] = json!(entries
                    .iter()
                    .map(|(_, outcome)| outcome)
                    .collect::<Vec<_>>());
                folded.push(template);
            }
        } else {
            folded.push(choice.clone());
        }
    }
    *choices = folded;
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

pub(crate) fn current_worker_grant(packet: &Value) -> Option<&Value> {
    let loop_state = &packet["context"]["loop"];
    let grant = &loop_state["currentMutation"];
    (grant["eventSha256"].is_string()
        && loop_state["currentGrantEventSha256"] == grant["eventSha256"]
        && grant["unit"] == "worker-mutation"
        && grant["runId"] == packet["context"]["run"]["id"]
        && grant["attempt"] == packet["attempt"]
        && grant["subjectSha256"] == packet["context"]["subject"]["sha256"]
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
                || label == "preview interrupted pre-return recovery"
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
    full_with_grant_offer(loaded, work, next, binding).0
}

/// Internal offer evidence comes from the exact form-selection branch, before
/// native projection. It is never reconstructed from the serialized form.
pub(crate) fn full_with_grant_offer(
    loaded: &Loaded,
    work: &str,
    next: &Value,
    binding: &str,
) -> (Value, Option<GrantOffer>) {
    let mut offer = None;
    let mut result = forms(loaded, work, next, binding, &mut offer);
    fill::attach(loaded, work, binding, &mut result);
    (result, offer)
}

fn forms(
    loaded: &Loaded,
    work: &str,
    next: &Value,
    binding: &str,
    offer: &mut Option<GrantOffer>,
) -> Value {
    match super::pre_return_recovery::action(loaded, work, next, binding) {
        Ok(Some(mut form)) => {
            let governor = next["packet"]["context"]["loop"]["state"].as_str();
            if matches!(
                governor,
                Some("replan_required" | "evidence_required" | "blocked")
            ) {
                form["choices"] = json!([]);
                form["governorBoundary"] = json!({"state":governor,
                    "meaning":"The existing governor requirement must be addressed before reconciliation; changing its binding may make this admission ineligible."});
                if matches!(governor, Some("replan_required" | "evidence_required")) {
                    if let Some(assignment) = next["assignment"].as_str() {
                        repair::attach(loaded, work, assignment, binding, next, &mut form, offer);
                    }
                }
            }
            return form;
        }
        Ok(None) => {}
        Err(error) => {
            return json!({"version":1,"state":"pre_return_evidence_unavailable",
            "binding":binding,"leadChoiceRequired":true,"choices":[],"mechanicalAction":Value::Null,
            "meaning":format!("Pre-return state could not be verified: {error}; no completion is offered.")})
        }
    }
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
            let mut report = choice(
                loaded,
                Route::new(["work", "report-lost-caller", work, "--json"]),
                "preview_lost_caller_report",
                &[],
                no_input(),
            );
            report["readOnly"] = json!(true);
            report["sideEffect"] = json!("no-change; previews a configured-Lead report, preserving unknown execution and no credit");
            choices.push(report);
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
        check_process_display::attach(loaded, work, &mut result, next);
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
        let grant = current_worker_grant(&next["packet"]);
        let governor_phase = next["packet"]["context"]["loop"]["state"].as_str();
        let held = has_held_results(next);
        if let Some(grant) = grant.filter(|_| !held) {
            let form = json!({
                "governorEventSha256": grant["eventSha256"],
                "operation": grant["operation"],
                "state": "current_for_this_worker_assignment",
                "use": "the exact current grant may support the present mutation; it grants no host permissions",
            });
            *offer = Some(GrantOffer {
                mutation: grant.clone(),
                form: form.clone(),
            });
            result["currentGrant"] = form;
        }
        let informed = next["packet"]["context"]["loop"]["completionInformation"].is_object();
        if let Some(next_request) = match governor_phase {
            Some("replan_required") if informed => None,
            Some("replan_required") => Some(json!({
                "requiredAction": "work replan",
                "meaning": "before another mutation request, complete a material re-plan or record meaningful new exact evidence; inspect current detail for available forms"
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
    repair::attach(loaded, work, assignment, binding, next, &mut result, offer);
    result
}

#[cfg(test)]
mod public_template_tests {
    use super::*;

    #[test]
    fn reviewer_unavailable_reasons_are_conditional_on_that_outcome() {
        let mut choices = vec![
            json!({"label":"unavailable","placeholders":["REASON"]}),
            json!({"label":"return","kind":"outcome_template",
                "outcomes":["approved","rework","unavailable"],"placeholders":["OUTCOME","REASON"]}),
            json!({"label":"approved","placeholders":["REASON"]}),
        ];
        public_unavailable_reason(&mut choices);
        for choice in &choices[..2] {
            assert_eq!(
                choice["inputConstraints"]["REASON"]["whenOutcome"]["unavailable"]["allowedValues"],
                json!(crate::run::state::FALLBACK_REASONS)
            );
            assert!(
                choice["inputConstraints"]["REASON"]["whenOutcome"]["unavailable"]["meaning"]
                    .as_str()
                    .unwrap()
                    .contains("only")
            );
            assert!(choice["inputConstraints"]["REASON"]
                .get("allowedValues")
                .is_none());
        }
        assert!(choices[2].get("inputConstraints").is_none());
        let mut existing = vec![json!({"label":"unavailable","placeholders":["REASON"],
            "inputConstraints":{"REASON":{"unknown":"preserve"}}})];
        public_unavailable_reason(&mut existing);
        assert_eq!(
            existing[0]["inputConstraints"]["REASON"]["unknown"],
            "preserve"
        );
        assert_eq!(
            existing[0]["inputConstraints"]["REASON"]["whenOutcome"]["unavailable"]
                ["allowedValues"],
            json!(crate::run::state::FALLBACK_REASONS)
        );
    }

    fn return_choice(outcome: &str) -> Value {
        json!({"label":outcome,
            "command":{"argv":["/bin/exitbind","work","return","smw_x","sma_x",
                "--current-binding","binding","--outcome",outcome,
                "--reason","<REASON>","--config","/tmp/config.json"],
                "sameExecutableRequired":false,"sameConfigRequired":false},
            "placeholders":["REASON"],
            "fill":{"choice":outcome,"options":{"REASON":"--reason"},
                "artifactOption":"--artifact-file"},
            "stdin":{"required":true,"kind":"result_artifact"},
            "sideEffect":"records the result"})
    }

    #[test]
    fn equivalent_returns_share_only_the_documented_semantic_slots() {
        let original = vec![return_choice("scoped"), return_choice("blocked")];
        let mut choices = original.clone();
        fold_return_choices(&mut choices, "/bin/exitbind", "/tmp/config.json");
        assert_eq!(choices.len(), 1);
        let template = &choices[0];
        assert_eq!(template["kind"], "outcome_template");
        assert_eq!(template["outcomes"], json!(["scoped", "blocked"]));
        assert_eq!(template["placeholders"], json!(["OUTCOME", "REASON"]));
        assert_eq!(template["fill"]["choice"], "<OUTCOME>");
        assert_eq!(template["fill"]["choicePlaceholder"], "OUTCOME");
        assert_eq!(template["fill"]["options"], json!({"REASON":"--reason"}));
        assert_eq!(template["stdin"], original[0]["stdin"]);
        for (index, outcome) in ["scoped", "blocked"].into_iter().enumerate() {
            let actual = template["command"]["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|word| {
                    if word == "<OUTCOME>" {
                        json!(outcome)
                    } else {
                        word.clone()
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual,
                *original[index]["command"]["argv"].as_array().unwrap()
            );
        }
    }

    #[test]
    fn differing_forms_partial_routes_and_other_actions_are_never_folded() {
        let baseline = return_choice("scoped");
        let mut cases = Vec::new();
        let mut other_binding = return_choice("blocked");
        other_binding["command"]["argv"][6] = json!("another-binding");
        cases.push(other_binding);
        let mut other_input = return_choice("blocked");
        other_input["stdin"]["required"] = json!(false);
        cases.push(other_input);
        let mut partial = return_choice("blocked");
        partial["command"]["sameConfigRequired"] = json!(true);
        cases.push(partial);
        let mut unknown = return_choice("blocked");
        unknown["unknown"] = json!("different semantic fact");
        cases.push(unknown);
        let mut mislabeled = return_choice("blocked");
        mislabeled["label"] = json!("different label");
        cases.push(mislabeled);
        let mut other_action = return_choice("blocked");
        other_action["command"]["argv"][2] = json!("check");
        cases.push(other_action);
        cases.push(return_choice("unavailable"));
        cases.push(return_choice("scoped"));
        for other in cases {
            let mut choices = vec![baseline.clone(), other];
            let original = choices.clone();
            fold_return_choices(&mut choices, "/bin/exitbind", "/tmp/config.json");
            assert_eq!(choices, original);
        }

        let unrelated = json!({"label":"preview recovery", "command":{"argv":["recovery"]}});
        let mut choices = vec![baseline, unrelated.clone(), return_choice("blocked")];
        fold_return_choices(&mut choices, "/bin/exitbind", "/tmp/config.json");
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0]["outcomes"], json!(["scoped", "blocked"]));
        assert_eq!(choices[1], unrelated);
    }
}

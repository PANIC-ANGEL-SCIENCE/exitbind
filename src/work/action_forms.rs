//! State-specific, executable next-action descriptors for the Lead.

use crate::config::Loaded;
use serde_json::{json, Value};

fn command(loaded: &Loaded, mut suffix: Vec<String>) -> Value {
    suffix.push("--config".into());
    let command = super::response_recovery::bounded_argv(suffix, loaded.path.to_str(), 16 * 1024);
    json!({
        "argv": command.argv,
        "sameConfigRequired": command.same_config,
        "sameExecutableRequired": command.same_executable,
    })
}

fn choice(
    loaded: &Loaded,
    suffix: Vec<String>,
    label: &str,
    placeholders: &[&str],
    input: Value,
) -> Value {
    json!({
        "label": label,
        "command": command(loaded, suffix),
        "placeholders": placeholders,
        "stdin": input,
        "sideEffect": "records only after the selected actor supplies the placeholders and executes the bound command",
    })
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
            "<REASON>".into(),
        ],
        label,
        &["REASON"],
        return_input(),
    )
}

pub(crate) fn project(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    let mut result = full(loaded, work, next, binding);
    if let Some(choices) = result.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            if let Some(object) = choice.as_object_mut() {
                object.remove("command");
                object.remove("sideEffect");
                object.remove("stdin");
            }
        }
    }
    result["detail"] = json!({
        "route": "current.details.grouped",
        "binding": binding,
        "summary": "follow the current grouped detail route for complete executable forms"
    });
    result
}

pub(crate) fn full(loaded: &Loaded, work: &str, next: &Value, binding: &str) -> Value {
    let Some(assignment) = next["assignment"].as_str() else {
        return json!({"version": 1, "state": next["action"], "binding": binding,
            "leadChoiceRequired": false, "choices": [],
            "mechanicalAction": if next["action"] == "check" {
                json!({"command": command(loaded, vec!["work".into(), "check".into(), work.into()]),
                    "meaning": "execute only the current frozen applicable check; failure requires a Lead decision"})
            } else { Value::Null }});
    };
    let outcomes = allowed_outcomes(next);
    let state = if next["packet"].get("pendingDisposition").is_some() {
        "pending_review_finding"
    } else if next["action"] == "lead_decision"
        && matches!(
            next["progress"]["reason"]["code"].as_str(),
            Some("check_failed" | "preservation_failed")
        )
    {
        "failed_check"
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
    let choices: Vec<Value> = if state == "pending_review_finding" {
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
                    "<REASON>".into(),
                    "--repair-boundary".into(),
                    "<REPAIR_BOUNDARY>".into(),
                    "--regression".into(),
                    "<REGRESSION>".into(),
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
                    "<REASON>".into(),
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
                    "<REASON>".into(),
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
                    "<REASON>".into(),
                    "--repair-boundary".into(),
                    "<REPAIR_BOUNDARY>".into(),
                    "--regression".into(),
                    "<REGRESSION>".into(),
                    "--successor-basis".into(),
                    "<SUCCESSOR_BASIS_JSON>".into(),
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
        choices
    } else if state == "failed_check" {
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
    result["choices"] = json!(choices);
    if next["action"] == "spawn" && next["role"] == "worker" {
        result["beforeEditing"] = json!({
            "command": command(loaded, vec!["work".into(), "permit".into(), work.into(),
                assignment.into(), "--operation".into(), "<OPERATION>".into()]),
            "placeholders": ["OPERATION"], "required": true,
            "meaning": "obtain allowed:true before editing; this form grants no host permission"});
    }
    result
}

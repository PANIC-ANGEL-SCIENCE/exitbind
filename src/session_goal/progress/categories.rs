use crate::config::Loaded;
use serde_json::{json, Value};

const MAX_ITEMS: usize = 32;

pub(super) fn project(loaded: Option<&Loaded>, record: &Value, project_selected: bool) -> Value {
    ["obligations", "findings", "blockers", "decisions", "externalActions"]
        .into_iter()
        .map(|category| {
            let state = record["categories"][category].as_str();
            let valid_state = matches!(state, Some("considered" | "none_applicable"));
            let items = record[category].as_array();
            let values = items.map(|items| items.iter().take(MAX_ITEMS).map(|item| {
                let mut value = json!({
                    "id": item["id"],
                    "disposition": item["disposition"],
                    "valid": matches!(item["disposition"].as_str(), Some("open" | "accepted" | "outside_scope" | "successor" | "direct")),
                });
                if item["disposition"] == "open" {
                    value["nextActions"] = json!(item_routes(loaded, record, category, item, project_selected));
                    value["ownerDecisionRequired"] = json!(true);
                } else if item["disposition"] == "direct" {
                    let (state, reason) = direct_state(loaded, record, item);
                    value["directEvidence"] = json!({"state":state,"reason":reason});
                    if state != "current" {
                        value["nextActions"] = json!(item_routes(loaded, record, category, item, project_selected)
                            .into_iter().filter(|route| route["disposition"] == "direct").collect::<Vec<_>>());
                        value["ownerDecisionRequired"] = json!(true);
                    }
                } else if matches!(item["disposition"].as_str(), Some("accepted" | "outside_scope" | "successor")) {
                    let (state, reason) = result_state(loaded, record, item);
                    value["resultEvidence"] = json!({"state":state,"reason":reason});
                    if state != "current" {
                        value["nextActions"] = json!(item_revalidation_route(loaded, record, category, item, project_selected, state));
                        value["ownerDecisionRequired"] = json!(true);
                    }
                }
                value
            }).collect::<Vec<_>>()).unwrap_or_default();
            let mut next_actions = Vec::new();
            if !valid_state {
                if let Some(loaded) = loaded {
                    next_actions.push(category_route(loaded, record, category, "--consider", project_selected));
                    if items.is_some_and(Vec::is_empty) {
                        next_actions.push(category_route(loaded, record, category, "--none-applicable", project_selected));
                    }
                }
            }
            (category.to_owned(), json!({
                "state": if valid_state { json!(state) } else { json!("unavailable") },
                "validStates":["considered","none_applicable"],
                "items":values,
                "itemsOmitted":items.map_or(0, |items| items.len().saturating_sub(MAX_ITEMS)),
                "validItemDispositions":["open","accepted","outside_scope","successor","direct"],
                "nextActions":next_actions,
                "ownerDecisionRequired":true,
            }))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Reuse the close validator's input-bound direct evidence check for both the
/// category view and the ready-to-close route. A route is offered only when a
/// Lead-reported item is stale or cannot be verified; executing it records a
/// fresh explicit owner observation through the existing incorporate command.
pub(crate) fn category_revalidation_route(
    loaded: &Loaded,
    record: &Value,
    project_selected: bool,
) -> Option<Value> {
    for category in crate::session_goal::CATEGORIES {
        for item in record[category].as_array().into_iter().flatten() {
            if item["disposition"] == "direct" {
                let (state, _) = direct_state(Some(loaded), record, item);
                if state != "current" {
                    return item_routes(Some(loaded), record, category, item, project_selected)
                        .into_iter()
                        .find(|route| route["disposition"] == "direct")
                        .map(|mut route| {
                            route["purpose"] = json!("revalidate_stale_direct_category");
                            route["evidenceState"] = json!(state);
                            route["effect"] = json!("records a new explicit Lead observation bound to current project inputs; it does not create or replay Work");
                            route
                        });
                }
            } else if matches!(
                item["disposition"].as_str(),
                Some("accepted" | "outside_scope" | "successor")
            ) {
                let (state, _) = result_state(Some(loaded), record, item);
                if state != "current" {
                    return Some(item_revalidation_route(
                        Some(loaded),
                        record,
                        category,
                        item,
                        project_selected,
                        state,
                    ));
                }
            }
        }
    }
    None
}

fn result_state(
    loaded: Option<&Loaded>,
    record: &Value,
    item: &Value,
) -> (&'static str, &'static str) {
    let Some(loaded) = loaded else {
        return ("unavailable", "project_context_unavailable");
    };
    let Some(references) = item["resultRefs"]
        .as_array()
        .filter(|refs| !refs.is_empty())
    else {
        return ("unavailable", "category_result_reference_missing");
    };
    for reference in references {
        let Some(reference) = reference.as_str() else {
            return ("unavailable", "category_result_reference_invalid");
        };
        let current = if super::super::requirements::named(record) {
            super::super::requirements::category_result_current(loaded, record, reference)
        } else {
            match super::super::recovery::result_ref_evidence(loaded, record, reference) {
                Ok(Some(evidence)) => Ok(evidence.accepted && evidence.artifact_current),
                Ok(None) => crate::run::result_ref_evidence(loaded, reference).map(|evidence| {
                    evidence.is_some_and(|evidence| evidence.accepted && evidence.artifact_current)
                }),
                Err(error) => Err(error),
            }
        };
        match current {
            Ok(true) => continue,
            Ok(false) => return ("stale", "category_result_not_current"),
            Err(_) => return ("unavailable", "category_result_unavailable"),
        }
    }
    ("current", "category_results_current")
}

fn item_revalidation_route(
    loaded: Option<&Loaded>,
    record: &Value,
    category: &str,
    item: &Value,
    project: bool,
    state: &str,
) -> Value {
    let Some(loaded) = loaded else {
        return Value::Null;
    };
    let (field, value) = match category {
        "obligations" => ("--obligation", item["text"].as_str()),
        "findings" => ("--finding", item["text"].as_str()),
        "blockers" => ("--blocker", item["text"].as_str()),
        "decisions" => ("--decision", item["text"].as_str()),
        "externalActions" => ("--external-action", item["action"].as_str()),
        _ => return Value::Null,
    };
    let Some(value) = value else {
        return Value::Null;
    };
    let disposition = item["disposition"].as_str().unwrap_or("accepted");
    let mut args = vec![
        "goal".into(),
        "incorporate".into(),
        "--goal-id".into(),
        record["goalId"].as_str().unwrap_or_default().into(),
        "--goal".into(),
        record["goal"].as_str().unwrap_or_default().into(),
        field.into(),
        value.into(),
        "--disposition".into(),
        disposition.into(),
        "--result-ref".into(),
        crate::emitted::slot("CURRENT_ACCEPTED_WORK"),
    ];
    if category == "externalActions" {
        args.extend([
            "--scope".into(),
            item["scope"].as_str().unwrap_or("in_scope").into(),
        ]);
    }
    if project {
        args.push("--project".into());
    }
    args.push("--json".into());
    let mut route = crate::work::compact::continuation_route(&loaded.path, args);
    route["purpose"] = json!("replace_stale_goal_category_evidence");
    route["evidenceState"] = json!(state);
    route["placeholders"] = json!({"CURRENT_ACCEPTED_WORK":"an accepted current Work the Lead decides supports this exact category item"});
    route["effect"] = json!("replaces this category item's result reference after explicit Lead re-evaluation; it does not create or replay Work");
    route["ownerDecisionRequired"] = json!(true);
    route["disposition"] = json!(disposition);
    route
}

fn direct_state(
    loaded: Option<&Loaded>,
    record: &Value,
    item: &Value,
) -> (&'static str, &'static str) {
    let Some(loaded) = loaded else {
        return ("unavailable", "project_inputs_unavailable");
    };
    match super::super::direct_completion_inputs(loaded, record, item) {
        Ok(Some((recorded, current))) if recorded == current => ("current", "current_inputs_match"),
        Ok(Some(_)) => ("stale", "direct_item_inputs_changed"),
        Ok(None) => ("unavailable", "direct_item_invalid"),
        Err(_) => ("unavailable", "direct_item_inputs_unavailable"),
    }
}

pub(super) fn attach_when_goal_is_valid(
    mut view: Value,
    loaded: Option<&Loaded>,
    record: &Value,
    project_selected: bool,
) -> Value {
    if record["goalId"]
        .as_str()
        .is_some_and(|id| !id.trim().is_empty())
        && record["goal"]
            .as_str()
            .is_some_and(|goal| !goal.trim().is_empty())
    {
        view["categoryDispositions"] = project(loaded, record, project_selected);
    }
    view
}

fn category_route(
    loaded: &Loaded,
    record: &Value,
    category: &str,
    choice: &str,
    project: bool,
) -> Value {
    let mut args = vec![
        "goal".into(),
        "incorporate".into(),
        "--goal-id".into(),
        record["goalId"].as_str().unwrap_or_default().into(),
        "--goal".into(),
        record["goal"].as_str().unwrap_or_default().into(),
        choice.into(),
        category.into(),
        "--json".into(),
    ];
    if project {
        args.push("--project".into());
    }
    crate::work::compact::continuation_route(&loaded.path, args)
}

fn item_routes(
    loaded: Option<&Loaded>,
    record: &Value,
    category: &str,
    item: &Value,
    project: bool,
) -> Vec<Value> {
    let Some(loaded) = loaded else {
        return Vec::new();
    };
    let (field, value) = match category {
        "obligations" => ("--obligation", item["text"].as_str()),
        "findings" => ("--finding", item["text"].as_str()),
        "blockers" => ("--blocker", item["text"].as_str()),
        "decisions" => ("--decision", item["text"].as_str()),
        "externalActions" => ("--external-action", item["action"].as_str()),
        _ => return Vec::new(),
    };
    let Some(value) = value else {
        return Vec::new();
    };
    ["accepted", "outside_scope", "successor", "direct"]
        .into_iter()
        .map(|choice| {
            let mut args = vec![
                "goal".into(),
                "incorporate".into(),
                "--goal-id".into(),
                record["goalId"].as_str().unwrap_or_default().into(),
                "--goal".into(),
                record["goal"].as_str().unwrap_or_default().into(),
                field.into(),
                value.into(),
            ];
            if choice == "direct" {
                args.push("--direct".into());
                if category == "externalActions" {
                    args.extend([
                        "--external-scope".into(),
                        item["scope"].as_str().unwrap_or("in_scope").into(),
                    ]);
                }
            } else {
                args.extend([
                    "--disposition".into(),
                    choice.into(),
                    "--result-ref".into(),
                    crate::emitted::slot("ACCEPTED_WORK"),
                ]);
                if category == "externalActions" {
                    args.extend([
                        "--scope".into(),
                        item["scope"].as_str().unwrap_or("in_scope").into(),
                    ]);
                }
            }
            if project {
                args.push("--project".into());
            }
            args.push("--json".into());
            let mut route = crate::work::compact::continuation_route(&loaded.path, args);
            route["ownerDecisionRequired"] = json!(true);
            route["disposition"] = json!(choice);
            route
        })
        .collect()
}

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
                    "ACCEPTED_WORK".into(),
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

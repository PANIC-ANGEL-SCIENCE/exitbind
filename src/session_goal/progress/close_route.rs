//! Emit only a currently executable whole-Goal close route.
use crate::config::Loaded;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn next_action(
    loaded: Option<&Loaded>,
    goal_root: Option<&Path>,
    project_selected: bool,
    record: &Value,
) -> Option<Value> {
    let loaded =
        loaded.filter(|_| goal_root.is_some() && !super::super::requirements::named(record))?;
    let root = goal_root.unwrap_or(&loaded.state_root);
    let goal_id = record["goalId"].as_str()?;
    let governed_ref = super::super::CATEGORIES
        .iter()
        .flat_map(|category| record[*category].as_array().into_iter().flatten())
        .filter(|item| item["disposition"] != "direct")
        .flat_map(|item| item["resultRefs"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .next();
    let (direct, result_ref) = if let Some(reference) = governed_ref {
        super::super::validate_governed_candidate(loaded, root, record, goal_id, reference).ok()?;
        (false, Some(reference))
    } else {
        super::super::validate_direct_candidate(loaded, root, record, goal_id, None).ok()?;
        (true, None)
    };
    let mut args = vec![
        "goal".into(),
        "close".into(),
        "--goal-id".into(),
        goal_id.into(),
    ];
    if direct {
        args.push("--direct".into());
    }
    if let Some(reference) = result_ref {
        args.extend(["--result-ref".into(), reference.into()]);
    }
    args.extend(["--view".into(), "event".into(), "--json".into()]);
    if project_selected {
        args.push("--project".into());
    }
    let mut route = crate::work::compact::continuation_route(&loaded.path, args);
    route["ownerDecisionRequired"] = json!(true);
    Some(route)
}

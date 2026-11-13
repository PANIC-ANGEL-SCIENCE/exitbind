// Projects bounded Work and Goal presentation fields.
// Compact Goal progress retains at most eight tasks; see `src/work/compact.rs`.
//! Bounded Work presentation projections.

use super::{bounded_text, bounded_value, copy_if_present};
use serde_json::{json, Map, Value};

pub(super) fn minimal_presentation(presentation: &Value) -> Value {
    let mut state = Map::new();
    if let Some(review) = presentation
        .get("state")
        .and_then(|state| state.get("review"))
    {
        state.insert("review".into(), bounded_value(review, 128));
    }
    let mut result = Map::new();
    result.insert("state".into(), Value::Object(state));
    if let Some(goal) = presentation.get("goalProgress") {
        let mut progress = Map::new();
        if let Some(overall) = goal
            .get("overall")
            .and_then(Value::as_str)
            .filter(|overall| {
                matches!(
                    *overall,
                    "not_requested"
                        | "context_only"
                        | "unavailable"
                        | "in_progress"
                        | "complete"
                        | "blocked"
                )
            })
        {
            progress.insert("overall".into(), json!(overall));
        }
        if let Some(decomposition) = goal.get("decomposition").filter(|value| {
            value.is_object() && serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 256)
        }) {
            progress.insert("decomposition".into(), decomposition.clone());
        }
        copy_if_present(goal, &mut progress, "adoptRoute");
        if !progress.is_empty() {
            result.insert("goalProgress".into(), Value::Object(progress));
        }
    }
    copy_if_present(presentation, &mut result, "terminal");
    Value::Object(result)
}

pub(crate) fn compact_presentation(presentation: &Value) -> Value {
    // Keep terminal and transition meaning, while bounding the additive goal
    // projection by field. Full goal status remains available from
    // `goal status --json`.
    let Some(object) = presentation.as_object() else {
        return Value::Null;
    };
    let mut result = object.clone();
    if let Some(goal) = object.get("goalProgress") {
        result.insert("goalProgress".into(), compact_goal_progress(goal));
    }
    Value::Object(result)
}

pub(crate) fn compact_goal_progress(goal: &Value) -> Value {
    let mut compact = Map::new();
    for key in [
        "overall",
        "decomposition",
        "resultReadiness",
        "workAssociation",
        "adoptRoute",
    ] {
        if let Some(value) = goal.get(key) {
            compact.insert(key.into(), value.clone());
        }
    }
    if let Some(value) = goal["goal"].as_str() {
        compact.insert("goal".into(), json!(bounded_text(value, 96)));
    }
    if let Some(tasks) = goal["tasks"].as_array() {
        let omitted = tasks.len().saturating_sub(8);
        let tasks = tasks
            .iter()
            .take(8)
            .map(|task| {
                json!({
                    "id": task["id"].as_str().map(|value| bounded_text(value, 64)),
                    "taskId": task["taskId"],
                    "label": task["label"].as_str().map(|value| bounded_text(value, 64)),
                    "state": task["state"],
                    "disposition": task["disposition"],
                    "performed": task["performed"],
                    "current": task["current"],
                    "currentReason": task["currentReason"],
                })
            })
            .collect::<Vec<_>>();
        compact.insert("tasks".into(), json!(tasks));
        compact.insert(
            "taskOmissions".into(),
            json!(omitted + goal["decomposition"]["omitted"].as_u64().unwrap_or(0) as usize),
        );
    }
    if let Some(value) = goal["systemText"].as_str() {
        if value.chars().count() <= 512 {
            compact.insert("systemText".into(), json!(value));
        } else {
            compact.insert("systemTextOmitted".into(), json!(true));
            compact.insert(
                "systemText".into(),
                json!("Goal status: requires detail | Result readiness: requires detail"),
            );
        }
    }
    Value::Object(compact)
}

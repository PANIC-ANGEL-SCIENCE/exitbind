//! Product-owned overall-goal and divided-task progress projection.
//!
//! This read-only view derives task state from the canonical Lead session-goal
//! record. Exact-result readiness remains a separate field and is never used
//! as a proxy for the external user's overall goal.

use crate::config::Loaded;
use serde_json::{json, Value};

const MAX_TASKS: usize = 32;
const MAX_DISPLAY_CHARS: usize = 180;
const SYSTEM_TEXT_MAX_GOAL_CHARS: usize = 64;
const SYSTEM_TEXT_MAX_TASK_CHARS: usize = 32;

pub(super) fn project(
    loaded: Option<&Loaded>,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
) -> Value {
    project_with_limit(loaded, record, rendered, result, MAX_TASKS, false)
}

fn project_with_limit(
    loaded: Option<&Loaded>,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
    task_limit: usize,
    include_canonical_id: bool,
) -> Value {
    let Some(record) = record else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "no canonical Lead goal",
            None,
            "unavailable",
        );
    };
    let Some(goal) = record["goal"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "canonical Lead goal is missing",
            None,
            "unavailable",
        );
    };
    let overall_state = overall_state(record, rendered);
    if super::requirements::named(record) {
        if let Some(loaded) = loaded {
            return named_progress(loaded, record, overall_state, result, task_limit);
        }
    }
    let Some(items) = record["obligations"].as_array() else {
        return unavailable(
            result,
            "task_decomposition_unavailable",
            "Lead task decomposition is unavailable",
            Some(goal),
            overall_state,
        );
    };
    if items.is_empty() {
        return unavailable(
            result,
            "task_decomposition_unavailable",
            "Lead has not divided the goal into tasks",
            Some(goal),
            overall_state,
        );
    }

    let mut tasks = Vec::new();
    let mut complete = 0usize;
    for item in items {
        let id = item["id"]
            .as_str()
            .or_else(|| item["text"].as_str())
            .unwrap_or("unknown task");
        let disposition = item["disposition"].as_str().unwrap_or("unknown");
        let performed = matches!(disposition, "accepted" | "direct");
        let current = if performed {
            task_current(loaded, record, item)
        } else {
            None
        };
        let state = match disposition {
            "accepted" | "direct" if current == Some(true) => {
                complete += 1;
                "complete"
            }
            "accepted" | "direct" => "stale",
            "outside_scope" => "outside_scope",
            "successor" => "superseded",
            "open" => "in_progress",
            _ => "unknown",
        };
        if tasks.len() < task_limit {
            let mut task = json!({
                "id": display(id),
                // `id` is a bounded human label for compatibility. The hash
                // is a bounded stable machine identity; the exact canonical
                // id is included only on the explicitly bound detail route.
                "taskId": crate::evidence::hash::text(id),
                "label": display(id),
                "state": state,
                "disposition": disposition,
                "performed": performed,
                "current": current,
                "currentReason": match current {
                    Some(true) => "current",
                    Some(false) => "historical_or_drifted",
                    None => "currentness_unavailable",
                },
            });
            if include_canonical_id {
                task["canonicalId"] = json!(id);
            }
            tasks.push(task);
        }
    }

    let decomposition = json!({
        "available": true,
        "total": items.len(),
        "complete": complete,
        "omitted": items.len().saturating_sub(tasks.len()),
    });
    let overall = overall_state;
    let result_readiness = readiness(result);
    let mut view = json!({
        "overall": overall,
        "goal": display(goal),
        "decomposition": decomposition,
        "tasks": tasks,
        "resultReadiness": result_readiness,
    });
    view["systemText"] = json!(system_text(
        overall,
        goal,
        complete,
        items.len(),
        &view["tasks"],
        &view["resultReadiness"],
    ));
    view
}

fn overall_state(record: &Value, rendered: &Value) -> &'static str {
    if rendered["explicitLeadClosure"] == true {
        "complete"
    } else if has_open_blocker(record) {
        "blocked"
    } else {
        "in_progress"
    }
}

fn named_progress(
    loaded: &Loaded,
    record: &Value,
    overall: &'static str,
    result: Option<&Value>,
    limit: usize,
) -> Value {
    let projection = match super::requirements::projection(loaded, record) {
        Ok(value) => value,
        Err(_) => {
            return unavailable(
                result,
                "named_coverage_unavailable",
                "named requirement evidence is unavailable",
                record["goal"].as_str(),
                overall,
            )
        }
    };
    let Some(rows) = projection["requirements"].as_array() else {
        return unavailable(
            result,
            "named_coverage_unavailable",
            "named requirement evidence is unavailable",
            record["goal"].as_str(),
            overall,
        );
    };
    let complete = rows.iter().filter(|row| row["satisfied"] == true).count();
    let tasks=rows.iter().take(limit).map(|row|json!({"id":row["id"],"taskId":crate::evidence::hash::text(row["id"].as_str().unwrap_or_default()),
        "label":display(row["text"].as_str().unwrap_or_default()),"state":if row["satisfied"]==true {json!("complete")} else {row["state"].clone()},
        "requirementRevision":row["revision"],"mapped":row["mapped"],"performed":row["satisfied"],"current":row["satisfied"],
        "currentReason":row["state"],"nextAction":row["nextAction"]})).collect::<Vec<_>>();
    let mut view = json!({"overall":overall,"goal":display(record["goal"].as_str().unwrap_or_default()),
        "decomposition":{"available":true,"total":rows.len(),"complete":complete,"omitted":rows.len().saturating_sub(tasks.len())},
        "tasks":tasks,"coverageConfirmed":projection["coverageConfirmed"],"nextAction":projection["nextAction"],"resultReadiness":readiness(result)});
    view["systemText"] = json!(system_text(
        overall,
        record["goal"].as_str().unwrap_or_default(),
        complete,
        rows.len(),
        &view["tasks"],
        &view["resultReadiness"]
    ));
    view
}

fn has_open_blocker(record: &Value) -> bool {
    record["blockers"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["disposition"] == "open"))
}

fn unavailable(
    result: Option<&Value>,
    code: &str,
    detail: &str,
    goal: Option<&str>,
    overall: &'static str,
) -> Value {
    let result_readiness = readiness(result);
    let mut view = json!({
        "overall": if goal.is_some() { overall } else { "unavailable" },
        "goal": goal.map(display),
        "decomposition": {"available": false, "reason": code},
        "tasks": [],
        "resultReadiness": result_readiness,
    });
    let goal_label = goal
        .map(|value| system_display(value, SYSTEM_TEXT_MAX_GOAL_CHARS))
        .map_or_else(
            || "UNAVAILABLE".to_owned(),
            |goal| {
                format!(
                    "{} | {goal}",
                    overall.replace('_', " ").to_ascii_uppercase()
                )
            },
        );
    view["systemText"] = json!(format!(
        "Goal: {goal_label} | Tasks: unavailable ({}) | Result readiness: {}",
        system_display(detail, 96),
        readiness_label(&view["resultReadiness"])
    ));
    view
}

fn readiness(result: Option<&Value>) -> Value {
    let Some(result) = result else {
        return json!({"state": "unavailable", "reason": "result_readiness_unavailable"});
    };
    json!({
        "state": result["state"].as_str().unwrap_or("unknown"),
        "applicable": result["applicable"],
        "reason": result["reason"]["code"],
        "detail": result["reason"]["detail"],
    })
}

fn readiness_label(value: &Value) -> &str {
    value["state"].as_str().unwrap_or("unknown")
}

fn system_text(
    overall: &str,
    goal: &str,
    complete: usize,
    total: usize,
    tasks: &Value,
    readiness: &Value,
) -> String {
    let task_text = tasks
        .as_array()
        .into_iter()
        .flatten()
        .take(3)
        .filter_map(|task| {
            Some(format!(
                "{}={}",
                system_display(task["id"].as_str()?, SYSTEM_TEXT_MAX_TASK_CHARS),
                task["state"].as_str()?
            ))
        })
        .collect::<Vec<_>>();
    let omitted = total.saturating_sub(task_text.len());
    let details = if task_text.is_empty() {
        String::new()
    } else {
        format!(" | {}", task_text.join(", "))
    };
    format!(
        "Goal: {} | {} | Tasks: {}/{} complete{} | Result readiness: {}",
        overall.replace('_', " ").to_ascii_uppercase(),
        system_display(goal, SYSTEM_TEXT_MAX_GOAL_CHARS),
        complete,
        total,
        if omitted > 0 {
            format!("{details} | +{omitted} tasks (details)")
        } else {
            details
        },
        readiness_label(readiness),
    )
}

fn display(value: &str) -> String {
    let value = value.trim();
    let shortened = value.chars().take(MAX_DISPLAY_CHARS).collect::<String>();
    if shortened.chars().count() < value.chars().count() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn system_display(value: &str, limit: usize) -> String {
    let safe = crate::run_presentation::inert(value.trim());
    let shortened = safe.chars().take(limit).collect::<String>();
    if shortened.chars().count() < safe.chars().count() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn task_current(loaded: Option<&Loaded>, record: &Value, item: &Value) -> Option<bool> {
    let loaded = loaded?;
    if item["disposition"] == "direct" {
        return super::direct_completion_inputs(loaded, record, item)
            .ok()
            .flatten()
            .map(|(expected, current)| expected == current);
    }
    let refs = item["resultRefs"].as_array()?;
    if refs.is_empty() {
        return Some(false);
    }
    let mut current = true;
    for reference in refs {
        let reference = reference.as_str()?;
        let evidence = crate::run::result_ref_evidence(loaded, reference)
            .ok()
            .flatten()?;
        current &= evidence.accepted
            && evidence.artifact_current
            && evidence.drift.is_none()
            && evidence.inputs_current == Some(true);
    }
    Some(current)
}

pub(super) fn project_for_work(
    loaded: &Loaded,
    work: &str,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
) -> Value {
    let Some(record) = record else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "no canonical Lead goal",
            None,
            "unavailable",
        );
    };
    if !record_bound_to_work(record, work) {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "canonical Lead goal is not bound to this Work",
            None,
            "unavailable",
        );
    }
    project_with_limit(
        Some(loaded),
        Some(record),
        rendered,
        result,
        MAX_TASKS,
        false,
    )
}

pub(super) fn project_detail_for_work(
    loaded: &Loaded,
    work: &str,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
) -> Value {
    let Some(record) = record else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "no canonical Lead goal",
            None,
            "unavailable",
        );
    };
    if !record_bound_to_work(record, work) {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "canonical Lead goal is not bound to this Work",
            None,
            "unavailable",
        );
    }
    project_with_limit(
        Some(loaded),
        Some(record),
        rendered,
        result,
        usize::MAX,
        true,
    )
}

fn record_bound_to_work(record: &Value, work: &str) -> bool {
    record["goalId"].as_str() == Some(work)
        || record["continuation"]["work"].as_str() == Some(work)
        || super::requirements::bound(record, work)
}

#[cfg(test)]
mod tests {
    use super::project;
    use serde_json::{json, Value};

    fn record(obligations: serde_json::Value) -> serde_json::Value {
        json!({
            "goal": "Ship the user's requested change",
            "obligations": obligations,
        })
    }

    #[test]
    fn task_completion_stays_separate_from_result_readiness() {
        let value = project(
            None,
            Some(&record(json!([
                {"id":"A", "disposition":"accepted"},
                {"id":"B", "disposition":"open"}
            ]))),
            &json!({"explicitLeadClosure": false}),
            Some(&json!({
                "applicable": true,
                "state": "BLOCKED",
                "reason": {"code":"check_missing", "detail":"check required"}
            })),
        );
        assert_eq!(value["overall"], "in_progress");
        assert_eq!(value["decomposition"]["complete"], 0);
        assert_eq!(value["tasks"][0]["state"], "stale");
        assert_eq!(value["tasks"][1]["state"], "in_progress");
        assert_eq!(value["resultReadiness"]["state"], "BLOCKED");
        assert!(value["systemText"]
            .as_str()
            .unwrap()
            .contains("Tasks: 0/2 complete"));
    }

    #[test]
    fn closure_can_complete_overall_goal_without_claiming_result_readiness() {
        let value = project(
            None,
            Some(&record(json!([
                {"id":"A", "disposition":"accepted"}
            ]))),
            &json!({"explicitLeadClosure": true}),
            Some(&json!({"applicable": true, "state": "READY", "reason": {}})),
        );
        assert_eq!(value["overall"], "complete");
        assert_eq!(value["resultReadiness"]["state"], "READY");
    }

    #[test]
    fn absent_decomposition_is_explicitly_unavailable() {
        let value = project(
            None,
            Some(&record(json!([]))),
            &json!({"explicitLeadClosure": false}),
            None,
        );
        assert_eq!(value["overall"], "in_progress");
        assert_eq!(value["goal"], "Ship the user's requested change");
        assert_eq!(value["decomposition"]["available"], false);
        assert!(value["systemText"]
            .as_str()
            .unwrap()
            .contains("Tasks: unavailable"));
    }

    #[test]
    fn unrelated_work_cannot_read_the_canonical_goal() {
        let record = json!({
            "goalId": "smw_bound",
            "goal": "Private external goal",
            "obligations": [{"id":"A", "disposition":"open"}],
            "continuation": Value::Null,
        });
        assert!(!super::record_bound_to_work(&record, "smw_other"));
        assert!(super::record_bound_to_work(&record, "smw_bound"));
    }

    #[test]
    fn open_blocker_blocks_goal_without_rewriting_task_history() {
        let value = project(
            None,
            Some(&json!({
                "goal": "Ship the user's requested change",
                "obligations": [{"id":"A", "disposition":"open"}],
                "blockers": [{"id":"B", "disposition":"open"}],
            })),
            &json!({"explicitLeadClosure": false}),
            None,
        );
        assert_eq!(value["overall"], "blocked");
        assert_eq!(value["tasks"][0]["state"], "in_progress");
        assert_eq!(value["decomposition"]["complete"], 0);
    }

    #[test]
    fn long_task_identity_has_stable_machine_token_and_bounded_label() {
        let id = format!("task-{}", "x".repeat(500));
        let expected = crate::evidence::hash::text(&id);
        let value = project(
            None,
            Some(&record(json!([{"id": id, "disposition": "open"}]))),
            &json!({"explicitLeadClosure": false}),
            None,
        );
        assert_eq!(value["tasks"][0]["taskId"], expected);
        assert!(value["tasks"][0]["label"].as_str().unwrap().ends_with('…'));
        assert!(value["tasks"][0]["label"].as_str().unwrap().chars().count() <= 181);
    }
}

//! Product-owned overall-goal and divided-task progress projection.
//!
//! This read-only view derives task state from the canonical Lead session-goal
//! record. Exact-result readiness remains a separate field and is never used
//! as a proxy for the external user's overall goal.

use crate::config::Loaded;
use serde_json::{json, Value};

pub(super) mod categories;
mod close_route;

const MAX_TASKS: usize = 32;
const MAX_DISPLAY_CHARS: usize = 180;
const SYSTEM_TEXT_MAX_GOAL_CHARS: usize = 64;
const SYSTEM_TEXT_MAX_TASK_CHARS: usize = 32;

#[derive(Clone, Copy)]
struct GoalProjectionContext<'a> {
    loaded: Option<&'a Loaded>,
    root: Option<&'a std::path::Path>,
    project_selected: bool,
}

/// Work can be governed without opting into a separate native session Goal.
/// This projection is only for verified absence, never failed association reads.
pub(super) fn not_requested(result: &Value) -> Value {
    let result_readiness = readiness(Some(result));
    json!({
        "overall":"not_requested", "goal":null,
        "decomposition":{"available":false,"reason":"native_session_goal_not_requested"},
        "tasks":[], "resultReadiness":result_readiness,
        "systemText":format!("Session Goal: not requested | Result readiness: {}",
            result_readiness["state"].as_str().unwrap_or("unavailable")),
    })
}

pub(super) fn project(
    loaded: Option<&Loaded>,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
) -> Value {
    project_with_limit(
        GoalProjectionContext {
            loaded,
            root: loaded.map(|loaded| loaded.state_root.as_path()),
            project_selected: false,
        },
        record,
        rendered,
        result,
        MAX_TASKS,
        false,
    )
}

pub(super) fn project_selected(
    loaded: &Loaded,
    root: &std::path::Path,
    project_selected: bool,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
) -> Value {
    project_with_limit(
        GoalProjectionContext {
            loaded: Some(loaded),
            root: Some(root),
            project_selected,
        },
        record,
        rendered,
        result,
        MAX_TASKS,
        false,
    )
}

/// A bounded Goal preview for a non-owner project. It deliberately has no
/// evidence evaluator, named-coverage projection, or whole-Goal terminal.
pub(super) fn context_only(record: &Value, result: Option<&Value>) -> Value {
    let mut rendered = super::presentation(Some(record));
    rendered["explicitLeadClosure"] = json!(false);
    let mut view = project(None, Some(record), &rendered, result);
    view["overall"] = json!("context_only");
    view["contextOnly"] = json!({
        "kind":"native_session_goal",
        "authority":"none",
        "ownerEvidence":"not_evaluated",
    });
    view.as_object_mut()
        .expect("Goal progress projection is an object")
        .remove("coverageConfirmed");
    view.as_object_mut()
        .expect("Goal progress projection is an object")
        .remove("nextAction");
    view["systemText"] = json!(format!(
        "Session Goal context only; owner evidence is not evaluated | {}",
        view["systemText"]
            .as_str()
            .unwrap_or("Goal preview unavailable")
    ));
    view
}

/// The Goal view, with the Lead-facing journey and lesson facts appended to
/// its system text once the view is complete.
fn project_with_limit(
    context: GoalProjectionContext<'_>,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
    task_limit: usize,
    include_canonical_id: bool,
) -> Value {
    let mut view = goal_view(
        context,
        record,
        rendered,
        result,
        task_limit,
        include_canonical_id,
    );
    if let (Some(loaded), Some(record)) = (context.loaded, record) {
        super::goal_summary::append(loaded, record, &mut view);
    }
    if let Some(record) = record {
        super::goal_summary::closed_head(record, &mut view);
    }
    view
}

fn goal_view(
    context: GoalProjectionContext<'_>,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
    task_limit: usize,
    include_canonical_id: bool,
) -> Value {
    let GoalProjectionContext {
        loaded,
        root: goal_root,
        project_selected,
    } = context;
    let Some(record) = record else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "no canonical Lead goal",
            None,
            "unavailable",
        );
    };
    let journey =
        loaded.and_then(|loaded| super::journey::progress_for_record(Some(loaded), record));
    let Some(goal) = record["goal"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    else {
        return include_journey(
            unavailable(
                result,
                "overall_goal_unavailable",
                "canonical Lead goal is missing",
                None,
                "unavailable",
            ),
            journey,
        );
    };
    let mut overall_state = overall_state(record, rendered);
    if super::goal_summary::closed_at_binding(record, journey.as_ref()) {
        overall_state = "complete";
    } else if overall_state == "complete"
        && journey
            .as_ref()
            .is_some_and(|value| value["state"] != "current")
    {
        overall_state = "in_progress";
    }
    if super::requirements::named(record) {
        if let Some(loaded) = loaded {
            let mut view = named_progress(
                loaded,
                goal_root.unwrap_or(&loaded.state_root),
                project_selected,
                record,
                overall_state,
                result,
                task_limit,
            );
            if let Some(journey) = journey {
                view["journeyCompletion"] = journey;
            }
            return view;
        }
    }
    let Some(items) = record["obligations"].as_array() else {
        let view = unavailable(
            result,
            "task_decomposition_unavailable",
            "Lead task decomposition is unavailable",
            Some(goal),
            overall_state,
        );
        return include_journey(
            with_bounded_owner_routes(view, loaded, goal_root, project_selected, record),
            journey,
        );
    };
    if items.is_empty() {
        let view = unavailable(
            result,
            "task_decomposition_unavailable",
            "Lead has not divided the goal into tasks",
            Some(goal),
            overall_state,
        );
        return include_journey(
            with_bounded_owner_routes(view, loaded, goal_root, project_selected, record),
            journey,
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
    let next_action = close_route::next_action(loaded, goal_root, project_selected, record);
    let mut view = json!({
        "overall": overall,
        "goal": display(goal),
        "decomposition": decomposition,
        "tasks": tasks,
        "resultReadiness": result_readiness,
        "categoryDispositions": categories::project(loaded, record, project_selected),
        "nextAction": next_action,
    });
    view["systemText"] = json!(system_text(
        overall,
        goal,
        complete,
        items.len(),
        &view["tasks"],
        &view["resultReadiness"],
    ));
    if let Some(journey) = journey {
        view["journeyCompletion"] = journey;
    }
    view
}

fn with_bounded_owner_routes(
    view: Value,
    loaded: Option<&Loaded>,
    goal_root: Option<&std::path::Path>,
    project_selected: bool,
    record: &Value,
) -> Value {
    let mut view = categories::attach_when_goal_is_valid(view, loaded, record, project_selected);
    view["nextAction"] = json!(close_route::next_action(
        loaded,
        goal_root,
        project_selected,
        record
    ));
    view
}

fn include_journey(mut view: Value, journey: Option<Value>) -> Value {
    if let Some(journey) = journey {
        view["journeyCompletion"] = journey;
    }
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
    root: &std::path::Path,
    project_selected: bool,
    record: &Value,
    overall: &'static str,
    result: Option<&Value>,
    limit: usize,
) -> Value {
    let projection =
        match super::requirements::projection_selected(loaded, root, project_selected, record) {
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
        "tasks":tasks,"coverageConfirmed":projection["coverageConfirmed"],"nextAction":projection["nextAction"],"resultReadiness":readiness(result),
        "categoryDispositions":categories::project(Some(loaded), record, project_selected)});
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

pub(super) fn unavailable(
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
        let evidence = match super::recovery::result_ref_evidence(loaded, record, reference).ok()? {
            Some(evidence) => (
                evidence.accepted,
                evidence.artifact_current,
                evidence.drift,
                evidence.inputs_current,
            ),
            None => {
                let evidence = crate::run::result_ref_evidence(loaded, reference)
                    .ok()
                    .flatten()?;
                (
                    evidence.accepted,
                    evidence.artifact_current,
                    evidence.drift,
                    evidence.inputs_current,
                )
            }
        };
        let (accepted, artifact_current, drift, inputs_current) = evidence;
        let supported = inputs_current != Some(true)
            && super::journey::result_ref_current_or_supported(
                loaded,
                record,
                reference,
                inputs_current,
            )
            .unwrap_or(false);
        current &= accepted
            && artifact_current
            && drift.is_none()
            && (inputs_current == Some(true) || supported);
    }
    Some(current)
}

pub(super) fn project_for_work(
    loaded: &Loaded,
    work: &str,
    goal_root: &std::path::Path,
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
    match record_bound_to_work_with_lineage(loaded, record, work) {
        Ok(true) => {}
        Ok(false) => {
            return unavailable(
                result,
                "overall_goal_unavailable",
                "canonical Lead goal is not bound to this Work",
                None,
                "unavailable",
            )
        }
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    }
    let mut projection = project_with_limit(
        GoalProjectionContext {
            loaded: Some(loaded),
            root: Some(goal_root),
            project_selected: false,
        },
        Some(record),
        rendered,
        result,
        MAX_TASKS,
        false,
    );
    let association = match work_association_with_lineage(loaded, record, work) {
        Ok(value) => value,
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    };
    if let Some(association) = association {
        projection["workAssociation"] = association_view(record, association, work);
    }
    projection
}

pub(super) fn project_detail_for_work(
    loaded: &Loaded,
    work: &str,
    goal_root: &std::path::Path,
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
    match record_bound_to_work_with_lineage(loaded, record, work) {
        Ok(true) => {}
        Ok(false) => {
            return unavailable(
                result,
                "overall_goal_unavailable",
                "canonical Lead goal is not bound to this Work",
                None,
                "unavailable",
            )
        }
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    }
    let mut projection = project_with_limit(
        GoalProjectionContext {
            loaded: Some(loaded),
            root: Some(goal_root),
            project_selected: false,
        },
        Some(record),
        rendered,
        result,
        usize::MAX,
        true,
    );
    let association = match work_association_with_lineage(loaded, record, work) {
        Ok(value) => value,
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    };
    if let Some(association) = association {
        projection["workAssociation"] = association_view(record, association, work);
    }
    projection
}

pub(super) fn record_bound_to_work(record: &Value, work: &str) -> bool {
    record["goalId"].as_str() == Some(work)
        || record["continuation"]["work"].as_str() == Some(work)
        || super::association::for_work(record, work).is_some()
        || super::requirements::bound(record, work)
}

pub(super) fn record_bound_to_work_with_lineage(
    loaded: &Loaded,
    record: &Value,
    work: &str,
) -> Result<bool, String> {
    if record_bound_to_work(record, work) {
        return Ok(true);
    }
    Ok(super::journey::member_for_work_with_lineage(loaded, record, work)?.is_some())
}

/// These projections serve the owner project (and legacy project Goals). A
/// Work whose exact start is recorded in this Goal's journey is shown as a
/// member; a link without membership keeps its context-only label.
fn association_view(record: &Value, association: Value, work: &str) -> Value {
    if association["kind"] == "verified_successor_lineage" {
        return association;
    }
    let member = super::journey::member_for_work(record, work).ok().flatten();
    if let Some(member) =
        member.filter(|member| member["startEventSha256"] == association["startEventSha256"])
    {
        return json!({
            "kind": "journey_member",
            "membershipProtocol": record["journeyMembershipProtocol"],
            "goalId": record["goalId"],
            "work": member["work"],
            "startEventSha256": member["startEventSha256"],
        });
    }
    json!({
        "kind": "context_only",
        "protocol": super::association::PROTOCOL,
        "work": association["work"],
        "startEventSha256": association["startEventSha256"],
    })
}

fn work_association_with_lineage(
    loaded: &Loaded,
    record: &Value,
    work: &str,
) -> Result<Option<Value>, String> {
    if let Some(association) = super::association::for_work(record, work) {
        return Ok(Some(association));
    }
    Ok(
        super::journey::member_for_work_with_lineage(loaded, record, work)?.map(|member| {
            json!({
                "kind":"verified_successor_lineage",
                "work":work,
                "originalMemberWork":member["work"],
                "originalStartEventSha256":member["startEventSha256"],
            })
        }),
    )
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
    fn category_projection_is_bounded_and_lists_only_recorded_dispositions() {
        let value = project(
            None,
            Some(&json!({
                "goal":"Bounded category view",
                "categories":{"obligations":"considered","findings":"none_applicable",
                    "blockers":"considered","decisions":"considered","externalActions":"considered"},
                "obligations":[{"id":"O1","disposition":"open"}],
                "findings":[],"blockers":[],"decisions":[],"externalActions":[]
            })),
            &json!({"explicitLeadClosure":false}),
            None,
        );
        assert_eq!(
            value["categoryDispositions"]["obligations"]["state"],
            "considered"
        );
        assert_eq!(
            value["categoryDispositions"]["obligations"]["items"][0]["disposition"],
            "open"
        );
        assert_eq!(
            value["categoryDispositions"]["obligations"]["validStates"],
            json!(["considered", "none_applicable"])
        );
        assert_eq!(
            value["categoryDispositions"]["obligations"]["validItemDispositions"],
            json!(["open", "accepted", "outside_scope", "successor", "direct"])
        );
        assert_eq!(
            value["categoryDispositions"]["findings"]["state"],
            "none_applicable"
        );
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

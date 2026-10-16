//! Validated overall-goal progress and automatic terminal presentation.

use super::*;

pub(crate) fn presentation(value: Option<&Value>) -> Value {
    let Some(record) = value else {
        return json!({"requestId":"unknown","explicitLeadClosure":false,"subgoals":["session goal not explicitly incorporated"],"findings":[],"blockers":[],"decisions":[],"externalActions":[]});
    };
    let request_id = format!(
        "{}:r{}",
        record["goalId"].as_str().unwrap_or("unknown"),
        record["revision"].as_u64().unwrap_or(0)
    );
    let closure_has_evidence = if record["closure"]["kind"] == "direct" {
        record["closure"]["inputsSha256"].is_string()
    } else {
        record["closure"]["resultRefs"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty() && refs.iter().all(Value::is_string))
    };
    let closure_current = record["closure"]["closed"] == true
        && record["closure"]["revision"] == record["revision"]
        && !unresolved(record)
        && closure_has_evidence;
    json!({
        "requestId":request_id,
        "explicitLeadClosure":closure_current,
        "completionMode": if record["closure"]["kind"] == "direct" { "direct" } else { "governed" },
        "subgoals":pending_items(record, "obligations", "subgoals"),
        "findings":pending_items(record, "findings", "findings"),
        "blockers":pending_items(record, "blockers", "blockers"),
        "decisions":pending_items(record, "decisions", "decisions"),
        "externalActions":pending_items(record, "externalActions", "external actions"),
        "source":record["source"],
        "revision":record["revision"]
    })
}

/// Display a cross-project native Goal without importing its closure or
/// owner evidence into the caller's configured project.
pub(crate) fn context_summary(
    value: Option<&Value>,
    owner: Option<&super::native::Owner>,
) -> Value {
    let Some(record) = value else {
        return json!({
            "contextOnly":true,
            "authority":"none",
            "ownerRequired":true,
            "goalProgress":progress::unavailable(None,"native_session_goal_unavailable","no native session Goal has been incorporated",None,"unavailable"),
        });
    };
    let goal_id = record["goalId"].as_str().unwrap_or_default();
    let progress = progress::context_only(record, None);
    json!({
        "contextOnly":true,
        "authority":"none",
        "ownerRequired":true,
        "goalIdSha256":crate::evidence::hash::text(goal_id),
        "goal":record["goal"],
        "revision":record["revision"],
        "ownerProjectId":owner.and_then(|value|value.project_id.clone()),
        "goalProgress":progress,
    })
}

fn pending_items(record: &Value, key: &str, label: &str) -> Vec<Value> {
    let unavailable = || vec![json!(format!("session goal {label} unavailable"))];
    let Some(category) = record["categories"][key].as_str() else {
        return unavailable();
    };
    if !matches!(category, "considered" | "none_applicable") {
        return unavailable();
    }
    let Some(items) = record[key].as_array() else {
        return unavailable();
    };
    let mut pending = Vec::new();
    for item in items {
        match item["disposition"].as_str() {
            Some("open") => {
                let value = if key == "externalActions" {
                    item["action"].as_str()
                } else {
                    item["text"].as_str()
                };
                let Some(value) = value else {
                    return unavailable();
                };
                pending.push(json!(value));
            }
            Some("accepted" | "outside_scope" | "successor" | "direct") => {}
            _ => return unavailable(),
        }
    }
    pending
}

pub(crate) fn presentation_for_loaded(
    loaded: &crate::config::Loaded,
    value: Option<&Value>,
) -> Result<Value, String> {
    presentation_for_selected(loaded, value, &loaded.state_root, false)
}

pub(crate) fn presentation_for_selected(
    loaded: &crate::config::Loaded,
    value: Option<&Value>,
    root: &std::path::Path,
    project_selected: bool,
) -> Result<Value, String> {
    let mut rendered = presentation(value);
    let readiness = current::evaluate(loaded, value, root, project_selected);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    rendered["goalProgress"] =
        progress::project_selected(loaded, root, project_selected, value, &rendered, None);
    rendered["terminal"] = json!(crate::presentation_events::whole_goal_terminal(&rendered));
    Ok(rendered)
}

/// Preserve standalone Work display while a bound goal owns its completion.
/// An accepted part cannot claim the overall goal, and stale/unknown closure
/// cannot display readiness. No canonical record is changed by this route.
pub(crate) fn terminal_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    standalone: Option<&'static str>,
) -> Result<Option<&'static str>, String> {
    if standalone.is_none() {
        return Ok(None);
    }
    let association = super::association::context_for_work_with_root(loaded, work);
    let (record, goal_root) = match association.context {
        super::association::WorkContext::Owner(record) => {
            let Some(root) = association.owner_root else {
                return Ok(None);
            };
            (Some(record), root)
        }
        super::association::WorkContext::Session(_)
        | super::association::WorkContext::NotRequested
        | super::association::WorkContext::Unavailable
        | super::association::WorkContext::Invalid => return Ok(standalone),
        super::association::WorkContext::Local => {
            (read(&loaded.state_root)?, loaded.state_root.clone())
        }
    };
    let Some(record) = record else {
        return Ok(standalone);
    };
    if !progress::record_bound_to_work_with_lineage(loaded, &record, work)? {
        return Ok(standalone);
    }
    let rendered = presentation_for_selected(loaded, Some(&record), &goal_root, false)?;
    Ok(crate::presentation_events::whole_goal_terminal(&rendered))
}

pub(crate) fn progress_for_loaded(
    loaded: &crate::config::Loaded,
    result: &Value,
) -> Result<Value, String> {
    let record = read(&loaded.state_root)?;
    let mut rendered = presentation(record.as_ref());
    let readiness = current::evaluate(loaded, record.as_ref(), &loaded.state_root, false);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project(
        Some(loaded),
        record.as_ref(),
        &rendered,
        Some(result),
    ))
}

/// Project progress for one Work while enforcing the existing canonical
/// goal-to-Work/continuation binding. A global goal record is not silently
/// disclosed to an unrelated Work.
pub(crate) fn progress_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
) -> Result<Value, String> {
    let association = super::association::context_for_work_with_root(loaded, work);
    match association.context {
        super::association::WorkContext::NotRequested => Ok(progress::not_requested(result)),
        super::association::WorkContext::Owner(record) => {
            let Some(root) = association.owner_root else {
                return Ok(progress::unavailable(
                    Some(result),
                    "native_session_goal_context_invalid",
                    "verified native session Goal root is unavailable",
                    None,
                    "unavailable",
                ));
            };
            progress_from_record(loaded, work, result, Some(&record), &root)
        }
        super::association::WorkContext::Session(record) => {
            Ok(context_progress(&record, work, result, false))
        }
        super::association::WorkContext::Unavailable => Ok(progress::unavailable(
            Some(result),
            "native_session_goal_unavailable",
            "no verified native session Goal association",
            None,
            "unavailable",
        )),
        super::association::WorkContext::Invalid => Ok(progress::unavailable(
            Some(result),
            "native_session_goal_context_invalid",
            "native session Goal context could not be verified",
            None,
            "unavailable",
        )),
        super::association::WorkContext::Local => local_progress_for_work(loaded, work, result),
    }
}

fn local_progress_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
) -> Result<Value, String> {
    let record = read(&loaded.state_root)?;
    progress_from_record(loaded, work, result, record.as_ref(), &loaded.state_root)
}

fn progress_from_record(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
    record: Option<&Value>,
    root: &std::path::Path,
) -> Result<Value, String> {
    let mut rendered = presentation(record);
    let readiness = current::evaluate(loaded, record, root, false);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project_for_work(
        loaded,
        work,
        root,
        record,
        &rendered,
        Some(result),
    ))
}

/// Full task projection for an already bound Work. Callers must paginate or
/// group the returned task data before placing it on a bounded response.
pub(crate) fn progress_detail_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
) -> Result<Value, String> {
    let association = super::association::context_for_work_with_root(loaded, work);
    match association.context {
        super::association::WorkContext::NotRequested => Ok(progress::not_requested(result)),
        super::association::WorkContext::Owner(record) => {
            let Some(root) = association.owner_root else {
                return Ok(progress::unavailable(
                    Some(result),
                    "native_session_goal_context_invalid",
                    "verified native session Goal root is unavailable",
                    None,
                    "unavailable",
                ));
            };
            progress_detail_from_record(loaded, work, result, Some(&record), &root)
        }
        super::association::WorkContext::Session(record) => {
            Ok(context_progress(&record, work, result, true))
        }
        super::association::WorkContext::Unavailable => Ok(progress::unavailable(
            Some(result),
            "native_session_goal_unavailable",
            "no verified native session Goal association",
            None,
            "unavailable",
        )),
        super::association::WorkContext::Invalid => Ok(progress::unavailable(
            Some(result),
            "native_session_goal_context_invalid",
            "native session Goal context could not be verified",
            None,
            "unavailable",
        )),
        super::association::WorkContext::Local => {
            local_progress_detail_for_work(loaded, work, result)
        }
    }
}

fn local_progress_detail_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
) -> Result<Value, String> {
    let record = read(&loaded.state_root)?;
    progress_detail_from_record(loaded, work, result, record.as_ref(), &loaded.state_root)
}

fn progress_detail_from_record(
    loaded: &crate::config::Loaded,
    work: &str,
    result: &Value,
    record: Option<&Value>,
    root: &std::path::Path,
) -> Result<Value, String> {
    let mut rendered = presentation(record);
    let readiness = current::evaluate(loaded, record, root, false);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project_detail_for_work(
        loaded,
        work,
        root,
        record,
        &rendered,
        Some(result),
    ))
}

fn context_progress(record: &Value, work: &str, result: &Value, detail: bool) -> Value {
    let mut view = progress::context_only(record, Some(result));
    if detail {
        view["detailRoute"] = json!({
            "state":"preview",
            "reason":"owner_project_evidence_required",
        });
    }
    if let Some(association) = super::association::for_work(record, work) {
        view["workAssociation"] = json!({
            "kind":"context_only",
            "protocol":super::association::PROTOCOL,
            "work":association["work"],
            "startEventSha256":association["startEventSha256"],
        });
    }
    view["goalIdentitySha256"] = json!(crate::evidence::hash::value(&record["goalId"]));
    view
}

pub(crate) fn record_for_work(loaded: &crate::config::Loaded, work: &str) -> Option<Value> {
    match super::association::context_for_work(loaded, work) {
        super::association::WorkContext::Owner(record)
        | super::association::WorkContext::Session(record) => {
            Some(super::association::public_record(&record))
        }
        super::association::WorkContext::Local => read(&loaded.state_root).ok().flatten(),
        super::association::WorkContext::NotRequested
        | super::association::WorkContext::Unavailable
        | super::association::WorkContext::Invalid => None,
    }
}

/// Preserve a separately selected project-local named contract when the
/// current native session also supplies its independent parent Goal context.
pub(crate) fn project_goal_progress_for_work(
    loaded: &crate::config::Loaded,
    work: &str,
) -> Result<Option<Value>, String> {
    // Legacy named Works are project scoped too. The named-scope companion
    // selects project or session independently from the optional parent
    // context; absence of a parent companion must not hide a local contract.
    super::requirements::project_progress_for_work(loaded, work)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_does_not_close_from_ready() {
        let value = json!({"goalId":"a","revision":1,"categories":{"obligations":"none_applicable","findings":"none_applicable","blockers":"none_applicable","decisions":"none_applicable","externalActions":"none_applicable"},"obligations":[],"findings":[],"blockers":[],"decisions":[],"externalActions":[],"closure":{"closed":false,"revision":null,"resultRefs":[]}});
        assert_eq!(presentation(Some(&value))["explicitLeadClosure"], false);
    }
}

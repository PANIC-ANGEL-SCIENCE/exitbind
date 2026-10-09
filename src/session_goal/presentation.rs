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
    let mut rendered = presentation(value);
    let readiness = current::evaluate(loaded, value);
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    rendered["goalProgress"] = progress::project(Some(loaded), value, &rendered, None);
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
    let record = read(&loaded.state_root)?;
    let Some(record) = record.filter(|record| progress::record_bound_to_work(record, work)) else {
        return Ok(standalone);
    };
    let rendered = presentation_for_loaded(loaded, Some(&record))?;
    Ok(crate::presentation_events::whole_goal_terminal(&rendered))
}

pub(crate) fn progress_for_loaded(
    loaded: &crate::config::Loaded,
    result: &Value,
) -> Result<Value, String> {
    let record = read(&loaded.state_root)?;
    let mut rendered = presentation(record.as_ref());
    let readiness = current::evaluate(loaded, record.as_ref());
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
    let record = read(&loaded.state_root)?;
    let mut rendered = presentation(record.as_ref());
    let readiness = current::evaluate(loaded, record.as_ref());
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project_for_work(
        loaded,
        work,
        record.as_ref(),
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
    let record = read(&loaded.state_root)?;
    let mut rendered = presentation(record.as_ref());
    let readiness = current::evaluate(loaded, record.as_ref());
    rendered["historicalLeadClosure"] = rendered["explicitLeadClosure"].clone();
    rendered["explicitLeadClosure"] = json!(readiness.is_current());
    rendered["currentReadiness"] = readiness.value();
    Ok(progress::project_detail_for_work(
        loaded,
        work,
        record.as_ref(),
        &rendered,
        Some(result),
    ))
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

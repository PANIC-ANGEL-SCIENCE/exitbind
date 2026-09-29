use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Releases before the check-routing correction could record a reviewer
/// approval after a failed main check. Accept only those producers while
/// replaying that historical event; current routing still applies afterward.
const PRODUCERS_WITH_HISTORICAL_REVIEW_ORDER: &[&str] = &[
    "0.20.0",
    "0.21.0",
    "0.23.1-rc.2",
    "0.24.0-rc.1",
    "0.24.0-rc.7",
    "0.25.0-rc.1",
];

pub(super) fn reviewer_assignment(state: &Value, event: &Value) -> Option<Value> {
    let lead = super::lead_stage(state).ok()?;
    if event["role"] != "reviewer"
        || event["outcome"] != "approved"
        || !historical_producer(event)
        || state["currentStage"] != lead
        || !has_failed_main_check(state)
    {
        return None;
    }
    let attempt = state["attempt"].as_u64()?;
    if event["attempt"] != attempt {
        return None;
    }
    state["plan"]["stages"]
        .as_array()?
        .iter()
        .find_map(|stage| {
            let has_reviewer = stage["agents"].as_array().is_some_and(|agents| {
                agents
                    .iter()
                    .any(|agent| agent["role"] == "reviewer" && agent["name"] == event["agent"])
            });
            if has_reviewer && stage["stage"] == event["stage"] {
                Some(json!({
                    "stage": stage["stage"],
                    "attempt": attempt,
                    "agent": event["agent"],
                    "role": "reviewer"
                }))
            } else {
                None
            }
        })
}

fn historical_producer(event: &Value) -> bool {
    event["producer"]["version"]
        .as_str()
        .is_some_and(|version| PRODUCERS_WITH_HISTORICAL_REVIEW_ORDER.contains(&version))
}

fn has_failed_main_check(state: &Value) -> bool {
    let workers = state["submissions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|submission| {
            submission["attempt"] == state["attempt"]
                && submission["role"] == "worker"
                && submission["outcome"] == "completed"
        })
        .filter_map(|submission| submission["eventSha256"].as_str().map(str::to_owned));
    let targets = workers.collect::<BTreeSet<_>>();
    state["checks"].as_array().is_some_and(|checks| {
        checks.iter().any(|check| {
            check["targetEventSha256"]
                .as_str()
                .is_some_and(|target| targets.contains(target))
                && check.get("requirementId").is_none()
                && super::check_stage::check_failed(check)
        })
    })
}

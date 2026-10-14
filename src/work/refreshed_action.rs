//! Bound navigation after a committed mutation; no extra state transition.
use crate::config::Loaded;
use serde_json::{json, Value};

pub(super) fn attach(loaded: &Loaded, work: &str, next: &Value, response: &mut Value) {
    let Some(binding) = next["current"]["binding"].as_str() else {
        return;
    };
    if next["action"] == "done"
        || response["projectionError"].is_string()
        || response["diagnostic"].is_object()
    {
        return;
    }
    let mut candidate = response.clone();
    candidate.as_object_mut().unwrap().remove("effectiveAction");
    candidate["current"] = super::compact::minimal_current(&next["current"]);
    candidate["actionForms"] = super::action_forms::full(loaded, work, next, binding);
    candidate = super::effective_action::attach(&candidate);
    if size(&candidate) <= super::compact::MAX_RESPONSE_BYTES {
        *response = candidate;
        return;
    }
    // The existing effectiveAction carries the same executable forms. Avoid
    // duplicating every argv when that alone exceeds the bounded response.
    candidate.as_object_mut().unwrap().remove("actionForms");
    candidate["effectiveAction"]["source"]["location"] = json!("effectiveAction");
    if size(&candidate) <= super::compact::MAX_RESPONSE_BYTES {
        *response = candidate;
        return;
    }
    // Exact mutation evidence has priority at the bound. Keep one small current
    // action route; complete instructions/evidence still use their existing reads.
    let mut candidate = response.clone();
    candidate.as_object_mut().unwrap().remove("effectiveAction");
    candidate["current"] = json!({
        "binding":binding, "action":next["current"]["action"],
        "readiness":next["current"]["readiness"],
        "details":{"grouped":next["current"]["details"]["grouped"]},
        "actionForm":{"state":next["current"]["actionForm"]["state"]},
    });
    candidate = super::effective_action::attach(&candidate);
    if candidate["effectiveAction"]["exists"] == true {
        if let Some(grant) = response.get("currentGrant") {
            candidate["effectiveAction"]["currentGrant"] = grant.clone();
        }
    }
    if size(&candidate) <= super::compact::MAX_RESPONSE_BYTES {
        *response = candidate;
    }
}

fn size(value: &Value) -> usize {
    serde_json::to_vec(value)
        .expect("navigation response is serializable")
        .len()
        + 1
}

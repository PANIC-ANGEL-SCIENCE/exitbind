//! One additive projection of existing canonical actions; no transition owner.
use serde_json::{json, Value};

pub(super) fn safe_action(kind: &str) -> Value {
    json!({"type":kind,"safe":true})
}

pub(crate) fn project(response: &Value) -> Value {
    let current = response
        .get("current")
        .or_else(|| response.pointer("/next/current"));
    let absent = Value::Null;
    let current = current.unwrap_or(&absent);
    let forms = response
        .get("actionForms")
        .unwrap_or(&current["actionForm"]);
    let kind = current["action"]
        .as_str()
        .or_else(|| response["next"]["action"].as_str());
    let kind = if forms["state"] == "pre_return_uncertain_result" {
        Some("spawn")
    } else if matches!(
        forms["state"].as_str(),
        Some(
            "interrupted_pre_return"
                | "pre_return_retry_interrupted"
                | "pre_return_ambiguous"
                | "pre_return_evidence_unavailable"
        )
    ) {
        Some("lead_decision")
    } else {
        kind
    };
    let kind = kind.or_else(|| match forms["state"].as_str() {
        Some("pending_assignment") => Some("spawn"),
        Some(
            "initial_scope"
            | "lead_acceptance"
            | "failed_check"
            | "pending_review_finding"
            | "failed_check_observation"
            | "unresolved_check_observation",
        ) => Some("lead_decision"),
        Some("check") => Some("check"),
        Some("done") => Some("done"),
        _ => None,
    });
    let effect = response["effect"].as_str().unwrap_or("no-change");
    let refused = response["outcome"] == "refused" || response["status"] == "refused";
    let blocked = refused
        || forms["recovery"]["state"] == "blocked"
        || matches!(effect, "held" | "unknown" | "uncertain" | "refused")
        || response["projectionError"].is_string()
        || response["diagnostic"].is_object();
    let exists = !blocked
        && matches!(kind, Some("spawn" | "check" | "lead_decision"))
        && (current["binding"].is_string() || response["binding"].is_string());
    let done = kind == Some("done") && !blocked;
    let binding = response.get("binding").unwrap_or(&current["binding"]);
    let delegation = if exists && kind == Some("spawn") {
        delegation(response, current, forms)
    } else {
        Value::Null
    };
    json!({"version":1,"delegation":delegation,"exists":exists,"kind":if exists { json!(kind) } else { Value::Null },
        "state":if exists { "action" } else if done { "done" } else if blocked { "blocked" } else { "unknown" },
        "binding":binding,"readiness":current["readiness"],"effect":effect,
        "command":if exists { forms["mechanicalAction"]["command"].clone() } else { Value::Null },
        "choices":if exists && response["actionForms"].is_object() { forms["choices"].clone() } else { json!([]) },
        "beforeEditing":if exists && response["actionForms"].is_object() { forms["beforeEditing"].clone() } else { Value::Null },
        "currentGrant":if exists && response["actionForms"].is_object() { forms["currentGrant"].clone() } else { Value::Null },
        "nextRequest":if exists && response["actionForms"].is_object() { forms["nextRequest"].clone() } else { Value::Null },
        "detail":if exists { current["details"]["grouped"].clone() } else { Value::Null },
        "requiresDetail":exists && !response["actionForms"].is_object(),
        "warnings":response.get("warnings").or_else(|| response.pointer("/next/warnings")).cloned().unwrap_or(json!([])),
        "blocker":if blocked { response.get("reason").cloned().unwrap_or(json!({"code":"effect_unresolved"})) } else { Value::Null },
        "source":{"version":1,"owner":"work actionForms/current","location":if response["actionForms"].is_object() { "actionForms" } else if response.get("current").is_some() { "current.actionForm" } else { "next.current.actionForm" }}})
}

/// Who performs the open assignment, never a refusal: the same line is true
/// for the Lead, who delegates it to that native agent, and for the agent
/// itself, who performs it.
fn delegation(response: &Value, current: &Value, forms: &Value) -> Value {
    let recipient = [
        &response["recipient"],
        &current["recipient"],
        &response["next"],
    ]
    .into_iter()
    .find(|candidate| candidate["agent"].is_string())
    .unwrap_or(&Value::Null);
    let agent = recipient["agent"].as_str().or(forms["actor"].as_str());
    let Some(agent) = agent else {
        return Value::Null;
    };
    let role = recipient["role"].as_str().unwrap_or(agent);
    let native = recipient["nativeName"]
        .as_str()
        .or(response["recipientContext"]["nativeName"].as_str())
        .or(response["next"]["packet"]["nativeTaskName"].as_str())
        .unwrap_or(agent);
    let assignment = recipient["assignment"]
        .as_str()
        .or(forms["assignment"].as_str())
        .or(current["assignment"].as_str());
    let task = assignment.map_or_else(
        || "the pending assignment".to_owned(),
        |assignment| format!("assignment {assignment}"),
    );
    let configured = if native == agent {
        String::new()
    } else {
        format!(" (configured as '{agent}')")
    };
    let text = format!("next action: {role} agent '{native}' performs {task}{configured}");
    json!({"nextAction":"delegate","agent":agent,"nativeName":native,"role":role,"assignment":assignment,"text":text,
        "meaning":"who performs this assignment, not a refusal: that agent uses its beforeEditing and choices forms; a Lead delegates the assignment to that native agent"})
}

pub(crate) fn attach(response: &Value) -> Value {
    if !crate::producer::exitbind_surface() || response.get("effectiveAction").is_some() {
        return response.clone();
    }
    let mut result = response.clone();
    result["effectiveAction"] = project(response);
    result
}

pub(crate) fn insert(response: &mut Value, canonical: &Value) {
    if crate::producer::exitbind_surface() {
        response["effectiveAction"] = project(canonical);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_never_manufacture_advancement_and_done_keeps_warnings() {
        let active = json!({"current":{"binding":"current","action":"spawn","readiness":"IN_PROGRESS","actionForm":{"choices":[]}}});
        assert_eq!(project(&active)["exists"], true);
        assert_eq!(project(&active)["delegation"], Value::Null);
        let delegated = json!({"recipient":{"agent":"builder","role":"worker","assignment":"sma_1"},
            "current":{"binding":"current","action":"spawn","actionForm":{"state":"pending_assignment"}}});
        assert_eq!(project(&delegated)["delegation"]["nextAction"], "delegate");
        assert_eq!(
            project(&delegated)["delegation"]["text"],
            "next action: worker agent 'builder' performs assignment sma_1"
        );
        let mut named = delegated.clone();
        named["recipient"]["nativeName"] = json!("sonic");
        assert_eq!(
            project(&named)["delegation"]["text"],
            "next action: worker agent 'sonic' performs assignment sma_1 (configured as 'builder')"
        );
        let mut check = delegated.clone();
        check["current"]["action"] = json!("check");
        assert_eq!(project(&check)["delegation"], Value::Null);
        assert!(project(&active)["currentGrant"].is_null());
        assert!(project(&active)["nextRequest"].is_null());
        for effect in ["held", "unknown", "uncertain", "refused"] {
            let mut value = active.clone();
            value["effect"] = json!(effect);
            assert_eq!(project(&value)["exists"], false);
            assert_eq!(project(&value)["state"], "blocked");
        }
        let done = json!({"current":{"binding":"closed","action":"done","readiness":"READY"},"next":{"action":"warning"},"warnings":["memory drift"]});
        assert_eq!(project(&done)["state"], "done");
        assert_eq!(project(&done)["exists"], false);
        assert_eq!(project(&done)["warnings"], done["warnings"]);
        assert_eq!(
            project(&json!({"next":{"action":"warning"}}))["exists"],
            false
        );
    }
}

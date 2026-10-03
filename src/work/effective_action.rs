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
    let kind = kind.or_else(|| match forms["state"].as_str() {
        Some("pending_assignment") => Some("spawn"),
        Some("initial_scope" | "lead_acceptance" | "failed_check" | "pending_review_finding") => {
            Some("lead_decision")
        }
        Some("check") => Some("check"),
        Some("done") => Some("done"),
        _ => None,
    });
    let effect = response["effect"].as_str().unwrap_or("no-change");
    let refused = response["outcome"] == "refused" || response["status"] == "refused";
    let blocked = refused
        || matches!(effect, "held" | "unknown" | "uncertain" | "refused")
        || response["projectionError"].is_string()
        || response["diagnostic"].is_object();
    let exists = !blocked
        && matches!(kind, Some("spawn" | "check" | "lead_decision"))
        && (current["binding"].is_string() || response["binding"].is_string());
    let done = kind == Some("done") && !blocked;
    let binding = response.get("binding").unwrap_or(&current["binding"]);
    json!({"version":1,"exists":exists,"kind":if exists { json!(kind) } else { Value::Null },
        "state":if exists { "action" } else if done { "done" } else if blocked { "blocked" } else { "unknown" },
        "binding":binding,"readiness":current["readiness"],"effect":effect,
        "command":if exists { forms["mechanicalAction"]["command"].clone() } else { Value::Null },
        "choices":if exists && response["actionForms"].is_object() { forms["choices"].clone() } else { json!([]) },
        "beforeEditing":if exists && response["actionForms"].is_object() { forms["beforeEditing"].clone() } else { Value::Null },
        "detail":if exists { current["details"]["grouped"].clone() } else { Value::Null },
        "requiresDetail":exists && !response["actionForms"].is_object(),
        "warnings":response.get("warnings").or_else(|| response.pointer("/next/warnings")).cloned().unwrap_or(json!([])),
        "blocker":if blocked { response.get("reason").cloned().unwrap_or(json!({"code":"effect_unresolved"})) } else { Value::Null },
        "source":{"version":1,"owner":"work actionForms/current","location":if response["actionForms"].is_object() { "actionForms" } else if response.get("current").is_some() { "current.actionForm" } else { "next.current.actionForm" }}})
}

pub(crate) fn attach(response: &Value) -> Value {
    if response.get("effectiveAction").is_some() {
        return response.clone();
    }
    let mut result = response.clone();
    result["effectiveAction"] = project(response);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_never_manufacture_advancement_and_done_keeps_warnings() {
        let active = json!({"current":{"binding":"current","action":"spawn","readiness":"IN_PROGRESS","actionForm":{"choices":[]}}});
        assert_eq!(project(&active)["exists"], true);
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

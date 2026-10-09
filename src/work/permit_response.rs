//! Small default permit result. Full assignment and governor history remain
//! available through the read-only work and event inspection routes.

use crate::{config::Loaded, run};
use serde_json::{json, Value};

use super::{agent_response, next_for, resolve};

/// Authorize one exact worker mutation, then return a compact action result.
pub(crate) fn permit(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    operation: &str,
    request_id: Option<&str>,
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" || action["role"] != "worker" {
        return Err("a worker assignment is required for a governed mutation".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let expected = run::AssignmentIdentity::from_action(&action)?;
    let permission = run::permit_for_assignment(
        loaded, &ledger, work, assignment, expected, operation, request_id,
    )?;
    let next = next_for(loaded, work, &ledger)?;
    Ok(agent_response(compact(
        work,
        assignment,
        operation,
        &ledger,
        &permission,
        &next,
    )))
}

pub(super) fn compact(
    work: &str,
    assignment: &str,
    operation: &str,
    ledger: &str,
    permission: &Value,
    next: &Value,
) -> Value {
    let event = &permission["event"];
    let event_sha = &event["eventSha256"];
    let allowed = permission["allowed"] == true;
    let current_grant = allowed && is_current_grant(permission, next, assignment);
    let mut response = json!({
        "compact": true,
        "work": work,
        "assignment": assignment,
        "operation": operation,
        "allowed": allowed,
        "effect": if event_sha.is_string() { "recorded" } else { "no-change" },
        "eventSha256": event_sha,
        "event": {"action": event["action"], "eventSha256": event_sha},
        "governor": {
            "enabled": permission["governor"]["enabled"],
            "state": permission["governor"]["state"],
            "reason": permission["governor"]["reason"],
        },
        "reference": {"ledger": ledger, "eventSha256": event_sha},
        "next": {
            "action": next["action"], "role": next["role"],
            "assignment": next["assignment"], "requiresExpansion": true,
        },
        "nextAction": {
            "type": "read_work", "safe": true,
            "commandSuffix": ["work", "next", work, "--full"],
            "sameExecutableRequired": true, "sameConfigRequired": true,
        },
        "audit": {
            "commandSuffix": ["run", "inspect", ledger, "--event", event_sha, "--json"],
            "sameExecutableRequired": true, "sameConfigRequired": true,
        },
    });
    if current_grant {
        response["currentGrant"] = json!({
            "governorEventSha256": permission["event"]["governorEvent"]["eventSha256"],
            "runEventSha256": event_sha,
            "operation": operation,
            "state": "current_for_this_worker_assignment",
            "use": "the exact current grant may support the present mutation; it does not grant host permissions",
        });
        if next["packet"]["context"]["loop"]["state"] == "replan_required" {
            response["nextRequest"] = json!({
                "requiredAction": "work replan",
                "meaning": "the current grant remains usable for its present mutation; re-plan is required before another mutation request",
            });
        } else if next["packet"]["context"]["loop"]["state"] == "evidence_required" {
            response["nextRequest"] = json!({
                "requiredAction": "work evidence",
                "meaning": "the current grant remains usable for its present mutation; new exact evidence is required before another mutation request",
            });
        }
    }
    if let Some(idempotent) = permission.get("idempotent") {
        response["idempotent"] = idempotent.clone();
    }
    response
}

fn is_current_grant(permission: &Value, next: &Value, assignment: &str) -> bool {
    let event = &permission["event"];
    let nested = &event["governorEvent"];
    let loop_state = &next["packet"]["context"]["loop"];
    let grant = &loop_state["currentMutation"];
    let outer_sha = event["eventSha256"].as_str();
    let nested_sha = nested["eventSha256"].as_str();
    let packet = &next["packet"];
    let context = &packet["context"];
    let consumed = outer_sha.is_some_and(|sha| {
        loop_state["consumedGrants"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(sha)))
    });
    let held = next.get("held").is_some_and(|value| !value.is_null())
        || next["heldResults"]
            .as_array()
            .is_some_and(|items| !items.is_empty());

    !held
        && event["action"] == "govern"
        && nested["action"] == "mutation"
        && matches!(
            loop_state["state"].as_str(),
            Some("ready" | "replan_required" | "evidence_required")
        )
        && nested_sha.is_some()
        && event["runId"] == context["run"]["id"]
        && event["stage"] == packet["stage"]
        && event["attempt"] == packet["attempt"]
        && event["agent"] == packet["agent"]
        && event["role"] == packet["role"]
        && event["subjectSha256"] == context["subject"]["sha256"]
        && event["assignmentSha256"] == crate::evidence::hash::text(assignment)
        && event["inputsSha256"] == nested["inputSha256"]
        && nested["unit"] == "worker-mutation"
        && nested_sha == grant["eventSha256"].as_str()
        && loop_state["currentGrantEventSha256"] == nested["eventSha256"]
        && loop_state["runId"] == nested["runId"]
        && grant["runId"] == nested["runId"]
        && grant["subjectSha256"] == nested["subjectSha256"]
        && grant["attempt"] == nested["attempt"]
        && grant["checkpoint"] == nested["checkpoint"]
        && grant["inputSha256"] == nested["inputSha256"]
        && nested["lineageSha256"] == loop_state["lineageSha256"]
        && nested["runId"] == context["run"]["id"]
        && nested["subjectSha256"] == context["subject"]["sha256"]
        && nested["attempt"] == packet["attempt"]
        && grant["carryLineage"] == true
        && !consumed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permit_result_is_bounded_and_keeps_decisive_fields() {
        let huge = "x".repeat(80_000);
        let permission = json!({
            "allowed": false,
            "event": {"action": "govern", "eventSha256": "event", "audit": huge},
            "governor": {"enabled": true, "state": "blocked", "history": huge},
        });
        let next = json!({"action": "lead_decision", "role": "lead", "assignment": "lead-a", "packet": huge});
        let result = compact(
            "work-a",
            "worker-a",
            "edit",
            "runs/work-a.jsonl",
            &permission,
            &next,
        );
        assert!(
            serde_json::to_vec(&result).unwrap().len() < crate::work::compact::MAX_RESPONSE_BYTES
        );
        assert_eq!(result["allowed"], false);
        assert_eq!(result["eventSha256"], "event");
        assert_eq!(result["governor"]["state"], "blocked");
        assert_eq!(result["next"]["role"], "lead");
        assert_eq!(result["audit"]["commandSuffix"][3], "--event");
    }

    #[test]
    fn historical_allowed_event_is_not_advertised_as_a_current_grant() {
        let permission = json!({
            "allowed": true,
            "event": {
                "action": "govern",
                "eventSha256": "outer-grant",
                "runId": "run", "stage": 1, "attempt": 2, "agent": "worker-a",
                "role": "worker", "subjectSha256": "subject",
                "assignmentSha256": crate::evidence::hash::text("worker-a"),
                "inputsSha256": "inputs",
                "governorEvent": {
                    "action": "mutation", "eventSha256": "nested-grant",
                    "runId": "run", "subjectSha256": "subject", "attempt": 2,
                    "checkpoint": 1, "inputSha256": "inputs", "carryLineage": true,
                    "lineageSha256": "lineage", "unit": "worker-mutation"
                }
            },
            "governor": {"state": "replan_required"}
        });
        let mut next = json!({"packet":{"stage":1,"attempt":2,"agent":"worker-a","role":"worker","context":{
          "run":{"id":"run"},"subject":{"sha256":"subject"},"loop":{
            "headSha256":"nested-grant", "runId":"run", "state":"replan_required",
            "currentGrantEventSha256":"nested-grant",
            "lineageSha256":"lineage",
            "currentMutation":{
                "eventSha256":"nested-grant", "runId":"run", "subjectSha256":"subject",
                "attempt":2, "checkpoint":1, "inputSha256":"inputs", "carryLineage":true,
                "unit":"worker-mutation"
            },
            "consumedGrants":[]
        }}}});
        assert!(is_current_grant(&permission, &next, "worker-a"));
        next["packet"]["context"]["loop"]["consumedGrants"] = json!(["outer-grant"]);
        assert!(!is_current_grant(&permission, &next, "worker-a"));
        next["packet"]["context"]["loop"]["consumedGrants"] = json!([]);
        next["packet"]["context"]["loop"]["headSha256"] = json!("later-event");
        assert!(is_current_grant(&permission, &next, "worker-a"));
        next["packet"]["context"]["loop"]["currentGrantEventSha256"] = json!("later-event");
        assert!(!is_current_grant(&permission, &next, "worker-a"));
        next["packet"]["context"]["loop"]["currentGrantEventSha256"] = json!("nested-grant");
        next["packet"]["context"]["loop"]["state"] = json!("unknown");
        assert!(!is_current_grant(&permission, &next, "worker-a"));
    }

    #[test]
    fn evidence_required_keeps_only_the_exact_present_grant_usable() {
        let permission = json!({
            "allowed": true,
            "event": {
                "action":"govern", "eventSha256":"outer-grant", "runId":"run",
                "stage":1, "attempt":2, "agent":"worker-a", "role":"worker",
                "subjectSha256":"subject", "assignmentSha256":crate::evidence::hash::text("worker-a"),
                "inputsSha256":"inputs", "governorEvent":{
                    "action":"mutation", "eventSha256":"nested-grant", "runId":"run",
                    "subjectSha256":"subject", "attempt":2, "checkpoint":1,
                    "inputSha256":"inputs", "carryLineage":true, "lineageSha256":"lineage",
                    "unit":"worker-mutation"
                }
            },
            "governor":{"state":"ready"}
        });
        let next = json!({"action":"spawn","role":"worker","assignment":"worker-a","packet":{
            "stage":1,"attempt":2,"agent":"worker-a","role":"worker","context":{
                "run":{"id":"run"},"subject":{"sha256":"subject"},"loop":{
                    "headSha256":"nested-grant","runId":"run","state":"evidence_required",
                    "lineageSha256":"lineage","currentGrantEventSha256":"nested-grant",
                    "currentMutation":{"eventSha256":"nested-grant","runId":"run",
                        "subjectSha256":"subject","attempt":2,"checkpoint":1,
                        "inputSha256":"inputs","carryLineage":true,"unit":"worker-mutation"},
                    "consumedGrants":[]
                }
            }
        }});
        assert!(is_current_grant(&permission, &next, "worker-a"));
        let result = compact(
            "work-a",
            "worker-a",
            "edit",
            "runs/work-a.jsonl",
            &permission,
            &next,
        );
        assert_eq!(result["currentGrant"]["runEventSha256"], "outer-grant");
        assert_eq!(result["nextRequest"]["requiredAction"], "work evidence");

        let mut held_next = next.clone();
        held_next["held"] = json!({"reference":"held-result","sha256":"held-sha","bytes":24});
        let held_result = compact(
            "work-a",
            "worker-a",
            "edit",
            "runs/work-a.jsonl",
            &permission,
            &held_next,
        );
        assert_eq!(held_result["allowed"], true);
        assert!(held_result.get("currentGrant").is_none());

        let mut stale = next;
        stale["packet"]["context"]["loop"]["currentGrantEventSha256"] = json!(null);
        assert!(!is_current_grant(&permission, &stale, "worker-a"));
    }
}

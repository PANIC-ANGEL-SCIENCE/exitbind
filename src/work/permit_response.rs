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
    if let Some(idempotent) = permission.get("idempotent") {
        response["idempotent"] = idempotent.clone();
    }
    response
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
}

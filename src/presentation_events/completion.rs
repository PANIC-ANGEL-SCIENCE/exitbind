//! Small completion events for the Lead's normal mutation-response channel.
use serde_json::{json, Value};

pub(crate) fn goal_completion(record: &Value, rendered: &Value) -> Option<Value> {
    let event = record["eventSha256"].as_str()?;
    if event.len() != 64
        || !event.bytes().all(|b| b.is_ascii_hexdigit())
        || rendered["explicitLeadClosure"] != true
        || rendered["currentReadiness"]["state"] != "current"
        || ![
            "subgoals",
            "findings",
            "blockers",
            "decisions",
            "externalActions",
        ]
        .iter()
        .all(|key| {
            rendered[*key]
                .as_array()
                .is_some_and(|items| items.is_empty())
        })
    {
        return None;
    }
    let mode = rendered["completionMode"].as_str()?;
    let terminal = match mode {
        "governed" => "This room remains nothing.\nEXIT READY",
        "direct" => "This room remains nothing.",
        _ => return None,
    };
    Some(json!({
        "id":format!("session-goal-complete:{event}"),
        "kind":"session_goal_completed", "revision":record["revision"],
        "mode":mode, "terminal":terminal,
        "delivery":"Print this completion once for this event ID in the current conversation; no status polling is needed.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_requires_current_entire_goal_and_has_one_stable_event_identity() {
        let record = json!({"eventSha256":"a".repeat(64),"revision":4});
        let mut rendered = json!({"explicitLeadClosure":true,"currentReadiness":{"state":"current"},
            "completionMode":"governed","subgoals":[],"findings":[],"blockers":[],"decisions":[],"externalActions":[]});
        let first = goal_completion(&record, &rendered).unwrap();
        assert_eq!(first, goal_completion(&record, &rendered).unwrap());
        assert_eq!(first["terminal"], "This room remains nothing.\nEXIT READY");
        for key in [
            "subgoals",
            "findings",
            "blockers",
            "decisions",
            "externalActions",
        ] {
            let mut pending = rendered.clone();
            pending[key] = json!(["unfinished"]);
            assert!(goal_completion(&record, &pending).is_none());
            pending.as_object_mut().unwrap().remove(key);
            assert!(goal_completion(&record, &pending).is_none());
        }
        for state in ["stale", "unknown"] {
            rendered["currentReadiness"]["state"] = json!(state);
            assert!(goal_completion(&record, &rendered).is_none());
        }
        rendered["currentReadiness"]["state"] = json!("current");
        rendered["completionMode"] = json!("direct");
        assert_eq!(
            goal_completion(&record, &rendered).unwrap()["terminal"],
            "This room remains nothing."
        );
        assert!(goal_completion(&json!({}), &rendered).is_none());
    }
}

// Binds a Lead stop decision to current native Goal state.
// Leaves stop unchanged without a verified session; see `src/host/runtime.rs`.
//! Bound a Lead stop to the existing native Goal and an explicit owner wait.
//!
//! This reads current state only. A missing or unverifiable selector, owner,
//! configuration, Goal, or host repeat flag leaves the host's stop unchanged.

use super::project::{self, Resolution, Source};
use crate::session_goal::native;
use serde_json::{Map, Value};

pub(super) fn block_reason(
    host: Option<&str>,
    payload: &Map<String, Value>,
) -> Option<&'static str> {
    if payload.get("stop_hook_active") != Some(&Value::Bool(false))
        || payload.get("agent_id").is_some_and(|id| !id.is_null())
    {
        return None;
    }
    let Some(reported) = payload.get("session_id").and_then(Value::as_str) else {
        return None;
    };
    let Some(host) = host else {
        return None;
    };
    let Ok(Some(session)) = native::host_session_readonly(host, reported) else {
        return None;
    };
    let snapshot = match session.recovery_snapshot(native::Access::ReadExisting) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return None,
        Err(_) => {
            if session.corrected_journal_for_stop().unwrap_or(false) {
                return Some("The session Goal is unreadable. Inspect its owner and journal with a compatible Exitbind reader before continuing.");
            }
            let Ok(Some(owner)) = session.legacy_owner_with_journal() else {
                return None;
            };
            // This candidate carries no Goal state or currentness. It lets the
            // existing resolver check the independently validated envelope
            // against the actual owner configuration before refusing Stop.
            let candidate = native::RecoverySnapshot {
                owner,
                latest: None,
                owner_config: None,
            };
            let Resolution::Project {
                loaded,
                source: Source::GoalOwner,
                ..
            } = project::resolve_for_session_readonly(payload, &session, &candidate)
            else {
                return None;
            };
            return crate::host::session_recovery::is_owner(&loaded, &candidate.owner).then_some(
                "The session Goal is unreadable. Inspect its owner and journal with a compatible Exitbind reader before continuing.",
            );
        }
    };
    let Resolution::Project {
        loaded,
        source: Source::GoalOwner,
        ..
    } = project::resolve_for_session_readonly(payload, &session, &snapshot)
    else {
        return None;
    };
    if !crate::host::session_recovery::is_owner(&loaded, &snapshot.owner) {
        return None;
    }
    let Some(goal) = snapshot.latest else {
        return None;
    };
    (goal["closure"]["closed"] == false && !declared_wait(&goal, payload))
        .then_some("The session Goal is open. Continue the authorized work.")
}

/// The Lead declares a genuine owner-only wait by recording its exact text as
/// an open Goal decision and emitting that same text as the entire message.
/// Text equality is a declaration, not proof of the question's semantics.
fn declared_wait(goal: &Value, payload: &Map<String, Value>) -> bool {
    let Some(message) = payload
        .get("last_assistant_message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
    else {
        return false;
    };
    goal["decisions"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|decision| {
            decision["disposition"] == "open"
                && decision["text"].as_str().map(str::trim) == Some(message)
        })
}

#[cfg(test)]
mod tests {
    use super::declared_wait;
    use serde_json::json;

    #[test]
    fn only_exact_open_decision_text_declares_an_owner_wait() {
        let goal = json!({"decisions":[
            {"text":"Which destination should I use?","disposition":"open"},
            {"text":"A previously answered question?","disposition":"accepted"}
        ]});
        for (message, expected) in [
            (" Which destination should I use?\n", true),
            ("The side answer. Which destination should I use?", false),
            ("Which destination should I use? I will continue.", false),
            ("Why stop now?", false),
            ("A previously answered question?", false),
            ("", false),
        ] {
            let payload = json!({"last_assistant_message":message});
            assert_eq!(declared_wait(&goal, payload.as_object().unwrap()), expected);
        }
    }
}

//! Recover retained prepared-child results without rerunning a host.

use super::{continuation, current_binding, load, record, return_result, same_host, with_intents};
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

pub(crate) fn recover(
    loaded: &Loaded,
    work: &str,
    intent_id: &str,
    fence: Option<(u64, u64)>,
) -> Result<Value, String> {
    let captured = load(loaded)?.into_iter().find(|intent| {
        intent["work"] == work
            && intent["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(intent_id))
            && intent["pendingReturn"] == true
    });
    if let Some(intent) = captured {
        if let Some((expected_revision, binding_revision)) = fence {
            let (goal, binding) = current_binding(loaded, work)?;
            if goal.as_u64() != Some(expected_revision)
                || binding["revision"].as_u64() != Some(binding_revision)
                || binding["session"] != intent["session"]
                || binding["revision"] != intent["bindingRevision"]
            {
                return Err(
                    "the captured child's receiving binding changed; refresh its current context"
                        .into(),
                );
            }
        } else {
            if intent["receiving"] != same_host::RECEIVING {
                return Err(format!("this child was received from another host; recover it with --context and a fresh token from {}", continuation(loaded, work)));
            }
            same_host::attributable(loaded, work, &intent)?;
        }
        return return_result::process(
            loaded,
            intent["id"]
                .as_str()
                .ok_or("child intent identity unavailable")?,
        );
    }
    with_intents(loaded, |intents| {
        let Some(intent) = intents.iter_mut().find(|intent| {
            intent["work"] == work
                && intent["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(intent_id))
        }) else {
            return Err("no prepared child with that intent for this work".into());
        };
        let Some(message) = intent["pendingResult"].as_str().map(str::to_owned) else {
            return Err("this child has no retained result to recover".into());
        };
        let Some((expected_revision, binding_revision)) = fence else {
            if intent["receiving"] != same_host::RECEIVING {
                let route = continuation(loaded, work);
                return Err(format!("this child was received from another host; recover it with --context and a fresh token from {route}"));
            }
            same_host::attributable(loaded, work, intent)?;
            // A legacy same-host stop records the hash; Work owns its return.
            intent["state"] = json!("recorded");
            intent["resultSha256"] = json!(hash::text(&message));
            if let Some(object) = intent.as_object_mut() {
                object.remove("pendingResult");
                object.remove("failure");
            }
            return Ok(json!({"work": work, "intent": intent["id"], "effect": "recorded"}));
        };
        let (goal, binding) = current_binding(loaded, work)?;
        if goal.as_u64() != Some(expected_revision)
            || binding["revision"].as_u64() != Some(binding_revision)
        {
            let route = continuation(loaded, work);
            return Err(format!(
                "work context is stale; read {route} for a fresh token"
            ));
        }
        if binding["session"] != intent["session"] {
            return Err("the child ran under another receiving session; its result cannot be attributed to the current binding".into());
        }
        intent["bindingRevision"] = json!(binding_revision);
        record(loaded, work, &goal, intent, &message)?;
        Ok(json!({"work": work, "intent": intent["id"], "effect": "recorded"}))
    })
}

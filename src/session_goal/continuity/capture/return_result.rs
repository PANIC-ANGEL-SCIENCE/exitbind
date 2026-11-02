//! Bridge an already captured host message to the existing Work return owner.

use crate::{config::Loaded, evidence::hash, work};
use serde_json::{json, Value};

/// Decide whether a new capture may enter the existing Work return path.
pub(super) fn validate_capture(
    loaded: &Loaded,
    work_id: &str,
    intent: &Value,
    message: &str,
) -> Result<(), String> {
    if intent["assignmentDelivery"]["state"] != "presented" {
        if intent["assignmentDelivery"]["reason"] == super::actor_checks::CODEX_UNVERIFIED {
            return Err("Codex direct-hook context limit is unverified; no automatic Work return was credited".into());
        }
        return Err("complete host assignment and evidence presentation is unavailable; no automatic Work return was credited".into());
    }
    if intent["directAssignment"] != true
        || !intent["assignment"].is_string()
        || intent["selected"]["assignment"] != intent["assignment"]
    {
        return Err("captured host assignment identity is unavailable".into());
    }
    if !super::selected_is_current(loaded, work_id, &intent["selected"]) {
        return Err("selected assignment changed before host result return".into());
    }
    let (_, binding) = super::binding_of(loaded, work_id, intent)?;
    if binding["session"] != intent["session"] || binding["revision"] != intent["bindingRevision"] {
        return Err("receiving binding changed before host result return".into());
    }
    let current = work::current_action(loaded, work_id)?;
    let delivery = &intent["assignmentDelivery"];
    let recipient = delivery["recipientSha256"]
        .as_str()
        .ok_or("complete delivered recipient proof is unavailable")?;
    let current_recipient = work::details::recipient_fingerprint(loaded, &current)?;
    if current_recipient != recipient {
        return Err(work::details::recipient_drift(
            loaded,
            work_id,
            recipient,
            &current_recipient,
        ));
    }
    let expected = delivery["instructionsSha256"]
        .as_str()
        .ok_or("complete delivered instruction proof is unavailable")?;
    let id = intent["id"]
        .as_str()
        .ok_or("child intent identity unavailable")?;
    let recorded = super::actor_checks::recorded_proof(intent);
    let host_context = match recorded {
        super::actor_checks::RecordedProof::Legacy => {
            super::context::legacy_host_context(loaded, work_id, id)?
        }
        super::actor_checks::RecordedProof::Selected { .. } => {
            super::context::host_context(loaded, work_id, id)?
        }
        super::actor_checks::RecordedProof::Invalid => {
            return Err("selected host actor presentation proof is unavailable or changed".into())
        }
    };
    if host_context["requiredInstructionsSha256"] != expected {
        return Err("delivered host instructions changed before host result return".into());
    }
    let actor_facts = match recorded {
        super::actor_checks::RecordedProof::Legacy => None,
        super::actor_checks::RecordedProof::Selected { facts, contract }
            if host_context["actorContractSha256"] == contract =>
        {
            Some(facts)
        }
        _ => return Err("selected host actor presentation proof is unavailable or changed".into()),
    };
    let exact =
        delivery["binding"].is_string() && current["current"]["binding"] == delivery["binding"];
    if exact && actor_facts.is_some_and(|facts| host_context["actorFactsSha256"] != facts) {
        let route = crate::emitted::refresh(work_id, &loaded.path);
        return Err(format!(
            "selected host actor facts changed after delivery; {route}"
        ));
    }
    let authorized = if exact {
        false
    } else {
        work::validate_child_delivery_successor(
            loaded,
            work_id,
            intent["assignment"].as_str().unwrap_or_default(),
            &current,
            delivery,
        )?
    };
    if !exact && !authorized {
        return Err("the presented Work binding changed before host result return".into());
    }
    let identity = &intent["directResultIdentity"];
    if current["role"] != identity["role"]
        || current["agent"] != identity["agent"]
        || current["packet"]["stage"] != identity["stage"]
        || current["packet"]["attempt"] != identity["attempt"]
    {
        return Err("host result identity differs from the current assignment".into());
    }
    work::action::validate_host_result(loaded, work_id, &current, message)
}

/// An ineligible observation remains inspectable and has no Work return.
pub(super) fn retain_raw(
    loaded: &Loaded,
    work_id: &str,
    intent: &mut Value,
    message: &str,
    reason: &str,
) {
    let diagnostic = crate::drift::split(reason)
        .map(|(context, value)| crate::drift::machine_with_context(context, value));
    let display = diagnostic
        .as_ref()
        .and_then(|value| value["error"].as_str())
        .unwrap_or(reason);
    let reason = display.chars().take(512).collect::<String>();
    let assignment = intent["assignment"].as_str().unwrap_or_default();
    let artifact = work::retain_observation(loaded, work_id, assignment, message.as_bytes());
    intent["state"] = json!("recorded");
    intent["rawObservation"] = match artifact {
        Ok(artifact) => {
            json!({"artifact":artifact,"sha256":hash::text(message),"bytes":message.len(),"reason":reason})
        }
        Err(error) => json!({"text":message,"sha256":hash::text(message),"bytes":message.len(),
            "reason":reason,"storageWarning":error.chars().take(256).collect::<String>()}),
    };
    if let Some(diagnostic) = diagnostic {
        intent["rawObservation"]["diagnostic"] = diagnostic;
    }
    if let Some(object) = intent.as_object_mut() {
        object.remove("pendingResult");
        object.remove("pendingReturn");
    }
}

/// The child intent already contains the complete bounded message before
/// this starts. A lost reply is resolved against the committed Work artifact,
/// never by executing the child again.
pub(super) fn process(loaded: &Loaded, id: &str) -> Result<Value, String> {
    super::with_intents(loaded, |intents| {
        let intent = intents
            .iter_mut()
            .find(|intent| intent["id"] == id)
            .ok_or("prepared child intent is unknown")?;
        if intent["pendingReturn"] != true {
            return Err("this child has no captured Work return to recover".into());
        }
        let work_id = intent["work"]
            .as_str()
            .ok_or("child work unavailable")?
            .to_owned();
        let assignment = intent["assignment"]
            .as_str()
            .ok_or("child assignment unavailable")?
            .to_owned();
        let message = intent["pendingResult"]
            .as_str()
            .ok_or("captured child result unavailable")?
            .to_owned();
        if intent["directAssignment"] != true
            || intent["selected"]["assignment"] != assignment
            || intent["resultSha256"] != hash::text(&message)
        {
            intent["failure"] = json!("captured host result identity or bytes changed");
            return Ok(
                json!({"work":work_id,"intent":id,"effect":"retained","reason":intent["failure"]}),
            );
        }
        let replay = work::replay_recorded_native(
            loaded,
            &work_id,
            &assignment,
            &intent["directResultIdentity"],
            message.as_bytes(),
        );
        let returned = if replay.is_ok() {
            replay
        } else if let Err(reason) = validate_capture(loaded, &work_id, intent, &message) {
            Err(reason)
        } else {
            let (_, binding) = super::binding_of(loaded, &work_id, intent)?;
            if binding["session"] != intent["session"]
                || binding["revision"] != intent["bindingRevision"]
            {
                Err("receiving binding changed before host result return".to_owned())
            } else {
                let current = work::next(loaded, &work_id)?;
                let action = &current["next"];
                let identity = &intent["directResultIdentity"];
                if action["role"] != identity["role"]
                    || action["agent"] != identity["agent"]
                    || action["packet"]["stage"] != identity["stage"]
                    || action["packet"]["attempt"] != identity["attempt"]
                {
                    Err("host result identity differs from the current assignment".to_owned())
                } else {
                    work::action::return_host_result(loaded, &work_id, action, &message).or_else(
                        |error| {
                            work::replay_recorded_native(
                                loaded,
                                &work_id,
                                &assignment,
                                identity,
                                message.as_bytes(),
                            )
                            .map_err(|_| error)
                        },
                    )
                }
            }
        };
        match returned {
            Ok(response)
                if response["effect"] == "recorded"
                    && response["status"] != "refused"
                    && response["eventSha256"].is_string() =>
            {
                intent["returnEventSha256"] = response["eventSha256"].clone();
                if let Some(object) = intent.as_object_mut() {
                    object.remove("pendingResult");
                    object.remove("pendingReturn");
                    object.remove("failure");
                }
                Ok(json!({"work":work_id,"intent":id,"effect":"recorded",
                    "eventSha256":response["eventSha256"],"idempotent":response["idempotent"]}))
            }
            Ok(response) => {
                intent["failure"] = json!(if response["effect"] == "held" {
                    "host result is held by the current governor; follow the current Work action"
                } else {
                    "host result was not accepted as a Work return; follow the current Work action"
                });
                if response["held"].is_object() {
                    intent["held"] = response["held"].clone();
                }
                Ok(
                    json!({"work":work_id,"intent":id,"effect":"retained", "reason":intent["failure"]}),
                )
            }
            Err(error) => {
                intent["failure"] = json!(error.chars().take(512).collect::<String>());
                Ok(
                    json!({"work":work_id,"intent":id,"effect":"retained", "reason":intent["failure"]}),
                )
            }
        }
    })
}

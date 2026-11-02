//! Carry one actually presented child form through its exact governor grant.

use super::{actor_checks, context, load, selected_is_current, text, with_intents};
use crate::{config::Loaded, evidence::hash, work};
use serde_json::{json, Value};

pub(crate) struct PermitTransition<'a> {
    pub work: &'a str,
    pub assignment: &'a str,
    pub presented_binding: &'a str,
    pub form_sha256: &'a str,
    pub request_id: &'a str,
    pub required_facts_sha256: &'a str,
    pub recipient_sha256: &'a str,
    pub actor_facts_sha256: Option<&'a str>,
    pub successor_binding: &'a str,
    pub run_event_sha256: &'a str,
    pub governor_event_sha256: &'a str,
}

pub(crate) fn has_presented(loaded: &Loaded, work: &str, assignment: &str) -> Result<bool, String> {
    Ok(super::load(loaded)?.iter().any(|intent| {
        intent["state"] == "claimed"
            && intent["work"] == work
            && intent["assignment"] == assignment
            && intent["directAssignment"] == true
            && intent["assignmentDelivery"]["state"] == "presented"
            && intent["assignmentDelivery"]["permitForm"].is_object()
    }))
}

fn matches(intent: &Value, proof: &PermitTransition<'_>) -> bool {
    intent["state"] == "claimed"
        && intent["work"] == proof.work
        && intent["assignment"] == proof.assignment
        && intent["directAssignment"] == true
        && intent["selected"]["assignment"] == proof.assignment
        && intent["assignmentDelivery"]["state"] == "presented"
        && intent["assignmentDelivery"]["binding"] == proof.presented_binding
        && intent["assignmentDelivery"]["permitForm"]["sha256"] == proof.form_sha256
        && intent["assignmentDelivery"]["permitForm"]["requestId"] == proof.request_id
        && intent["assignmentDelivery"]["requiredFactsSha256"] == proof.required_facts_sha256
        && intent["assignmentDelivery"]["recipientSha256"] == proof.recipient_sha256
        && match actor_checks::recorded_proof(intent) {
            actor_checks::RecordedProof::Legacy => true,
            actor_checks::RecordedProof::Selected { facts, .. } => {
                Some(facts) == proof.actor_facts_sha256
            }
            actor_checks::RecordedProof::Invalid => false,
        }
}

/// Called only with a transition proved by the current Work/ledger owner.
/// The presented binding remains immutable and older ambiguous captures do
/// not acquire this additive successor.
pub(crate) fn advance(loaded: &Loaded, proof: &PermitTransition<'_>) -> Result<bool, String> {
    if !super::load(loaded)?
        .iter()
        .any(|intent| matches(intent, proof))
    {
        return Ok(false);
    }
    super::with_intents(loaded, |intents| {
        let indices = intents
            .iter()
            .enumerate()
            .filter(|(_, intent)| matches(intent, proof))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [index] = indices[..] else {
            return Err("presented child permit transition is ambiguous".into());
        };
        let intent = &intents[index];
        let (_, receiving) = super::binding_of(loaded, proof.work, intent)?;
        if receiving["session"] != intent["session"]
            || receiving["revision"] != intent["bindingRevision"]
            || !super::selected_is_current(loaded, proof.work, &intent["selected"])
        {
            return Err("child receiving binding or assignment changed during permit".into());
        }
        work::details::ensure_action_binding(loaded, proof.work, Some(proof.successor_binding))?;
        let current = work::current_action(loaded, proof.work)?;
        if work::details::recipient_fingerprint(loaded, &current)? != proof.recipient_sha256 {
            return Err("child recipient instructions changed during permit".into());
        }
        let intent_id = intent["id"]
            .as_str()
            .ok_or("child intent identity unavailable")?;
        let recorded = actor_checks::recorded_proof(intent);
        let instructions = match recorded {
            actor_checks::RecordedProof::Legacy => {
                super::context::legacy_host_context(loaded, proof.work, intent_id)?
            }
            actor_checks::RecordedProof::Selected { .. } => {
                super::context::host_context(loaded, proof.work, intent_id)?
            }
            actor_checks::RecordedProof::Invalid => {
                return Err("child selected actor presentation changed during permit".into())
            }
        };
        if !intent["assignmentDelivery"]["instructionsSha256"].is_string()
            || instructions["requiredInstructionsSha256"]
                != intent["assignmentDelivery"]["instructionsSha256"]
        {
            return Err("child presented instructions changed during permit".into());
        }
        if let actor_checks::RecordedProof::Selected { contract, .. } = recorded {
            if instructions["actorContractSha256"] != contract {
                return Err("child selected actor presentation changed during permit".into());
            }
        }
        let identity = &intent["directResultIdentity"];
        if current["current"]["binding"] != proof.successor_binding
            || current["assignment"] != proof.assignment
            || current["role"] != identity["role"]
            || current["agent"] != identity["agent"]
            || current["packet"]["stage"] != identity["stage"]
            || current["packet"]["attempt"] != identity["attempt"]
            || current["packet"]["context"]["loop"]["currentGrantEventSha256"]
                != proof.governor_event_sha256
        {
            return Err("current child grant identity changed during permit".into());
        }
        intents[index]["assignmentDelivery"]["authorizedSuccessor"] = json!({
            "binding":proof.successor_binding,
            "runEventSha256":proof.run_event_sha256,
            "governorEventSha256":proof.governor_event_sha256,
        });
        Ok(true)
    })
}

/// Record only the product's completed host presentation; this does not
/// assert that a model read or followed the delivered bytes.
pub(crate) fn note_delivery(
    loaded: &Loaded,
    payload: &serde_json::Map<String, Value>,
    binding: Option<&str>,
    permit_form: Option<&Value>,
    facts: Option<(&str, &str, &str, &str, &str)>,
) -> Result<(), String> {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return Ok(());
    };
    if !load(loaded)?.iter().any(|intent| {
        intent["state"] == "claimed"
            && intent["session"] == session
            && intent["nativeChild"] == child
            && intent["directAssignment"] == true
    }) {
        return Ok(());
    }
    with_intents(loaded, |intents| {
        let Some(intent) = intents.iter_mut().find(|intent| {
            intent["state"] == "claimed"
                && intent["session"] == session
                && intent["nativeChild"] == child
                && intent["directAssignment"] == true
        }) else {
            return Ok(());
        };
        // A duplicate start cannot replace an already presented historical
        // proof or promote an unavailable first presentation into credit.
        if intent.get("assignmentDelivery").is_some() {
            return Ok(());
        }
        let work = intent["work"].as_str().ok_or("child work unavailable")?;
        let presented = binding.is_some_and(|expected| {
            crate::work::details::ensure_action_binding(loaded, work, Some(expected)).is_ok()
                && selected_is_current(loaded, work, &intent["selected"])
                && facts.is_some_and(|(required, recipient, instructions, actor, contract)| {
                    required.len() == 64
                        && recipient.len() == 64
                        && instructions.len() == 64
                        && actor.len() == 64
                        && contract.len() == 64
                        && crate::work::current_action(loaded, work)
                            .and_then(|current| {
                                crate::work::details::recipient_fingerprint(loaded, &current)
                            })
                            .is_ok_and(|current| current == recipient)
                        && intent["id"]
                            .as_str()
                            .and_then(|id| context::host_context(loaded, work, id).ok())
                            .is_some_and(|current| {
                                current["requiredInstructionsSha256"] == instructions
                                    && current["actorFactsSha256"] == actor
                                    && current["actorContractSha256"] == contract
                            })
                })
        });
        let proof = if presented {
            permit_form.and_then(|form| {
                let current = crate::work::current_action(loaded, work).ok()?;
                let current_binding = binding?;
                let canonical =
                    crate::work::action_forms::full(loaded, work, &current, current_binding);
                if canonical["beforeEditing"] != *form {
                    return None;
                }
                let request_id = crate::work::action_forms::permit_request_id(form)?;
                Some(json!({"sha256":hash::value(form),"requestId":request_id}))
            })
        } else {
            None
        };
        intent["assignmentDelivery"] = json!({"state": if presented { "presented" } else { "unavailable" },
            "binding":if presented { binding } else { None },
            "meaning":"complete product presentation only; native model use is not inferred"});
        intent["selectedDeliveryProtocol"] = json!(1);
        if !presented && intent["host"] == "codex" {
            intent["assignmentDelivery"]["reason"] = json!(super::actor_checks::CODEX_UNVERIFIED);
        }
        if let Some((required, recipient, instructions, actor, contract)) =
            facts.filter(|_| presented)
        {
            intent["assignmentDelivery"]["requiredFactsSha256"] = json!(required);
            intent["assignmentDelivery"]["recipientSha256"] = json!(recipient);
            intent["assignmentDelivery"]["instructionsSha256"] = json!(instructions);
            intent["assignmentDelivery"]["actorFactsSha256"] = json!(actor);
            intent["assignmentDelivery"]["actorContractSha256"] = json!(contract);
        }
        if let Some(proof) = proof {
            intent["assignmentDelivery"]["permitForm"] = proof;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_permit_proof_requires_the_exact_work_assignment_form_and_facts() {
        let actor = "a".repeat(64);
        let proof = PermitTransition {
            work: "work-a",
            assignment: "assignment-a",
            presented_binding: "binding-a",
            form_sha256: "form-a",
            request_id: "request-a",
            required_facts_sha256: "facts-a",
            recipient_sha256: "recipient-a",
            actor_facts_sha256: Some(&actor),
            successor_binding: "binding-b",
            run_event_sha256: "run-event",
            governor_event_sha256: "governor-event",
        };
        let original = json!({"state":"claimed", "work":"work-a", "assignment":"assignment-a",
            "host":"claude", "selectedDeliveryProtocol":1,
            "directAssignment":true, "selected":{"assignment":"assignment-a"},
            "assignmentDelivery":{"state":"presented", "binding":"binding-a",
                "permitForm":{"sha256":"form-a","requestId":"request-a"},
                "requiredFactsSha256":"facts-a", "recipientSha256":"recipient-a",
                "actorFactsSha256":actor.clone(), "actorContractSha256":"b".repeat(64)}});
        assert!(matches(&original, &proof));
        for pointer in [
            "/work",
            "/assignment",
            "/selected/assignment",
            "/assignmentDelivery/binding",
            "/assignmentDelivery/permitForm/sha256",
            "/assignmentDelivery/permitForm/requestId",
            "/assignmentDelivery/requiredFactsSha256",
            "/assignmentDelivery/recipientSha256",
            "/assignmentDelivery/actorFactsSha256",
            "/assignmentDelivery/actorContractSha256",
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = json!("different");
            assert!(!matches(&changed, &proof), "mismatch at {pointer}");
        }
        let mut legacy = original.clone();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("selectedDeliveryProtocol");
        let delivery = legacy["assignmentDelivery"].as_object_mut().unwrap();
        delivery.remove("actorFactsSha256");
        delivery.remove("actorContractSha256");
        assert!(
            matches(&legacy, &proof),
            "valid older deliveries retain their existing path"
        );
        legacy["assignmentDelivery"]["actorFactsSha256"] = Value::Null;
        assert!(
            !matches(&legacy, &proof),
            "malformed selected proof is not legacy absence"
        );
    }
}

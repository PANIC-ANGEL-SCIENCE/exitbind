//! Carry one actually presented child form through its exact governor grant.

use crate::{config::Loaded, work};
use serde_json::{json, Value};

pub(crate) struct PermitTransition<'a> {
    pub work: &'a str,
    pub assignment: &'a str,
    pub presented_binding: &'a str,
    pub form_sha256: &'a str,
    pub request_id: &'a str,
    pub required_facts_sha256: &'a str,
    pub recipient_sha256: &'a str,
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
        let instructions = super::context::host_context(loaded, proof.work, intent_id)?;
        if !intent["assignmentDelivery"]["instructionsSha256"].is_string()
            || instructions["requiredInstructionsSha256"]
                != intent["assignmentDelivery"]["instructionsSha256"]
        {
            return Err("child presented instructions changed during permit".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_permit_proof_requires_the_exact_work_assignment_form_and_facts() {
        let proof = PermitTransition {
            work: "work-a",
            assignment: "assignment-a",
            presented_binding: "binding-a",
            form_sha256: "form-a",
            request_id: "request-a",
            required_facts_sha256: "facts-a",
            recipient_sha256: "recipient-a",
            successor_binding: "binding-b",
            run_event_sha256: "run-event",
            governor_event_sha256: "governor-event",
        };
        let original = json!({"state":"claimed", "work":"work-a", "assignment":"assignment-a",
            "directAssignment":true, "selected":{"assignment":"assignment-a"},
            "assignmentDelivery":{"state":"presented", "binding":"binding-a",
                "permitForm":{"sha256":"form-a","requestId":"request-a"},
                "requiredFactsSha256":"facts-a", "recipientSha256":"recipient-a"}});
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
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = json!("different");
            assert!(!matches(&changed, &proof), "mismatch at {pointer}");
        }
    }
}

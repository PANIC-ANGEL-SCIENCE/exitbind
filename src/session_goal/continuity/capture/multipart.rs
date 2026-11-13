// Resolves pending Claude delivery for a claimed child intent.
// Refuses ambiguous multipart identities; see `src/session_goal/continuity/capture.rs`.
//! Pending Claude delivery belongs to the already claimed child intent.

use super::{context, delivery, load, text, with_intents};
use crate::{
    config::Loaded,
    evidence::hash,
    host::runtime::multipart::{output_digest, Bundle, Facts, Piece, SLOTS},
};
use serde_json::{json, Map, Value};

pub(super) fn already_claimed<'a>(
    intents: &'a [Value],
    session: &str,
    child: &str,
) -> Option<&'a Value> {
    intents.iter().find(|intent| {
        intent["state"] == "claimed"
            && intent["session"] == session
            && intent["nativeChild"] == child
    })
}

fn owned(intent: &Value, payload: &Map<String, Value>) -> bool {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return false;
    };
    intent["state"] == "claimed"
        && intent["host"] == "claude"
        && intent["directAssignment"] == true
        && intent["session"] == session
        && intent["nativeChild"] == child
        && super::same_observed_agent_type(intent, text(payload, "agent_type"))
}

fn owned_index(intents: &[Value], payload: &Map<String, Value>) -> Result<Option<usize>, String> {
    let mut matching = intents
        .iter()
        .enumerate()
        .filter(|(_, intent)| owned(intent, payload));
    let first = matching.next().map(|(index, _)| index);
    if matching.next().is_some() {
        return Err("child multipart intent identity is ambiguous".into());
    }
    Ok(first)
}

fn owned_work(loaded: &Loaded, payload: &Map<String, Value>) -> Result<Option<String>, String> {
    let intents = load(loaded)?;
    let Some(index) = owned_index(&intents, payload)? else {
        return Ok(None);
    };
    let work = intents[index]["work"]
        .as_str()
        .ok_or("child multipart Work identity is unavailable")?;
    Ok(Some(work.to_owned()))
}

fn identity(intent: &Value) -> String {
    hash::value(&json!({
        "intent":intent["id"], "host":intent["host"], "session":intent["session"],
        "child":intent["nativeChild"], "agentType":intent["observedAgentType"],
        "requestedAgentType":intent["agentType"],
        "receiving":intent["receiving"], "bindingRevision":intent["bindingRevision"],
        "work":intent["work"], "assignment":intent["assignment"],
        "selected":intent["selected"], "perspectives":intent["perspectives"],
        "capabilityBinding":intent["capabilityBinding"],
        "specialistBinding":intent["specialistBinding"],
        "directResultIdentity":intent["directResultIdentity"],
        "retryOf":intent["retryOf"], "retryFence":intent["retryFence"]
    }))
}

fn facts(value: &Value) -> Option<Facts<'_>> {
    let facts = (
        value["requiredFactsSha256"].as_str()?,
        value["requiredRecipientSha256"].as_str()?,
        value["requiredInstructionsSha256"].as_str()?,
        value["actorFactsSha256"].as_str()?,
        value["actorContractSha256"].as_str()?,
    );
    [facts.0, facts.1, facts.2, facts.3, facts.4]
        .iter()
        .all(|item| {
            item.len() == 64
                && item
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .then_some(facts)
}

fn permit(value: &Value) -> Option<&Value> {
    value["requiredAssignment"]["currentAction"]
        .get("beforeEditing")
        .filter(|form| form.is_object())
}

fn snapshot(child: &Value) -> Value {
    json!({
        "binding":child["requiredAssignmentBinding"],
        "requiredFactsSha256":child["requiredFactsSha256"],
        "requiredRecipientSha256":child["requiredRecipientSha256"],
        "requiredInstructionsSha256":child["requiredInstructionsSha256"],
        "actorFactsSha256":child["actorFactsSha256"],
        "actorContractSha256":child["actorContractSha256"],
        "permit":permit(child),
    })
}

fn current(loaded: &Loaded, intent: &Value, saved: &Value) -> bool {
    let (Some(work), Some(binding), Some(facts)) = (
        intent["work"].as_str(),
        saved["binding"].as_str(),
        facts(saved),
    ) else {
        return false;
    };
    let Ok(now) = context::candidate_host_context(loaded, work, intent) else {
        return false;
    };
    now["deliveryHost"] == "claude"
        && now["intent"] == intent["id"]
        && now["work"] == intent["work"]
        && now["assignment"] == intent["assignment"]
        && now["bindingRevision"] == intent["bindingRevision"]
        && now["configuredAgent"] == intent["selected"]["agent"]
        && delivery::matching_facts(&now, binding, facts)
        && permit(&now) == saved.get("permit").filter(|value| value.is_object())
}

fn invalidate_intent(intent: &mut Value) {
    if let Some(pending) = intent
        .get_mut("multipartDelivery")
        .and_then(Value::as_object_mut)
    {
        pending.remove("parts");
        pending.insert("state".into(), json!("invalid"));
    } else if intent["multipartDelivery"].is_null() {
        intent["multipartDelivery"] = json!({"state":"invalid"});
    }
}

/// Retain receipt hashes with the intent, but discard pending context bytes
/// when the child ends. A stopped partial bundle cannot be promoted later.
pub(super) fn retire(intent: &mut Value) {
    let Some(pending) = intent
        .get_mut("multipartDelivery")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    pending.remove("parts");
    if pending.get("state").and_then(Value::as_str) == Some("pending") {
        pending.insert("state".into(), json!("incomplete"));
    }
}

fn present(intent: &mut Value, saved: &Value, proof: Option<Value>) {
    intent["assignmentDelivery"] = json!({
        "state":"presented", "binding":saved["binding"],
        "requiredFactsSha256":saved["requiredFactsSha256"],
        "recipientSha256":saved["requiredRecipientSha256"],
        "instructionsSha256":saved["requiredInstructionsSha256"],
        "actorFactsSha256":saved["actorFactsSha256"],
        "actorContractSha256":saved["actorContractSha256"],
        "meaning":"complete product presentation only; native model use is not inferred"
    });
    intent["selectedDeliveryProtocol"] = json!(1);
    if let Some(proof) = proof {
        intent["assignmentDelivery"]["permitForm"] = proof;
    }
}

pub(super) fn select(
    loaded: &Loaded,
    payload: &Map<String, Value>,
    child: &Value,
    bundle: &Bundle,
    slot: usize,
) -> Result<Option<Piece>, String> {
    if slot >= SLOTS || bundle.parts.is_empty() || bundle.parts.len() > SLOTS {
        return Err("invalid child multipart slot or bundle".into());
    }
    let Some(work) = owned_work(loaded, payload)? else {
        return Ok(None);
    };
    let result = crate::work::launch_fence::with_live_work(loaded, &work, || {
        with_intents(loaded, |intents| {
            let Some(index) = owned_index(intents, payload)? else {
                return Ok(Ok(None));
            };
            let intent = &mut intents[index];
            if intent["work"] != work {
                invalidate_intent(intent);
                return Ok(Err(
                    "child multipart Work identity does not match its fence",
                ));
            }
            if intent.get("assignmentDelivery").is_some() {
                return Ok(Ok(None));
            }
            if intent["multipartDelivery"]["state"] == "invalid" {
                return Ok(Err("child multipart delivery was invalidated"));
            }
            let saved = snapshot(child);
            if child["intent"] != intent["id"]
                || child["work"] != intent["work"]
                || child["assignment"] != intent["assignment"]
                || child["bindingRevision"] != intent["bindingRevision"]
                || !current(loaded, intent, &saved)
            {
                invalidate_intent(intent);
                return Ok(Err(
                    "child multipart facts or identity changed before emission",
                ));
            }
            let part_hashes = bundle
                .parts
                .iter()
                .map(|part| output_digest(part))
                .collect::<Result<Vec<_>, _>>()?;
            let pending = &intent["multipartDelivery"];
            if !pending.is_null() {
                if pending["state"] != "pending"
                    || pending["identity"] != identity(intent)
                    || pending["facts"] != saved
                    || pending["bundleSha256"] != bundle.digest
                    || pending["parts"] != json!(bundle.parts)
                    || pending["partSha256"] != json!(part_hashes)
                {
                    invalidate_intent(intent);
                    return Ok(Err(
                        "child multipart bundle does not match the selected bundle",
                    ));
                }
            } else {
                intent["multipartDelivery"] = json!({
                    "state":"pending", "identity":identity(intent), "facts":saved,
                    "bundleSha256":bundle.digest, "parts":bundle.parts,
                    "partSha256":part_hashes, "receipts":vec![Value::Null; bundle.parts.len()]
                });
            }
            let Some((part, digest)) = bundle.parts.get(slot).zip(part_hashes.get(slot)) else {
                return Ok(Ok(None));
            };
            Ok(Ok(Some(Piece {
                bundle: bundle.digest.clone(),
                slot,
                digest: digest.clone(),
                text: part.clone(),
            })))
        })
    })?;
    result.map_err(str::to_owned)
}

/// Apply one exact receipt. Exact duplicates are harmless; conflicts never
/// contribute a distinct piece. The caller still owns currentness checks.
fn apply_receipt(
    pending: &mut Value,
    bundle: &str,
    slot: usize,
    piece: &str,
) -> Result<bool, &'static str> {
    if pending["state"] != "pending" || pending["bundleSha256"] != bundle {
        return Err("child multipart receipt names another bundle");
    }
    let hashes = pending["partSha256"]
        .as_array()
        .ok_or("child multipart hashes are missing")?
        .clone();
    if hashes.is_empty()
        || hashes.len() > SLOTS
        || hashes.get(slot).and_then(Value::as_str) != Some(piece)
    {
        return Err("child multipart receipt conflicts with the selected part");
    }
    let total = hashes.len();
    let receipts = pending["receipts"]
        .as_array_mut()
        .ok_or("child multipart receipts are missing")?;
    if receipts.len() != total {
        return Err("child multipart receipt count is invalid");
    }
    // A prior receipt is creditable only for its own selected index.
    // Inspect every entry before counting a complete set.
    if receipts
        .iter()
        .zip(&hashes)
        .any(|(receipt, hash)| !receipt.is_null() && receipt != hash)
    {
        return Err("child multipart contains a conflicting receipt");
    }
    if !receipts[slot].is_null() && receipts[slot] != piece {
        return Err("child multipart receipt conflicts with an earlier emission");
    }
    receipts[slot] = json!(piece);
    Ok(receipts.iter().all(Value::is_string))
}

fn intact_parts(pending: &Value) -> bool {
    let (Some(parts), Some(hashes)) = (
        pending["parts"].as_array(),
        pending["partSha256"].as_array(),
    ) else {
        return false;
    };
    !parts.is_empty()
        && parts.len() <= SLOTS
        && parts.len() == hashes.len()
        && parts.iter().zip(hashes).all(|(part, expected)| {
            part.as_str()
                .and_then(|part| output_digest(part).ok())
                .is_some_and(|digest| expected == &digest)
        })
}

pub(super) fn receipt(
    loaded: &Loaded,
    payload: &Map<String, Value>,
    bundle: &str,
    slot: usize,
    piece: &str,
) -> Result<(), String> {
    let work =
        owned_work(loaded, payload)?.ok_or("child multipart intent is unavailable at receipt")?;
    let result = crate::work::launch_fence::with_live_work(loaded, &work, || {
        with_intents(loaded, |intents| {
            let index = owned_index(intents, payload)?
                .ok_or("child multipart intent is unavailable at receipt")?;
            let intent = &mut intents[index];
            if intent["work"] != work {
                invalidate_intent(intent);
                return Ok(Err(
                    "child multipart Work identity does not match its fence",
                ));
            }
            if intent["assignmentDelivery"]["state"] == "presented" {
                return Ok(Ok(()));
            }
            let pending = &intent["multipartDelivery"];
            let saved = pending["facts"].clone();
            if pending["identity"] != identity(intent)
                || saved["binding"].as_str().is_none()
                || facts(&saved).is_none()
                || !intact_parts(pending)
            {
                invalidate_intent(intent);
                return Ok(Err(
                    "child multipart facts or identity are invalid at receipt",
                ));
            }
            let complete =
                match apply_receipt(&mut intent["multipartDelivery"], bundle, slot, piece) {
                    Ok(complete) => complete,
                    Err(error) => {
                        invalidate_intent(intent);
                        return Ok(Err(error));
                    }
                };
            if complete {
                // Nonfinal receipts cannot confer credit. Independently
                // reconstruct strict currentness before the final promotion.
                if !current(loaded, intent, &saved) {
                    invalidate_intent(intent);
                    return Ok(Err("child multipart facts changed before promotion"));
                }
                let work = intent["work"].as_str().unwrap_or_default();
                let binding = saved["binding"].as_str().unwrap_or_default();
                let form = saved.get("permit").filter(|value| value.is_object());
                let proof = delivery::permit_proof(loaded, work, binding, form);
                if form.is_some() && proof.is_none() {
                    invalidate_intent(intent);
                    return Ok(Err("child multipart permit form is no longer canonical"));
                }
                present(intent, &saved, proof);
                let pending = intent["multipartDelivery"]
                    .as_object_mut()
                    .expect("pending object");
                pending.remove("parts");
                pending.insert("state".into(), json!("complete"));
            }
            Ok(Ok(()))
        })
    })?;
    result.map_err(str::to_owned)
}

pub(super) fn invalidate(loaded: &Loaded, payload: &Map<String, Value>) -> Result<(), String> {
    if owned_work(loaded, payload)?.is_none() {
        return Ok(());
    }
    with_intents(loaded, |intents| {
        if let Some(index) = owned_index(intents, payload)? {
            let intent = &mut intents[index];
            if intent.get("assignmentDelivery").is_none() {
                invalidate_intent(intent);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> Value {
        json!({"state":"pending", "bundleSha256":"bundle", "partSha256":["a","b","c"], "receipts":[null,null,null]})
    }

    #[test]
    fn reversed_and_duplicate_receipts_require_every_distinct_piece() {
        let mut pending = pending();
        assert!(!apply_receipt(&mut pending, "bundle", 2, "c").unwrap());
        assert!(!apply_receipt(&mut pending, "bundle", 2, "c").unwrap());
        assert!(!apply_receipt(&mut pending, "bundle", 0, "a").unwrap());
        assert!(apply_receipt(&mut pending, "bundle", 1, "b").unwrap());
    }

    #[test]
    fn conflicting_and_foreign_receipts_are_not_creditable() {
        for (bundle, slot, piece) in [("other", 0, "a"), ("bundle", 0, "b"), ("bundle", 3, "c")] {
            let mut pending = pending();
            assert!(apply_receipt(&mut pending, bundle, slot, piece).is_err());
            assert!(pending["receipts"]
                .as_array()
                .unwrap()
                .iter()
                .all(Value::is_null));
        }
        let mut pending = pending();
        pending["receipts"][0] = json!("different");
        assert!(apply_receipt(&mut pending, "bundle", 1, "b").is_err());
    }

    #[test]
    fn identity_includes_session_child_revision_and_assignment() {
        let intent = json!({"id":"i","host":"claude","session":"s","nativeChild":"c", "bindingRevision":3,"work":"w","assignment":"a","selected":{"agent":"sonic"}});
        let original = identity(&intent);
        for key in [
            "session",
            "nativeChild",
            "bindingRevision",
            "work",
            "assignment",
            "selected",
        ] {
            let mut changed = intent.clone();
            changed[key] = json!("changed");
            assert_ne!(identity(&changed), original, "{key}");
        }
    }

    #[test]
    fn selection_requires_one_exact_session_child_and_agent() {
        let intent = json!({"state":"claimed","host":"claude","directAssignment":true,
            "session":"session","nativeChild":"child","observedAgentType":"sonic"});
        let payload = json!({"session_id":"session","agent_id":"child","agent_type":"sonic"});
        let input = payload.as_object().unwrap();
        assert_eq!(owned_index(&[intent.clone()], input).unwrap(), Some(0));
        assert!(owned_index(&[intent.clone(), intent.clone()], input).is_err());
        for key in ["session_id", "agent_id", "agent_type"] {
            let mut changed = payload.clone();
            changed[key] = json!("other");
            assert_eq!(
                owned_index(&[intent.clone()], changed.as_object().unwrap()).unwrap(),
                None
            );
        }
    }

    #[test]
    fn child_end_retires_payload_but_keeps_emission_receipts() {
        for state in ["pending", "complete"] {
            let mut intent = json!({"multipartDelivery":{
                "state":state, "parts":["private context"], "receipts":["receipt"]
            }});
            retire(&mut intent);
            assert!(intent["multipartDelivery"].get("parts").is_none());
            assert_eq!(intent["multipartDelivery"]["receipts"][0], "receipt");
            assert_eq!(
                intent["multipartDelivery"]["state"],
                if state == "pending" {
                    "incomplete"
                } else {
                    "complete"
                }
            );
        }
    }

    #[test]
    fn invalidation_keeps_recorded_receipts_without_reviving_the_bundle() {
        let mut intent = json!({"multipartDelivery":{
            "state":"pending", "parts":["private context"], "receipts":["receipt"]
        }});
        invalidate_intent(&mut intent);
        assert_eq!(intent["multipartDelivery"]["state"], "invalid");
        assert_eq!(intent["multipartDelivery"]["receipts"][0], "receipt");
        assert!(intent["multipartDelivery"].get("parts").is_none());
        let mut unknown = json!({"multipartDelivery":"unknown"});
        invalidate_intent(&mut unknown);
        assert_eq!(unknown["multipartDelivery"], "unknown");
    }
}

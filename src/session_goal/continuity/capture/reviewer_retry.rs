//! One same-session retry of an ended, uncredited read-only reviewer.
//!
//! The original observation stays uncredited. This is an admission fence for a
//! new child, not a claim that the failed presentation had a Work binding.

use super::{raw, same_host, storage, MAX_KEPT};
use crate::{config::Loaded, evidence::hash, work};
use serde_json::{json, Map, Value};

fn digest(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn read_only(packet: &Value) -> bool {
    let boundary = &packet["declaredBoundary"];
    [
        "write",
        "commands",
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryReject",
        "memoryRevoke",
        "memoryExpire",
        "memoryForget",
    ]
    .iter()
    .all(|name| boundary[*name].as_array().is_some_and(Vec::is_empty))
}

fn target<'a>(
    intents: &'a [Value],
    work: &str,
    assignment: &str,
    id: &str,
    binding: &Value,
) -> Result<&'a Value, String> {
    let mut matches = intents.iter().filter(|intent| intent["id"] == id);
    let old = matches.next().ok_or("retry source child is unavailable")?;
    if matches.next().is_some()
        || old["work"] != work
        || old["assignment"] != assignment
        || old["directAssignment"] != true
        || old["directResultIdentity"]["role"] != "reviewer"
        || old["state"] != "recorded"
        || old["receiving"] != same_host::RECEIVING
        || old["session"] != binding["session"]
        || old["host"] != binding["host"]
        || old["bindingRevision"] != binding["revision"]
        || old["nativeChild"].as_str().map_or(true, str::is_empty)
        || old["assignmentDelivery"]["state"] != "unavailable"
        || !old["rawObservation"].is_object()
        || !old["pendingResult"].is_null()
        || (!old["pendingReturn"].is_null() && old["pendingReturn"] != false)
        || has_marker(old)
        || !old["returnEventSha256"].is_null()
        || !old["conflict"].is_null()
    {
        return Err("retry source is not an ended uncredited reviewer from this session".into());
    }
    Ok(old)
}

fn current_fence(binding: &Value, sections: &Value, assignment: &str) -> Result<Value, String> {
    let packet = &sections["assignment"]["assignment"];
    let checks = &packet["checkEvidence"];
    let passing = checks.as_array().is_some_and(|items| {
        !items.is_empty()
            && items
                .iter()
                .all(|item| item["status"] == "passed" && item["acquisition"] == "observed")
    });
    let inputs = &sections["assignment"]["residual"]["snapshot"]["inputsSha256"];
    let subject = &packet["context"]["subject"]["sha256"];
    if sections["assignment"]["available"] != true
        || packet["role"] != "reviewer"
        || packet["context"]["next"]["assignment"] != assignment
        || packet["context"]["next"]["role"] != "reviewer"
        || !read_only(packet)
        || !passing
        || sections["evidence"]["checkEvidence"] != *checks
        || digest(binding).is_none()
        || digest(inputs).is_none()
        || digest(subject).is_none()
    {
        return Err("retry requires a current read-only reviewer, exact subject and inputs, and passing observed checks".into());
    }
    Ok(
        json!({"binding":binding,"inputsSha256":inputs,"subjectSha256":subject,
        "assignment":assignment,"checkEvidenceSha256":hash::value(checks)}),
    )
}

fn observed_source(
    loaded: &Loaded,
    work: &str,
    old: &Value,
    receiving: &Value,
) -> Result<Value, String> {
    let digest = raw::verified_for_binding(loaded, work, old, receiving)?;
    if digest != old["rawObservation"]["sha256"] {
        return Err("retry source observation changed".into());
    }
    Ok(digest)
}

pub(super) fn admission(
    loaded: &Loaded,
    work: &str,
    next: &Value,
    binding: &Value,
    old_id: &str,
    intents: &[Value],
) -> Result<Value, String> {
    if digest(&json!(old_id)).is_none() || next["role"] != "reviewer" || next["action"] != "spawn" {
        return Err("retry requires an exact current reviewer intent".into());
    }
    let assignment = next["assignment"]
        .as_str()
        .ok_or("retry assignment is unavailable")?;
    let old = target(intents, work, assignment, old_id, binding)?;
    let old_sha = observed_source(loaded, work, old, binding)?;
    let detail = work::readable::current(loaded, work)?;
    if detail["binding"] != next["current"]["binding"] {
        return Err("current reviewer changed during retry admission".into());
    }
    let mut fence = current_fence(&detail["binding"], &detail["sections"], assignment)?;
    fence["oldObservationSha256"] = old_sha;
    Ok(fence)
}

/// Navigation is advisory; preparation independently repeats this decision
/// while both the Work and the intent state are locked.
pub(super) fn route(loaded: &Loaded, work: &str, next: &Value, binding: &Value) -> Option<String> {
    let intents = super::load(loaded).ok()?;
    validate_markers(&intents).ok()?;
    if storage::version(loaded).ok()? == 1 && !storage::can_upgrade(&intents) {
        return None;
    }
    let assignment = next["assignment"].as_str()?;
    let mut candidates = intents.iter().filter(|intent| {
        intent["work"] == work
            && intent["assignment"] == assignment
            && intent["state"] == "recorded"
            && intent["assignmentDelivery"]["state"] == "unavailable"
    });
    let old = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let id = old["id"].as_str()?;
    let retries = intents
        .iter()
        .filter(|item| item["retryOf"] == id)
        .collect::<Vec<_>>();
    if retries.len() > 1
        || retries
            .first()
            .is_some_and(|item| item["state"] != "prepared")
        || (retries.is_empty() && intents.len() >= MAX_KEPT)
    {
        return None;
    }
    admission(loaded, work, next, binding, id, &intents).ok()?;
    Some(id.to_owned())
}

pub(super) fn once(intents: &[Value], old_id: &str, id: &str) -> Result<(), String> {
    validate_markers(intents)?;
    let retries = intents
        .iter()
        .filter(|item| item["retryOf"] == old_id)
        .collect::<Vec<_>>();
    match retries.as_slice() {
        [] if intents.len() < MAX_KEPT => Ok(()),
        [existing] if existing["id"] == id && existing["state"] == "prepared" => {
            verify_identity(existing)
        }
        _ => Err("this reviewer retry is already used or the child intent bound is full".into()),
    }
}

pub(super) fn has_marker(intent: &Value) -> bool {
    ["retryOf", "retryFence", "retryReturnAttempted"]
        .iter()
        .any(|name| intent.get(*name).is_some())
}

/// A partial retry record must never be interpreted as an ordinary intent.
pub(super) fn marker(intent: &Value) -> Result<Option<&str>, String> {
    if !has_marker(intent) {
        return Ok(None);
    }
    let old = digest(&intent["retryOf"]).ok_or("reviewer retry marker is malformed")?;
    if !intent["retryFence"].is_object() {
        return Err("reviewer retry fence is malformed".into());
    }
    verify_identity(intent)?;
    Ok(Some(old))
}

fn validate_markers(intents: &[Value]) -> Result<(), String> {
    for intent in intents {
        marker(intent)?;
    }
    Ok(())
}

// Reconstruct the exact immutable intent used to create its existing ID.
// Delivery, claim and result fields are intentionally not part of this hash.
fn verify_identity(intent: &Value) -> Result<(), String> {
    let object = intent
        .as_object()
        .ok_or("reviewer retry intent is malformed")?;
    let mut identity = Map::new();
    for name in [
        "work",
        "session",
        "host",
        "bindingRevision",
        "assignment",
        "agentType",
        "selected",
        "perspectives",
        "capabilityBinding",
        "directAssignment",
        "directResultIdentity",
        "specialistBinding",
        "retryOf",
        "retryFence",
        "receiving",
    ] {
        if let Some(value) = object.get(name) {
            identity.insert(name.to_owned(), value.clone());
        }
    }
    let id = digest(&intent["id"]).ok_or("reviewer retry identity is unavailable")?;
    if hash::value(&Value::Object(identity)) != id {
        return Err("reviewer retry identity or frozen fence changed".into());
    }
    Ok(())
}

/// Recheck the fresh fence using the assignment already acquired for this
/// host presentation or return; never refresh an unavailable old delivery.
pub(super) fn ensure_fence(
    loaded: &Loaded,
    work: &str,
    intent: &Value,
    binding: &Value,
    sections: &Value,
) -> Result<(), String> {
    let Some(old_id) = marker(intent)? else {
        return Ok(());
    };
    let intents = super::load(loaded)?;
    let receiving = super::binding_of(loaded, work, intent)?.1;
    let assignment = intent["assignment"]
        .as_str()
        .ok_or("retry assignment is unavailable")?;
    let old = target(&intents, work, assignment, old_id, &receiving)?;
    let old_sha = observed_source(loaded, work, old, &receiving)?;
    let mut fresh = current_fence(binding, sections, assignment)?;
    fresh["oldObservationSha256"] = old_sha;
    if fresh != intent["retryFence"] {
        return Err("reviewer retry fence changed; prepare no child or return".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections() -> Value {
        let checks = json!([{"status":"passed","acquisition":"observed"}]);
        json!({"assignment":{"available":true,"assignment":{"role":"reviewer",
            "declaredBoundary":{"write":[],"commands":[],"memoryWrite":[],"memoryReview":[],
                "memoryPromote":[],"memoryReject":[],"memoryRevoke":[],"memoryExpire":[],"memoryForget":[]},
            "checkEvidence":checks,"context":{"next":{"assignment":"assignment-a","role":"reviewer"},
                "subject":{"sha256":"a".repeat(64)}}},
            "residual":{"snapshot":{"inputsSha256":"b".repeat(64)}}},
            "evidence":{"checkEvidence":checks}})
    }

    #[test]
    fn retry_fence_requires_read_only_current_observed_facts() {
        let binding = json!("c".repeat(64));
        let original = sections();
        assert!(current_fence(&binding, &original, "assignment-a").is_ok());
        for field in [
            "write",
            "commands",
            "memoryWrite",
            "memoryReview",
            "memoryPromote",
            "memoryReject",
            "memoryRevoke",
            "memoryExpire",
            "memoryForget",
        ] {
            let mut changed = original.clone();
            changed["assignment"]["assignment"]["declaredBoundary"][field] = json!(["permission"]);
            assert!(
                current_fence(&binding, &changed, "assignment-a").is_err(),
                "{field}"
            );
        }
        for change in [
            "unobserved",
            "failed",
            "alias",
            "inputs",
            "subject",
            "assignment",
            "missing-rights",
        ] {
            let mut changed = original.clone();
            match change {
                "unobserved" => {
                    changed["assignment"]["assignment"]["checkEvidence"][0]["acquisition"] =
                        json!("reported")
                }
                "failed" => {
                    changed["assignment"]["assignment"]["checkEvidence"][0]["status"] =
                        json!("failed")
                }
                "alias" => changed["evidence"]["checkEvidence"][0]["status"] = json!("failed"),
                "inputs" => {
                    changed["assignment"]["residual"]["snapshot"]["inputsSha256"] = Value::Null
                }
                "subject" => {
                    changed["assignment"]["assignment"]["context"]["subject"]["sha256"] =
                        json!("bad")
                }
                "assignment" => {
                    changed["assignment"]["assignment"]["context"]["next"]["assignment"] =
                        json!("different")
                }
                "missing-rights" => {
                    changed["assignment"]["assignment"]["declaredBoundary"]
                        .as_object_mut()
                        .unwrap()
                        .remove("memoryForget");
                }
                _ => unreachable!(),
            }
            assert!(
                current_fence(&binding, &changed, "assignment-a").is_err(),
                "{change}"
            );
        }
    }

    #[test]
    fn frozen_retry_identity_rejects_changed_fence_or_request() {
        let mut intent = json!({"work":"work-a","session":"session-a","host":"claude",
            "bindingRevision":0,"assignment":"assignment-a","agentType":"reviewer",
            "selected":{"assignment":"assignment-a"},"perspectives":[],"capabilityBinding":null,
            "directAssignment":true,"directResultIdentity":{"role":"reviewer"},
            "retryOf":"a".repeat(64),"retryFence":{"binding":"b".repeat(64)},"receiving":"same-host"});
        let id = hash::value(&intent);
        intent["id"] = json!(id);
        intent["state"] = json!("prepared");
        assert!(verify_identity(&intent).is_ok());
        let mut changed = intent.clone();
        changed["retryFence"]["binding"] = json!("c".repeat(64));
        assert!(verify_identity(&changed).is_err());
        changed = intent.clone();
        changed["perspectives"] = json!([{"id":"changed"}]);
        assert!(verify_identity(&changed).is_err());
        changed = intent.clone();
        changed.as_object_mut().unwrap().remove("retryOf");
        assert!(marker(&changed).is_err());
        changed = intent.clone();
        changed.as_object_mut().unwrap().remove("retryFence");
        assert!(marker(&changed).is_err());
        intent["state"] = json!("claimed");
        intent["assignmentDelivery"] = json!({"state":"presented"});
        assert!(verify_identity(&intent).is_ok());
    }

    #[test]
    fn retry_source_and_once_refuse_reuse_or_unknown_completion() {
        let old_id = "a".repeat(64);
        let binding = json!({"host":"claude","session":"session-a","revision":0});
        let old = json!({"id":old_id,"work":"work-a","assignment":"assignment-a",
            "directAssignment":true,"directResultIdentity":{"role":"reviewer"},"state":"recorded",
            "receiving":"same-host","host":"claude","session":"session-a","bindingRevision":0,
            "nativeChild":"old-child","assignmentDelivery":{"state":"unavailable"},"rawObservation":{}});
        assert!(target(&[old.clone()], "work-a", "assignment-a", &old_id, &binding).is_ok());
        for change in [
            "pending",
            "unknown-pending",
            "return",
            "child",
            "session",
            "chain",
            "conflict",
        ] {
            let mut changed = old.clone();
            match change {
                "pending" => changed["pendingResult"] = json!("unresolved"),
                "unknown-pending" => changed["pendingReturn"] = json!("unknown"),
                "return" => changed["returnEventSha256"] = json!("event"),
                "child" => changed["nativeChild"] = Value::Null,
                "session" => changed["session"] = json!("other"),
                "chain" => changed["retryOf"] = json!("earlier"),
                "conflict" => changed["conflict"] = json!({"resultSha256":"other"}),
                _ => unreachable!(),
            }
            assert!(
                target(&[changed], "work-a", "assignment-a", &old_id, &binding).is_err(),
                "{change}"
            );
        }
        let mut prepared = json!({"retryOf":old_id,"retryFence":{}});
        let retry_id = hash::value(&prepared);
        prepared["id"] = json!(retry_id);
        prepared["state"] = json!("prepared");
        assert!(once(&[prepared.clone()], &old_id, &retry_id).is_ok());
        assert!(once(&[prepared.clone()], &old_id, "different").is_err());
        let mut claimed = prepared;
        claimed["state"] = json!("claimed");
        assert!(once(&[claimed], &old_id, &retry_id).is_err());
        assert!(once(&vec![json!({}); MAX_KEPT], &old_id, "new").is_err());
    }
}

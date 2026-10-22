//! Expansion of one opaque reference from the current Work view: a grouped
//! or detail section, the bound ledger history or one event, or an observed
//! check's bounded log. Caller-supplied paths and hashes never substitute for
//! the packet-bound reference.
use super::{details, next_from, readable, resolve, text_page, text_page::hex};
use crate::{config::Loaded, evidence::hash, run};
use serde_json::{json, Value};

/// Expand one opaque reference emitted by the current context projection.
/// Paths and hashes supplied by a caller are never accepted as a substitute
/// for the packet-bound reference.
pub(crate) fn expand(loaded: &Loaded, work: &str, reference: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let next = next_from(loaded, work, &snapshot)?;
    if readable::is_grouped(reference) {
        return readable::expand(loaded, work, &snapshot, &next, reference);
    }
    if details::is_section(reference) {
        return details::expand(loaded, work, &snapshot, &next, reference);
    }
    let packet = crate::work::packet::project(work, &snapshot, &next, &loaded.path)?;
    let context = packet
        .get("context")
        .ok_or("current packet has no context projection")?;
    let canonical = find_reference(context, reference)
        .ok_or("reference is not an opaque reference from the current context")?;
    if canonical["exact"] != true || canonical["id"].as_str().is_none() {
        return Err("reference is malformed".into());
    }
    let (_path, events, source) = crate::run::ledger::load(loaded, &ledger)?;
    let raw_sha = hash::bytes(source.as_bytes());
    if canonical["sha256"] != raw_sha
        || canonical["headEventSha256"]
            != events
                .last()
                .and_then(|event| event["eventSha256"].as_str())
                .unwrap_or_default()
        || canonical["eventCount"].as_u64() != Some(events.len() as u64)
    {
        return Err("reference is stale or ledger bytes changed".into());
    }
    let kind = canonical["kind"].as_str().unwrap_or_default();
    match kind {
        "ledger_history" => Ok(expansion_response(
            &canonical,
            "history",
            source.as_bytes(),
            json!({"events": events}),
        )),
        "ledger_event" => {
            let wanted = canonical["selector"]
                .as_str()
                .ok_or("event reference has no selected event")?;
            let (index, event) = events
                .iter()
                .enumerate()
                .find(|(_, event)| event["eventSha256"].as_str() == Some(wanted))
                .ok_or("selected event is not in the referenced ledger")?;
            let line = source
                .split('\n')
                .filter(|line| !line.is_empty())
                .nth(index)
                .ok_or("selected event bytes are unavailable")?;
            Ok(expansion_response(
                &canonical,
                "event",
                line.as_bytes(),
                json!({"event": event, "eventIndex": index}),
            ))
        }
        "check_log" => expand_check_log(loaded, &canonical, &events),
        _ => Err("reference kind is unsupported".into()),
    }
}

fn find_reference(value: &Value, requested: &str) -> Option<Value> {
    if let Some(object) = value.as_object() {
        let matches = object.get("id").and_then(Value::as_str) == Some(requested);
        let expandable = matches
            && object.get("exact") == Some(&Value::Bool(true))
            && object
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    matches!(kind, "ledger_history" | "ledger_event" | "check_log")
                });
        if expandable {
            return Some(value.clone());
        }
        for child in object.values() {
            if let Some(found) = find_reference(child, requested) {
                return Some(found);
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            if let Some(found) = find_reference(child, requested) {
                return Some(found);
            }
        }
    }
    None
}

fn expansion_response(reference: &Value, kind: &str, bytes: &[u8], metadata: Value) -> Value {
    let mut response = json!({
        "valid": true,
        "kind": kind,
        "reference": reference,
        "encoding": "hex",
        "bytes": bytes.len(),
        "contentHex": hex(bytes),
        "contentText": text_page::text(bytes),
    });
    if let (Some(object), Some(extra)) = (response.as_object_mut(), metadata.as_object()) {
        object.extend(
            extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    response
}

fn expand_check_log(loaded: &Loaded, reference: &Value, events: &[Value]) -> Result<Value, String> {
    let wanted = reference["checkEventSha256"]
        .as_str()
        .ok_or("check-log reference has no check event")?;
    let event = events
        .iter()
        .find(|event| event["eventSha256"].as_str() == Some(wanted))
        .ok_or("check-log event is not in the referenced ledger")?;
    if event["action"] != "check" || event["acquisition"] != "observed" {
        return Err("check-log reference does not name an observed check".into());
    }
    let stdout = crate::run::artifact::read(loaded, &event["stdout"], "stdout")?;
    let stderr = crate::run::artifact::read(loaded, &event["stderr"], "stderr")?;
    if reference["stdout"] != crate::context::artifact_reference(&event["stdout"], "stdout")
        || reference["stderr"] != crate::context::artifact_reference(&event["stderr"], "stderr")
    {
        return Err("check-log artifact reference is stale or tampered".into());
    }
    Ok(json!({
        "valid": true,
        "kind": "check_log",
        "reference": reference,
        "event": event,
        "stdout": bounded_log_value(&stdout),
        "stderr": bounded_log_value(&stderr),
    }))
}

const MAX_LOG_DISPLAY_BYTES: usize = 64 * 1024;

fn bounded_log_value(artifact: &crate::run::artifact::VerifiedArtifact) -> Value {
    let displayed = artifact.preview.len().min(MAX_LOG_DISPLAY_BYTES);
    json!({
        "encoding": "hex",
        "bytes": artifact.bytes,
        "totalBytes": artifact.bytes,
        "sha256": artifact.sha256,
        "displayedBytes": displayed,
        "truncated": artifact.bytes > displayed as u64,
        "contentHex": hex(&artifact.preview[..displayed]),
        "contentText": text_page::text(&artifact.preview[..displayed]),
    })
}

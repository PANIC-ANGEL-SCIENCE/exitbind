use serde_json::{json, Map, Value};

use super::{bounded_text, copy_if_present};

pub(super) fn compact_next(next: &Value) -> Value {
    let mut result = Map::new();
    for key in [
        "action",
        "status",
        "assignment",
        "role",
        "agent",
        "stage",
        "attempt",
        "resolvedActor",
        "outcomes",
        "check",
        "progress",
        "held",
        "heldResults",
    ] {
        copy_if_present(next, &mut result, key);
    }
    if let Some(packet) = next.get("packet") {
        for key in ["stage", "attempt"] {
            if !result.contains_key(key) {
                copy_if_present(packet, &mut result, key);
            }
        }
        if !result.contains_key("goal") {
            if let Some(goal) = packet["goal"].as_str() {
                result.insert("goal".into(), json!(bounded_text(goal, 192)));
            }
        }
        result.insert("packet".into(), compact_packet(packet));
    }
    if let Some(warnings) = next.get("warnings").and_then(Value::as_array) {
        result.insert(
            "warnings".into(),
            Value::Array(
                warnings
                    .iter()
                    .filter_map(|warning| {
                        let Some(object) = warning.as_object() else {
                            return warning.is_string().then(|| warning.clone());
                        };
                        let mut item = Map::new();
                        for key in ["classification", "code", "reason", "error"] {
                            if let Some(value) = object.get(key) {
                                item.insert(key.to_owned(), value.clone());
                            }
                        }
                        Some(Value::Object(item))
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(result)
}

pub(super) fn preserve_next_omission_markers(source: &Value, target: &mut Value) {
    if let Some(target) = target.as_object_mut() {
        for key in ["requiresExpansion", "constraintsOmitted"] {
            copy_if_present(source, target, key);
        }
    }
}

fn compact_packet(packet: &Value) -> Value {
    let Some(object) = packet.as_object() else {
        return Value::Null;
    };
    let mut result = object.clone();
    result.remove("context");
    Value::Object(result)
}

pub(super) fn assignment_summary(packet: &Value) -> Value {
    let Some(object) = packet.as_object() else {
        return Value::Null;
    };
    let mut result = Map::new();
    for key in [
        "stage",
        "attempt",
        "agent",
        "displayName",
        "nativeTaskName",
        "role",
        "goal",
        "purpose",
        "runtime",
        "declaredBoundary",
        "architectureContractSha256",
        "basisSha256",
        "reviewDecisionSha256",
        "upstreamArtifacts",
        "preservationAssignment",
        "checkEvidence",
    ] {
        if let Some(value) = object.get(key) {
            result.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(result)
}

pub(super) fn minimal_next(next: &Value) -> Value {
    let mut result = Map::new();
    for key in [
        "action",
        "status",
        "assignment",
        "role",
        "agent",
        "stage",
        "attempt",
        "resolvedActor",
        "check",
        "heldResultsCount",
        "heldResultsOmissions",
        "requiresExpansion",
        "constraintsOmitted",
    ] {
        copy_if_present(next, &mut result, key);
    }
    if let Some(held) = next.get("held").filter(|held| !held.is_null()) {
        result.insert("held".into(), minimal_held(held));
    }
    if let Some(held) = next.get("heldResults").and_then(Value::as_array) {
        result.insert("heldResultsCount".into(), json!(held.len()));
        let summaries = held.iter().map(minimal_held).collect::<Vec<_>>();
        if serde_json::to_vec(&summaries).is_ok_and(|bytes| bytes.len() <= 1024) {
            result.insert("heldResults".into(), json!(summaries));
            result.insert("heldResultsOmissions".into(), json!(0));
        } else {
            result.insert("heldResultsOmissions".into(), json!(held.len()));
        }
    }
    if let Some(packet) = next.get("packet") {
        for key in ["stage", "attempt"] {
            if !result.contains_key(key) {
                copy_if_present(packet, &mut result, key);
            }
        }
        if !result.contains_key("goal") {
            if let Some(goal) = packet["goal"].as_str() {
                result.insert("goal".into(), json!(bounded_text(goal, 192)));
            }
        }
    }
    Value::Object(result)
}

fn minimal_held(held: &Value) -> Value {
    let Some(object) = held.as_object() else {
        return Value::Null;
    };
    let mut result = Map::new();
    for key in ["reference", "sha256", "bytes", "action"] {
        if let Some(value) = object.get(key) {
            if serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 256) {
                result.insert(key.to_owned(), value.clone());
            }
        }
    }
    Value::Object(result)
}

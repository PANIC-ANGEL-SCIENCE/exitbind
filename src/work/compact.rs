//! Bounded default projection for agent-facing work responses.
//!
//! The full response remains the compatibility surface.  This view keeps the
//! decision and its stable identities, while making omitted detail reachable
//! through the existing `work expand` references.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const MAX_RESPONSE_BYTES: usize = 8 * 1024;

/// Project a validated full work response into the bounded default envelope.
///
/// The projection never slices a string or raw byte payload.  Detail is
/// omitted by field, and exact expandable references are copied as complete
/// values so `work expand` remains the recovery path.
pub(crate) fn project(
    response: &Value,
    config_path: &Path,
    invoked: &str,
) -> Result<Value, String> {
    let mut references = BTreeMap::new();
    collect_references(response, &mut references);

    let mut result = Map::new();
    result.insert("compact".into(), json!(true));
    for key in ["status", "effect", "reason"] {
        copy_if_present(response, &mut result, key);
    }
    copy_if_present(response, &mut result, "work");
    if let Some(next) = response.get("next") {
        result.insert("next".into(), compact_next(next));
    }
    if let Some(help) = response
        .get("residual")
        .and_then(|value| value.get("humanHelp"))
    {
        result.insert("humanHelp".into(), help.clone());
        if let Some(action) = help.get("nextAction") {
            result.insert("nextAction".into(), action.clone());
        }
    }
    if let Some(obligations) = compact_obligations(response) {
        result.insert("obligations".into(), obligations);
    }
    if let Some(subject) = current_subject(response) {
        result.insert("currentSubject".into(), subject);
    }
    for key in ["recorder", "ledgerProducer", "recorderLimitation"] {
        copy_if_present(response, &mut result, key);
    }
    if let Some(presentation) = response.get("presentation") {
        result.insert("presentation".into(), compact_presentation(presentation));
    }
    result.insert(
        "references".into(),
        Value::Array(references.into_values().collect()),
    );
    result.insert("omitted".into(), omitted_fields());
    result.insert("truncated".into(), json!(false));
    result.insert(
        "fullCommand".into(),
        full_command(response, config_path, invoked),
    );

    let mut result = Value::Object(result);
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        let all_references = result["references"].as_array().cloned().unwrap_or_default();
        let representative = representative_references(&all_references);
        result["references"] = Value::Array(representative);
        result["referenceOmissions"] = json!(all_references
            .len()
            .saturating_sub(result["references"].as_array().map_or(0, Vec::len)));
        result["truncated"] = json!(true);
    }
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        if let Some(next) = result.get_mut("next") {
            if let Some(packet) = next.get_mut("packet") {
                *packet = assignment_summary(packet);
            }
        }
        result["omitted"] = json!([
            "residual detail",
            "assignment packet detail",
            "duplicated packet context and recovery context",
            "nonessential assignment fields"
        ]);
        result["truncated"] = json!(true);
    }
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        let next = minimal_next(&result["next"]);
        let actor = result["nextAction"]["actor"].clone();
        let references = representative_references(
            result["references"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default(),
        );
        result["next"] = next;
        result["nextAction"] = json!({
            "actor": actor,
            "command": Value::Null,
            "summary": "Open the full response with fullCommand."
        });
        let action = result["nextAction"].clone();
        if let Some(help) = result.get_mut("humanHelp") {
            *help = json!({
                "nextAction": action,
                "preservationAssignment": help["preservationAssignment"],
                "preservationEvidence": help["preservationEvidence"],
                "checkInstruction": help["checkInstruction"],
            });
        }
        result
            .as_object_mut()
            .expect("compact object")
            .remove("reason");
        result
            .as_object_mut()
            .expect("compact object")
            .remove("obligations");
        result["references"] = Value::Array(references);
        result["omitted"] = json!(["large assignment and action detail; use fullCommand"]);
        result["truncated"] = json!(true);
    }
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        let emergency = json!({
            "compact": true,
            "status": response["status"],
            "work": response["work"],
            "next": {
                "action": response["next"]["action"],
                "assignment": response["next"]["assignment"],
                "held": response["next"]["held"],
            },
            "nextAction": {"command": null, "summary": "Open the full response with fullCommand."},
            "humanHelp": {
                "preservationAssignment": response["residual"]["humanHelp"]["preservationAssignment"],
                "checkInstruction": response["residual"]["humanHelp"]["checkInstruction"],
            },
            "presentation": {"terminal": response["presentation"]["terminal"]},
            "fullCommand": full_command(response, config_path, invoked),
            "omitted": ["large response detail; use fullCommand"],
            "truncated": true,
        });
        if serialized_len(&emergency)? < MAX_RESPONSE_BYTES {
            return Ok(emergency);
        }
        let work = response["work"].as_str().filter(|value| value.len() <= 128);
        let assignment = response["next"]["assignment"]
            .as_str()
            .filter(|value| value.len() <= 128);
        return Ok(json!({
            "compact": true,
            "status": "unresolved",
            "work": work,
            "next": {"action": "inspect", "assignment": assignment},
            "nextAction": {"command": null, "summary": "Open the full response."},
            "humanHelp": {"preservationAssignment": response["residual"]["humanHelp"]["preservationAssignment"]},
            "fullCommand": full_command(response, config_path, invoked),
            "omitted": ["oversized response detail"],
            "truncated": true,
        }));
    }
    Ok(result)
}

fn serialized_len(value: &Value) -> Result<usize, String> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| error.to_string())
}

fn compact_next(next: &Value) -> Value {
    let mut result = Map::new();
    for key in [
        "action",
        "status",
        "assignment",
        "role",
        "agent",
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

fn compact_packet(packet: &Value) -> Value {
    let Some(object) = packet.as_object() else {
        return Value::Null;
    };
    let mut result = object.clone();
    result.remove("context");
    Value::Object(result)
}

fn assignment_summary(packet: &Value) -> Value {
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

fn minimal_next(next: &Value) -> Value {
    let mut result = Map::new();
    for key in [
        "action",
        "status",
        "assignment",
        "role",
        "agent",
        "resolvedActor",
        "check",
        "held",
    ] {
        copy_if_present(next, &mut result, key);
    }
    if let Some(held) = next.get("heldResults").and_then(Value::as_array) {
        result.insert("heldResultsCount".into(), json!(held.len()));
        result.insert("heldResultsOmissions".into(), json!(held.len()));
    }
    Value::Object(result)
}

fn compact_obligations(response: &Value) -> Option<Value> {
    let residual = response.get("residual")?;
    let mut value = Map::new();
    for key in [
        "remaining",
        "doNotRepeat",
        "stillValid",
        "alreadyEstablished",
    ] {
        if let Some(item) = residual.get(key) {
            value.insert(key.to_owned(), item.clone());
        }
    }
    Some(Value::Object(value))
}

fn omitted_fields() -> Value {
    json!([
        "residual detail",
        "assignment packet context",
        "duplicated context recovery detail"
    ])
}

fn full_command(response: &Value, config_path: &Path, invoked: &str) -> Value {
    let caller = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_else(|| crate::compatibility::profile().caller.to_owned());
    let Some(config) = config_path.to_str() else {
        return Value::Null;
    };
    if caller.len() + config.len() > MAX_RESPONSE_BYTES - 1024 {
        return Value::Null;
    }
    if invoked == "next" {
        let Some(work) = response.get("work").and_then(Value::as_str) else {
            return Value::Null;
        };
        json!([caller, "work", "next", work, "--full", "--config", config])
    } else {
        json!([caller, "work", "resume", "--full", "--config", config])
    }
}

fn representative_references(references: &[Value]) -> Vec<Value> {
    let mut result = Vec::new();
    for kind in ["ledger_history", "ledger_event", "check_log"] {
        if let Some(reference) = references
            .iter()
            .find(|reference| reference["kind"].as_str() == Some(kind))
        {
            result.push(reference.clone());
        }
    }
    result
}

fn compact_presentation(presentation: &Value) -> Value {
    // Presentation is already a small, state-derived compatibility surface.
    // Keep it intact so terminal and transition meaning cannot change here.
    presentation.clone()
}

fn current_subject(response: &Value) -> Option<Value> {
    response
        .get("currentSubject")
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| {
            response
                .get("residual")
                .and_then(|value| value.get("currentSubject"))
                .cloned()
        })
        .or_else(|| {
            response
                .get("next")
                .and_then(|value| value.get("currentSubject"))
                .cloned()
        })
        .or_else(|| {
            response
                .get("residual")
                .and_then(|value| value.get("context"))
                .and_then(|value| value.get("subject"))
                .cloned()
        })
}

fn copy_if_present(source: &Value, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

fn collect_references(value: &Value, references: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(object) => {
            if object.get("exact") == Some(&Value::Bool(true))
                && object.get("id").and_then(Value::as_str).is_some_and(|_| {
                    matches!(
                        object.get("kind").and_then(Value::as_str),
                        Some("ledger_history" | "ledger_event" | "check_log")
                    )
                })
            {
                let id = object["id"].as_str().expect("checked reference id");
                references
                    .entry(id.to_owned())
                    .or_insert_with(|| value.clone());
            }
            object
                .values()
                .for_each(|child| collect_references(child, references));
        }
        Value::Array(items) => items
            .iter()
            .for_each(|child| collect_references(child, references)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{project, MAX_RESPONSE_BYTES};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn projection_keeps_decision_identity_subject_recorder_and_references() {
        let reference = json!({
            "id": "ref:history",
            "kind": "ledger_history",
            "work": "smw_work",
            "sha256": "a".repeat(64),
            "headEventSha256": "b".repeat(64),
            "eventCount": 2,
            "exact": true,
        });
        let response = json!({
            "status": "resumed",
            "work": "smw_work",
            "next": {
                "action": "spawn",
                "assignment": "sma_assignment",
                "role": "worker",
                "agent": "worker",
                "packet": {"context": {"expansions": [reference.clone()]}}
            },
            "residual": {
                "currentSubject": {"sha256": "c".repeat(64)},
                "context": {"expansions": [reference.clone()]},
                "humanHelp": {
                    "nextAction": {"actor": "worker", "command": ["work", "next"]},
                    "preservationAssignment": {"route": "FORMAL", "quality": "FULL"}
                },
                "large": "x".repeat(60_000)
            },
            "recorder": {"name": "exitbind", "version": "0.25.0", "commit": null},
            "ledgerProducer": {"name": "exitbind", "version": "0.25.0", "commit": null},
        });
        let compact = project(&response, Path::new("/tmp/exitbind.json"), "next").unwrap();
        assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
        assert_eq!(compact["next"]["action"], "spawn");
        assert_eq!(compact["next"]["assignment"], "sma_assignment");
        assert_eq!(compact["nextAction"]["actor"], "worker");
        assert_eq!(
            compact["humanHelp"]["preservationAssignment"]["route"],
            "FORMAL"
        );
        assert_eq!(compact["currentSubject"]["sha256"], "c".repeat(64));
        assert_eq!(compact["recorder"]["name"], "exitbind");
        assert_eq!(compact["references"][0], reference);
        assert_eq!(compact["truncated"], false);
        assert!(compact["omitted"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn oversized_exact_reference_set_keeps_representatives_and_counts_omissions() {
        let references = (0..200)
            .map(|index| {
                json!({
                    "id": format!("ref:{index}"),
                    "kind": "ledger_event",
                    "work": "smw_work",
                    "sha256": "a".repeat(64),
                    "headEventSha256": "b".repeat(64),
                    "eventCount": index,
                    "selector": "c".repeat(64),
                    "exact": true,
                })
            })
            .collect::<Vec<_>>();
        let response = json!({"work": "smw_work", "next": {"action": "check"}, "residual": {"context": {"expansions": references}}});
        let compact = project(&response, Path::new("/tmp/exitbind.json"), "next").unwrap();
        assert_eq!(compact["truncated"], true);
        assert_eq!(compact["referenceOmissions"], 199);
        assert_eq!(compact["references"].as_array().unwrap().len(), 1);
        assert_eq!(compact["references"][0]["kind"], "ledger_event");
        assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
    }

    #[test]
    fn oversized_held_set_names_all_omissions_and_exact_full_route() {
        let held = (0..60)
            .map(|index| json!({"reference": format!("hld_{index}"), "detail": "x".repeat(400)}))
            .collect::<Vec<_>>();
        let response = json!({
            "work": "smw_work",
            "next": {"action": "spawn", "assignment": "sma_assignment", "heldResults": held},
            "residual": {"humanHelp": {"preservationAssignment": {"route": "FORMAL", "quality": "FULL"}}}
        });
        let compact = project(
            &response,
            Path::new("/tmp/other project/exitbind.json"),
            "next",
        )
        .unwrap();
        assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
        assert_eq!(compact["truncated"], true);
        assert_eq!(compact["next"]["heldResultsCount"], 60);
        assert_eq!(compact["next"]["heldResultsOmissions"], 60);
        assert!(compact["next"].get("heldResults").is_none());
        assert_eq!(
            compact["humanHelp"]["preservationAssignment"]["quality"],
            "FULL"
        );
        assert_eq!(
            compact["fullCommand"][6],
            "/tmp/other project/exitbind.json"
        );
    }

    #[test]
    fn resume_recovery_keeps_verb_and_long_config_path() {
        let path = format!("/tmp/{}/exitbind.json", "directory/".repeat(300));
        let response =
            json!({"status": "resumed", "work": "smw_work", "next": {"action": "check"}});
        let compact = project(&response, Path::new(&path), "resume").unwrap();
        assert_eq!(compact["fullCommand"][2], "resume");
        assert_eq!(compact["fullCommand"][5], path);
        assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
    }
}

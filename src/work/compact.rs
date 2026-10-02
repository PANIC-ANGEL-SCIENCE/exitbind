//! Bounded default projection for agent-facing work responses.
//!
//! The full response remains the compatibility surface.  This view keeps the
//! decision and its stable identities, while making omitted detail reachable
//! through the existing `work expand` references.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::response_recovery::{bounded_argv, recovery_command};

pub(crate) const MAX_RESPONSE_BYTES: usize = 8 * 1024;

pub(crate) fn continuation_route(config_path: &Path, mut suffix: Vec<String>) -> Value {
    suffix.push("--config".into());
    let recovery = bounded_argv(suffix, config_path.to_str(), 1024);
    json!({
        "command": recovery.argv,
        "sameConfigRequired": recovery.same_config,
        "sameExecutableRequired": recovery.same_executable,
    })
}

pub(crate) fn continuation_mutation(
    record: &Value,
    work: &str,
    action: &str,
    appended: bool,
    config_path: &Path,
) -> Result<Value, String> {
    let route = continuation_route(
        config_path,
        vec!["work".into(), "continuation".into(), work.into()],
    );
    let result = json!({
        "compact": true,
        "work": work,
        "action": action,
        "goalRevision": record["revision"],
        "effect": if appended { "appended" } else { "unchanged" },
        "eventSha256": record["eventSha256"],
        "nextAction": {"type":"read","safe":true,"command":route["command"],
            "sameConfigRequired":route["sameConfigRequired"],
            "sameExecutableRequired":route["sameExecutableRequired"]},
    });
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        return Err("continuation mutation response exceeds the output budget".into());
    }
    Ok(result)
}

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
    let recovery = recovery_command(response, config_path, invoked, MAX_RESPONSE_BYTES - 2048);

    let mut result = Map::new();
    result.insert("compact".into(), json!(true));
    for key in ["status", "effect", "reason"] {
        copy_if_present(response, &mut result, key);
    }
    copy_if_present(response, &mut result, "work");
    if let Some(current) = response.pointer("/next/current") {
        result.insert("current".into(), current.clone());
    }
    for key in ["selection", "history", "focus", "discovery"] {
        copy_if_present(response, &mut result, key);
    }
    if response["reason"]["code"] == "unreadable_candidate" {
        copy_if_present(response, &mut result, "nextAction");
    }
    for key in ["works", "unreadable"] {
        copy_if_present(response, &mut result, key);
    }
    if let Some(continuation) = response.get("continuation") {
        let bytes = serialized_len(continuation)?;
        if bytes <= 2_500 {
            result.insert("continuation".into(), continuation.clone());
        } else {
            let route = continuation_route(
                config_path,
                vec![
                    "work".into(),
                    "continuation".into(),
                    response["work"].as_str().unwrap_or("").into(),
                ],
            );
            result.insert(
                "continuation".into(),
                json!({
                    "work": continuation["work"],
                    "goalRevision": continuation["goalRevision"],
                    "binding": continuation["binding"],
                    "requiresExpansion": true,
                    "command": route["command"],
                    "sameConfigRequired": route["sameConfigRequired"],
                    "sameExecutableRequired": route["sameExecutableRequired"],
                }),
            );
        }
    }
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
    result.insert("fullCommand".into(), recovery.argv.clone());
    if recovery.same_config {
        result.insert("fullCommandSameConfigRequired".into(), json!(true));
    }
    if recovery.same_executable {
        result.insert("fullCommandSameExecutableRequired".into(), json!(true));
    }

    let mut result = Value::Object(result);
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        // Full preservation constraints are still available through fullCommand.
        // Both aliases must shrink together so neither can defeat the bound.
        if let Some(assignment) = result.pointer_mut("/humanHelp/preservationAssignment") {
            *assignment = preservation_summary(assignment);
        }
        if result
            .pointer("/next/packet/preservationAssignment")
            .is_some()
        {
            result["next"]["packet"] = Value::Null;
            result["next"]["requiresExpansion"] = json!(true);
            result["next"]["constraintsOmitted"] = json!(true);
        }
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
        if let Some(help) = result.get_mut("humanHelp") {
            *help = json!({
                "nextAction": help["nextAction"],
                "ownerDecision": help["ownerDecision"],
                "whatHappened": help["whatHappened"].as_str().map(|value| bounded_text(value, 512)),
                "preservationAssignment": help["preservationAssignment"],
                "preservationEvidence": help["preservationEvidence"],
                "checkInstruction": help["checkInstruction"],
            });
        }
        result
            .as_object_mut()
            .expect("compact object")
            .remove("reason");
        if let Some(remaining) = response["residual"]["remaining"].as_array() {
            result["obligations"] = json!({
                "remaining": remaining.iter().take(8).cloned().collect::<Vec<_>>(),
                "remainingOmissions": remaining.len().saturating_sub(8),
            });
        }
        result["references"] = Value::Array(references);
        result["omitted"] = json!(["large assignment and action detail; use fullCommand"]);
        result["truncated"] = json!(true);
    }
    if serialized_len(&result)? + 1 > MAX_RESPONSE_BYTES {
        let mut emergency = json!({
            "compact": true,
            "current": response["next"]["current"],
            "status": response["status"],
            "work": response["work"],
            "next": {
                "action": response["next"]["action"],
                "assignment": response["next"]["assignment"],
                "held": response["next"]["held"],
            },
            "nextAction": {"command": null, "summary": "Open the full response with fullCommand."},
            "humanHelp": {
                "preservationAssignment": preservation_summary(&response["residual"]["humanHelp"]["preservationAssignment"]),
                "checkInstruction": response["residual"]["humanHelp"]["checkInstruction"],
            },
            "presentation": {
                "terminal": response["presentation"]["terminal"],
                "goalProgress": compact_goal_progress(&response["presentation"]["goalProgress"]),
            },
            "fullCommand": recovery.argv.clone(),
            "fullCommandSameConfigRequired": recovery.same_config,
            "fullCommandSameExecutableRequired": recovery.same_executable,
            "omitted": ["large response detail; use fullCommand"],
            "truncated": true,
        });
        for key in ["works", "unreadable"] {
            copy_if_present(
                response,
                emergency.as_object_mut().expect("compact object"),
                key,
            );
        }
        if let Some(counts) = candidate_counts(response) {
            emergency["candidateCounts"] = counts;
        }
        omit_false_recovery_requirements(&mut emergency);
        if serialized_len(&emergency)? < MAX_RESPONSE_BYTES {
            return Ok(emergency);
        }
        let work = response["work"].as_str().filter(|value| value.len() <= 128);
        let assignment = response["next"]["assignment"]
            .as_str()
            .filter(|value| value.len() <= 128);
        let mut minimal = json!({
            "compact": true,
            "status": "unresolved",
            "current": response["next"]["current"],
            "work": work,
            "next": {"action": "inspect", "assignment": assignment},
            "nextAction": {"command": null, "summary": "Open the full response."},
            "humanHelp": {"preservationAssignment": preservation_summary(&response["residual"]["humanHelp"]["preservationAssignment"])},
            "fullCommand": recovery.argv.clone(),
            "fullCommandSameConfigRequired": recovery.same_config,
            "fullCommandSameExecutableRequired": recovery.same_executable,
            "omitted": ["oversized response detail"],
            "truncated": true,
        });
        if let Some(counts) = candidate_counts(response) {
            minimal["candidateCounts"] = counts;
            minimal["omitted"] = json!(["candidate details; use fullCommand"]);
        }
        omit_false_recovery_requirements(&mut minimal);
        if serialized_len(&minimal)? < MAX_RESPONSE_BYTES {
            return Ok(minimal);
        }
        // A pathological path or identifier must never make the bounded
        // endpoint emit an unbounded response.  Keep an executable argv prefix
        // and require the caller to supply the exact current config value.
        let mut fallback = json!({
            "compact": true,
            "status": "unresolved",
            "current": response["next"]["current"],
            "work": work,
            "next": {"action": "inspect", "assignment": assignment},
            "humanHelp": {"preservationAssignment": preservation_summary(&response["residual"]["humanHelp"]["preservationAssignment"])},
            "fullCommand": recovery.argv,
            "fullCommandSameConfigRequired": recovery.same_config,
            "fullCommandSameExecutableRequired": recovery.same_executable,
            "omitted": ["oversized response detail; use the required current executable and config"],
            "truncated": true,
        });
        if let Some(counts) = candidate_counts(response) {
            fallback["candidateCounts"] = counts;
            fallback["omitted"] =
                json!(["candidate details; use the required current executable and config"]);
        }
        omit_false_recovery_requirements(&mut fallback);
        if serialized_len(&fallback)? + 1 > MAX_RESPONSE_BYTES {
            return Err("bounded recovery response exceeds the output budget".into());
        }
        return Ok(fallback);
    }
    Ok(result)
}

fn omit_false_recovery_requirements(value: &mut Value) {
    for key in [
        "fullCommandSameConfigRequired",
        "fullCommandSameExecutableRequired",
    ] {
        if value[key] == false {
            value.as_object_mut().expect("compact object").remove(key);
        }
    }
}

fn preservation_summary(assignment: &Value) -> Value {
    if !assignment.is_object() {
        return Value::Null;
    }
    json!({
        "route": assignment["route"].as_str().filter(|value| value.len() <= 32),
        "quality": assignment["quality"].as_str().filter(|value| value.len() <= 32),
        "requirementsCount": assignment["requirements"].as_array().map_or(0, Vec::len),
        "requiresExpansion": true,
    })
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
        "stage",
        "attempt",
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

fn candidate_counts(response: &Value) -> Option<Value> {
    if response.get("works").is_none() && response.get("unreadable").is_none() {
        return None;
    }
    Some(json!({
        "works": response.get("works").and_then(Value::as_array).map_or(0, Vec::len),
        "unreadable": response.get("unreadable").and_then(Value::as_array).map_or(0, Vec::len),
    }))
}

fn omitted_fields() -> Value {
    json!([
        "residual detail",
        "assignment packet context",
        "duplicated context recovery detail"
    ])
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
    // Keep terminal and transition meaning, while bounding the additive goal
    // projection by field. Full goal status remains available from
    // `goal status --json`.
    let Some(object) = presentation.as_object() else {
        return Value::Null;
    };
    let mut result = object.clone();
    if let Some(goal) = object.get("goalProgress") {
        result.insert("goalProgress".into(), compact_goal_progress(goal));
    }
    Value::Object(result)
}

pub(crate) fn compact_goal_progress(goal: &Value) -> Value {
    let mut compact = Map::new();
    for key in ["overall", "decomposition", "resultReadiness"] {
        if let Some(value) = goal.get(key) {
            compact.insert(key.into(), value.clone());
        }
    }
    if let Some(value) = goal["goal"].as_str() {
        compact.insert("goal".into(), json!(bounded_text(value, 96)));
    }
    if let Some(tasks) = goal["tasks"].as_array() {
        let omitted = tasks.len().saturating_sub(8);
        let tasks = tasks
            .iter()
            .take(8)
            .map(|task| {
                json!({
                    "id": task["id"].as_str().map(|value| bounded_text(value, 64)),
                    "taskId": task["taskId"],
                    "label": task["label"].as_str().map(|value| bounded_text(value, 64)),
                    "state": task["state"],
                    "disposition": task["disposition"],
                    "performed": task["performed"],
                    "current": task["current"],
                    "currentReason": task["currentReason"],
                })
            })
            .collect::<Vec<_>>();
        compact.insert("tasks".into(), json!(tasks));
        compact.insert(
            "taskOmissions".into(),
            json!(omitted + goal["decomposition"]["omitted"].as_u64().unwrap_or(0) as usize),
        );
    }
    if let Some(value) = goal["systemText"].as_str() {
        if value.chars().count() <= 512 {
            compact.insert("systemText".into(), json!(value));
        } else {
            compact.insert("systemTextOmitted".into(), json!(true));
            compact.insert(
                "systemText".into(),
                json!("Goal status: requires detail | Result readiness: requires detail"),
            );
        }
    }
    Value::Object(compact)
}

fn bounded_text(value: &str, max: usize) -> String {
    let mut text = value.chars().take(max).collect::<String>();
    if text.chars().count() < value.chars().count() {
        text.push('…');
    }
    text
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
#[path = "compact_tests.rs"]
mod tests;

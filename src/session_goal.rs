//! Canonical, Lead-owned evolving session goal state.
//!
//! The JSONL file is an append-only history. The last valid record is the
//! current revision; every incorporation and closure appends a new record.

use serde_json::{json, Value};
use std::path::Path;

mod continuity;
pub(crate) use continuity::{
    claim_child, continuation_bind, continuation_child, continuation_record, continuation_section,
    continuation_view, finalize_child, prepare_child,
};

const FILE: &str = "session-goal.jsonl";
const CATEGORIES: [&str; 5] = [
    "obligations",
    "findings",
    "blockers",
    "decisions",
    "externalActions",
];

fn relative() -> String {
    format!(
        "{}/{}",
        crate::project::layout_types::state_namespace(),
        FILE
    )
}

fn raw_history(root: &Path) -> Result<Option<String>, String> {
    match crate::project::path::secure_bytes_observation(root, &relative(), "session goal") {
        crate::project::path::SecureBytesResult::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "session goal is not valid UTF-8".into()),
        crate::project::path::SecureBytesResult::Absent(_) => {
            let legacy = root.join(format!(
                "{}/session-goal.json",
                crate::project::layout_types::state_namespace()
            ));
            match std::fs::symlink_metadata(legacy) {
                Ok(_) => Err("legacy session goal format is unbound and unsupported".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.to_string()),
            }
        }
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn records(root: &Path) -> Result<Vec<Value>, String> {
    let raw = raw_history(root)?;
    records_from_raw(raw.as_deref())
}

fn records_from_raw(raw: Option<&str>) -> Result<Vec<Value>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut lines = raw.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut parsed = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            return Err(format!(
                "invalid canonical session goal line {}: empty line",
                index + 1
            ));
        }
        let value: Value = serde_json::from_str(line).map_err(|error| {
            format!("invalid canonical session goal line {}: {error}", index + 1)
        })?;
        validate_record(&value, parsed.last(), index + 1)?;
        parsed.push(value);
    }
    Ok(parsed)
}

pub(crate) fn read(root: &Path) -> Result<Option<Value>, String> {
    Ok(records(root)?.pop())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_record(value: &Value, previous: Option<&Value>, line: usize) -> Result<(), String> {
    let object = value.as_object().ok_or_else(|| {
        format!("invalid canonical session goal line {line}: record is not an object")
    })?;
    let hash = object
        .get("eventSha256")
        .and_then(Value::as_str)
        .filter(|hash| valid_hash(hash))
        .ok_or_else(|| {
            format!("invalid canonical session goal line {line}: missing or invalid event hash")
        })?;
    let mut without_hash = value.clone();
    without_hash
        .as_object_mut()
        .expect("validated object")
        .remove("eventSha256");
    if crate::evidence::hash::value(&without_hash) != hash {
        return Err(format!(
            "invalid canonical session goal line {line}: event hash mismatch"
        ));
    }
    let expected_previous_hash = previous
        .map(|record| record["eventSha256"].clone())
        .unwrap_or(Value::Null);
    if object.get("previousEventSha256") != Some(&expected_previous_hash) {
        return Err(format!(
            "invalid canonical session goal line {line}: predecessor hash mismatch"
        ));
    }
    let revision = object
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!("invalid canonical session goal line {line}: revision is missing")
        })?;
    let goal_id = object
        .get("goalId")
        .and_then(Value::as_str)
        .filter(|goal_id| !goal_id.trim().is_empty())
        .ok_or_else(|| {
            format!("invalid canonical session goal line {line}: goal identity is missing")
        })?;
    if let Some(previous) = previous {
        let prior_revision = previous["revision"].as_u64().ok_or_else(|| {
            format!(
                "invalid canonical session goal line {}: prior revision is missing",
                line - 1
            )
        })?;
        if revision != prior_revision.saturating_add(1) {
            return Err(format!(
                "invalid canonical session goal line {line}: revision is not continuous"
            ));
        }
        let expected_predecessor = json!({
            "goalId": previous["goalId"],
            "revision": prior_revision,
        });
        if object.get("predecessor") != Some(&expected_predecessor) {
            return Err(format!(
                "invalid canonical session goal line {line}: predecessor linkage mismatch"
            ));
        }
        let expected_successor = if previous["goalId"].as_str() != Some(goal_id) {
            json!({"goalId": previous["goalId"], "revision": prior_revision})
        } else {
            Value::Null
        };
        if object.get("successorOf") != Some(&expected_successor) {
            return Err(format!(
                "invalid canonical session goal line {line}: successor linkage mismatch"
            ));
        }
    } else if revision != 1
        || object.get("predecessor") != Some(&Value::Null)
        || object.get("successorOf") != Some(&Value::Null)
    {
        return Err(format!(
            "invalid canonical session goal line {line}: initial linkage is invalid"
        ));
    }
    Ok(())
}

fn history_ledger(root: &Path) -> Result<crate::run::ledger::LedgerPath, String> {
    crate::run::ledger::ledger_path(root, &relative(), true)
}

pub(crate) fn sealed(mut value: Value, previous: Option<&Value>) -> Value {
    value["previousEventSha256"] = previous
        .and_then(|record| record["eventSha256"].as_str())
        .map_or(Value::Null, |hash| json!(hash));
    value["eventSha256"] = json!(crate::evidence::hash::value(&value));
    value
}

pub(crate) fn mutate<F>(root: &Path, operation: F) -> Result<Value, String>
where
    F: FnOnce(Option<&Value>) -> Result<Value, String>,
{
    let ledger = history_ledger(root)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = raw_history(root)?;
        let history = records_from_raw(raw.as_deref())?;
        let previous = history.last();
        let record = operation(previous)?;
        if previous == Some(&record) {
            return Ok(record);
        }
        crate::run::ledger::append(
            &ledger,
            &record,
            raw.is_none(),
            raw.as_deref().unwrap_or_default(),
        )?;
        Ok(record)
    })
}

fn text(value: Option<&str>, name: &str) -> Result<String, String> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Err(format!("session goal requires --{name}"));
    }
    Ok(value.to_owned())
}

fn items(previous: Option<&Value>, key: &str) -> Vec<Value> {
    previous
        .and_then(|value| value[key].as_array())
        .cloned()
        .unwrap_or_default()
}

fn category_set(value: Option<&str>, name: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    for category in &values {
        if !CATEGORIES.contains(&category.as_str()) {
            return Err(format!("unknown session-goal category '{category}'"));
        }
    }
    if values.is_empty() {
        return Err(format!("session goal requires --{name}"));
    }
    Ok(values)
}

fn result_refs(value: Option<&str>) -> Result<Vec<Value>, String> {
    match value {
        Some(value) => Ok(vec![json!(text(Some(value), "result-ref")?)]),
        None => Ok(Vec::new()),
    }
}

fn current_inputs(loaded: &crate::config::Loaded) -> Result<String, String> {
    crate::run::inputs::fingerprint(loaded)
}

fn direct_item(
    list: &mut Vec<Value>,
    id: String,
    text_value: String,
    goal_id: &str,
    revision: u64,
    inputs_sha256: &str,
    category: &str,
) -> Result<(), String> {
    if let Some(existing) = list.iter().find(|item| item["id"] == id) {
        if !matches!(
            existing["disposition"].as_str(),
            Some("open") | Some("direct")
        ) {
            return Err(format!(
                "cannot replace governed completion in {category} with direct completion"
            ));
        }
    }
    let item = json!({
        "id": id,
        "text": text_value,
        "disposition": "direct",
        "resultRefs": [],
        "directCompletion": {
            "kind": "lead_reported",
            "goalId": goal_id,
            "revision": revision,
            "inputsSha256": inputs_sha256,
        },
    });
    if let Some(existing) = list.iter_mut().find(|item| item["id"] == id) {
        *existing = item;
    } else {
        list.push(item);
    }
    Ok(())
}

fn direct_external_item(
    list: &mut Vec<Value>,
    action: String,
    scope: &str,
    goal_id: &str,
    revision: u64,
    inputs_sha256: &str,
) -> Result<(), String> {
    if !matches!(scope, "in_scope" | "outside_scope") {
        return Err("--scope must be in_scope or outside_scope".into());
    }
    if let Some(existing) = list.iter().find(|item| item["id"] == action) {
        if !matches!(
            existing["disposition"].as_str(),
            Some("open") | Some("direct")
        ) {
            return Err(
                "cannot replace governed completion in external actions with direct completion"
                    .into(),
            );
        }
    }
    let item = json!({
        "id": action,
        "action": action,
        "scope": scope,
        "disposition": "direct",
        "resultRefs": [],
        "directCompletion": {
            "kind": "lead_reported",
            "goalId": goal_id,
            "revision": revision,
            "inputsSha256": inputs_sha256,
        },
    });
    if let Some(existing) = list.iter_mut().find(|item| item["id"] == action) {
        *existing = item;
    } else {
        list.push(item);
    }
    Ok(())
}

fn update_item(
    list: &mut Vec<Value>,
    id: String,
    text_value: String,
    disposition: Option<&str>,
    refs: Vec<Value>,
    category: &str,
) -> Result<(), String> {
    let disposition = disposition.unwrap_or("open");
    if !matches!(
        disposition,
        "open" | "accepted" | "outside_scope" | "successor"
    ) {
        return Err(format!("invalid disposition for {category}"));
    }
    if disposition != "open" && refs.is_empty() {
        return Err(format!("resolved {category} requires --result-ref"));
    }
    let item = json!({
        "id": id,
        "text": text_value,
        "disposition": disposition,
        "resultRefs": refs,
    });
    if let Some(existing) = list.iter_mut().find(|item| item["id"] == id) {
        *existing = item;
    } else {
        list.push(item);
    }
    Ok(())
}

fn external_item(
    list: &mut Vec<Value>,
    action: String,
    scope: &str,
    disposition: Option<&str>,
    refs: Vec<Value>,
) -> Result<(), String> {
    if !matches!(scope, "in_scope" | "outside_scope") {
        return Err("--scope must be in_scope or outside_scope".into());
    }
    let disposition = disposition.unwrap_or(if scope == "outside_scope" {
        "outside_scope"
    } else {
        "open"
    });
    if !matches!(
        disposition,
        "open" | "accepted" | "outside_scope" | "successor"
    ) {
        return Err("invalid external-action disposition".into());
    }
    if disposition != "open" && refs.is_empty() {
        return Err("resolved external action requires --result-ref".into());
    }
    let item = json!({
        "id": action,
        "action": action,
        "scope": scope,
        "disposition": disposition,
        "resultRefs": refs,
    });
    if let Some(existing) = list.iter_mut().find(|item| item["id"] == action) {
        *existing = item;
    } else {
        list.push(item);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn incorporate(
    loaded: &crate::config::Loaded,
    goal_id: &str,
    goal: &str,
    obligation: Option<&str>,
    finding: Option<&str>,
    blocker: Option<&str>,
    decision: Option<&str>,
    external_action: Option<&str>,
    external_scope: Option<&str>,
    disposition: Option<&str>,
    result_ref: Option<&str>,
    considered: Option<&str>,
    none_applicable: Option<&str>,
) -> Result<Value, String> {
    let goal_id = text(Some(goal_id), "goal-id")?;
    let goal = text(Some(goal), "goal")?;
    let considered = category_set(considered, "consider")?;
    let none_applicable = category_set(none_applicable, "none-applicable")?;
    if considered
        .iter()
        .any(|category| none_applicable.contains(category))
    {
        return Err("a category cannot be both considered and none-applicable".into());
    }
    let refs = result_refs(result_ref)?;
    mutate(&loaded.state_root, |previous| {
        let revision = previous
            .and_then(|value| value["revision"].as_u64())
            .unwrap_or(0)
            .saturating_add(1);
        let mut obligations = items(previous, "obligations");
        let mut findings = items(previous, "findings");
        let mut blockers = items(previous, "blockers");
        let mut decisions = items(previous, "decisions");
        let mut external_actions = items(previous, "externalActions");
        let supplied = [
            ("obligations", obligation.is_some()),
            ("findings", finding.is_some()),
            ("blockers", blocker.is_some()),
            ("decisions", decision.is_some()),
            ("externalActions", external_action.is_some()),
        ];
        if let Some(value) = obligation {
            let value = text(Some(value), "obligation")?;
            update_item(
                &mut obligations,
                value.clone(),
                value,
                disposition,
                refs.clone(),
                "obligations",
            )?;
        }
        if let Some(value) = finding {
            let value = text(Some(value), "finding")?;
            update_item(
                &mut findings,
                value.clone(),
                value,
                disposition,
                refs.clone(),
                "findings",
            )?;
        }
        if let Some(value) = blocker {
            let value = text(Some(value), "blocker")?;
            update_item(
                &mut blockers,
                value.clone(),
                value,
                disposition,
                refs.clone(),
                "blockers",
            )?;
        }
        if let Some(value) = decision {
            let value = text(Some(value), "decision")?;
            update_item(
                &mut decisions,
                value.clone(),
                value,
                disposition,
                refs.clone(),
                "decisions",
            )?;
        }
        if let Some(value) = external_action {
            external_item(
                &mut external_actions,
                text(Some(value), "external-action")?,
                external_scope.unwrap_or("in_scope"),
                disposition,
                refs,
            )?;
        }
        let mut category_state = previous
            .and_then(|value| value["categories"].as_object().cloned())
            .unwrap_or_default();
        for (category, present) in supplied {
            if present || considered.iter().any(|item| item == category) {
                category_state.insert(category.into(), json!("considered"));
            }
        }
        for category in &none_applicable {
            if supplied
                .iter()
                .any(|(name, present)| *name == category && *present)
            {
                return Err(format!(
                    "{category} has facts and cannot be none-applicable"
                ));
            }
            category_state.insert(category.clone(), json!("none_applicable"));
        }
        let predecessor =
            previous.map(|value| json!({"goalId": value["goalId"], "revision": value["revision"]}));
        let successor_of = previous
            .filter(|value| value["goalId"] != goal_id)
            .map(|value| json!({"goalId": value["goalId"], "revision": value["revision"]}));
        Ok(sealed(
            json!({
                "kind": "lead_session_goal",
                "version": if previous.is_some_and(|record| record["goalId"] == goal_id && !record["continuation"].is_null()) { 3 } else { 2 },
                "goalId": goal_id,
                "revision": revision,
                "goal": goal,
                "source": {"owner": "lead", "kind": "explicit_incorporation"},
                "predecessor": predecessor,
                "successorOf": successor_of,
                "obligations": obligations,
                "findings": findings,
                "blockers": blockers,
                "decisions": decisions,
                "externalActions": external_actions,
                "categories": category_state,
                "closure": {"closed": false, "revision": Value::Null, "resultRefs": []},
                "continuation": previous.filter(|record| record["goalId"] == goal_id).map_or(Value::Null, |record| record["continuation"].clone()),
            }),
            previous,
        ))
    })
}

/// Record one explicit Lead-reported direct completion.  This is deliberately
/// separate from governed result references: the claim is bound to the
/// current product inputs and goal revision, but it is not check evidence.
pub(crate) fn direct_complete(
    loaded: &crate::config::Loaded,
    goal_id: &str,
    goal: &str,
    category: &str,
    item: &str,
    external_scope: Option<&str>,
) -> Result<Value, String> {
    let goal_id = text(Some(goal_id), "goal-id")?;
    let goal = text(Some(goal), "goal")?;
    let item = text(Some(item), category)?;
    if !CATEGORIES.contains(&category) {
        return Err(format!("unknown session-goal category '{category}'"));
    }
    mutate(&loaded.state_root, |previous| {
        let revision = previous
            .and_then(|value| value["revision"].as_u64())
            .unwrap_or(0)
            .saturating_add(1);
        let inputs_sha256 = current_inputs(loaded)?;
        let mut obligations = items(previous, "obligations");
        let mut findings = items(previous, "findings");
        let mut blockers = items(previous, "blockers");
        let mut decisions = items(previous, "decisions");
        let mut external_actions = items(previous, "externalActions");
        match category {
            "obligations" => direct_item(
                &mut obligations,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "findings" => direct_item(
                &mut findings,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "blockers" => direct_item(
                &mut blockers,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "decisions" => direct_item(
                &mut decisions,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "externalActions" => direct_external_item(
                &mut external_actions,
                item.clone(),
                external_scope.unwrap_or("in_scope"),
                &goal_id,
                revision,
                &inputs_sha256,
            )?,
            _ => unreachable!(),
        }
        let mut category_state = previous
            .and_then(|value| value["categories"].as_object().cloned())
            .unwrap_or_default();
        category_state.insert(category.into(), json!("considered"));
        let predecessor =
            previous.map(|value| json!({"goalId": value["goalId"], "revision": value["revision"]}));
        let successor_of = previous
            .filter(|value| value["goalId"] != goal_id)
            .map(|value| json!({"goalId": value["goalId"], "revision": value["revision"]}));
        Ok(sealed(
            json!({
                "kind": "lead_session_goal",
                "version": if previous.is_some_and(|record| record["goalId"] == goal_id && !record["continuation"].is_null()) { 3 } else { 2 },
                "goalId": goal_id,
                "revision": revision,
                "goal": goal,
                "source": {"owner": "lead", "kind": "explicit_direct_completion"},
                "predecessor": predecessor,
                "successorOf": successor_of,
                "obligations": obligations,
                "findings": findings,
                "blockers": blockers,
                "decisions": decisions,
                "externalActions": external_actions,
                "categories": category_state,
                "closure": {"closed": false, "revision": Value::Null, "resultRefs": []},
                "continuation": previous.filter(|record| record["goalId"] == goal_id).map_or(Value::Null, |record| record["continuation"].clone()),
            }),
            previous,
        ))
    })
}

fn category_resolved(record: &Value, key: &str) -> bool {
    if !matches!(
        record["categories"][key].as_str(),
        Some("considered") | Some("none_applicable")
    ) {
        return false;
    }
    record[key].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            if item["disposition"].as_str() == Some("direct") {
                item["resultRefs"].as_array().is_some_and(Vec::is_empty)
                    && item["directCompletion"]["kind"] == "lead_reported"
                    && item["directCompletion"]["goalId"].is_string()
                    && item["directCompletion"]["revision"].is_u64()
                    && item["directCompletion"]["inputsSha256"].is_string()
            } else {
                matches!(
                    item["disposition"].as_str(),
                    Some("accepted") | Some("outside_scope") | Some("successor")
                ) && item["resultRefs"]
                    .as_array()
                    .is_some_and(|refs| !refs.is_empty())
            }
        })
    })
}

fn unresolved(record: &Value) -> bool {
    if continuity::has_unresolved(record) {
        return true;
    }
    CATEGORIES
        .iter()
        .any(|category| !category_resolved(record, category))
}

fn direct_completion_inputs(
    loaded: &crate::config::Loaded,
    record: &Value,
    item: &Value,
) -> Result<Option<(String, String)>, String> {
    let completion = &item["directCompletion"];
    if item["disposition"] != "direct"
        || !item["resultRefs"].as_array().is_some_and(Vec::is_empty)
        || completion["kind"] != "lead_reported"
        || completion["goalId"] != record["goalId"]
    {
        return Ok(None);
    }
    let Some(revision) = completion["revision"].as_u64() else {
        return Ok(None);
    };
    let Some(current_revision) = record["revision"].as_u64() else {
        return Ok(None);
    };
    let Some(expected_inputs) = completion["inputsSha256"].as_str() else {
        return Ok(None);
    };
    if revision == 0 || revision > current_revision {
        return Ok(None);
    }
    Ok(Some((expected_inputs.to_owned(), current_inputs(loaded)?)))
}

fn close_result_ref(loaded: &crate::config::Loaded, reference: &str) -> Result<(), String> {
    let Some(evidence) = crate::run::result_ref_evidence(loaded, reference)? else {
        return Err("session goal result reference is not a governed result".into());
    };
    if !evidence.accepted || !evidence.artifact_current {
        return Err("session goal result reference is not a valid accepted result".into());
    }
    warn_result_ref_drift(reference, evidence.drift.as_ref(), evidence.inputs_current);
    Ok(())
}

fn warn_result_ref_drift(reference: &str, drift: Option<&Value>, inputs_current: bool) {
    if let Some(warning) = drift {
        let classification = warning["classification"].as_str().unwrap_or("input_drift");
        eprintln!(
            "warning: {classification} detected for accepted result {reference}; retaining its recorded evidence"
        );
    }
    if !inputs_current {
        eprintln!(
            "warning: tested_inputs_drift detected for accepted result {reference}; retaining recorded tested-input identity"
        );
    }
}

fn warn_direct_completion_drift(
    category: &str,
    recorded_inputs_sha256: &str,
    current_inputs_sha256: &str,
) {
    if recorded_inputs_sha256 != current_inputs_sha256 {
        eprintln!(
            "warning: tested_inputs_drift detected for {category}; retaining its recorded completion"
        );
    }
}

fn accepted_result_ref(loaded: &crate::config::Loaded, reference: &str) -> bool {
    let Ok(Some(evidence)) = crate::run::result_ref_evidence(loaded, reference) else {
        return false;
    };
    if !evidence.accepted || !evidence.artifact_current {
        return false;
    }
    warn_result_ref_drift(reference, evidence.drift.as_ref(), evidence.inputs_current);
    true
}

pub(crate) fn close(
    loaded: &crate::config::Loaded,
    goal_id: &str,
    result_ref: &str,
) -> Result<Value, String> {
    let result_ref = text(Some(result_ref), "result-ref")?;
    mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical session goal is open")?;
        if previous["goalId"].as_str() != Some(goal_id) {
            return Err("session goal identity does not match the current revision".into());
        }
        if unresolved(previous) {
            return Err("session goal has unresolved or unconsidered obligations, findings, blockers, decisions, or external actions".into());
        }
        if !continuity::support_current(loaded, previous)? {
            return Err("continuation support is stale or incomplete".into());
        }
        for category in CATEGORIES {
            if let Some(items) = previous[category].as_array() {
                for item in items {
                    if item["disposition"] == "open" {
                        continue;
                    }
                    if item["disposition"] == "direct" {
                        let Some((recorded, current)) =
                            direct_completion_inputs(loaded, previous, item)?
                        else {
                            return Err(format!("{category} direct completion is invalid"));
                        };
                        warn_direct_completion_drift(category, &recorded, &current);
                        continue;
                    }
                    let Some(refs) = item["resultRefs"].as_array() else {
                        return Err(format!("{category} has malformed result references"));
                    };
                    if refs.is_empty() {
                        return Err(format!("{category} has no governed result evidence"));
                    }
                    for reference in refs {
                        let Some(reference) = reference.as_str() else {
                            return Err(format!("{category} has malformed result references"));
                        };
                        close_result_ref(loaded, reference)?;
                    }
                }
            }
        }
        close_result_ref(loaded, &result_ref)?;
        let revision = previous["revision"]
            .as_u64()
            .ok_or("session goal revision is missing")?;
        let mut record = previous.clone();
        record
            .as_object_mut()
            .expect("validated session goal record")
            .remove("eventSha256");
        record["revision"] = json!(revision + 1);
        record["predecessor"] = json!({"goalId": goal_id, "revision": revision});
        record["successorOf"] = Value::Null;
        record["closure"] = json!({
            "closed": true,
            "revision": revision + 1,
            "resultRefs": [result_ref],
            "owner": "lead",
        });
        Ok(sealed(record, Some(previous)))
    })
}

/// Close a goal from explicit Lead-reported direct facts.  A governed result
/// may be supplied for a mixed goal; when it is absent, no run is required.
pub(crate) fn close_direct(
    loaded: &crate::config::Loaded,
    goal_id: &str,
    result_ref: Option<&str>,
) -> Result<Value, String> {
    let result_ref = result_ref
        .map(|value| text(Some(value), "result-ref"))
        .transpose()?;
    mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical session goal is open")?;
        if previous["goalId"].as_str() != Some(goal_id) {
            return Err("session goal identity does not match the current revision".into());
        }
        if unresolved(previous) {
            return Err("session goal has unresolved or unconsidered obligations, findings, blockers, decisions, or external actions".into());
        }
        if !continuity::support_current(loaded, previous)? {
            return Err("continuation support is stale or incomplete".into());
        }
        let mut has_governed_evidence = false;
        for category in CATEGORIES {
            if let Some(items) = previous[category].as_array() {
                for item in items {
                    if item["disposition"] == "open" {
                        continue;
                    }
                    if item["disposition"] == "direct" {
                        let Some((recorded, current)) =
                            direct_completion_inputs(loaded, previous, item)?
                        else {
                            return Err(format!("{category} direct completion is invalid"));
                        };
                        warn_direct_completion_drift(category, &recorded, &current);
                        continue;
                    }
                    has_governed_evidence = true;
                    let Some(refs) = item["resultRefs"].as_array() else {
                        return Err(format!("{category} has malformed result references"));
                    };
                    if refs.is_empty() {
                        return Err(format!("{category} has no governed result evidence"));
                    }
                    for reference in refs {
                        let Some(reference) = reference.as_str() else {
                            return Err(format!("{category} has malformed result references"));
                        };
                        close_result_ref(loaded, reference)?;
                    }
                }
            }
        }
        if has_governed_evidence && result_ref.is_none() {
            return Err(
                "mixed session goal requires --result-ref for governed closure evidence".into(),
            );
        }
        if let Some(result_ref) = result_ref.as_deref() {
            close_result_ref(loaded, result_ref)?;
        }
        let inputs_sha256 = current_inputs(loaded)?;
        let revision = previous["revision"]
            .as_u64()
            .ok_or("session goal revision is missing")?;
        let mut record = previous.clone();
        record
            .as_object_mut()
            .expect("validated session goal record")
            .remove("eventSha256");
        record["revision"] = json!(revision + 1);
        record["predecessor"] = json!({"goalId": goal_id, "revision": revision});
        record["successorOf"] = Value::Null;
        record["closure"] = json!({
            "closed": true,
            "revision": revision + 1,
            "resultRefs": result_ref.map_or_else(|| json!([]), |value| json!([value])),
            "owner": "lead",
            "kind": "direct",
            "inputsSha256": inputs_sha256,
        });
        Ok(sealed(record, Some(previous)))
    })
}

pub(crate) fn presentation(value: Option<&Value>) -> Value {
    let Some(record) = value else {
        return json!({"requestId":"unknown","explicitLeadClosure":false,"subgoals":["session goal not explicitly incorporated"],"findings":[],"blockers":[],"decisions":[],"externalActions":[]});
    };
    let request_id = format!(
        "{}:r{}",
        record["goalId"].as_str().unwrap_or("unknown"),
        record["revision"].as_u64().unwrap_or(0)
    );
    let closure_has_evidence = if record["closure"]["kind"] == "direct" {
        record["closure"]["inputsSha256"].is_string()
    } else {
        record["closure"]["resultRefs"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty() && refs.iter().all(Value::is_string))
    };
    let closure_current = record["closure"]["closed"] == true
        && record["closure"]["revision"] == record["revision"]
        && !unresolved(record)
        && closure_has_evidence;
    json!({
        "requestId":request_id,
        "explicitLeadClosure":closure_current,
        "completionMode": if record["closure"]["kind"] == "direct" { "direct" } else { "governed" },
        "subgoals":pending_items(record, "obligations", "subgoals"),
        "findings":pending_items(record, "findings", "findings"),
        "blockers":pending_items(record, "blockers", "blockers"),
        "decisions":pending_items(record, "decisions", "decisions"),
        "externalActions":pending_items(record, "externalActions", "external actions"),
        "source":record["source"],
        "revision":record["revision"]
    })
}

fn pending_items(record: &Value, key: &str, label: &str) -> Vec<Value> {
    let unavailable = || vec![json!(format!("session goal {label} unavailable"))];
    let Some(category) = record["categories"][key].as_str() else {
        return unavailable();
    };
    if !matches!(category, "considered" | "none_applicable") {
        return unavailable();
    }
    let Some(items) = record[key].as_array() else {
        return unavailable();
    };
    let mut pending = Vec::new();
    for item in items {
        match item["disposition"].as_str() {
            Some("open") => {
                let value = if key == "externalActions" {
                    item["action"].as_str()
                } else {
                    item["text"].as_str()
                };
                let Some(value) = value else {
                    return unavailable();
                };
                pending.push(json!(value));
            }
            Some("accepted" | "outside_scope" | "successor" | "direct") => {}
            _ => return unavailable(),
        }
    }
    pending
}

fn governed_refs_current(loaded: &crate::config::Loaded, record: &Value) -> Result<bool, String> {
    let Some(closure_refs) = record["closure"]["resultRefs"].as_array() else {
        return Ok(false);
    };
    if record["closure"]["kind"] == "direct" {
        let Some(recorded_inputs) = record["closure"]["inputsSha256"].as_str() else {
            return Ok(false);
        };
        let current_inputs = current_inputs(loaded)?;
        if recorded_inputs != current_inputs {
            eprintln!(
                "warning: tested_inputs_drift detected for direct session-goal closure; retaining its recorded closure"
            );
        }
    } else if closure_refs.is_empty() {
        return Ok(false);
    }
    for reference in closure_refs {
        let Some(reference) = reference.as_str() else {
            return Ok(false);
        };
        if !accepted_result_ref(loaded, reference) {
            return Ok(false);
        }
    }
    for category in CATEGORIES {
        let Some(items) = record[category].as_array() else {
            return Ok(false);
        };
        for item in items {
            if item["disposition"].as_str() == Some("open") {
                continue;
            }
            if item["disposition"].as_str() == Some("direct") {
                let Some((recorded, current)) = direct_completion_inputs(loaded, record, item)?
                else {
                    return Ok(false);
                };
                warn_direct_completion_drift("direct session-goal item", &recorded, &current);
                continue;
            }
            let Some(refs) = item["resultRefs"].as_array() else {
                return Ok(false);
            };
            if refs.is_empty() {
                return Ok(false);
            }
            for reference in refs {
                let Some(reference) = reference.as_str() else {
                    return Ok(false);
                };
                if !accepted_result_ref(loaded, reference) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

pub(crate) fn presentation_for_loaded(
    loaded: &crate::config::Loaded,
    value: Option<&Value>,
) -> Result<Value, String> {
    let mut rendered = presentation(value);
    if rendered["explicitLeadClosure"] == true {
        let Some(record) = value else {
            rendered["explicitLeadClosure"] = Value::Bool(false);
            return Ok(rendered);
        };
        if !governed_refs_current(loaded, record)? || !continuity::support_current(loaded, record)?
        {
            rendered["explicitLeadClosure"] = Value::Bool(false);
        }
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_does_not_close_from_ready() {
        let value = json!({"goalId":"a","revision":1,"categories":{"obligations":"none_applicable","findings":"none_applicable","blockers":"none_applicable","decisions":"none_applicable","externalActions":"none_applicable"},"obligations":[],"findings":[],"blockers":[],"decisions":[],"externalActions":[],"closure":{"closed":false,"revision":null,"resultRefs":[]}});
        assert_eq!(presentation(Some(&value))["explicitLeadClosure"], false);
    }
}

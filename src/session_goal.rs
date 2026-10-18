//! Canonical, Lead-owned evolving session goal state.
//!

use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) mod association;
mod closure;
mod continuity;
pub(crate) mod journey;
pub(crate) mod native;
pub(crate) mod quarantine;
pub(crate) mod recovery;
pub(crate) mod requirements;
pub(crate) use closure::{
    close, close_direct, direct_complete, validate_direct_candidate, validate_governed_candidate,
    DirectCompletionRequest,
};
mod current;
mod presentation;
mod progress;
pub(crate) use presentation::{
    context_summary, presentation, presentation_for_loaded, presentation_for_selected,
    progress_detail_for_work, progress_for_loaded, progress_for_work,
    project_goal_progress_for_work, record_for_work, terminal_for_work,
};
pub(crate) mod usage;
pub(crate) use continuity::{
    child_context, claim_child, claimed_child_context, continuation_bind, continuation_child,
    continuation_record, continuation_section, continuation_view, finalize_child, prepare_child,
    prepared_children_for_work, recover_child,
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

/// Bounded, read-only Goal metadata acquisition for host continuation hooks.
/// This validates the complete retained hash chain but deliberately does not
/// reduce currentness, usage, evidence, or acceptance.
pub(crate) fn read_for_recovery(root: &Path) -> Result<Option<Value>, String> {
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    const MAX_EVENTS: usize = 256;
    const BUDGET: Duration = Duration::from_millis(750);
    let started = Instant::now();
    let relative = relative();
    let raw = match crate::project::path::secure_bytes_observation_bounded(
        root,
        &relative,
        "native recovery Goal journal",
        MAX_BYTES,
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) => Some(
            String::from_utf8(bytes)
                .map_err(|_| "native recovery Goal journal is not valid UTF-8")?,
        ),
        crate::project::path::SecureBytesResult::Absent(_) => {
            let legacy = root.join(format!(
                "{}/session-goal.json",
                crate::project::layout_types::state_namespace()
            ));
            match std::fs::symlink_metadata(legacy) {
                Ok(_) => {
                    return Err("legacy session Goal format is unsupported for recovery".into())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("native recovery Goal lookup failed: {error}")),
            }
        }
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => {
            return Err(format!("native recovery Goal lookup failed: {error}"))
        }
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => {
            return Err(format!("native recovery Goal lookup unsupported: {error}"))
        }
    };
    let Some(raw) = raw else {
        return Ok(None);
    };
    // Count physical records before allocating per-line storage. Blank lines
    // are corruption (including interior blank lines); one final newline is
    // merely the normal ledger terminator.
    if raw.is_empty() {
        return Err("invalid canonical session goal journal: empty physical record".into());
    }
    let newline_count = raw.as_bytes().iter().filter(|byte| **byte == b'\n').count();
    let physical = newline_count.saturating_sub(usize::from(raw.ends_with('\n')))
        + usize::from(!raw.ends_with('\n'));
    if physical > MAX_EVENTS {
        return Err("native recovery Goal journal exceeds 256 physical records; use the exact Goal read route".into());
    }
    let mut previous: Option<Value> = None;
    let mut count = 0usize;
    for (index, line) in raw.split_terminator('\n').enumerate() {
        if started.elapsed() > BUDGET {
            return Err("native recovery Goal read exceeded its 750 ms cooperative budget".into());
        }
        if line.trim().is_empty() {
            return Err(format!(
                "invalid canonical session goal line {}: empty physical record",
                index + 1
            ));
        }
        count += 1;
        if count > MAX_EVENTS {
            return Err("native recovery Goal journal exceeds 256 physical records; use the exact Goal read route".into());
        }
        let value: Value = serde_json::from_str(line).map_err(|error| {
            format!("invalid canonical session goal line {}: {error}", index + 1)
        })?;
        validate_record(&value, previous.as_ref(), index + 1)?;
        previous = Some(value);
        if started.elapsed() > BUDGET {
            return Err("native recovery Goal read exceeded its 750 ms cooperative budget".into());
        }
    }
    if raw.ends_with("\n\n") {
        return Err(
            "invalid canonical session goal journal: trailing blank physical record".into(),
        );
    }
    if started.elapsed() > BUDGET {
        return Err("native recovery Goal read exceeded its 750 ms cooperative budget".into());
    }
    Ok(previous)
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
    association::validate_record(value, line)?;
    journey::validate_record(value, line)?;
    recovery::validate_record(value, line)?;
    quarantine::validate_record(value, line)?;
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
    if let Some(previous) = previous {
        association::preserve_same_goal(previous, &mut value);
        journey::preserve_same_goal(previous, &mut value);
        recovery::preserve_same_goal(previous, &mut value);
        quarantine::preserve_same_goal(previous, &mut value);
    }
    value["previousEventSha256"] = previous
        .and_then(|record| record["eventSha256"].as_str())
        .map_or(Value::Null, |hash| json!(hash));
    value["eventSha256"] = json!(crate::evidence::hash::value(&value));
    value
}

/// Append a native Goal record while allowing a recovery request to locate an
/// earlier exact replay under the same journal lock. The ordinary mutation
/// path deliberately keeps its latest-record-only contract.
pub(crate) fn mutate_history<F>(root: &Path, operation: F) -> Result<Value, String>
where
    F: FnOnce(&[Value]) -> Result<Value, String>,
{
    let ledger = history_ledger(root)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = raw_history(root)?;
        let history = records_from_raw(raw.as_deref())?;
        let record = operation(&history)?;
        if history.iter().any(|saved| saved == &record) {
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

pub(crate) fn with_history<T, F>(root: &Path, operation: F) -> Result<T, String>
where
    F: FnOnce(&[Value]) -> Result<T, String>,
{
    let ledger = history_ledger(root)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = raw_history(root)?;
        let history = records_from_raw(raw.as_deref())?;
        operation(&history)
    })
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
    root: &Path,
    _loaded: &crate::config::Loaded,
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
    native_session: bool,
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
    mutate(root, |previous| {
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
        let mut next = json!({
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
        });
        journey::transition(previous, &mut next, native_session)?;
        Ok(sealed(next, previous))
    })
}

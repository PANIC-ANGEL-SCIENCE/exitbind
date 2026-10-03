//! Secure, bounded prospective usage ingestion.

use super::{
    LIFETIMES, MAX_JOURNAL_BYTES, MAX_USAGE_EVENTS, PHASES, SCOPES, SEMANTICS, SOURCES, STATUSES,
};
use serde_json::{json, Value};

/// Record a bounded host/native observation for an existing Work.  This is a
/// private observation ledger beside the native journals; it never changes
/// the canonical goal revision, Work packet, grant state, or check evidence.
/// The CLI owns argument parsing and calls this API after explicit admission.
pub(crate) fn record_numeric_event(
    loaded: &crate::config::Loaded,
    work: &str,
    event: &Value,
) -> Result<Value, String> {
    if !crate::work::valid_work_handle(work) {
        return Err("usage observation requires a valid Work identity".into());
    }
    let _ = crate::work::resolve(loaded, work)?;
    validate_optional_goal(loaded, event)?;
    let normalized = normalize_event(work, event)?;
    let ledger = usage_ledger(loaded, work)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = super::read_usage_raw(loaded, work)?;
        let existing = super::parse_usage_records(raw.as_deref(), work)?;
        if let Some(previous) = existing
            .iter()
            .find(|value| value["id"] == normalized["id"])
        {
            if previous == &normalized {
                return Ok(json!({"status":"replayed", "id":normalized["id"], "work":work}));
            }
            return Err("usage observation identity conflicts with an existing record".into());
        }
        if existing.len() >= MAX_USAGE_EVENTS {
            return Err("usage observation limit reached; no observation recorded".into());
        }
        validate_monotonic(&existing, &normalized)?;
        let bytes = serde_json::to_vec(&normalized).map_err(|error| error.to_string())?;
        let current_bytes = raw.as_ref().map_or(0, String::len);
        if current_bytes.saturating_add(bytes.len()).saturating_add(1) > MAX_JOURNAL_BYTES as usize
        {
            return Err("usage observation ledger exceeds its byte bound".into());
        }
        crate::run::ledger::append(
            &ledger,
            &normalized,
            raw.is_none(),
            raw.as_deref().unwrap_or_default(),
        )?;
        Ok(json!({"status":"recorded", "id":normalized["id"], "work":work}))
    })
}

/// Record a goal-owned observation whose Work association is optional.  The
/// canonical goal identity is checked before the observation ledger is
/// touched; this keeps setup/root/direct events available before child Work
/// exists without changing the semantic goal revision.
pub(crate) fn record_goal_numeric_event(
    loaded: &crate::config::Loaded,
    event: &Value,
) -> Result<Value, String> {
    let goal_id = bounded_id(event.get("goalId"), "goalId")?
        .ok_or("goal usage observation requires goalId")?;
    validate_goal(loaded, &goal_id)?;
    match event.get("work") {
        None | Some(Value::Null) => {}
        Some(Value::String(work)) if crate::work::valid_work_handle(work) => {
            let _ = crate::work::resolve(loaded, work)?;
        }
        Some(Value::String(_)) => return Err("usage observation Work identity is invalid".into()),
        Some(_) => return Err("usage observation work is invalid".into()),
    }
    let normalized = normalize_event("__goal__", event)?;
    let ledger = goal_usage_ledger(loaded)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = super::read_goal_usage_raw(loaded)?;
        let existing = parse_goal_records(raw.as_deref())?;
        if let Some(previous) = existing
            .iter()
            .find(|value| value["id"] == normalized["id"])
        {
            if previous == &normalized {
                return Ok(json!({"status":"replayed", "id":normalized["id"], "goalId":goal_id}));
            }
            return Err("goal usage observation identity conflicts with an existing record".into());
        }
        if existing.len() >= MAX_USAGE_EVENTS {
            return Err("goal usage observation limit reached; no observation recorded".into());
        }
        validate_monotonic(&existing, &normalized)?;
        let bytes = serde_json::to_vec(&normalized).map_err(|error| error.to_string())?;
        let current_bytes = raw.as_ref().map_or(0, String::len);
        if current_bytes.saturating_add(bytes.len()).saturating_add(1) > MAX_JOURNAL_BYTES as usize
        {
            return Err("goal usage observation ledger exceeds its byte bound".into());
        }
        crate::run::ledger::append(
            &ledger,
            &normalized,
            raw.is_none(),
            raw.as_deref().unwrap_or_default(),
        )?;
        Ok(json!({"status":"recorded", "id":normalized["id"], "goalId":goal_id}))
    })
}

pub(crate) fn goal_records(
    loaded: &crate::config::Loaded,
    goal_id: &str,
) -> Result<Vec<Value>, String> {
    let goal = crate::session_goal::read(&loaded.state_root)?
        .ok_or("goal usage observation requires an existing canonical goal")?;
    if goal["goalId"].as_str() != Some(goal_id) {
        return Err("goal usage observation is not bound to the current canonical goal".into());
    }
    let raw = super::read_goal_usage_raw(loaded)?;
    let records = parse_goal_records(raw.as_deref())?;
    let mut selected = Vec::new();
    for record in records {
        if record["goalId"].as_str() != Some(goal_id) {
            continue;
        }
        if let Some(work) = record["work"].as_str() {
            if !crate::work::valid_work_handle(work) {
                return Err("goal usage observation has an invalid Work identity".into());
            }
            let _ = crate::work::resolve(loaded, work)?;
        }
        selected.push(record);
    }
    Ok(selected)
}

fn goal_usage_ledger(
    loaded: &crate::config::Loaded,
) -> Result<crate::run::ledger::LedgerPath, String> {
    let directory = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("native-actions");
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, &directory)?;
    crate::run::ledger::ledger_path(
        &loaded.state_root,
        &format!(
            "{}/native-actions/usage.jsonl",
            crate::project::layout_types::state_namespace()
        ),
        true,
    )
}

fn validate_goal(loaded: &crate::config::Loaded, goal_id: &str) -> Result<(), String> {
    let goal = crate::session_goal::read(&loaded.state_root)?
        .ok_or("goal usage observation requires an existing canonical goal")?;
    if goal["goalId"].as_str() != Some(goal_id) {
        return Err("goal usage observation is not bound to the current canonical goal".into());
    }
    Ok(())
}

fn validate_optional_goal(loaded: &crate::config::Loaded, event: &Value) -> Result<(), String> {
    let Some(goal_id) = bounded_id(event.get("goalId"), "goalId")? else {
        return Ok(());
    };
    validate_goal(loaded, &goal_id)
}

fn parse_goal_records(raw: Option<&str>) -> Result<Vec<Value>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() || records.len() >= MAX_USAGE_EVENTS {
            return Err("goal usage observation ledger is corrupt or over its event bound".into());
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| "goal usage observation ledger is corrupt".to_owned())?;
        if !value["goalId"]
            .as_str()
            .is_some_and(|goal_id| !goal_id.is_empty() && goal_id.len() <= 128)
        {
            return Err("goal usage observation has no bounded goal identity".into());
        }
        if let Some(work) = value["work"].as_str() {
            if !crate::work::valid_work_handle(work) {
                return Err("goal usage observation has an invalid Work identity".into());
            }
        } else if !value["work"].is_null() {
            return Err("goal usage observation has an invalid Work association".into());
        }
        let normalized = normalize_event("__goal__", &value)?;
        if normalized != value {
            return Err("goal usage observation ledger has noncanonical fields".into());
        }
        records.push(normalized);
    }
    Ok(records)
}

fn usage_ledger(
    loaded: &crate::config::Loaded,
    work: &str,
) -> Result<crate::run::ledger::LedgerPath, String> {
    let directory = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("native-actions")
        .join(work);
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, &directory)?;
    crate::run::ledger::ledger_path(&loaded.state_root, &super::usage_relative(work), true)
}

fn bounded_id(value: Option<&Value>, name: &str) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() && value.len() <= 128 => {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(format!("usage observation {name} is invalid")),
    }
}

pub(super) fn normalize_event(work: &str, event: &Value) -> Result<Value, String> {
    let object = event
        .as_object()
        .ok_or("usage observation must be an object")?;
    const ALLOWED_FIELDS: [&str; 23] = [
        "work",
        "id",
        "source",
        "scope",
        "phase",
        "status",
        "semantics",
        "lifetime",
        "goalId",
        "taskId",
        "role",
        "assignment",
        "attempt",
        "invocation",
        "turn",
        "sessionMode",
        "adapterVersion",
        "counterId",
        "reset",
        "parentEventId",
        "overlapGroup",
        "values",
        "version",
    ];
    if object
        .keys()
        .any(|key| !ALLOWED_FIELDS.contains(&key.as_str()))
    {
        return Err("usage observation contains an unsupported field".into());
    }
    if let Some(version) = object.get("version") {
        if version.as_u64() != Some(1) {
            return Err("usage observation version is unsupported".into());
        }
    }
    let associated_work = if work == "__goal__" {
        match object.get("work") {
            None | Some(Value::Null) => Value::Null,
            Some(Value::String(value)) if !value.is_empty() && value.len() <= 128 => {
                Value::String(value.clone())
            }
            Some(_) => return Err("usage observation work is invalid".into()),
        }
    } else {
        if let Some(value) = object.get("work") {
            if value.as_str() != Some(work) {
                return Err("usage observation Work identity does not match".into());
            }
        }
        Value::String(work.to_owned())
    };
    let id = bounded_id(object.get("id"), "id")?.ok_or("usage observation requires id")?;
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| SOURCES.contains(value))
        .ok_or("usage observation source is unsupported")?;
    let scope = object
        .get("scope")
        .and_then(Value::as_str)
        .filter(|value| SCOPES.contains(value))
        .ok_or("usage observation scope is unsupported")?;
    let phase = object
        .get("phase")
        .and_then(Value::as_str)
        .filter(|value| PHASES.contains(value))
        .ok_or("usage observation phase is unsupported")?;
    let status = object.get("status").map_or(Ok("observed"), |value| {
        value
            .as_str()
            .filter(|value| STATUSES.contains(value))
            .ok_or("usage observation status is unsupported")
    })?;
    let semantics = object
        .get("semantics")
        .and_then(Value::as_str)
        .filter(|value| SEMANTICS.contains(value))
        .ok_or("usage observation semantics is unsupported")?;
    let lifetime = object
        .get("lifetime")
        .and_then(Value::as_str)
        .filter(|value| LIFETIMES.contains(value))
        .ok_or("usage observation lifetime is unsupported")?;
    let counter_id = bounded_id(object.get("counterId"), "counterId")?;
    if semantics == "cumulative" && counter_id.is_none() {
        return Err("cumulative usage observation requires counterId".into());
    }
    let values = match object.get("values") {
        None | Some(Value::Null) => None,
        Some(Value::Object(values)) => Some(values),
        Some(_) => return Err("usage observation values must be an object".into()),
    };
    if values.is_some_and(|values| {
        values.keys().any(|key| {
            !matches!(
                key.as_str(),
                "inputTokens" | "cachedInputTokens" | "outputTokens"
            )
        })
    }) {
        return Err("usage observation contains an unsupported numeric field".into());
    }
    let number = |name: &str| -> Result<Option<u64>, String> {
        match values.and_then(|values| values.get(name)) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(value)) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("usage observation {name} is outside u64")),
            Some(_) => Err(format!("usage observation {name} is not numeric")),
        }
    };
    let input = number("inputTokens")?;
    let cached = number("cachedInputTokens")?;
    let output = number("outputTokens")?;
    let reset = object.get("reset").map_or(Ok(false), |value| {
        value.as_bool().ok_or("usage observation reset is invalid")
    })?;
    let optional = |name: &str| bounded_id(object.get(name), name);
    let attempt = match object.get("attempt") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::Number(value)) => json!(value
            .as_u64()
            .ok_or("usage observation attempt is invalid")?),
        Some(_) => return Err("usage observation attempt is invalid".into()),
    };
    Ok(json!({
        "version": 1,
        "work": associated_work,
        "id": id,
        "source": source,
        "scope": scope,
        "phase": phase,
        "status": status,
        "semantics": semantics,
        "lifetime": lifetime,
        "goalId": optional("goalId")?,
        "taskId": optional("taskId")?,
        "role": optional("role")?,
        "assignment": optional("assignment")?,
        "attempt": attempt,
        "invocation": optional("invocation")?,
        "turn": optional("turn")?,
        "sessionMode": optional("sessionMode")?,
        "adapterVersion": optional("adapterVersion")?,
        "counterId": counter_id,
        "reset": reset,
        "parentEventId": optional("parentEventId")?,
        "overlapGroup": optional("overlapGroup")?,
        "values": {
            "inputTokens": input,
            "cachedInputTokens": cached,
            "outputTokens": output,
        },
    }))
}

fn validate_monotonic(existing: &[Value], current: &Value) -> Result<(), String> {
    if current["semantics"] != "cumulative" {
        return Ok(());
    }
    let Some(counter_id) = current["counterId"].as_str() else {
        return Err("cumulative usage observation requires counterId".into());
    };
    let prior = existing.iter().rev().find(|value| {
        value["semantics"] == "cumulative"
            && value["counterId"].as_str() == Some(counter_id)
            && value["source"] == current["source"]
            && value["goalId"] == current["goalId"]
            && value["work"] == current["work"]
    });
    let Some(prior) = prior else { return Ok(()) };
    let fields = ["inputTokens", "cachedInputTokens", "outputTokens"];
    let decreased = fields.iter().any(|field| {
        current["values"][*field]
            .as_u64()
            .zip(prior["values"][*field].as_u64())
            .is_some_and(|(now, before)| now < before)
    });
    if decreased && current["reset"] != true {
        return Err("cumulative usage counter decreased without reset".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_goal::usage::numeric_event;
    use std::collections::BTreeSet;

    fn event() -> Value {
        json!({
            "id": "host-1",
            "source": "host_reported",
            "scope": "direct",
            "phase": "closeout",
            "status": "observed",
            "semantics": "delta",
            "lifetime": "invocation",
            "goalId": "goal-1",
            "taskId": "task-1",
            "role": "lead",
            "assignment": "assignment-1",
            "attempt": 2,
            "invocation": "invocation-1",
            "turn": "turn-1",
            "sessionMode": "persistent",
            "adapterVersion": "codex-1",
            "values": {"inputTokens": 0, "cachedInputTokens": 0, "outputTokens": 3},
        })
    }

    #[test]
    fn normalizes_zero_and_allowlisted_associations_without_text() {
        let normalized = normalize_event("smw_123", &event()).unwrap();
        assert_eq!(normalized["work"], "smw_123");
        assert_eq!(normalized["values"]["inputTokens"], 0);
        assert_eq!(normalized["values"]["outputTokens"], 3);
        assert_eq!(normalized["taskId"], "task-1");
        assert!(normalized.get("summary").is_none());
    }

    #[test]
    fn cumulative_decrease_requires_explicit_reset() {
        let mut first = event();
        first["id"] = json!("counter-1");
        first["semantics"] = json!("cumulative");
        first["counterId"] = json!("provider-counter");
        first["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
        let first = normalize_event("smw_123", &first).unwrap();
        let mut second = first.clone();
        second["id"] = json!("counter-2");
        second["values"]["inputTokens"] = json!(3);
        assert!(validate_monotonic(&[first], &second).is_err());
        second["reset"] = json!(true);
        let prior = normalize_event("smw_123", &event()).unwrap();
        let mut prior = prior;
        prior["semantics"] = json!("cumulative");
        prior["counterId"] = json!("provider-counter");
        prior["values"] = json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
        assert!(validate_monotonic(&[prior], &second).is_ok());
    }

    #[test]
    fn parent_and_native_snapshots_never_become_additive_values() {
        let mut parent = event();
        parent["id"] = json!("parent");
        let mut child = event();
        child["id"] = json!("child");
        child["parentEventId"] = json!("parent");
        let mut ids = BTreeSet::new();
        let mut overlaps = BTreeSet::new();
        let mut turns = BTreeSet::new();
        let mut cumulative = std::collections::BTreeMap::new();
        let (_, parent_values) = numeric_event(
            &normalize_event("smw_123", &parent).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut cumulative,
        )
        .unwrap();
        let (_, child_values) = numeric_event(
            &normalize_event("smw_123", &child).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut cumulative,
        )
        .unwrap();
        assert!(parent_values.is_some());
        assert!(child_values.is_none());

        let mut native = event();
        native["id"] = json!("native");
        native["source"] = json!("native_observation");
        let (_, native_values) = numeric_event(
            &normalize_event("smw_123", &native).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut cumulative,
        )
        .unwrap();
        assert!(native_values.is_none());
    }

    #[test]
    fn failed_execution_with_known_delta_remains_additive() {
        let mut failed = event();
        failed["id"] = json!("failed");
        failed["status"] = json!("failed");
        let mut ids = BTreeSet::new();
        let mut overlaps = BTreeSet::new();
        let mut turns = BTreeSet::new();
        let mut cumulative = std::collections::BTreeMap::new();
        let (_, values) = numeric_event(
            &normalize_event("smw_123", &failed).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut cumulative,
        )
        .unwrap();
        assert_eq!(values, Some([0, 0, 3]));

        let mut missing = failed;
        missing["id"] = json!("failed-missing");
        missing["values"] = Value::Null;
        let (_, values) = numeric_event(
            &normalize_event("smw_123", &missing).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut cumulative,
        )
        .unwrap();
        assert!(values.is_none());
    }

    #[test]
    fn cumulative_and_per_turn_contracts_derive_only_known_deltas() {
        let mut ids = BTreeSet::new();
        let mut overlaps = BTreeSet::new();
        let mut turns = BTreeSet::new();
        let mut counters = std::collections::BTreeMap::new();

        let mut cumulative = event();
        cumulative["semantics"] = json!("cumulative");
        cumulative["lifetime"] = json!("session");
        cumulative["counterId"] = json!("counter-1");
        cumulative["values"] =
            json!({"inputTokens": 10, "cachedInputTokens": 2, "outputTokens": 4});
        let first = numeric_event(
            &normalize_event("smw_123", &cumulative).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut counters,
        )
        .unwrap();
        assert_eq!(first.1, None);

        cumulative["id"] = json!("counter-2");
        cumulative["values"] =
            json!({"inputTokens": 12, "cachedInputTokens": 3, "outputTokens": 5});
        let second = numeric_event(
            &normalize_event("smw_123", &cumulative).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut counters,
        )
        .unwrap();
        assert_eq!(second.1, Some([2, 1, 1]));

        let mut per_turn = event();
        per_turn["id"] = json!("turn-1");
        per_turn["semantics"] = json!("per_turn");
        per_turn["lifetime"] = json!("session");
        let (_, values) = numeric_event(
            &normalize_event("smw_123", &per_turn).unwrap(),
            &mut ids,
            &mut overlaps,
            &mut turns,
            &mut counters,
        )
        .unwrap();
        assert_eq!(values, Some([0, 0, 3]));
    }

    #[test]
    fn goal_event_keeps_optional_work_link_and_rejects_unbounded_fields() {
        let mut goal = event();
        goal["work"] = json!("smw_123");
        let normalized = normalize_event("__goal__", &goal).unwrap();
        assert_eq!(normalized["work"], "smw_123");
        assert_eq!(normalized["goalId"], "goal-1");

        goal["rawText"] = json!("must never be persisted");
        assert!(normalize_event("__goal__", &goal).is_err());
    }

    #[test]
    fn goal_records_are_partitionable_by_canonical_goal_identity() {
        let first = normalize_event("__goal__", &event()).unwrap();
        let mut second = first.clone();
        second["goalId"] = json!("goal-2");
        let raw = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );
        let records = parse_goal_records(Some(&raw)).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["goalId"], "goal-1");
        assert_eq!(records[1]["goalId"], "goal-2");
    }
}

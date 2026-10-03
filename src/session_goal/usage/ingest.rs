//! Secure, bounded prospective usage ingestion.

use super::{
    LIFETIMES, MAX_JOURNAL_BYTES, MAX_USAGE_EVENTS, PHASES, SCOPES, SEMANTICS, SOURCES, STATUSES,
};
use serde_json::{json, Value};

const NATIVE_GAPS: [&str; 9] = [
    "missing_thread_id",
    "missing_turn_completion",
    "missing_usage",
    "missing_command_outcome",
    "missing_final_result",
    "prompt_delivery_failed",
    "missing_command_exit_code",
    "missing_command_identity",
    "unknown_gap",
];
const NATIVE_LABELS: [&str; 6] = [
    "unobserved",
    "not_counted",
    "unknown",
    "observed",
    "observed_or_missing",
    "replayed",
];

pub(super) fn bounded_native_account(account: &Value) -> Value {
    let mut gaps: Vec<Value> = Vec::new();
    if let Some(items) = account["coverage"]["gaps"].as_array() {
        for item in items.iter().take(super::MAX_GAPS) {
            let value = item
                .as_str()
                .filter(|value| NATIVE_GAPS.contains(value))
                .unwrap_or("unknown_gap");
            if !gaps.iter().any(|gap| gap.as_str() == Some(value)) {
                gaps.push(json!(value));
            }
        }
    }
    let input = account["usage"]["inputTokens"].as_u64();
    let cached = account["usage"]["cachedInputTokens"].as_u64();
    let output = account["usage"]["outputTokens"].as_u64();
    let valid_usage = cached
        .zip(input)
        .map_or(true, |(cached, input)| cached <= input)
        && input
            .zip(output)
            .map_or(true, |(input, output)| input.checked_add(output).is_some());
    let base_status = account["status"]
        .as_str()
        .filter(|value| STATUSES.contains(value))
        .unwrap_or("unknown");
    let turn = account["coverage"]["turn"]
        .as_str()
        .filter(|value| matches!(*value, "completed" | "failed" | "interrupted" | "unknown"));
    let coverage_present = account["coverage"]["gaps"].is_array()
        && turn.is_some_and(|value| value != "unknown")
        && account["coverage"]["unobservedItems"].as_u64() == Some(0);
    let status = if base_status == "observed"
        && (!(valid_usage && input.is_some() && cached.is_some() && output.is_some())
            || !coverage_present
            || !gaps.is_empty())
    {
        "missing"
    } else {
        base_status
    };
    let source = account["source"]
        .as_str()
        .filter(|value| {
            matches!(
                *value,
                "turn.completed" | "native_observation" | "unavailable"
            )
        })
        .unwrap_or("unavailable");
    let scope = account["scope"]
        .as_str()
        .filter(|value| matches!(*value, "native_turn" | "native"))
        .unwrap_or("native_turn");
    let counter_semantics = account["counterSemantics"]
        .as_str()
        .filter(|value| *value == "provider_turn_snapshot")
        .unwrap_or("unknown");
    let label = |name: &str| {
        account["coverage"][name]
            .as_str()
            .filter(|value| NATIVE_LABELS.contains(value))
            .map_or(Value::Null, |value| json!(value))
    };
    json!({
        "status": status,
        "scope": scope,
        "source": source,
        "counterSemantics": counter_semantics,
        "additive": false,
        "usage": {
            "inputTokens": input,
            "cachedInputTokens": cached,
            "outputTokens": output,
        },
        "coverage": {
            "turn": turn,
            "commands": account["coverage"]["commands"]
                .as_u64()
                .filter(|value| *value <= 65_536),
            "unobservedItems": account["coverage"]["unobservedItems"]
                .as_u64()
                .filter(|value| *value <= 65_536),
            "rootSetup": label("rootSetup"),
            "parentChild": label("parentChild"),
            "retry": label("retry"),
            "replay": label("replay"),
            "gaps": gaps,
        },
    })
}

pub(crate) fn record_numeric_event(
    loaded: &crate::config::Loaded,
    work: &str,
    event: &Value,
) -> Result<Value, String> {
    if !crate::work::valid_work_handle(work) {
        return Err("usage observation requires a valid Work identity".into());
    }
    let _ = crate::work::resolve(loaded, work)?;
    if bounded_id(event.get("goalId"), "goalId")?.is_some() {
        match event.get("work") {
            None | Some(Value::Null) => {}
            Some(Value::String(value)) if value == work => {}
            Some(Value::String(_)) => {
                return Err("usage observation Work identity does not match".into())
            }
            Some(_) => return Err("usage observation work is invalid".into()),
        }
        let mut linked = event.clone();
        linked["work"] = Value::String(work.to_owned());
        return record_goal_numeric_event(loaded, &linked);
    }
    validate_optional_goal(loaded, event)?;
    let normalized = normalize_event(work, event)?;
    validate_contract(&normalized)?;
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
    validate_contract(&normalized)?;
    let ledger = goal_usage_ledger(loaded)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = super::read_goal_usage_raw(loaded)?;
        let existing = parse_goal_records(raw.as_deref())?;
        if let Some(previous) = existing.iter().find(|value| {
            value["id"] == normalized["id"] && value["goalId"] == normalized["goalId"]
        }) {
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
        if !canonical_record_matches(&value, &normalized) {
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
    const ALLOWED_FIELDS: [&str; 24] = [
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
        "sessionId",
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
    if let (Some(input), Some(cached)) = (input, cached) {
        if cached > input {
            return Err("usage observation cache exceeds input".into());
        }
    }
    if let (Some(input), Some(output)) = (input, output) {
        if input.checked_add(output).is_none() {
            return Err("usage observation total overflows u64".into());
        }
    }
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
        "sessionId": optional("sessionId")?,
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

pub(super) fn canonical_record_matches(value: &Value, normalized: &Value) -> bool {
    if value == normalized {
        return true;
    }
    if value.get("sessionId").is_some() {
        return false;
    }
    let mut legacy = normalized.clone();
    if let Some(object) = legacy.as_object_mut() {
        object.remove("sessionId");
    }
    value == &legacy
}

fn validate_contract(event: &Value) -> Result<(), String> {
    let semantics = event["semantics"].as_str().unwrap_or_default();
    let lifetime = event["lifetime"].as_str().unwrap_or_default();
    let needs_stream = matches!(semantics, "cumulative" | "per_turn");
    if semantics == "cumulative" && event["counterId"].as_str().is_none() {
        return Err("cumulative usage observation requires counterId".into());
    }
    if needs_stream && lifetime == "session" {
        if event["sessionId"].as_str().is_none() {
            return Err("session usage observation requires sessionId".into());
        }
        if event["counterId"].as_str().is_none() {
            return Err("session usage observation requires counterId".into());
        }
    }
    if needs_stream && lifetime == "goal" {
        if event["goalId"].as_str().is_none() {
            return Err("goal usage observation requires goalId".into());
        }
        if event["counterId"].as_str().is_none() {
            return Err("goal usage observation requires counterId".into());
        }
    }
    Ok(())
}

pub(super) fn stream_key(event: &Value) -> Result<String, &'static str> {
    let fields = [
        event["source"].clone(),
        event["goalId"].clone(),
        event["work"].clone(),
        event["scope"].clone(),
        event["lifetime"].clone(),
        event["sessionId"].clone(),
        event["counterId"].clone(),
        event["adapterVersion"].clone(),
    ];
    serde_json::to_string(&fields).map_err(|_| "stream_identity")
}

fn validate_monotonic(existing: &[Value], current: &Value) -> Result<(), String> {
    if current["semantics"] != "cumulative" {
        return Ok(());
    }
    let stream = stream_key(current).map_err(str::to_owned)?;
    let prior = existing.iter().rev().find(|value| {
        value["semantics"] == "cumulative"
            && stream_key(value).ok().as_deref() == Some(stream.as_str())
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
    if current["reset"] != true {
        let delta_input = current["values"]["inputTokens"]
            .as_u64()
            .zip(prior["values"]["inputTokens"].as_u64())
            .and_then(|(now, before)| now.checked_sub(before));
        let delta_cached = current["values"]["cachedInputTokens"]
            .as_u64()
            .zip(prior["values"]["cachedInputTokens"].as_u64())
            .and_then(|(now, before)| now.checked_sub(before));
        if let (Some(input), Some(cached)) = (delta_input, delta_cached) {
            if cached > input {
                return Err("cumulative cache delta exceeds input delta".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

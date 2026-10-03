//! Bounded prospective numeric observations for an existing Work/goal view.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

mod ingest;
pub(crate) use ingest::{goal_records, record_goal_numeric_event, record_numeric_event};

const MAX_JOURNAL_BYTES: u64 = 256 * 1024;
const MAX_RECORDS: usize = 32;
const MAX_GAPS: usize = 32;
const MAX_USAGE_EVENTS: usize = 128;
const PHASES: [&str; 6] = [
    "setup",
    "implementation",
    "review",
    "repair",
    "resume",
    "closeout",
];
const SCOPES: [&str; 5] = ["setup", "root", "direct", "native", "child"];
const SOURCES: [&str; 3] = ["host_reported", "native_observation", "activity"];
const STATUSES: [&str; 5] = ["observed", "missing", "failed", "repair", "unknown"];
const SEMANTICS: [&str; 3] = ["delta", "per_turn", "cumulative"];
const LIFETIMES: [&str; 3] = ["invocation", "session", "goal"];

/// Read native observations without inventing an aggregate counter. Native
/// completion usage is a provider snapshot, so replay, retry, parent/child and
/// direct/setup work remain explicit coverage states until disjoint deltas are
/// proven.
pub(crate) fn for_work(loaded: &crate::config::Loaded, work: &str) -> Value {
    for_work_with_limit(loaded, work, 4)
}

pub(crate) fn details_for_work(loaded: &crate::config::Loaded, work: &str) -> Value {
    for_work_with_limit(loaded, work, MAX_RECORDS)
}

pub(crate) fn details_for_goal(loaded: &crate::config::Loaded, goal_id: &str) -> Value {
    let events = match goal_records(loaded, goal_id) {
        Ok(events) => events,
        Err(error) => {
            return json!({"status":"unknown", "scope":"goal", "goalId":goal_id, "reason":error, "totals":Value::Null, "records":[]})
        }
    };
    let mut records = Vec::new();
    let mut gaps = Vec::new();
    let mut phases = unknown_coverage("unobserved");
    let mut aggregate = [0u64; 3];
    let mut aggregate_count = 0usize;
    let mut seen_ids = BTreeSet::new();
    let mut overlap_ids = BTreeSet::new();
    let mut turn_ids = BTreeSet::new();
    let mut cumulative = BTreeMap::new();
    for event in events {
        if records.len() >= MAX_RECORDS {
            push_gap(&mut gaps, "record_limit");
            break;
        }
        match numeric_event(
            &event,
            &mut seen_ids,
            &mut overlap_ids,
            &mut turn_ids,
            &mut cumulative,
        ) {
            Ok((record, values)) => {
                let phase = record["phase"].as_str().unwrap_or("unknown");
                if PHASES.contains(&phase) {
                    phases["phases"][phase] = json!("observed");
                }
                let scope = record["scope"].as_str().unwrap_or("unknown");
                if SCOPES.contains(&scope) {
                    let scope_key = if scope == "root" { "rootLead" } else { scope };
                    phases["scopes"][scope_key] = json!("observed");
                }
                if let Some(values) = values {
                    if aggregate[0].checked_add(values[0]).is_some()
                        && aggregate[1].checked_add(values[1]).is_some()
                        && aggregate[2].checked_add(values[2]).is_some()
                    {
                        aggregate[0] += values[0];
                        aggregate[1] += values[1];
                        aggregate[2] += values[2];
                        aggregate_count += 1;
                    } else {
                        push_gap(&mut gaps, "aggregate_overflow");
                    }
                } else if record["semantics"] == "cumulative"
                    && record["source"] != "native_observation"
                {
                    push_gap(&mut gaps, "cumulative_baseline");
                }
                records.push(record);
            }
            Err(reason) => push_gap(&mut gaps, reason),
        }
    }
    let mut result = account(
        goal_id,
        records,
        aggregate,
        aggregate_count,
        phases,
        gaps,
        MAX_RECORDS,
    );
    result["scope"] = json!("goal");
    result["goalId"] = json!(goal_id);
    result
}

fn usage_relative(work: &str) -> String {
    format!(
        "{}/native-actions/{work}/usage.jsonl",
        crate::project::layout_types::state_namespace()
    )
}

fn read_usage_raw(loaded: &crate::config::Loaded, work: &str) -> Result<Option<String>, String> {
    read_observation_raw(loaded, &usage_relative(work), "usage observation ledger")
}

pub(super) fn read_goal_usage_raw(
    loaded: &crate::config::Loaded,
) -> Result<Option<String>, String> {
    read_observation_raw(
        loaded,
        &format!(
            "{}/native-actions/usage.jsonl",
            crate::project::layout_types::state_namespace()
        ),
        "goal usage observation ledger",
    )
}

fn read_observation_raw(
    loaded: &crate::config::Loaded,
    relative: &str,
    label: &str,
) -> Result<Option<String>, String> {
    match crate::project::path::secure_bytes_observation_bounded(
        &loaded.state_root,
        relative,
        label,
        MAX_JOURNAL_BYTES,
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "usage observation ledger is not UTF-8".into()),
        crate::project::path::SecureBytesResult::Absent(_) => Ok(None),
        crate::project::path::SecureBytesResult::Unsafe(error)
        | crate::project::path::SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        crate::project::path::SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn parse_usage_records(raw: Option<&str>, work: &str) -> Result<Vec<Value>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.len() > MAX_JOURNAL_BYTES as usize {
        return Err("usage observation ledger exceeds its byte bound".into());
    }
    let mut records = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            return Err("usage observation ledger contains an empty line".into());
        }
        if records.len() >= MAX_USAGE_EVENTS {
            return Err("usage observation ledger exceeds its event bound".into());
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| "usage observation ledger is corrupt".to_owned())?;
        let normalized = ingest::normalize_event(work, &value)?;
        if !ingest::canonical_record_matches(&value, &normalized) {
            return Err("usage observation ledger has noncanonical fields".into());
        }
        records.push(normalized);
    }
    Ok(records)
}

fn for_work_with_limit(loaded: &crate::config::Loaded, work: &str, display_limit: usize) -> Value {
    if !crate::work::valid_work_handle(work) {
        return json!({"status":"unknown", "scope":"goal_work", "reason":"invalid_work_identity", "totals":Value::Null, "records":[], "coverage": unknown_coverage("unknown")});
    }
    let root = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("native-actions")
        .join(work);
    let mut records = Vec::new();
    let mut gaps = Vec::new();
    let mut phases = unknown_coverage("unobserved");
    let mut aggregate = [0u64; 3];
    let mut aggregate_count = 0usize;
    let mut seen_ids = BTreeSet::new();
    let mut overlap_ids = BTreeSet::new();
    let mut turn_ids = BTreeSet::new();
    let mut cumulative = BTreeMap::new();
    let mut numeric_events = Vec::new();
    match crate::session_goal::read(&loaded.state_root) {
        Ok(Some(goal)) => {
            if let Some(goal_id) = goal["goalId"].as_str() {
                match goal_records(loaded, goal_id) {
                    Ok(events) => {
                        for event in events {
                            if event["work"].as_str() != Some(work) {
                                continue;
                            }
                            numeric_events.push(event);
                        }
                    }
                    Err(_) => push_gap(&mut gaps, "goal_usage_unreadable"),
                }
            }
        }
        Ok(None) => {}
        Err(_) => push_gap(&mut gaps, "goal_unreadable"),
    }
    match read_usage_raw(loaded, work) {
        Ok(Some(raw)) => match parse_usage_records(Some(&raw), work) {
            Ok(events) => {
                for event in events {
                    numeric_events.push(event);
                }
            }
            Err(_) => push_gap(&mut gaps, "usage_ledger_corrupt"),
        },
        Ok(None) => {}
        Err(_) => push_gap(&mut gaps, "usage_ledger_unreadable"),
    }
    {
        let mut consume = |event: &Value| {
            if records.len() >= MAX_RECORDS {
                push_gap(&mut gaps, "record_limit");
                return;
            }
            match numeric_event(
                event,
                &mut seen_ids,
                &mut overlap_ids,
                &mut turn_ids,
                &mut cumulative,
            ) {
                Ok((record, values)) => {
                    let phase = record["phase"].as_str().unwrap_or("unknown");
                    if PHASES.contains(&phase) {
                        phases["phases"][phase] = json!("observed");
                    }
                    let scope = record["scope"].as_str().unwrap_or("unknown");
                    if SCOPES.contains(&scope) {
                        let scope_key = if scope == "root" { "rootLead" } else { scope };
                        phases["scopes"][scope_key] = json!("observed");
                    }
                    if let Some(values) = values {
                        if aggregate[0].checked_add(values[0]).is_some()
                            && aggregate[1].checked_add(values[1]).is_some()
                            && aggregate[2].checked_add(values[2]).is_some()
                        {
                            aggregate[0] += values[0];
                            aggregate[1] += values[1];
                            aggregate[2] += values[2];
                            aggregate_count += 1;
                        } else {
                            push_gap(&mut gaps, "aggregate_overflow");
                        }
                    } else if record["semantics"] == "cumulative"
                        && record["source"] != "native_observation"
                    {
                        push_gap(&mut gaps, "cumulative_baseline");
                    }
                    records.push(record);
                }
                Err(reason) => push_gap(&mut gaps, reason),
            }
        };
        for event in numeric_events {
            consume(&event);
        }
    }
    match std::fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            push_gap(&mut gaps, "native_directory_unsafe");
            return account(
                work,
                records,
                aggregate,
                aggregate_count,
                phases,
                gaps,
                display_limit,
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return account(
                work,
                records,
                aggregate,
                aggregate_count,
                phases,
                gaps,
                display_limit,
            );
        }
        Err(_) => {
            push_gap(&mut gaps, "native_directory_unreadable");
            return account(
                work,
                records,
                aggregate,
                aggregate_count,
                phases,
                gaps,
                display_limit,
            );
        }
    }
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return account(
                work,
                records,
                aggregate,
                aggregate_count,
                phases,
                gaps,
                display_limit,
            )
        }
        Err(_) => {
            push_gap(&mut gaps, "native_directory_unreadable");
            return account(
                work,
                records,
                aggregate,
                aggregate_count,
                phases,
                gaps,
                display_limit,
            );
        }
    };
    let mut journal_ids = BTreeSet::new();
    let mut journal_assignments = BTreeSet::new();
    let mut native_records = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            push_gap(&mut gaps, "journal_listing");
            continue;
        };
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            push_gap(&mut gaps, "journal_metadata");
            continue;
        };
        let name = path.file_name().and_then(|value| value.to_str());
        if metadata.file_type().is_symlink() {
            push_gap(&mut gaps, "journal_unsafe");
            continue;
        }
        if !metadata.is_file() {
            push_gap(&mut gaps, "journal_not_regular");
            continue;
        }
        if metadata.len() > MAX_JOURNAL_BYTES {
            push_gap(&mut gaps, "journal_oversize");
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("json")
            || name.is_some_and(|name| name.starts_with('.') || name.ends_with(".schema.json"))
        {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&loaded.state_root) else {
            push_gap(&mut gaps, "journal_path");
            continue;
        };
        let Some(relative) = relative.to_str() else {
            push_gap(&mut gaps, "journal_path");
            continue;
        };
        let bytes = match crate::project::path::secure_bytes_observation_bounded(
            &loaded.state_root,
            relative,
            "native usage journal",
            MAX_JOURNAL_BYTES,
        ) {
            crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
            crate::project::path::SecureBytesResult::Absent(_) => {
                push_gap(&mut gaps, "journal_missing");
                continue;
            }
            crate::project::path::SecureBytesResult::Unsafe(_)
            | crate::project::path::SecureBytesResult::Unreadable(_) => {
                push_gap(&mut gaps, "journal_unreadable");
                continue;
            }
            #[cfg(not(unix))]
            crate::project::path::SecureBytesResult::Unsupported(_) => {
                push_gap(&mut gaps, "journal_unreadable");
                continue;
            }
        };
        let Ok(journal) = serde_json::from_slice::<Value>(&bytes) else {
            push_gap(&mut gaps, "journal_corrupt");
            continue;
        };
        if journal["work"].as_str() != Some(work) {
            push_gap(&mut gaps, "journal_work_mismatch");
            continue;
        }
        let completed = journal["status"] == "completed";
        if !completed {
            push_gap(&mut gaps, "failed_or_incomplete");
        }
        let Some(assignment) = journal["assignment"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 128)
        else {
            push_gap(&mut gaps, "journal_identity");
            continue;
        };
        let Some(operation) = journal["operationId"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 128)
        else {
            push_gap(&mut gaps, "journal_identity");
            continue;
        };
        let journal_id = format!("{assignment}:{operation}");
        if !journal_ids.insert(journal_id) {
            push_gap(&mut gaps, "journal_replay");
            continue;
        }
        if !journal_assignments.insert(assignment.to_owned()) {
            push_gap(&mut gaps, "retry_unknown");
        }
        let account = journal_account(&journal);
        if !completed && account.is_null() && journal["priorFailure"].is_null() {
            continue;
        }
        if native_records.len() >= MAX_USAGE_EVENTS {
            push_gap(&mut gaps, "record_limit");
            continue;
        }
        native_records.push(json!({
            "id": format!("native:{assignment}:{operation}"),
            "source": "native_observation",
            "scope": "native",
            "phase": journal_phase(&journal),
            "status": if completed { account["status"].as_str().filter(|value| STATUSES.contains(value)).unwrap_or("missing") } else { "failed" },
            "semantics": "provider_turn_snapshot",
            "lifetime": "invocation",
            "goalId": journal["goalId"].as_str().filter(|value| !value.is_empty() && value.len() <= 128),
            "taskId": journal["taskId"].as_str().filter(|value| !value.is_empty() && value.len() <= 128),
            "role": journal["role"].as_str().filter(|value| !value.is_empty() && value.len() <= 128),
            "assignment": assignment,
            "attempt": journal["attempt"],
            "invocation": operation,
            "turn": journal["observation"]["turn"],
            "sessionMode": journal["sessionMode"],
            "adapterVersion": journal["adapterVersion"],
            "operation": operation,
            "account": if account.is_object() { ingest::bounded_native_account(&account) } else { json!({"status":"missing", "reason":"legacy_observation"}) },
            "priorFailure": bounded_prior_failure(&journal["priorFailure"]),
        }));
    }
    records.extend(native_records);
    records.sort_by(|left, right| {
        left["id"]
            .as_str()
            .cmp(&right["id"].as_str())
            .then_with(|| {
                left["assignment"]
                    .as_str()
                    .cmp(&right["assignment"].as_str())
            })
            .then_with(|| left["operation"].as_str().cmp(&right["operation"].as_str()))
    });
    if records.len() > MAX_RECORDS {
        push_gap(&mut gaps, "record_limit");
        records.truncate(MAX_RECORDS);
    }
    account(
        work,
        records,
        aggregate,
        aggregate_count,
        phases,
        gaps,
        display_limit,
    )
}

fn account(
    work: &str,
    records: Vec<Value>,
    aggregate: [u64; 3],
    aggregate_count: usize,
    mut phases: Value,
    mut gaps: Vec<&str>,
    display_limit: usize,
) -> Value {
    gaps.sort_unstable();
    if records.iter().any(|record| record["scope"] == "native") {
        phases["scopes"]["native"] = json!("observed_or_missing");
    }
    let record_count = records.len();
    let record_omissions = record_count.saturating_sub(display_limit);
    let records = records.into_iter().take(display_limit).collect::<Vec<_>>();
    let totals = if aggregate_count == 0 {
        Value::Null
    } else {
        let total = aggregate[0].checked_add(aggregate[2]);
        json!({
            "inputTokens": aggregate[0],
            "cachedInputTokens": aggregate[1],
            "outputTokens": aggregate[2],
            "totalTokens": total,
            "cacheRatio": if aggregate[0] == 0 { Value::Null } else { json!(aggregate[1] as f64 / aggregate[0] as f64) },
            "events": aggregate_count,
        })
    };
    json!({
        "status": if record_count == 0 && gaps.is_empty() { "unknown" } else { "partial" },
        "scope": "goal_work",
        "work": work,
        "counterSemantics": if aggregate_count == 0 {
            "nonadditive_or_unknown"
        } else {
            "bounded_disjoint_delta_subset"
        },
        "totals": totals,
        "records": records,
        "recordCount": record_count,
        "recordOmissions": record_omissions,
        "recordsRoute": "current.details.grouped",
        "coverage": {"phases": phases, "parentChild": "not_counted", "replay": "not_counted", "retry": "unknown", "gaps": gaps},
        "limitation": "No aggregate is claimed until disjoint provider deltas and complete root coverage are proven.",
    })
}

fn unknown_coverage(state: &str) -> Value {
    let phases = PHASES
        .iter()
        .map(|phase| ((*phase).to_owned(), json!(state)))
        .collect::<serde_json::Map<_, _>>();
    let scopes = SCOPES
        .iter()
        .map(|scope| {
            let key = if *scope == "root" { "rootLead" } else { *scope };
            (key.to_owned(), json!(state))
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"phases":phases, "scopes":scopes})
}

fn push_gap(gaps: &mut Vec<&'static str>, reason: &'static str) {
    if gaps.len() < MAX_GAPS && !gaps.contains(&reason) {
        gaps.push(reason);
    }
}

fn journal_account(journal: &Value) -> Value {
    if journal["observation"]["outcomeAccount"].is_object() {
        return journal["observation"]["outcomeAccount"].clone();
    }
    let usage = &journal["observation"]["usage"];
    if !usage.is_object() {
        return Value::Null;
    }
    let input = usage["inputTokens"].as_u64();
    let cached = usage["cachedInputTokens"].as_u64();
    let output = usage["outputTokens"].as_u64();
    json!({
        "status": if input.is_some() && cached.is_some() && output.is_some() { "observed" } else { "missing" },
        "scope": "native_turn",
        "source": "native_observation",
        "counterSemantics": "provider_turn_snapshot",
        "additive": false,
        "usage": {"inputTokens": input, "cachedInputTokens": cached, "outputTokens": output},
        "coverage": {
            "turn": journal["observation"]["turn"],
            "commands": journal["observation"]["commands"].as_array().map_or(0, Vec::len),
            "unobservedItems": journal["observation"]["unobservedItemCount"],
            "rootSetup": "unobserved",
            "parentChild": "not_counted",
            "retry": "unknown",
            "replay": "not_counted",
            "gaps": journal["observation"]["coverageGaps"],
        },
    })
}

fn journal_phase(journal: &Value) -> &'static str {
    if journal["priorFailure"].is_object() {
        "repair"
    } else if journal["role"] == "reviewer" {
        "review"
    } else if journal["role"] == "lead" {
        "closeout"
    } else if journal["attempt"]
        .as_u64()
        .is_some_and(|attempt| attempt > 1)
    {
        "resume"
    } else {
        "implementation"
    }
}

fn bounded_prior_failure(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    let account = journal_account(value);
    let operation = value["operationId"]
        .as_str()
        .or_else(|| value["operation"].as_str())
        .filter(|item| !item.is_empty() && item.len() <= 128);
    json!({
        "operation": operation,
        "status": value["status"].as_str().filter(|item| STATUSES.contains(item)).unwrap_or("unknown"),
        "account": if account.is_object() { ingest::bounded_native_account(&account) } else { Value::Null },
    })
}

fn numeric_event(
    event: &Value,
    seen_ids: &mut BTreeSet<String>,
    overlap_ids: &mut BTreeSet<String>,
    turn_ids: &mut BTreeSet<String>,
    cumulative: &mut BTreeMap<String, [u64; 3]>,
) -> Result<(Value, Option<[u64; 3]>), &'static str> {
    let id = event["id"]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .ok_or("event_identity")?;
    if !seen_ids.insert(id.to_owned()) {
        return Err("duplicate_event");
    }
    let source = event["source"]
        .as_str()
        .filter(|value| SOURCES.contains(value))
        .ok_or("event_source")?;
    let scope = event["scope"]
        .as_str()
        .filter(|value| SCOPES.contains(value))
        .ok_or("event_scope")?;
    let phase = event["phase"]
        .as_str()
        .filter(|value| PHASES.contains(value))
        .ok_or("event_phase")?;
    let semantics = event["semantics"]
        .as_str()
        .filter(|value| SEMANTICS.contains(value))
        .ok_or("event_semantics")?;
    let lifetime = event["lifetime"]
        .as_str()
        .filter(|value| LIFETIMES.contains(value))
        .ok_or("event_lifetime")?;
    let status = event["status"]
        .as_str()
        .filter(|value| STATUSES.contains(value))
        .unwrap_or("unknown");
    let countable_status = matches!(status, "observed" | "failed" | "repair");
    if !countable_status && event.get("values").and_then(Value::as_object).is_none() {
        return Ok((
            json!({
                "id":id,
                "source":source,
                "scope":scope,
                "phase":phase,
                "semantics":semantics,
                "lifetime":lifetime,
                "status":status,
            }),
            None,
        ));
    }
    let parent_raw = event.get("parentEventId");
    let parent = parent_raw
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 128);
    if parent_raw.is_some() && !parent_raw.is_some_and(Value::is_null) && parent.is_none() {
        return Err("parent_identity");
    }
    let overlap_raw = event.get("overlapGroup");
    let overlap = overlap_raw
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 128);
    if overlap_raw.is_some() && !overlap_raw.is_some_and(Value::is_null) && overlap.is_none() {
        return Err("overlap_identity");
    }
    if let Some(overlap) = overlap {
        if !overlap_ids.insert(overlap.to_owned()) {
            return Err("overlap_unknown");
        }
    }
    let values = event
        .get("values")
        .and_then(Value::as_object)
        .ok_or("event_values")?;
    let numbers = ["inputTokens", "cachedInputTokens", "outputTokens"]
        .map(|name| values.get(name).and_then(Value::as_u64));
    let projected_values = json!({
        "inputTokens": numbers[0],
        "cachedInputTokens": numbers[1],
        "outputTokens": numbers[2],
    });
    if numbers[0]
        .zip(numbers[1])
        .is_some_and(|(input, cached)| cached > input)
    {
        return Err("cache_subset");
    }
    if let (Some(input), Some(output)) = (numbers[0], numbers[2]) {
        if input.checked_add(output).is_none() {
            return Err("total_overflow");
        }
    }
    let additive_contract =
        countable_status && source != "native_observation" && overlap.is_none() && parent.is_none();
    let values = if additive_contract && semantics == "delta" && lifetime == "invocation" {
        complete_numbers(numbers)
    } else if additive_contract && semantics == "per_turn" && matches!(lifetime, "session" | "goal")
    {
        let stream_declared = event["counterId"].as_str().is_some()
            && ((lifetime == "session" && event["sessionId"].as_str().is_some())
                || (lifetime == "goal" && event["goalId"].as_str().is_some()));
        if !stream_declared {
            None
        } else {
            let stream = crate::session_goal::usage::ingest::stream_key(event)?;
            let turn = event["turn"]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 128)
                .ok_or("turn_identity")?;
            let turn_key = format!("{stream}|{turn}");
            if !turn_ids.insert(turn_key) {
                return Err("per_turn_replay");
            }
            complete_numbers(numbers)
        }
    } else if additive_contract
        && semantics == "cumulative"
        && matches!(lifetime, "session" | "goal")
    {
        let stream_declared = event["counterId"].as_str().is_some()
            && ((lifetime == "session" && event["sessionId"].as_str().is_some())
                || (lifetime == "goal" && event["goalId"].as_str().is_some()));
        if !stream_declared {
            None
        } else {
            let current = complete_numbers(numbers);
            match current {
                None => None,
                Some(current) => {
                    let key = crate::session_goal::usage::ingest::stream_key(event)?;
                    let previous = cumulative.insert(key, current);
                    if event["reset"] == true {
                        Some(current)
                    } else if let Some(previous) = previous {
                        let delta = [
                            current[0]
                                .checked_sub(previous[0])
                                .ok_or("cumulative_decrease")?,
                            current[1]
                                .checked_sub(previous[1])
                                .ok_or("cumulative_decrease")?,
                            current[2]
                                .checked_sub(previous[2])
                                .ok_or("cumulative_decrease")?,
                        ];
                        if delta[1] > delta[0] {
                            return Err("cumulative_cache_delta_subset");
                        }
                        Some(delta)
                    } else {
                        None
                    }
                }
            }
        }
    } else {
        None
    };
    Ok((
        json!({
            "id":id,
            "source":source,
            "scope":scope,
            "phase":phase,
            "semantics":semantics,
            "lifetime":lifetime,
            "status":status,
            "goalId":event["goalId"],
            "taskId":event["taskId"],
            "role":event["role"],
            "assignment":event["assignment"],
            "attempt":event["attempt"],
            "invocation":event["invocation"],
            "turn":event["turn"],
            "sessionMode":event["sessionMode"],
            "adapterVersion":event["adapterVersion"],
            "sessionId":event["sessionId"],
            "counterId":event["counterId"],
            "reset":event["reset"],
            "values":projected_values,
            "parentEventId":parent,
            "overlap": if parent.is_some() { json!("parent") } else if overlap.is_some() { json!("group") } else { Value::Null }
        }),
        values,
    ))
}

fn complete_numbers(numbers: [Option<u64>; 3]) -> Option<[u64; 3]> {
    match (numbers[0], numbers[1], numbers[2]) {
        (Some(input), Some(cached), Some(output)) => Some([input, cached, output]),
        _ => None,
    }
}

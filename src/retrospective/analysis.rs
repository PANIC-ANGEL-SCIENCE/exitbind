//! Conservative projection from supported Codex records to after-done findings.

use super::{
    git::Repository,
    source::{self, SourceData},
};
use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct Event {
    reference: String,
    session: String,
    timestamp: Option<DateTime<FixedOffset>>,
    role: String,
    kind: String,
    text: String,
    req: Option<String>,
}

pub fn inspect(
    repo: &Repository,
    source_data: &SourceData,
    since: DateTime<FixedOffset>,
    until: DateTime<FixedOffset>,
) -> Value {
    let mut events = Vec::new();
    let mut sessions = HashMap::<String, (Option<String>, Option<String>)>::new();
    let mut duplicate = HashSet::new();
    let mut current_session = String::new();
    let mut current_file = None;
    for (file, line, bytes) in source::lines(source_data) {
        if current_file != Some(file) {
            current_file = Some(file);
            current_session.clear();
        }
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            continue;
        };
        let body = value.get("payload").unwrap_or(&value);
        let typ = value
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| matches!(*kind, "session_meta" | "response_item"))
            .or_else(|| body.get("type").and_then(Value::as_str))
            .or_else(|| value.get("kind").and_then(Value::as_str))
            .unwrap_or("");
        let session = if typ == "session_meta" {
            string_field(body, &["id"])
                .or_else(|| string_field(body, &["session_id", "sessionId"]))
                .unwrap_or_default()
        } else if current_session.is_empty() {
            string_field(body, &["session_id", "sessionId"]).unwrap_or_default()
        } else {
            current_session.clone()
        };
        if session.is_empty() {
            continue;
        }
        if typ == "session_meta" {
            current_session = session.clone();
        }
        let id = string_field(&value, &["id", "event_id", "eventId"])
            .or_else(|| string_field(body, &["id", "event_id", "eventId"]));
        let dedup_key = id
            .clone()
            .unwrap_or_else(|| crate::evidence::hash::bytes(bytes));
        if !duplicate.insert(dedup_key) {
            continue;
        }
        let cwd = string_field(body, &["cwd", "workdir", "working_directory"]).or_else(|| {
            body.get("git")
                .and_then(|v| string_field(v, &["root", "repo", "repository"]))
        });
        let parent = string_field(
            body,
            &[
                "forked_from_id",
                "forkedFromId",
                "parent_thread_id",
                "parentThreadId",
                "continuation_of",
                "continuationOf",
            ],
        );
        if typ == "session_meta" || body.get("cwd").is_some() || body.get("git").is_some() {
            sessions.insert(session.clone(), (cwd.clone(), parent.clone()));
        }
        let timestamp = string_field(&value, &["timestamp", "created_at", "createdAt", "time"])
            .or_else(|| string_field(body, &["timestamp", "created_at", "createdAt", "time"]))
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok());
        let (role, text, kind) = visible_payload(body, typ);
        let req = requirement(&text);
        if !text.is_empty() || !kind.is_empty() {
            let reference = format!("r:{}:{}:{}", &source_data.digest[..16], file, line);
            events.push(Event {
                reference,
                session,
                timestamp,
                role,
                kind,
                text,
                req,
            });
        }
    }
    let canonical_repo = repo.root.clone();
    let mut usable_sessions = BTreeSet::new();
    for (session, (cwd, _)) in &sessions {
        if cwd
            .as_deref()
            .is_some_and(|cwd| canonical_path(cwd).as_deref() == Some(canonical_repo.as_path()))
        {
            usable_sessions.insert(session.clone());
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (session, (_, parent)) in &sessions {
            if let Some(parent) = parent {
                if usable_sessions.contains(parent)
                    && session_can_join_repo(session, &sessions, canonical_repo.as_path())
                    && usable_sessions.insert(session.clone())
                {
                    changed = true;
                }
                if usable_sessions.contains(session)
                    && session_can_join_repo(parent, &sessions, canonical_repo.as_path())
                    && usable_sessions.insert(parent.clone())
                {
                    changed = true;
                }
            }
        }
    }
    events.retain(|event| usable_sessions.contains(&event.session));
    events.sort_by_key(|event| event.timestamp);
    let mut claims = Vec::new();
    let timestamp_ambiguous = events
        .iter()
        .filter(|event| event.timestamp.is_none())
        .count();
    for event in &events {
        if event.timestamp.is_none()
            || !in_window(event.timestamp, since, until)
            || event.role != "assistant"
        {
            continue;
        }
        if let Some(scope) = completion_scope(&event.text) {
            claims.push((event.clone(), scope));
        }
    }
    let mut findings = Vec::new();
    let claims_observed = claims.len();
    let mut activity_counts = BTreeMap::<String, usize>::new();
    let mut counted_activity = HashSet::new();
    let mut counted_requirements = HashSet::new();
    let mut candidates = Vec::new();
    let mut no_relevant_activity = 0usize;
    let mut abstained = 0usize;
    for (claim, scope) in claims {
        if claim.req.is_none() {
            abstained += 1;
            candidates.push(json!({"reference": claim.reference, "reason": "missing_explicit_requirement_marker"}));
            continue;
        }
        let mut later = Vec::new();
        for event in &events {
            if event.reference == claim.reference
                || event.timestamp.is_none()
                || event.timestamp <= claim.timestamp
            {
                continue;
            }
            if !in_window(event.timestamp, since, until)
                || !linked(&claim, event, &sessions)
                || event.role == "user"
            {
                continue;
            }
            let event_kind = classify_activity(event);
            if event_kind.is_empty() {
                continue;
            }
            if event_kind == "scope_change" {
                continue;
            }
            if claim.req.is_none() || event.req.is_none() || claim.req != event.req {
                continue;
            }
            if counted_activity.insert(event.reference.clone()) {
                *activity_counts.entry(event_kind.to_owned()).or_default() += 1;
            }
            later.push((event.clone(), event_kind.to_owned()));
        }
        let decisive = later
            .iter()
            .filter(|(_, kind)| matches!(kind.as_str(), "repair" | "resolution"))
            .collect::<Vec<_>>();
        let reopened = decisive.iter().any(|(_, kind)| kind == "repair");
        if !reopened || scope == "implementation_only" {
            if !reopened {
                no_relevant_activity += 1;
            }
            if scope == "implementation_only" {
                abstained += 1;
            }
            continue;
        }
        let requirement_key = (
            lineage_root(&claim.session, &sessions, &usable_sessions),
            claim.req.clone(),
        );
        if !counted_requirements.insert(requirement_key) {
            continue;
        }
        let resolved = decisive
            .last()
            .is_some_and(|(_, kind)| kind == "resolution");
        let outcome = if resolved {
            "reopened_later_resolved"
        } else {
            "last_observed_unresolved"
        };
        let mut counts = BTreeMap::new();
        for (_, kind) in &later {
            *counts.entry(kind.clone()).or_insert(0usize) += 1;
        }
        let last = decisive.last().map(|(event, _)| event);
        findings.push(json!({
            "id": format!("finding:{}", &claim.reference[2..]),
            "claim": { "reference": claim.reference, "scope": scope, "language": language(&claim.text), "text": scrub(&claim.text) },
            "requirement": claim.req,
            "linkage_confidence": if later.iter().any(|(event, _)| event.session != claim.session) { "explicit_lineage" } else { "same_session_with_matching_requirement" },
            "activity": {
                "counts": counts,
                "references": later.iter().map(|(e, _)| e.reference.clone()).collect::<Vec<_>>(),
                "outcome_references": decisive.iter().map(|(event, _)| event.reference.clone()).collect::<Vec<_>>()
            },
            "outcome": outcome,
            "last_observed": last.and_then(|e| e.timestamp.map(|t| t.to_rfc3339())),
            "cutoff": until.to_rfc3339(),
            "evidence": std::iter::once(claim.reference.clone()).chain(later.iter().map(|(e, _)| e.reference.clone())).collect::<Vec<_>>(),
        }));
    }
    let status = if !source_data.unsupported_versions.is_empty() {
        "unsupported"
    } else if source_data.malformed > 0
        || source_data.oversized > 0
        || source_data.unsupported_shapes > 0
        || source_data.truncated
        || timestamp_ambiguous > 0
        || source_data.metadata_missing
        || source_data.files_missing_metadata > 0
    {
        "partial"
    } else {
        "complete"
    };
    json!({
        "format": "retrospective-v1", "status": status, "adapter": source::ADAPTER, "adapter_version": source::VERSION,
        "repository": { "head": repo.head, "dirty": repo.dirty },
        "interval": { "since": since.to_rfc3339(), "until": until.to_rfc3339() },
        "source": { "digest": source_data.digest, "records": source_data.records, "files": source_data.files.len() },
        "coverage": { "status": status, "records_read": source_data.records, "records_malformed": source_data.malformed, "records_oversized": source_data.oversized, "records_unsupported": source_data.unsupported_shapes, "timestamps_ambiguous": timestamp_ambiguous, "metadata_missing": source_data.metadata_missing, "files_missing_metadata": source_data.files_missing_metadata, "bytes_read": source_data.bytes_read, "record_cutoff": source::MAX_RECORDS, "truncated": source_data.truncated, "sessions_linked": usable_sessions.len(), "activity_counts": activity_counts, "unsupported_versions": source_data.unsupported_versions },
        "usage": { "status": "unavailable", "reason": "native rollout usage attribution is not supported" },
        "candidates": candidates,
        "coverage_summary": { "claims_observed": claims_observed, "findings": findings.len(), "abstained": abstained, "no_relevant_activity": no_relevant_activity },
        "findings": findings,
        "real_case": "UNVERIFIED"
    })
}

fn linked(
    claim: &Event,
    event: &Event,
    sessions: &HashMap<String, (Option<String>, Option<String>)>,
) -> bool {
    if claim.session == event.session {
        return true;
    }
    let mut current = event.session.as_str();
    for _ in 0..16 {
        let Some((_, Some(parent))) = sessions.get(current) else {
            break;
        };
        if parent == &claim.session {
            return true;
        }
        current = parent;
    }
    let mut current = claim.session.as_str();
    for _ in 0..16 {
        let Some((_, Some(parent))) = sessions.get(current) else {
            break;
        };
        if parent == &event.session {
            return true;
        }
        current = parent;
    }
    false
}

fn session_can_join_repo(
    session: &str,
    sessions: &HashMap<String, (Option<String>, Option<String>)>,
    canonical_repo: &Path,
) -> bool {
    let Some((cwd, _)) = sessions.get(session) else {
        return true;
    };
    let Some(cwd) = cwd.as_deref() else {
        return true;
    };
    canonical_path(cwd).is_some_and(|cwd| cwd == canonical_repo)
}

fn lineage_root(
    session: &str,
    sessions: &HashMap<String, (Option<String>, Option<String>)>,
    usable_sessions: &BTreeSet<String>,
) -> String {
    let mut current = session;
    for _ in 0..16 {
        let Some((_, Some(parent))) = sessions.get(current) else {
            break;
        };
        if !usable_sessions.contains(parent) {
            break;
        }
        current = parent;
    }
    current.to_owned()
}

fn visible_payload(value: &Value, typ: &str) -> (String, String, String) {
    let inner_type = value.get("type").and_then(Value::as_str).unwrap_or(typ);
    if matches!(inner_type, "reasoning" | "encrypted_content" | "thinking") {
        return (String::new(), String::new(), String::new());
    }
    let role = string_field(value, &["role"]).unwrap_or_else(|| {
        if matches!(
            inner_type,
            "function_call_output" | "custom_tool_call_output"
        ) {
            "tool".into()
        } else if typ == "response_item" {
            "assistant".into()
        } else {
            "".into()
        }
    });
    let text = text_value(
        value
            .get("content")
            .or_else(|| value.get("text"))
            .or_else(|| value.get("output"))
            .or_else(|| value.get("message")),
    );
    let kind = if typ == "session_meta" {
        "session_meta"
    } else if matches!(
        inner_type,
        "function_call_output" | "custom_tool_call_output"
    ) {
        "tool"
    } else if typ == "response_item" || !text.is_empty() {
        "message"
    } else {
        ""
    };
    (role, text, kind.into())
}

fn text_value(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::String(s) => s
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .take(16_384)
            .collect(),
        Value::Array(values) => values
            .iter()
            .filter(|v| {
                v.get("type").and_then(Value::as_str).map_or(true, |kind| {
                    matches!(
                        kind,
                        "input_text"
                            | "output_text"
                            | "text"
                            | "message"
                            | "tool_output"
                            | "function_call_output"
                    )
                })
            })
            .map(|v| text_value(Some(v)))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(map) => ["text", "content", "output", "message"]
            .iter()
            .find_map(|key| map.get(*key))
            .map(|v| text_value(Some(v)))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn canonical_path(value: &str) -> Option<PathBuf> {
    std::fs::canonicalize(value).ok()
}
fn in_window(
    value: Option<DateTime<FixedOffset>>,
    since: DateTime<FixedOffset>,
    until: DateTime<FixedOffset>,
) -> bool {
    value.is_some_and(|v| v >= since && v <= until)
}

fn completion_scope(text: &str) -> Option<&'static str> {
    for raw in text.lines() {
        let line = raw.trim().to_lowercase();
        if line.is_empty()
            || line.starts_with('>')
            || line.starts_with("quote:")
            || line.contains("will be done")
            || line.contains("will complete")
            || line.contains("would be done")
            || line.contains("not done")
            || line.contains("not complete")
        {
            continue;
        }
        if line.contains("tests remain")
            || line.contains("tests fail")
            || line.contains("expected to fail")
            || line.contains("tests are expected")
            || line.contains("implementation complete")
            || line.contains("구현 완료")
        {
            return Some("implementation_only");
        }
        if line == "done"
            || line.contains("done.")
            || line.contains("task complete")
            || line.contains("completed the task")
            || line.contains("all requirements")
            || line.contains("작업을 완료")
            || line.contains("완료했습니다")
        {
            return Some("task");
        }
    }
    None
}

fn classify_activity(event: &Event) -> &'static str {
    let text = event.text.to_lowercase();
    if event.role == "user"
        || text.lines().any(|line| {
            let line = line.trim();
            line.starts_with('>') || line.starts_with("quote:")
        })
    {
        return "";
    }
    if text.contains("new requirement")
        || text.contains("scope change")
        || text.contains("scope changed")
        || text.contains("새 요구")
    {
        return "scope_change";
    }
    if text.contains("not fixed")
        || text.contains("not resolved")
        || text.contains("not verified")
        || text.contains("not passed")
        || text.contains("did not pass")
        || text.contains("never passed")
        || text.contains("unfixed")
        || text.contains("unresolved")
        || text.contains("unverified")
        || text.contains("will be fixed")
        || text.contains("will be resolved")
    {
        return "";
    }
    if has_positive_reopening(&text) {
        return "repair";
    }
    if text.contains("resolved")
        || text.contains("fixed")
        || text.contains("all tests pass")
        || text.contains(" passed")
        || text.starts_with("passed")
        || text.contains("verified")
        || text.contains("repair complete")
        || text.contains("repair succeeded")
        || text.contains("rework complete")
        || text.contains("retry succeeded")
        || text.contains("닫혔")
        || text.contains("해결")
    {
        return "resolution";
    }
    if NON_REOPENING_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
    {
        return "";
    }
    if text.contains("review") || text.contains("검토") {
        return "review";
    }
    if event.kind == "tool" {
        return "tool_outcome";
    }
    ""
}

const NON_REOPENING_MARKERS: &[&str] = &[
    "failed as expected",
    "expected to fail",
    "expected failure",
    "0 failed",
    "0 test failed",
    "0 tests failed",
    "zero failed",
    "zero test failed",
    "zero tests failed",
    "none failed",
    "no test failed",
    "no tests failed",
    "no failure",
    "no failures",
    "did not fail",
    "didn't fail",
    "without failure",
    "no repair required",
    "no repair needed",
    "repair not required",
    "repair is not required",
    "repair unnecessary",
    "does not need repair",
    "did not require repair",
    "repair completed",
    "repair complete",
    "repair succeeded",
    "repair successful",
    "successful repair",
    "rework completed",
    "rework complete",
    "rework succeeded",
    "successful rework",
    "retry succeeded",
    "retry successful",
];

fn has_positive_reopening(text: &str) -> bool {
    if has_positive_failure_count(text) {
        return true;
    }
    let mut remaining = text.to_owned();
    for marker in NON_REOPENING_MARKERS {
        remaining = remaining.replace(marker, "");
    }
    remaining.contains("retry")
        || remaining.contains("rework")
        || remaining.contains("repair")
        || remaining.contains("failing")
        || remaining.contains("failed")
        || remaining.contains("failure")
        || remaining.contains("실패")
        || remaining.contains("수정")
}

fn has_positive_failure_count(text: &str) -> bool {
    let tokens = text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(index, token)| {
        let Ok(count) = token.parse::<u64>() else {
            return false;
        };
        count > 0
            && (tokens.get(index + 1) == Some(&"failed")
                || matches!(tokens.get(index + 1), Some(&"test" | &"tests"))
                    && tokens.get(index + 2) == Some(&"failed"))
    })
}

fn requirement(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for token in lower.split_whitespace() {
        let token = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '#');
        if token.starts_with("req-")
            || token.starts_with("issue-")
            || token.starts_with("task-")
            || token.starts_with('#')
        {
            return Some(token.to_owned());
        }
    }
    None
}

fn language(text: &str) -> &'static str {
    if text.chars().any(|c| ('가'..='힣').contains(&c)) {
        "ko"
    } else {
        "en"
    }
}
fn scrub(text: &str) -> String {
    let text = text
        .split_whitespace()
        .map(|word| {
            if word.starts_with('/') || word.contains("://") {
                "<redacted>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(500)
        .collect()
}

pub fn expand(
    repo: &Repository,
    source_data: &SourceData,
    reference: &str,
    since: DateTime<FixedOffset>,
    until: DateTime<FixedOffset>,
) -> Result<Value, String> {
    let allowed = inspect(repo, source_data, since, until);
    let allowed = allowed["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|finding| {
            finding["evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>();
    if !allowed.contains(reference) {
        return Err("reference is not an emitted claim or evidence reference".into());
    }
    let prefix = format!("r:{}:", &source_data.digest[..16]);
    let rest = reference
        .strip_prefix(&prefix)
        .ok_or("reference is not bound to this source snapshot")?;
    let (file, line) = rest.split_once(':').ok_or("reference format is invalid")?;
    let file: usize = file.parse().map_err(|_| "reference file is invalid")?;
    let line: usize = line.parse().map_err(|_| "reference line is invalid")?;
    let source_file = source_data
        .files
        .iter()
        .find(|f| f.index == file)
        .ok_or("reference file is unavailable")?;
    let bytes = source_file
        .bytes
        .split(|b| *b == b'\n')
        .nth(line.saturating_sub(1))
        .ok_or("reference line is unavailable")?;
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "reference record is malformed")?;
    let body = value.get("payload").unwrap_or(&value);
    let timestamp = string_field(&value, &["timestamp", "created_at", "createdAt", "time"])
        .or_else(|| string_field(body, &["timestamp", "created_at", "createdAt", "time"]))
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok());
    if !in_window(timestamp, since, until) {
        return Err("reference is outside the explicit interval".into());
    }
    Ok(
        json!({ "format": "retrospective-expansion-v1", "reference": reference, "timestamp": timestamp.map(|v| v.to_rfc3339()), "role": visible_payload(body, value.get("type").and_then(Value::as_str).unwrap_or("" )).0, "content": scrub(&visible_payload(body, value.get("type").and_then(Value::as_str).unwrap_or("" )).1) }),
    )
}

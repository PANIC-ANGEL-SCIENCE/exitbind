use super::*;
fn has_extension(record: &Value) -> bool {
    record.get("workAssociationProtocol").is_some() || record.get("workAssociations").is_some()
}

pub(in crate::session_goal) fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

pub(in crate::session_goal) fn valid_work(value: &str) -> bool {
    value.strip_prefix("smw_").is_some_and(valid_sha)
}

pub(in crate::session_goal) fn entries(record: &Value) -> Result<Vec<Value>, String> {
    if !has_extension(record) {
        return Ok(Vec::new());
    }
    if record["workAssociationProtocol"].as_u64() != Some(PROTOCOL) {
        return Err("unsupported context-only Work association protocol".into());
    }
    let values = record["workAssociations"]
        .as_array()
        .ok_or("context-only Work associations are malformed")?;
    if values.is_empty() || values.len() > MAX_ASSOCIATIONS {
        return Err("context-only Work association bound is invalid".into());
    }
    let mut works = std::collections::BTreeSet::new();
    for value in values {
        let object = value
            .as_object()
            .ok_or("context-only Work association entry is malformed")?;
        if object.len() != 3 || object.get("kind").and_then(Value::as_str) != Some("context_only") {
            return Err("context-only Work association entry is malformed".into());
        }
        let work = object
            .get("work")
            .and_then(Value::as_str)
            .filter(|value| valid_work(value))
            .ok_or("context-only Work association has an invalid Work identity")?;
        let start = object
            .get("startEventSha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha(value))
            .ok_or("context-only Work association has an invalid start identity")?;
        if !works.insert(work.to_owned()) || start.trim().is_empty() {
            return Err("context-only Work association identity is duplicated".into());
        }
    }
    Ok(values.clone())
}

pub(in crate::session_goal) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    entries(record)
        .map(|_| ())
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))
}

/// Retain the extension only while ordinary updates continue the same Goal ID.
pub(in crate::session_goal) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] != next["goalId"] || !has_extension(previous) || has_extension(next) {
        return;
    }
    next["workAssociationProtocol"] = previous["workAssociationProtocol"].clone();
    next["workAssociations"] = previous["workAssociations"].clone();
}
pub(in crate::session_goal) fn for_work(record: &Value, work: &str) -> Option<Value> {
    entries(record)
        .ok()?
        .into_iter()
        .find(|entry| entry["work"].as_str() == Some(work))
        .or_else(|| {
            (record[VERIFIED_CONTEXT_FIELD]["work"].as_str() == Some(work))
                .then(|| record[VERIFIED_CONTEXT_FIELD].clone())
        })
}

pub(in crate::session_goal) fn public_record(record: &Value) -> Value {
    let mut public = record.clone();
    if let Some(object) = public.as_object_mut() {
        object.remove(VERIFIED_CONTEXT_FIELD);
    }
    public
}

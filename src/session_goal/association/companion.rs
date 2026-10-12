use super::*;
use serde_json::{json, Value};
use std::io::Write;

const COMPANION_MAX_BYTES: u64 = 4096;

/// Shared immutable sidecar writer for native context and named-Goal scope.
/// Common Work/start binding, size, privacy and digest rules live here.
pub(crate) struct PrivateCompanionWrite<'a> {
    pub(crate) suffix: &'a str,
    pub(crate) label: &'a str,
    pub(crate) kind: &'a str,
    pub(crate) protocol: u64,
    pub(crate) start_event_sha256: &'a str,
    pub(crate) fields: Value,
}

pub(crate) fn write_private_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    companion: PrivateCompanionWrite<'_>,
) -> Result<(), String> {
    let PrivateCompanionWrite {
        suffix,
        label,
        kind,
        protocol,
        start_event_sha256,
        fields,
    } = companion;
    if !valid_work(work) || !valid_sha(start_event_sha256) {
        return Err("Work companion identity is malformed".into());
    }
    let mut value = json!({
        "kind": kind,
        "protocol": protocol,
        "work": work,
        "startEventSha256": start_event_sha256,
    });
    let fields = fields
        .as_object()
        .ok_or("Work companion payload is malformed")?;
    for (key, field) in fields {
        if ["kind", "protocol", "work", "startEventSha256", "sha256"].contains(&key.as_str()) {
            return Err("Work companion payload uses a reserved field".into());
        }
        value
            .as_object_mut()
            .expect("new companion object")
            .insert(key.clone(), field.clone());
    }
    value["sha256"] = json!(crate::evidence::hash::value(&value));
    let bytes = value.to_string();
    if bytes.len() > COMPANION_MAX_BYTES as usize {
        return Err("Work companion exceeds its bounded size".into());
    }
    let relative = companion_path_with_suffix(work, suffix)?;
    let mut file = crate::project::path::secure_create_new(&loaded.state_root, &relative, label)?;
    file.write_all(bytes.as_bytes())
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

/// Shared bounded single-link reader and exact Work/start/digest validation.
/// A missing companion stays distinguishable from an invalid one.
pub(crate) fn read_private_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    suffix: &str,
    label: &str,
    kind: &str,
    protocol: u64,
    expected_fields: &[&str],
) -> Result<Option<Value>, String> {
    use crate::project::path::SecureBytesResult;
    let relative = companion_path_with_suffix(work, suffix)?;
    let bytes = match crate::project::path::secure_bytes_observation_single_link_bounded(
        &loaded.state_root,
        &relative,
        label,
        COMPANION_MAX_BYTES,
    ) {
        SecureBytesResult::Bytes(bytes) => bytes,
        SecureBytesResult::Absent(_) => return Ok(None),
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => {
            return Err(error)
        }
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => return Err(error),
    };
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| format!("{label} is malformed"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} is malformed"))?;
    if object.len() != expected_fields.len() + 5
        || value["kind"].as_str() != Some(kind)
        || value["protocol"].as_u64() != Some(protocol)
        || value["work"].as_str() != Some(work)
        || !value["startEventSha256"].as_str().is_some_and(valid_sha)
        || expected_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(format!("{label} is invalid or unsupported"));
    }
    let digest = value["sha256"]
        .as_str()
        .filter(|digest| valid_sha(digest))
        .ok_or_else(|| format!("{label} digest is invalid"))?;
    let mut unsigned = value.clone();
    unsigned
        .as_object_mut()
        .expect("validated object")
        .remove("sha256");
    if crate::evidence::hash::value(&unsigned) != digest {
        return Err(format!("{label} digest mismatch"));
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work start event is missing")?;
    if start["action"] != "start" || start["eventSha256"] != value["startEventSha256"] {
        return Err(format!("{label} does not match the exact Work start"));
    }
    Ok(Some(value))
}

fn companion_path_with_suffix(work: &str, suffix: &str) -> Result<String, String> {
    if !valid_work(work) {
        return Err("Work identity is malformed".into());
    }
    let token = work.strip_prefix("smw_").expect("validated Work prefix");
    if suffix.is_empty() || suffix.contains('/') || suffix.contains('\\') || suffix.contains('\0') {
        return Err("Work companion suffix is invalid".into());
    }
    Ok(format!(
        "{}/runs/work-{token}{suffix}",
        crate::project::layout_types::state_namespace()
    ))
}

pub(in crate::session_goal) fn validate_companion(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("Work Goal context companion is malformed")?;
    if object.len() != 11
        || value["kind"] != COMPANION_KIND
        || value["protocol"] != 1
        || !value["work"].as_str().is_some_and(valid_work)
        || !value["startEventSha256"].as_str().is_some_and(valid_sha)
        || !value["sessionKey"].as_str().is_some_and(valid_sha)
        || !value["parentGoalId"]
            .as_str()
            .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128)
        || !value["parentObservedGoalEventSha256"]
            .as_str()
            .is_some_and(valid_sha)
        || !(value["ownerProjectId"].is_null() || value["ownerProjectId"].is_string())
        || !value["ownerIdentitySha256"].as_str().is_some_and(valid_sha)
        || !value["host"]
            .as_str()
            .is_some_and(|host| matches!(host, "codex" | "claude"))
    {
        return Err("Work Goal context companion is invalid or unsupported".into());
    }
    Ok(())
}

//! Find unresolved admissions by assignment, independent of producer identity.
//!
//! The caller holds the run ledger lock. An interrupted attempt must not be
//! bypassed by changing the executable, the result, or other binding inputs.

use super::{directory_path, read_record, record_path};
use crate::config::Loaded;
use serde_json::Value;
use std::fs;

pub(super) fn assert_no_unresolved(
    loaded: &Loaded,
    binding: &Value,
    current_id: &str,
    state: &Value,
) -> Result<(), String> {
    super::recovery::assert_predecessor_unchanged(loaded, state, binding)?;
    for attempt in unresolved(loaded, binding)? {
        let id = attempt["attemptId"]
            .as_str()
            .ok_or("missing attempt identity")?;
        if id == current_id {
            continue;
        }
        if super::recovery::permits_predecessor(loaded, state, binding, &attempt)? {
            continue;
        }
        return Err(format!(
            "pre-return attempt {id} has an uncertain outcome; inspect it and do not replay"
        ));
    }
    Ok(())
}

/// Unresolved attempts for the same stable assignment. Exact-attempt replay
/// still belongs to `latest_or_create`, not to this classifier.
pub(super) fn unresolved(loaded: &Loaded, binding: &Value) -> Result<Vec<Value>, String> {
    let mut unresolved = Vec::new();
    let directory = directory_path(loaded);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err("pre-return journal directory is not a regular directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(unresolved),
        Err(error) => return Err(format!("pre-return admissions are unavailable: {error}")),
    }
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => return Err(format!("pre-return admissions are unavailable: {error}")),
    };
    for (index, entry) in entries.enumerate() {
        if index >= 65_536 {
            return Err("pre-return journal exceeds the bounded admission scan".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("pre-return journal filename is not valid UTF-8")?;
        let Some(id) = name
            .strip_prefix("pre-return-")
            .and_then(|name| name.strip_suffix(".attempt.json"))
        else {
            continue;
        };
        let attempt = read_record(&entry.path())?;
        if attempt["version"] != 1
            || attempt["kind"] != "pre_return_attempt"
            || attempt["state"] != "admitted"
            || attempt["attemptId"] != id
            || !attempt["binding"].is_object()
            || !attempt["policy"].is_object()
            || crate::evidence::hash::value(&serde_json::json!({
                "binding": attempt["binding"], "limits": attempt["policy"]
            })) != id
        {
            return Err("pre-return admission identity is invalid".into());
        }
        let saved = &attempt["binding"];
        if ["runId", "ledger", "agent", "role"].iter().any(|field| {
            !saved[*field]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        }) || ["stage", "attempt"]
            .iter()
            .any(|field| saved[*field].as_u64().is_none())
        {
            return Err("pre-return admission assignment identity is invalid".into());
        }
        // A changed artifact, policy, assignment packet, or producer must not
        // make an unfinished execution disappear. A new run or role is a
        // different operation and does not inherit this admission.
        if ["runId", "ledger", "stage", "attempt", "agent", "role"]
            .iter()
            .any(|field| saved.get(*field).is_none() || saved[*field] != binding[*field])
        {
            continue;
        }
        let result_path = record_path(loaded, id, "result")?;
        if !result_path.exists() {
            unresolved.push(attempt);
            continue;
        }
        let result = read_record(&result_path)?;
        if result["attemptId"] != id
            || result["bindingSha256"] != crate::evidence::hash::value(saved)
            || result["policySha256"] != crate::evidence::hash::value(&attempt["policy"])
        {
            return Err("pre-return result identity mismatch".into());
        }
        if matches!(result["state"].as_str(), Some("passed" | "failed")) {
            validate_terminal(loaded, saved, &result)?;
        } else {
            unresolved.push(attempt);
        }
    }
    unresolved.sort_by(|a, b| a["attemptId"].as_str().cmp(&b["attemptId"].as_str()));
    Ok(unresolved)
}

/// A terminal spelling alone is not evidence that this admission ended.
fn validate_terminal(loaded: &Loaded, binding: &Value, result: &Value) -> Result<(), String> {
    let outcome_valid = match result["state"].as_str() {
        Some("passed") => result["exitCode"] == 0 && result["failureKind"].is_null(),
        Some("failed") => match result["exitCode"].as_i64() {
            Some(code) => code != 0 && result["failureKind"] == "nonzero_exit",
            None => result["exitCode"].is_null() && result["failureKind"] == "terminated",
        },
        _ => false,
    };
    if result["version"] != 1
        || result["kind"] != "pre_return_result"
        || !binding["checkerExecutableSha256"]
            .as_str()
            .is_some_and(|value| {
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        || !crate::producer::valid(&binding["producer"])
        || result["producer"]["executableSha256"] != binding["checkerExecutableSha256"]
        || result["producer"]["producer"] != binding["producer"]
        || result.get("exitCode").is_none()
        || result.get("failureKind").is_none()
        || !outcome_valid
    {
        return Err("pre-return result identity mismatch".into());
    }
    for stream in ["stdout", "stderr"] {
        let artifact = result
            .get(stream)
            .ok_or("pre-return result has an incomplete capture")?;
        crate::run::artifact::read_exact(
            loaded,
            artifact,
            "pre-return capture",
            crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
        )?;
    }
    Ok(())
}

//! Find unresolved admissions by assignment, independent of producer identity.
//!
//! The caller holds the run ledger lock. An interrupted attempt must not be
//! bypassed by changing the executable, the result, or other binding inputs.

use super::{
    directory_path, journal::attempt_prefix, read_record, record_path, unavailable_refusal,
};
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
    for attempt in unresolved(loaded, binding, state)? {
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
pub(super) fn attempts(
    loaded: &Loaded,
    state: &Value,
    binding: Option<&Value>,
) -> Result<Vec<Value>, String> {
    let protocol = state[super::ownership::FIELD].as_u64();
    let owned_events = super::ownership::events(state, binding);
    let mut owned = Vec::new();
    for event in &owned_events {
        let id = event["admission"]["attemptId"]
            .as_str()
            .ok_or("missing pre-return admission identity")?;
        let attempt = super::ownership::read_owned(loaded, state, event).map_err(|error| {
            unavailable_refusal(
                loaded,
                state,
                &event["admission"]["assignment"],
                id,
                &format!("owning admission record is unavailable: {error}"),
            )
        })?;
        validate_admission(&attempt, id).map_err(|error| {
            unavailable_refusal(loaded, state, &event["admission"]["assignment"], id, &error)
        })?;
        owned.push(attempt);
    }
    let scoped_prefix = (protocol == Some(super::ownership::PROTOCOL))
        .then(|| attempt_prefix(state))
        .transpose()?;
    let all_owned = super::ownership::events(state, None);

    // Protocol 2 scans only this run's physical namespace to catch an orphan
    // even if its owning ledger event was lost. Earlier starts scan the flat
    // namespace because an unreadable record cannot be attributed elsewhere.
    let mut attempts = Vec::new();
    let directory = directory_path(loaded);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err("pre-return journal directory is not a regular directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(owned),
        Err(error) => return Err(format!("pre-return admissions are unavailable: {error}")),
    }
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => return Err(format!("pre-return admissions are unavailable: {error}")),
    };
    let mut examined = 0usize;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if scoped_prefix
            .as_ref()
            .is_some_and(|prefix| !name.as_encoded_bytes().starts_with(prefix.as_bytes()))
        {
            continue;
        }
        if examined >= 65_536 {
            return Err("pre-return journal exceeds the bounded admission scan".into());
        }
        examined += 1;
        let name = name
            .to_str()
            .ok_or("pre-return journal filename is not valid UTF-8")?;
        let id = if let Some(prefix) = &scoped_prefix {
            let Some(suffix) = name.strip_prefix(prefix.as_str()) else {
                continue;
            };
            match suffix.strip_suffix(".attempt.json") {
                Some(id)
                    if id.len() == 64
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) =>
                {
                    id
                }
                _ => {
                    let problem = "owned admission filename is invalid";
                    return Err(binding.map_or_else(
                        || format!("pre-return {problem}; execution is unknown; do not replay"),
                        |identity| unavailable_refusal(loaded, state, identity, "unknown", problem),
                    ));
                }
            }
        } else {
            let Some(id) = name
                .strip_prefix("pre-return-")
                .and_then(|name| name.strip_suffix(".attempt.json"))
            else {
                continue;
            };
            // Protocol-2 names have their own physical run owner; do not
            // interpret them as protocol-1's flat attempt identity.
            if let Some((run, attempt)) = id.split_once('-') {
                if [run, attempt].iter().all(|value| {
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                }) {
                    if state["runId"] == run {
                        return Err("pre-return admission has an unexpected owner filename".into());
                    }
                    continue;
                }
            }
            id
        };
        let attempt = read_record(&entry.path()).map_err(|error| {
            if scoped_prefix.is_some() {
                if let Some(identity) = binding {
                    return unavailable_refusal(
                        loaded,
                        state,
                        identity,
                        id,
                        &format!("owned admission record is unavailable: {error}"),
                    );
                }
            }
            error
        })?;
        validate_admission(&attempt, id).map_err(|error| {
            if scoped_prefix.is_some() {
                if let Some(identity) = binding {
                    return unavailable_refusal(loaded, state, identity, id, &error);
                }
            }
            error
        })?;
        if scoped_prefix.is_some() {
            let event = all_owned
                .iter()
                .find(|event| event["admission"]["attemptId"] == id);
            let Some(event) = event else {
                if let Some(identity) = binding {
                    return Err(unavailable_refusal(
                        loaded,
                        state,
                        identity,
                        id,
                        "admission is not recorded in its owning ledger",
                    ));
                }
                return Err(format!("pre-return attempt {id}: admission is not recorded in its owning ledger; execution is unknown; preserve the record and do not replay"));
            };
            super::ownership::verify_owned(&attempt, event).map_err(|error| {
                binding.map_or_else(
                    || error.clone(),
                    |identity| unavailable_refusal(loaded, state, identity, id, &error),
                )
            })?;
            continue;
        }
        if protocol == Some(super::ownership::LEGACY_PROTOCOL)
            && binding.is_none()
            && attempt["binding"]["runId"] != state["runId"]
        {
            continue;
        }
        // A changed artifact, policy, assignment packet, or producer must not
        // make an unfinished execution disappear. A new run or role is a
        // different operation and does not inherit this admission.
        if binding.is_some_and(|binding| {
            ["runId", "ledger", "stage", "attempt", "agent", "role"]
                .iter()
                .any(|field| {
                    attempt["binding"].get(*field).is_none()
                        || attempt["binding"][*field] != binding[*field]
                })
        }) {
            continue;
        }
        if protocol == Some(super::ownership::LEGACY_PROTOCOL)
            && !owned_events
                .iter()
                .any(|event| event["admission"]["attemptId"] == id)
        {
            if binding.is_none() {
                return Err(format!("pre-return attempt {id}: admission is not recorded in its owning ledger; execution is unknown; preserve the record and do not replay"));
            }
            return Err(unavailable_refusal(
                loaded,
                state,
                &attempt["binding"],
                id,
                "admission is not recorded in its owning ledger",
            ));
        }
        attempts.push(attempt);
    }
    if scoped_prefix.is_some() {
        Ok(owned)
    } else {
        Ok(attempts)
    }
}

pub(super) fn validate_admission(attempt: &Value, id: &str) -> Result<(), String> {
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
    Ok(())
}

pub(super) fn read_attempt(loaded: &Loaded, state: &Value, id: &str) -> Result<Value, String> {
    let attempt = if state.get(super::ownership::FIELD).is_some() {
        let event = super::ownership::events(state, None)
            .into_iter()
            .find(|event| event["admission"]["attemptId"] == id)
            .ok_or("pre-return admission is not recorded in its owning ledger")?;
        super::ownership::read_owned(loaded, state, event)?
    } else {
        read_record(&record_path(loaded, id, "attempt")?)?
    };
    validate_admission(&attempt, id)?;
    Ok(attempt)
}

pub(super) fn unresolved(
    loaded: &Loaded,
    binding: &Value,
    state: &Value,
) -> Result<Vec<Value>, String> {
    let mut unresolved = Vec::new();
    for attempt in attempts(loaded, state, Some(binding))? {
        let id = attempt["attemptId"]
            .as_str()
            .ok_or("missing attempt identity")?;
        let result_path = record_path(loaded, id, "result")?;
        if !result_path.exists() {
            unresolved.push(attempt);
            continue;
        }
        let result = read_record(&result_path)?;
        if !validate_saved(loaded, &attempt, &result)? {
            unresolved.push(attempt);
        }
    }
    unresolved.sort_by(|a, b| a["attemptId"].as_str().cmp(&b["attemptId"].as_str()));
    Ok(unresolved)
}

/// The same saved-result validation serves predecessor classification and
/// exact-result reuse, including the record that will actually be reused.
pub(super) fn validate_saved(
    loaded: &Loaded,
    attempt: &Value,
    result: &Value,
) -> Result<bool, String> {
    let binding = &attempt["binding"];
    if result["attemptId"] != attempt["attemptId"]
        || result["bindingSha256"] != crate::evidence::hash::value(binding)
        || result["policySha256"] != crate::evidence::hash::value(&attempt["policy"])
    {
        return Err("pre-return result identity mismatch".into());
    }
    match result["state"].as_str() {
        Some("passed" | "failed") => {
            validate_terminal(loaded, binding, result)?;
            Ok(true)
        }
        Some("uncertain") => {
            validate_producer(binding, result)?;
            if !result["failureKind"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            {
                return Err("pre-return result identity mismatch".into());
            }
            let ended = super::execution::known_ended(result);
            if ended {
                validate_captures(loaded, result, false)?;
            }
            Ok(ended)
        }
        _ => Ok(false),
    }
}

/// A terminal spelling alone is not evidence that this admission ended.
fn validate_terminal(loaded: &Loaded, binding: &Value, result: &Value) -> Result<(), String> {
    validate_producer(binding, result)?;
    let outcome_valid = match result["state"].as_str() {
        Some("passed") => result["exitCode"] == 0 && result["failureKind"].is_null(),
        Some("failed") => match result["exitCode"].as_i64() {
            Some(code) => code != 0 && result["failureKind"] == "nonzero_exit",
            None => result["exitCode"].is_null() && result["failureKind"] == "terminated",
        },
        _ => false,
    };
    if result.get("exitCode").is_none() || result.get("failureKind").is_none() || !outcome_valid {
        return Err("pre-return result identity mismatch".into());
    }
    validate_captures(loaded, result, true)
}

fn validate_producer(binding: &Value, result: &Value) -> Result<(), String> {
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
    {
        return Err("pre-return result identity mismatch".into());
    }
    Ok(())
}

fn validate_captures(loaded: &Loaded, result: &Value, required: bool) -> Result<(), String> {
    for stream in ["stdout", "stderr"] {
        let Some(artifact) = result.get(stream) else {
            if required {
                return Err("pre-return result has an incomplete capture".into());
            }
            continue;
        };
        crate::run::artifact::read_exact(
            loaded,
            artifact,
            "pre-return capture",
            crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
        )?;
    }
    Ok(())
}

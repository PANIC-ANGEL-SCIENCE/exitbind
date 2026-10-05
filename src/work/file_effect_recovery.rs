//! Reconciliation belongs to the existing file effect owner. An exact admitted
//! request may finish after revocation, but cannot change its target or bytes.
use super::{durable_write, owner_binding, replacement_mode, result, WriteRequest};
use crate::{config::Loaded, evidence::hash, project::path};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn reconcile(
    loaded: &Loaded,
    request: &WriteRequest<'_>,
    content: &str,
    parameters: &str,
    previous: &str,
    root: &Path,
    journal: &Path,
) -> Result<Value, String> {
    let mut record: Value =
        serde_json::from_str(previous).map_err(|_| "file effect journal is malformed")?;
    if record["version"] != 1
        || record["parametersSha256"] != parameters
        || record["work"] != request.work
        || record["assignment"] != request.assignment
        || record["operation"] != request.operation
        || record["path"] != request.path
        || record["resultSha256"] != hash::text(content)
        || !matches!(record["status"].as_str(), Some("admitted" | "completed"))
    {
        return Err("file operation identity conflicts with its recorded request".into());
    }
    let completed = record["status"] == "completed";
    if record.get("ownerSha256").is_some() {
        if record["ownerSha256"] != owner_binding(loaded)? {
            return Err(
                "recorded effect governing instance changed; automatic reconciliation is refused"
                    .into(),
            );
        }
    } else if !completed {
        return Err("legacy admitted effect lacks a governing instance binding; explicit inspection is required".into());
    }
    let now = match path::secure_bytes_observation_single_link_bounded(
        root,
        request.path,
        "file reconciliation",
        super::MAX_BYTES,
    ) {
        path::SecureBytesResult::Bytes(bytes) => {
            Some(String::from_utf8(bytes).map_err(|_| "file reconciliation target must be UTF-8")?)
        }
        path::SecureBytesResult::Absent(_) => None,
        _ => return Err("file reconciliation target is unavailable or unsafe".into()),
    };
    let observed = now
        .as_deref()
        .map(hash::text)
        .unwrap_or_else(|| "absent".into());
    let already_written = observed == hash::text(content);
    if completed && !already_written {
        return Err("recorded file result changed; replay cannot overwrite it".into());
    }
    if !already_written {
        if observed != request.expected {
            return Err(
                "admitted file result is ambiguous; recovery cannot overwrite changed bytes".into(),
            );
        }
        if !crate::project::portability::writable(root) {
            return Err("unsupported file reconciliation: project source is read-only; admitted intent is retained".into());
        }
        let target = root.join(request.path);
        let mode = replacement_mode(&target, now.is_some())?;
        durable_write(&target, content, mode, now.as_deref(), root)?;
    }
    if !completed {
        record["status"] = json!("completed");
        durable_write(
            journal,
            &format!("{record}\n"),
            Some(0o600),
            Some(previous),
            &loaded.state_root,
        )?;
    }
    let mut response = result(request, parameters, already_written);
    response["recovered"] = json!(!completed);
    Ok(response)
}

//! Typed run-artifact evidence and immutable-byte revalidation.

use crate::{config, config::Loaded};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

const VERIFY_BUFFER_BYTES: usize = 64 * 1024;

pub(crate) struct VerifiedArtifact {
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    pub(crate) preview: Vec<u8>,
}

pub(crate) fn evidence(
    loaded: &Loaded,
    requested_root: Option<&str>,
    requested: &str,
) -> Result<Value, String> {
    let (root_name, root) = match requested_root.unwrap_or("product") {
        "product" => ("product", &loaded.product_root),
        "state" => ("state", &loaded.state_root),
        _ => return Err("--artifact-root must be product or state".into()),
    };
    let path = confined(root, requested)?;
    if fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(format!("artifact must not be a symlink: {requested}"));
    }
    let real = config::file(root, requested)?;
    if root_name == "state" {
        let targets = [real.as_path()];
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    }
    let verified = verify_file(&real, 0).map_err(|error| error.to_string())?;
    Ok(json!({
        "root": root_name,
        "path": config::rel(root, &real)?,
        "sha256": verified.sha256,
        "bytes": verified.bytes,
    }))
}

pub(crate) fn assert_current(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if let Some(path) = drift_warning(loaded, state)? {
        return Err(format!(
            "artifact drift detected: {}",
            path["artifact"].as_str().unwrap_or("unknown")
        ));
    }
    Ok(())
}

/// Return an ordinary artifact drift as a warning while preserving strict
/// failures for malformed records, unsafe paths, and symlinks.
pub(crate) fn drift_warning(loaded: &Loaded, state: &Value) -> Result<Option<Value>, String> {
    let mut drift = None;
    for item in state["submissions"].as_array().unwrap_or(&Vec::new()) {
        if !verify_submission(loaded, item)? {
            drift.get_or_insert(
                item["artifact"]["path"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
            );
        }
    }
    for check in state["checks"].as_array().unwrap_or(&Vec::new()) {
        for stream in ["stdout", "stderr"] {
            if let Some(artifact) = check.get(stream) {
                if let Err(error) = read(loaded, artifact, "check log") {
                    if is_missing(&error) {
                        drift.get_or_insert(format!("{stream} log"));
                    } else {
                        return Err(error);
                    }
                }
            }
        }
    }
    Ok(drift.map(|artifact| {
        json!({
            "error": "artifact drift detected after run start",
            "classification": "artifact_drift",
            "artifact": artifact,
        })
    }))
}

fn verify_submission(loaded: &Loaded, item: &Value) -> Result<bool, String> {
    let root = match item["artifact"]["root"].as_str().unwrap_or("product") {
        "product" => &loaded.product_root,
        "state" => &loaded.state_root,
        _ => return Err("artifact root is invalid".into()),
    };
    let requested = item["artifact"]["path"]
        .as_str()
        .ok_or("artifact path is invalid")?;
    let path = match confined(root, requested) {
        Ok(path) => path,
        Err(error) if error.starts_with("declared file does not exist") => return Ok(false),
        Err(error) => return Err(error),
    };
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("artifact must not be a symlink: {requested}"));
    }
    let real = config::file(root, requested)?;
    if config::rel(root, &real)? != requested {
        return Err(format!("artifact path changed: {requested}"));
    }
    let verified = match verify_file(&real, 0) {
        Ok(verified) => verified,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    let bytes_match = artifact_bytes_match(&item["artifact"], verified.bytes)?;
    Ok(bytes_match && item["artifact"]["sha256"] == verified.sha256)
}

fn artifact_bytes_match(artifact: &Value, actual: u64) -> Result<bool, String> {
    match artifact.get("bytes") {
        // Older ledgers predate byte counts. Their hash remains the historical
        // integrity check; current evidence always records this field.
        None => Ok(true),
        Some(value) => Ok(value.as_u64().ok_or("artifact byte count is invalid")? == actual),
    }
}

fn is_missing(error: &str) -> bool {
    error.contains("No such file or directory") || error.starts_with("declared file does not exist")
}

pub(crate) fn read(
    loaded: &Loaded,
    artifact: &Value,
    label: &str,
) -> Result<VerifiedArtifact, String> {
    if artifact["root"] != "state" {
        return Err(format!("{label} artifact root is invalid"));
    }
    let requested = artifact["path"]
        .as_str()
        .ok_or_else(|| format!("{label} artifact path is invalid"))?;
    let path = confined(&loaded.state_root, requested)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} artifact is not a regular file"));
    }
    let real = config::file(&loaded.state_root, requested)?;
    if config::rel(&loaded.state_root, &real)? != requested {
        return Err(format!("{label} artifact path changed"));
    }
    let verified = verify_file(&real, 64 * 1024).map_err(|error| error.to_string())?;
    let bytes_match = artifact_bytes_match(artifact, verified.bytes)?;
    if !bytes_match || artifact["sha256"] != verified.sha256 {
        return Err(format!("{label} artifact bytes changed"));
    }
    Ok(verified)
}

fn verify_file(path: &Path, preview_limit: usize) -> std::io::Result<VerifiedArtifact> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut preview = Vec::with_capacity(preview_limit.min(VERIFY_BUFFER_BYTES));
    let mut buffer = [0u8; VERIFY_BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("artifact byte count overflowed"))?;
        let remaining = preview_limit.saturating_sub(preview.len());
        preview.extend_from_slice(&buffer[..count.min(remaining)]);
    }
    Ok(VerifiedArtifact {
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
        preview,
    })
}

fn confined(root: &Path, requested: &str) -> Result<std::path::PathBuf, String> {
    let portable = requested.replace('\\', "/");
    if requested.trim().is_empty()
        || requested.contains('\0')
        || Path::new(requested).is_absolute()
        || portable.starts_with('/')
        || portable.contains(":/")
        || portable == ".."
        || portable.starts_with("../")
    {
        return Err(format!("path escapes project root: {requested}"));
    }
    let path = root.join(requested);
    let parent = path.parent().ok_or("path escapes project root")?;
    let real_parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
    if !real_parent.starts_with(root) {
        return Err(format!("path escapes project root: {requested}"));
    }
    if !path.exists() {
        return Err(format!("declared file does not exist: {requested}"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::artifact_bytes_match;
    use serde_json::json;

    #[test]
    fn historical_missing_bytes_are_allowed_but_malformed_bytes_refuse() {
        assert!(artifact_bytes_match(&json!({"sha256": "hash"}), 4).unwrap());
        assert!(artifact_bytes_match(&json!({"bytes": 4}), 4).unwrap());
        assert!(!artifact_bytes_match(&json!({"bytes": 3}), 4).unwrap());
        assert!(artifact_bytes_match(&json!({"bytes": "4"}), 4).is_err());
    }
}

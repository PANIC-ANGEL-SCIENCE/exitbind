use super::hash;
use crate::{config::Loaded, project::path as project_path};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) const CANONICAL_PATH: &str = "soulmate/harness/harness-manifest.json";
const LEGACY_PATH: &str = "harness-manifest.json";
pub(crate) fn canonical_path(loaded: &Loaded) -> &'static str {
    if loaded.path.file_name().and_then(|name| name.to_str()) == Some("exitbind.json") {
        "exitbind/harness/harness-manifest.json"
    } else {
        CANONICAL_PATH
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    version: u64,
    project: Project,
    harness: Harness,
    activations: Vec<Activation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Project {
    id: String,
    session: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Harness {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    kind: Kind,
    name: String,
    evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification: Option<Verification>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Verification {
    verifier: String,
    #[serde(rename = "artifactSha256")]
    artifact_sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Skill,
    Perspective,
    Ponytail,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    Configured,
    Presented,
    AgentDeclared,
    HookObserved,
    /// A separate verifier's off-box claim; Soulmate validates and binds the
    /// claim but does not hash a local artifact or authenticate the verifier.
    IndependentlyVerified,
}

pub(crate) fn load(loaded: &Loaded, requested: &str) -> Result<Value, String> {
    if !supported_path(loaded, requested) {
        let canonical = canonical_path(loaded);
        return Err(format!(
            "harness manifest must be {canonical} or the legacy path {LEGACY_PATH}"
        ));
    }
    crate::config::file(&loaded.control_root, requested)?;
    let bytes = project_path::secure_bytes(&loaded.control_root, requested, "harness manifest")?;
    if bytes.len() > 64 * 1024 {
        return Err("harness manifest exceeds the 65536-byte limit".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid harness manifest JSON: {error}"))?;
    validate(&manifest, loaded)?;
    recorded(&manifest, &bytes, requested)
}

pub(crate) fn verify(loaded: &Loaded, recorded: &Value) -> Result<bool, String> {
    let object = recorded
        .as_object()
        .filter(|object| object.len() == 7)
        .ok_or("unsupported or malformed harness receipt")?;
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| supported_path(loaded, path))
        .ok_or("unsupported or malformed harness receipt")?;

    match crate::config::file(&loaded.control_root, path) {
        Ok(_) => {}
        Err(error) if error.starts_with("declared file does not exist:") => return Ok(false),
        Err(error) => return Err(error),
    }
    Ok(load(loaded, path)? == *recorded)
}

/// Read and validate the exact raw manifest named by a receipt. The returned
/// value is intentionally kept in memory by the native away adapter; it is
/// never copied into a receipt or recovery state.
pub(crate) fn raw_for_receipt(loaded: &Loaded, recorded: &Value) -> Result<String, String> {
    let object = recorded
        .as_object()
        .filter(|object| object.len() == 7)
        .ok_or("unsupported or malformed harness receipt")?;
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| supported_path(loaded, path))
        .ok_or("unsupported or malformed harness receipt")?;
    let expected = object
        .get("manifestSha256")
        .and_then(Value::as_str)
        .filter(|hash| valid_sha256(hash))
        .ok_or("unsupported or malformed harness receipt")?;
    let bytes = project_path::secure_bytes(&loaded.control_root, path, "harness manifest")?;
    if hash::bytes(&bytes) != expected {
        return Err("harness manifest changed since receipt creation".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid harness manifest JSON: {error}"))?;
    validate(&manifest, loaded)?;
    String::from_utf8(bytes).map_err(|_| "harness manifest must be UTF-8".to_owned())
}

fn recorded(manifest: &Manifest, bytes: &[u8], path: &str) -> Result<Value, String> {
    let activations = manifest
        .activations
        .iter()
        .map(|activation| {
            let mut value = json!({
                "kind": activation.kind,
                "nameSha256": hash::text(&activation.name),
                "evidence": activation.evidence,
            });
            if let Some(verification) = &activation.verification {
                value["verification"] = json!({
                    "verifierSha256": hash::text(&verification.verifier),
                    "artifactSha256": verification.artifact_sha256,
                });
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "path": path,
        "manifestSha256": hash::bytes(bytes),
        "manifestVersion": manifest.version,
        "project": {
            "idSha256": hash::text(&manifest.project.id),
            "sessionSha256": hash::text(&manifest.project.session),
        },
        "harness": {
            "nameSha256": hash::text(&manifest.harness.name),
            "versionSha256": hash::text(&manifest.harness.version),
        },
        "activations": activations,
        "privacy": "raw-manifest-values-omitted",
    }))
}

fn supported_path(loaded: &Loaded, path: &str) -> bool {
    matches!(path, LEGACY_PATH) || path == canonical_path(loaded) || path == CANONICAL_PATH
}

fn validate(manifest: &Manifest, loaded: &Loaded) -> Result<(), String> {
    if manifest.version != 1 {
        return Err("harness manifest version must be 1".into());
    }
    for (label, value) in [
        ("project.id", manifest.project.id.as_str()),
        ("project.session", manifest.project.session.as_str()),
        ("harness.name", manifest.harness.name.as_str()),
        ("harness.version", manifest.harness.version.as_str()),
    ] {
        if !valid_token(value) {
            return Err(format!("harness manifest {label} must be a portable token"));
        }
    }
    if loaded
        .project_id
        .as_deref()
        .is_some_and(|id| id != manifest.project.id)
    {
        return Err("harness manifest project.id does not match configured project.id".into());
    }
    if manifest.activations.is_empty() || manifest.activations.len() > 64 {
        return Err("harness manifest activations must contain 1 to 64 entries".into());
    }
    for activation in &manifest.activations {
        if !valid_token(&activation.name) {
            return Err("harness activation name must be a portable token".into());
        }
        match (&activation.evidence, &activation.verification) {
            (Evidence::IndependentlyVerified, Some(verification)) => {
                if !valid_token(&verification.verifier) {
                    return Err(
                        "independently_verified evidence requires a portable verifier".into(),
                    );
                }
                if !valid_sha256(&verification.artifact_sha256) {
                    return Err("artifactSha256 must be 64 lowercase hexadecimal characters".into());
                }
            }
            (Evidence::IndependentlyVerified, None) => {
                return Err("independently_verified evidence requires verification".into())
            }
            (_, Some(_)) => {
                return Err(
                    "verification is allowed only for independently_verified evidence".into(),
                )
            }
            (_, None) => {}
        }
    }
    Ok(())
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'@' | b'+')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

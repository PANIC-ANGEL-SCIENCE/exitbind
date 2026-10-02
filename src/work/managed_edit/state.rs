//! Private, bounded assignment binding and read baselines.
use crate::{config::Loaded, evidence::hash, project::path};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub(super) const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub version: u32,
    pub work: String,
    pub assignment: String,
    pub executable: String,
    pub executable_sha256: String,
    pub config: String,
    pub config_sha256: String,
    pub product_root: String,
    pub control_root: String,
    pub state_root: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reads {
    pub version: u32,
    pub files: BTreeMap<String, Baseline>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    pub generation: u64,
    /// None records an actual absent-file observation, not an unreadable file.
    pub content: Option<String>,
    pub expected_sha256: String,
    pub request: Option<Submission>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Submission {
    pub content_sha256: String,
    pub status: Status,
    pub refusal: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Submitted,
    Completed,
    Refused,
}

pub(super) fn expected(baseline: &Baseline) -> String {
    baseline.expected_sha256.clone()
}

pub(super) fn operation(session: &str, file: &str, baseline: &Baseline) -> String {
    hash::value(&serde_json::json!([session, file, baseline.generation]))
}

pub(super) fn relative(session: &str, leaf: &str) -> Result<String, String> {
    if !digest(session) {
        return Err("managed edit session is invalid; use the assignment's supplied tool".into());
    }
    Ok(format!(
        "{}/managed-edits/{session}/{leaf}",
        crate::project::layout_types::state_namespace()
    ))
}

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub(super) fn observe(
    root: &std::path::Path,
    name: &str,
    max: u64,
) -> Result<Option<Vec<u8>>, String> {
    match path::secure_bytes_observation_bounded(root, name, "managed edit state", max) {
        path::SecureBytesResult::Bytes(bytes) => Ok(Some(bytes)),
        path::SecureBytesResult::Absent(_) => Ok(None),
        _ => Err("managed edit state is unreadable or unsafe; no edit admitted".into()),
    }
}

pub(super) fn binding(loaded: &Loaded, work: &str, assignment: &str) -> Result<Binding, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = std::fs::canonicalize(executable).map_err(|e| e.to_string())?;
    let executable_sha256 = hash::bytes(&std::fs::read(&executable).map_err(|e| e.to_string())?);
    Ok(Binding {
        version: 1,
        work: work.into(),
        assignment: assignment.into(),
        executable: text(executable)?,
        executable_sha256,
        config: text(std::fs::canonicalize(&loaded.path).map_err(|e| e.to_string())?)?,
        config_sha256: hash::text(&loaded.source),
        product_root: text(loaded.product_root.clone())?,
        control_root: text(loaded.control_root.clone())?,
        state_root: text(loaded.state_root.clone())?,
    })
}

/// Immutable preparation anchor beside the canonical Work ledger, independent
/// of the disposable session directory. Caller holds the canonical Work lock.
pub(super) fn preparation_path(
    loaded: &Loaded,
    ledger: &crate::run::ledger::LedgerPath,
    session: &str,
) -> Result<String, String> {
    if !digest(session) {
        return Err("managed edit session is invalid".into());
    }
    let relative = ledger
        .path
        .strip_prefix(&loaded.state_root)
        .map_err(|_| "managed preparation escapes StateRoot")?
        .to_str()
        .ok_or("managed preparation path is not UTF-8")?;
    Ok(format!("{relative}.managed-{session}.json"))
}

pub(super) fn preparation(
    loaded: &Loaded,
    ledger: &crate::run::ledger::LedgerPath,
    session: &str,
    binding: &Binding,
) -> Result<bool, String> {
    let Some(bytes) = observe(
        &loaded.state_root,
        &preparation_path(loaded, ledger, session)?,
        16 * 1024,
    )?
    else {
        return Ok(false);
    };
    let saved: Binding = serde_json::from_slice(&bytes)
        .map_err(|_| "managed preparation registry is corrupt; no session reconstructed")?;
    if &saved != binding {
        return Err(
            "managed preparation registry binding changed; no session reconstructed".into(),
        );
    }
    Ok(true)
}

pub(super) fn record_preparation(
    loaded: &Loaded,
    ledger: &crate::run::ledger::LedgerPath,
    session: &str,
    binding: &Binding,
) -> Result<(), String> {
    use std::io::Write;
    let target = loaded
        .state_root
        .join(preparation_path(loaded, ledger, session)?);
    let parent = target
        .parent()
        .ok_or("managed preparation parent missing")?;
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(&target).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(binding).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    // Refuse on failed durability, leaving the anchor unavailable rather than
    // removing it and mistaking an interrupted preparation for first use.
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| e.to_string())
}

fn text(path: PathBuf) -> Result<String, String> {
    path.into_os_string()
        .into_string()
        .map_err(|_| "managed edit binding path is not UTF-8".into())
}

pub(super) fn load_binding(loaded: &Loaded, session: &str) -> Result<Binding, String> {
    let bytes = observe(
        &loaded.state_root,
        &relative(session, "binding.json")?,
        16 * 1024,
    )?
    .ok_or("managed edit binding is missing; do not reconstruct it")?;
    let saved: Binding =
        serde_json::from_slice(&bytes).map_err(|_| "managed edit binding is corrupt")?;
    if hash::value(&serde_json::json!([saved.work, saved.assignment])) != session
        || saved != binding(loaded, &saved.work, &saved.assignment)?
    {
        return Err("managed edit executable, configuration or assignment binding changed; no edit admitted".into());
    }
    Ok(saved)
}

pub(super) fn load_reads(loaded: &Loaded, session: &str) -> Result<(Reads, String), String> {
    let bytes = observe(
        &loaded.state_root,
        &relative(session, "reads.json")?,
        MAX_STATE_BYTES,
    )?
    .ok_or("managed read state is missing; do not recreate it or retry blindly")?;
    let reads: Reads =
        serde_json::from_slice(&bytes).map_err(|_| "managed read state is corrupt")?;
    if reads.version != 1
        || reads.files.len() > 64
        || reads.files.iter().any(|(file, b)| {
            super::file_path(file).is_err()
                || b.generation == 0
                || b.expected_sha256
                    != b.content
                        .as_deref()
                        .map(hash::text)
                        .unwrap_or_else(|| "absent".into())
                || b.content
                    .as_ref()
                    .is_some_and(|s| s.len() as u64 > super::file_effect::MAX_BYTES)
                || b.request.as_ref().is_some_and(|r| {
                    !digest(&r.content_sha256)
                        || (r.status == Status::Refused) != r.refusal.is_some()
                        || r.refusal.as_ref().is_some_and(|s| s.len() > 4096)
                })
        })
    {
        return Err("managed read state has invalid fields; no edit admitted".into());
    }
    let original = String::from_utf8(bytes).map_err(|_| "managed read state is not UTF-8")?;
    Ok((reads, original))
}

pub(super) fn save_reads(
    loaded: &Loaded,
    session: &str,
    reads: &Reads,
    prior: &str,
) -> Result<(), String> {
    let bytes = serde_json::to_string(reads).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_STATE_BYTES || reads.files.len() > 64 {
        return Err("managed read storage bound reached; no edit admitted".into());
    }
    crate::host::settings::atomic_write(
        &loaded.state_root.join(relative(session, "reads.json")?),
        &bytes,
        Some(0o600),
        Some(prior),
        &loaded.state_root,
    )
}

use crate::{config, evidence::hash, host::codegraph, session_goal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const FILE: &str = "recovery-hint.json";
pub(super) const MAX_BYTES: u64 = 16 * 1024;
pub(super) const MAX_TEXT: usize = 4096;
pub(super) const MAX_ITEMS: usize = 8;
pub(super) const HOOK_BUDGET: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Hint {
    pub(super) kind: String,
    pub(super) protocol: u64,
    pub(super) host: String,
    pub(super) session_key: String,
    pub(super) owner_config_path: String,
    pub(super) owner_config_sha256: String,
    pub(super) owner_project_id: Option<String>,
    pub(super) owner_identity_sha256: String,
    pub(super) goal_id: String,
    pub(super) goal_revision: u64,
    pub(super) goal_event_sha256: String,
    pub(super) current_step: String,
    pub(super) current_work: WorkSelection,
    pub(super) latest_corrections: Vec<String>,
    /// The Work each saved Lead decision was bound to when it was recorded,
    /// by position in `latest_corrections`; empty when none is bound. The
    /// Lead act on that Work records the decision and removes it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) latest_correction_works: Vec<Option<String>>,
    pub(super) evidence_refs: Vec<Reference>,
    pub(super) work_refs: Vec<WorkRef>,
    pub(super) pending_operation: Option<PendingOperation>,
    pub(super) guidance_refs: Vec<Reference>,
    pub(super) last_checkpoint: Option<Checkpoint>,
    /// The hashed identity of the session that wrote this record when it is
    /// not the journal's own session: a session that adopted the journal.
    /// Two sessions sharing one journal share one record; the last writer
    /// wins and is named here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) recorded_by_session_key: Option<String>,
    pub(super) sha256: String,
}

impl Hint {
    /// The hashed identity of the session that wrote this record.
    pub(super) fn recorded_by(&self) -> &str {
        self.recorded_by_session_key
            .as_deref()
            .unwrap_or(&self.session_key)
    }

    /// Whether this record belongs to `session`'s journal, its fixed owner
    /// and the Goal `goal_id`.
    pub(super) fn belongs_to(
        &self,
        session: &session_goal::native::Session,
        owner: &session_goal::native::Owner,
        goal_id: &Value,
    ) -> bool {
        self.host == session.host
            && self.session_key == session.key
            && self.owner_identity_sha256 == owner.identity_sha256
            && self.owner_project_id == owner.project_id
            && *goal_id == self.goal_id.as_str()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Checkpoint {
    pub(super) event: String,
    pub(super) goal_revision: u64,
    pub(super) goal_event_sha256: String,
}

pub(super) fn checkpoint_is_ambiguous_older(previous: &Checkpoint, next: &Checkpoint) -> bool {
    previous.event == "PostCompact"
        && next.event == "PreCompact"
        && previous.goal_revision == next.goal_revision
        && previous.goal_event_sha256 == next.goal_event_sha256
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WorkSelection {
    pub(super) state: String,
    pub(super) work: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkRef {
    pub(crate) work: String,
    pub(crate) config_path: String,
    pub(crate) config_sha256: String,
    pub(crate) start_event_sha256: String,
    pub(crate) relation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Reference {
    pub(super) label: String,
    pub(super) root: String,
    pub(super) path: String,
    /// Optional exact Goal-member Work whose validated configuration supplies
    /// the product/control/state roots for this reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) work: Option<String>,
    pub(super) sha256: String,
    pub(super) producer_executable_sha256: String,
    pub(super) consumption: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PendingOperation {
    GoalStatus {
        description: String,
    },
    WorkDetail {
        description: String,
        work: String,
    },
    EvidenceFile {
        description: String,
        handle: String,
        root: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        work: Option<String>,
        sha256: String,
        bytes: u64,
    },
}

pub(super) fn current_executable_sha256() -> Result<String, String> {
    crate::producer::build_identity()["executableSha256"]
        .as_str()
        .filter(|value| valid_sha(value))
        .map(str::to_owned)
        .ok_or_else(|| "current executable identity is unavailable".into())
}

pub(super) fn current_executable_path() -> Result<String, String> {
    crate::emitted::canonical_executable()
}

pub(super) fn source_root<'a>(loaded: &'a config::Loaded, name: &str) -> Result<&'a Path, String> {
    match name {
        "product" => Ok(&loaded.product_root),
        "control" => Ok(&loaded.control_root),
        "state" => Ok(&loaded.state_root),
        _ => Err("recovery reference root must be product, control, or state".into()),
    }
}

pub(super) fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// One line of at most `max` bytes with no surrounding space; a refusal
/// states the limit and what the value broke.
pub(super) fn bounded_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    let broken = if value.trim().is_empty() {
        "it is empty".to_owned()
    } else if value.len() > max {
        format!("it has {} bytes", value.len())
    } else if value.chars().any(char::is_control) {
        "it has a line break or control character".to_owned()
    } else if value.trim() != value {
        "it starts or ends with a space".to_owned()
    } else {
        return Ok(());
    };
    Err(format!(
        "session recovery {field} must be one line of 1 to {max} bytes with no leading or trailing space; {broken}"
    ))
}

pub(super) fn safe_relative_path(value: &str) -> bool {
    if value.is_empty() || value.contains('\\') || Path::new(value).is_absolute() {
        return false;
    }
    let parts = Path::new(value).components().collect::<Vec<_>>();
    if !parts
        .iter()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return false;
    }
    let Some(normalized) = parts
        .iter()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    normalized.join("/") == value
}

pub(super) fn canonical_path(path: &Path, label: &str) -> Result<String, String> {
    fs::canonicalize(path)
        .map_err(|error| format!("{label} cannot be canonicalized: {error}"))?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} is not valid UTF-8"))
}

pub(super) fn hint_path(root: &Path) -> PathBuf {
    root.join(FILE)
}

pub(super) fn unseal(mut value: Hint) -> Result<Hint, String> {
    let claimed = value.sha256.clone();
    value.sha256.clear();
    let body = serde_json::to_value(&value).map_err(|error| error.to_string())?;
    if !valid_sha(&claimed) || hash::value(&body) != claimed {
        return Err("session recovery hint integrity check failed".into());
    }
    value.sha256 = claimed;
    Ok(value)
}

pub(super) fn validate_hint(hint: &Hint) -> Result<(), String> {
    if hint.kind != "exitbind_session_recovery_hint"
        || hint.protocol != 2
        || !matches!(hint.host.as_str(), "codex" | "claude")
        || !valid_sha(&hint.session_key)
        || !valid_sha(&hint.owner_config_sha256)
        || !valid_sha(&hint.owner_identity_sha256)
        || !valid_sha(&hint.goal_event_sha256)
        || !Path::new(&hint.owner_config_path).is_absolute()
        || hint.goal_id.trim().is_empty()
        || hint.goal_id.len() > 128
        || hint.goal_revision == 0
        || hint.latest_corrections.len() > MAX_ITEMS
        || hint.evidence_refs.len() > MAX_ITEMS
        || hint.work_refs.len() > MAX_ITEMS
        || hint.guidance_refs.len() > MAX_ITEMS
        || hint
            .recorded_by_session_key
            .as_deref()
            .is_some_and(|key| !valid_sha(key) || key == hint.session_key)
    {
        return Err("native session recovery hint has invalid bounded fields".into());
    }
    bounded_text(&hint.owner_config_path, "owner configuration path", 4096)?;
    bounded_text(&hint.current_step, "current step", MAX_TEXT)?;
    let selection_ok = match hint.current_work.state.as_str() {
        "unresolved" | "not_selected" => hint.current_work.work.is_none(),
        "known" => hint
            .current_work
            .work
            .as_deref()
            .is_some_and(|work| !work.trim().is_empty()),
        _ => false,
    };
    if !selection_ok {
        return Err(
            "native session recovery Work locator must be explicitly known or unresolved".into(),
        );
    }
    if let Some(work) = hint.current_work.work.as_deref() {
        if !hint
            .work_refs
            .iter()
            .any(|reference| reference.work == work)
        {
            return Err(
                "known currentWork must match an exact retained Goal-member reference".into(),
            );
        }
    }
    for item in &hint.latest_corrections {
        bounded_text(item, "correction", 512)?;
    }
    if !hint.latest_correction_works.is_empty()
        && hint.latest_correction_works.len() != hint.latest_corrections.len()
    {
        return Err(
            "native session recovery decision Works must name one Work per decision".into(),
        );
    }
    for work in hint.latest_correction_works.iter().flatten() {
        bounded_text(work, "decision Work locator", 128)?;
    }
    for item in hint.evidence_refs.iter().chain(&hint.guidance_refs) {
        bounded_text(&item.label, "reference label", 128)?;
        bounded_text(&item.path, "reference path", 512)?;
        if let Some(work) = item.work.as_deref() {
            bounded_text(work, "reference Work locator", 128)?;
        }
        let caller_read = item.consumption == "caller_reported_read"
            && matches!(item.root.as_str(), "product" | "control" | "state");
        let generated_result = item.consumption == "generated_query_result"
            && hint
                .evidence_refs
                .iter()
                .any(|reference| std::ptr::eq(reference, item))
            && item.root == "state"
            && codegraph::valid_artifact_path(&item.path)
            && item.work.is_some()
            && item.label.starts_with("CodeGraph:");
        if !valid_sha(&item.sha256)
            || !valid_sha(&item.producer_executable_sha256)
            || (!caller_read && !generated_result)
            || !safe_relative_path(&item.path)
        {
            return Err("session recovery reference lacks a safe source binding".into());
        }
        if item.work.as_deref().is_some_and(|work| {
            !hint
                .work_refs
                .iter()
                .any(|reference| reference.work == work)
        }) {
            return Err(
                "session recovery reference Work must be an exact retained Goal member".into(),
            );
        }
    }
    if let Some(checkpoint) = &hint.last_checkpoint {
        if !matches!(checkpoint.event.as_str(), "PreCompact" | "PostCompact")
            || !valid_sha(&checkpoint.goal_event_sha256)
        {
            return Err("native session recovery checkpoint is malformed".into());
        }
    }
    if let Some(pending) = &hint.pending_operation {
        match pending {
            PendingOperation::GoalStatus { description } => {
                bounded_text(description, "pending operation", 512)?
            }
            PendingOperation::WorkDetail { description, work } => {
                bounded_text(description, "pending operation", 512)?;
                bounded_text(work, "pending Work", 128)?;
            }
            PendingOperation::EvidenceFile {
                description,
                handle,
                root,
                path,
                work,
                sha256,
                bytes,
            } => {
                bounded_text(description, "pending operation", 512)?;
                bounded_text(handle, "pending evidence handle", 128)?;
                bounded_text(path, "pending evidence path", 512)?;
                if let Some(work) = work.as_deref() {
                    bounded_text(work, "pending evidence Work locator", 128)?;
                }
                if !matches!(root.as_str(), "product" | "control" | "state")
                    || !valid_sha(sha256)
                    || *bytes > 256 * 1024
                    || !safe_relative_path(path)
                {
                    return Err("pending evidence reference is unsafe or unbounded".into());
                }
                if work.as_deref().is_some_and(|work| {
                    !hint
                        .work_refs
                        .iter()
                        .any(|reference| reference.work == work)
                }) {
                    return Err(
                        "pending evidence Work must be an exact retained Goal member".into(),
                    );
                }
            }
        }
        if let PendingOperation::WorkDetail { work, .. } = pending {
            if !hint
                .work_refs
                .iter()
                .any(|reference| &reference.work == work)
            {
                return Err(
                    "pending Work detail must match an exact retained Goal-member reference".into(),
                );
            }
        }
    }
    for item in &hint.work_refs {
        bounded_text(&item.work, "Work locator", 128)?;
        bounded_text(&item.config_path, "Work configuration path", 4096)?;
        if !Path::new(&item.config_path).is_absolute()
            || !valid_sha(&item.config_sha256)
            || !valid_sha(&item.start_event_sha256)
            || item.relation != "member"
        {
            return Err("session recovery Work reference is malformed".into());
        }
    }
    Ok(())
}

pub(super) fn read_hint(root: &Path) -> Result<Option<Hint>, String> {
    use crate::project::path::SecureBytesResult;
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        root,
        FILE,
        "native session recovery hint",
        MAX_BYTES,
    ) {
        SecureBytesResult::Absent(_) => Ok(None),
        SecureBytesResult::Bytes(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "native session recovery hint is malformed JSON")?;
            let hint: Hint = serde_json::from_value(value)
                .map_err(|_| "native session recovery hint has an unsupported shape")?;
            let hint = unseal(hint)?;
            validate_hint(&hint)?;
            Ok(Some(hint))
        }
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => Err(error),
    }
}

pub(super) fn write_hint_locked(
    session: &session_goal::native::Session,
    hint: &Hint,
) -> Result<(), String> {
    use crate::project::path::SecureBytesResult;
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        &session.root,
        FILE,
        "existing native session recovery hint",
        MAX_BYTES,
    ) {
        SecureBytesResult::Absent(_) | SecureBytesResult::Bytes(_) => {}
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => {
            return Err(error)
        }
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => return Err(error),
    }
    let sequence = format!(
        "{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    );
    let temporary = format!("{FILE}.tmp.{sequence}");
    let bytes = serde_json::to_vec(hint).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("session recovery hint exceeds 16 KiB".into());
    }
    let mut file = crate::project::path::secure_create_new(
        &session.root,
        &temporary,
        "native session recovery hint temporary",
    )?;
    let target = hint_path(&session.root);
    let staged = session.root.join(&temporary);
    let result = (|| -> Result<(), String> {
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&staged, &target).map_err(|error| error.to_string())?;
        fs::File::open(&session.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{bounded_text, safe_relative_path};

    #[test]
    fn a_text_refusal_states_the_limit_and_what_was_broken() {
        assert!(bounded_text("step", "current step", 8).is_ok());
        for (value, broken) in [
            ("", "it is empty"),
            ("123456789", "it has 9 bytes"),
            ("a\nb", "line break"),
            (" padded", "starts or ends with a space"),
        ] {
            let error = bounded_text(value, "current step", 8).unwrap_err();
            assert!(error.contains("one line of 1 to 8 bytes"), "{error}");
            assert!(error.contains(broken), "{error}");
        }
    }

    #[test]
    fn safe_relative_path_preserves_unicode_and_rejects_noncanonical_components() {
        assert!(safe_relative_path("src/解析.rs"));
        assert!(safe_relative_path("src/café.rs"));
        assert!(safe_relative_path("src/cafe\u{301}.rs"));
        assert!(!safe_relative_path(""));
        assert!(!safe_relative_path("../src/lib.rs"));
        assert!(!safe_relative_path("src//lib.rs"));
        assert!(!safe_relative_path("src/./lib.rs"));
        assert!(!safe_relative_path("src\\lib.rs"));
    }
}

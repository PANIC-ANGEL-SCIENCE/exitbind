//! Native-host session identity and its private, owner-bound Goal journal root.
//!
//! Host identifiers are correlation locators only. They are hashed before
//! entering paths or persisted records and never grant host authority.

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const OWNER_FILE: &str = "owner.json";
const OWNER_MAX_BYTES: u64 = 4096;

#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) host: &'static str,
    pub(crate) key: String,
    pub(crate) root: PathBuf,
}

#[derive(Clone)]
pub(crate) struct Owner {
    pub(crate) project_id: Option<String>,
    pub(crate) identity_sha256: String,
}

pub(crate) struct Scope {
    pub(crate) root: PathBuf,
    pub(crate) session: Option<Session>,
    pub(crate) owner: Option<Owner>,
    pub(crate) owner_matches: bool,
}

impl Scope {
    pub(crate) fn is_session(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn require_owner(&self) -> Result<(), String> {
        if !self.is_session() || self.owner_matches {
            return Ok(());
        }
        if self.owner.is_some() {
            return Err("native session Goal is owned by another configured project; this project may read context only".into());
        }
        Err(
            "no native session Goal owner is established; explicitly incorporate the Goal first"
                .into(),
        )
    }

    /// `require_owner` for a command that also accepts `--project`: a foreign
    /// owner's refusal names that route, before the command writes anything.
    pub(crate) fn require_owner_or_project(&self) -> Result<(), String> {
        self.require_owner().map_err(|error| {
            if self.owner.is_some() {
                format!(
                    "{error}; rerun with --project to record in this project's own Goal journal"
                )
            } else {
                error
            }
        })
    }
}

/// Resolve only host-reported selectors belonging to the current process.
/// A missing selector is distinct from malformed, mixed, or conflicting IDs.
pub(crate) fn current() -> Result<Option<Session>, String> {
    match current_identity()? {
        Some((host, identity)) => session(host, Some(identity)),
        None => Ok(None),
    }
}

/// The host and the host-reported session identity `current` resolves, for a
/// record that a hook later matches against its payload `session_id`.
pub(crate) fn current_identity() -> Result<Option<(&'static str, String)>, String> {
    let codex_thread = selector("CODEX_THREAD_ID")?;
    let codex_session = selector("CODEX_SESSION_ID")?;
    let claude_session = selector("CLAUDE_CODE_SESSION_ID")?;
    let claude_export = selector("EXITBIND_NATIVE_SESSION_ID")?;
    let codex_present = codex_thread.is_some() || codex_session.is_some();
    let claude_present = claude_session.is_some() || claude_export.is_some();
    if codex_present && claude_present {
        return Err("conflicting native host session selectors are present".into());
    }
    if codex_present {
        if codex_thread.is_some() && codex_session.is_some() && codex_thread != codex_session {
            return Err("Codex session selectors disagree; native Goal lookup was declined".into());
        }
        return Ok(codex_thread.or(codex_session).map(|id| ("codex", id)));
    }
    if claude_present {
        return Ok(claude_identity(claude_session, claude_export).map(|id| ("claude", id)));
    }
    Ok(None)
}

/// Claude Code's own `CLAUDE_CODE_SESSION_ID` names the current process's
/// session. The exported selector serves processes that lack it; after a host
/// `/clear` that export can still name the cleared session, so the host value
/// wins without a refusal or any notice to the agent.
fn claude_identity(host: Option<String>, export: Option<String>) -> Option<String> {
    host.or(export)
}

fn selector(name: &str) -> Result<Option<String>, String> {
    let Some(value) = env::var_os(name) else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{name} is not valid UTF-8; native Goal lookup was declined"))?;
    if value.trim().is_empty()
        || value.len() > 512
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "{name} is invalid; native Goal lookup was declined"
        ));
    }
    Ok(Some(value))
}

fn session(host: &'static str, identity: Option<String>) -> Result<Option<Session>, String> {
    let Some(identity) = identity else {
        return Ok(None);
    };
    let key = session_key(host, &identity);
    let base = base_directory()?;
    Ok(Some(Session {
        host,
        key: key.clone(),
        root: base.join(format!("session-{key}")),
    }))
}

/// The hashed locator for a host-reported session identity. Display readers
/// derive the same key from the host's own session id.
pub(crate) fn session_key(host: &str, identity: &str) -> String {
    hash::value(&json!({"host":host,"identity":identity}))
}

pub(crate) fn base_directory() -> Result<PathBuf, String> {
    let raw = match env::var_os("EXITBIND_SESSION_GOALS_DIR") {
        Some(value) => PathBuf::from(value),
        None => match env::var_os("XDG_STATE_HOME") {
            Some(value) => PathBuf::from(value).join("exitbind/session-goals"),
            None => env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or("native session Goal storage needs an absolute private state directory")?
                .join(".local/state/exitbind/session-goals"),
        },
    };
    if !raw.is_absolute()
        || raw
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("native session Goal storage directory must be absolute and normalized".into());
    }
    Ok(raw)
}

impl Session {
    pub(crate) fn ensure_root(&self, create: bool) -> Result<bool, String> {
        let base = self
            .root
            .parent()
            .ok_or("native session Goal root has no parent")?;
        if create {
            crate::project::portability::create_private_directory(base)?;
        } else if !directory_exists(base)? {
            return Ok(false);
        } else {
            crate::project::portability::verify_private_directory(base)?;
        }
        if create {
            crate::project::portability::create_private_directory(&self.root)?;
            crate::project::portability::create_private_directory(
                &self
                    .root
                    .join(crate::project::layout_types::state_namespace()),
            )?;
            return Ok(true);
        }
        if !directory_exists(&self.root)? {
            return Ok(false);
        }
        crate::project::portability::verify_private_directory(&self.root)?;
        let state = self
            .root
            .join(crate::project::layout_types::state_namespace());
        if directory_exists(&state)? {
            crate::project::portability::verify_private_directory(&state)?;
        }
        Ok(true)
    }

    pub(crate) fn owner(&self) -> Result<Option<Owner>, String> {
        if !self.ensure_root(false)? {
            return Ok(None);
        }
        let Some(bytes) = read_private(
            &self.root,
            OWNER_FILE,
            "native session Goal owner",
            OWNER_MAX_BYTES,
        )?
        else {
            return Ok(None);
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "native session Goal owner record is malformed".to_owned())?;
        validate_owner(&value, &self.key, self.host)?;
        Ok(Some(Owner {
            project_id: value["ownerProjectId"].as_str().map(str::to_owned),
            identity_sha256: value["ownerIdentitySha256"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        }))
    }

    fn create_owner(&self, loaded: &Loaded) -> Result<Owner, String> {
        self.ensure_root(true)?;
        if let Some(owner) = self.owner()? {
            return Ok(owner);
        }
        // A journal without its fixed owner record cannot be adopted by a
        // later project, even if it happens to have the same session key.
        if super::read(&self.root)?.is_some() {
            return Err(
                "native session Goal journal exists without its owner record; refusing adoption"
                    .into(),
            );
        }
        let owner = owner_for(loaded)?;
        let mut value = json!({
            "kind":"native_session_goal_owner",
            "protocol":1,
            "host":self.host,
            "sessionKey":self.key,
            "ownerProjectId":owner.project_id,
            "ownerIdentitySha256":owner.identity_sha256,
        });
        value["sha256"] = json!(hash::value(&value));
        let mut file = crate::project::path::secure_create_new(
            &self.root,
            OWNER_FILE,
            "native session Goal owner",
        )?;
        use std::io::Write;
        file.write_all(value.to_string().as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        Ok(owner)
    }
}

pub(crate) fn scope(loaded: &Loaded, establish_owner: bool) -> Result<Scope, String> {
    let Some(session) = current()? else {
        return Ok(project_scope(loaded));
    };
    scope_for(loaded, session, establish_owner)
}

/// A Claude Code session named by a hook payload rather than the environment.
pub(crate) fn claude_session(identity: &str) -> Result<Option<Session>, String> {
    session("claude", Some(identity.to_owned()))
}

/// A session a stored record names by host and host-reported identity.
pub(crate) fn host_session(host: &str, identity: &str) -> Result<Option<Session>, String> {
    match host {
        "claude" => session("claude", Some(identity.to_owned())),
        "codex" => session("codex", Some(identity.to_owned())),
        _ => Err("native session host is unsupported".into()),
    }
}

pub(crate) fn scope_for(
    loaded: &Loaded,
    session: Session,
    establish_owner: bool,
) -> Result<Scope, String> {
    let owner = if establish_owner {
        Some(session.create_owner(loaded)?)
    } else {
        session.owner()?
    };
    let local = owner_for(loaded).ok();
    let owner_matches = owner
        .as_ref()
        .zip(local.as_ref())
        .is_some_and(|(left, right)| {
            left.identity_sha256 == right.identity_sha256 && left.project_id == right.project_id
        });
    Ok(Scope {
        root: session.root.clone(),
        session: Some(session),
        owner,
        owner_matches,
    })
}

/// Ordinary Work creation treats session context as optional. Invalid,
/// conflicting or unavailable host selectors decline association but do not
/// block the Work itself. Named Goal operations continue to use strict scope().
pub(crate) fn optional_work_scope(loaded: &Loaded) -> Option<Scope> {
    scope(loaded, false).ok()
}

pub(crate) fn project_scope(loaded: &Loaded) -> Scope {
    Scope {
        root: loaded.state_root.clone(),
        session: None,
        owner: None,
        owner_matches: true,
    }
}

pub(crate) fn owner_for(loaded: &Loaded) -> Result<Owner, String> {
    owner_for_roots(
        &loaded.product_root,
        &loaded.state_root,
        loaded.project_id.clone(),
    )
}

/// The owner identity a project with these roots has, also before its
/// configuration is written.
pub(crate) fn owner_for_roots(
    product_root: &Path,
    state_root: &Path,
    project_id: Option<String>,
) -> Result<Owner, String> {
    let product = canonical_text(product_root, "project root")?;
    let state = canonical_text(state_root, "project state root")?;
    Ok(Owner {
        project_id: project_id.clone(),
        identity_sha256: hash::value(&json!({
            "productRoot":product,
            "stateRoot":state,
            "projectId":project_id,
        })),
    })
}

fn canonical_text(path: &Path, label: &str) -> Result<String, String> {
    fs::canonicalize(path)
        .map_err(|error| format!("{label} cannot be canonicalized: {error}"))?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} is not valid UTF-8"))
}

fn directory_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(info) if info.is_dir() && !info.file_type().is_symlink() => Ok(true),
        Ok(_) => Err("native session Goal storage refuses symlinks or non-directory paths".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn read_private(
    root: &Path,
    relative: &str,
    label: &str,
    max: u64,
) -> Result<Option<Vec<u8>>, String> {
    use crate::project::path::SecureBytesResult;
    match crate::project::path::secure_bytes_observation_single_link_bounded(
        root, relative, label, max,
    ) {
        SecureBytesResult::Bytes(bytes) => Ok(Some(bytes)),
        SecureBytesResult::Absent(_) => Ok(None),
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => Err(error),
    }
}

fn validate_owner(value: &Value, key: &str, host: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("native session Goal owner record is malformed")?;
    if object.len() != 7
        || value["kind"] != "native_session_goal_owner"
        || value["protocol"] != 1
        || value["sessionKey"] != key
        || !value["ownerIdentitySha256"]
            .as_str()
            .is_some_and(valid_hash)
        || !(value["ownerProjectId"].is_null() || value["ownerProjectId"].is_string())
        || value["host"].as_str() != Some(host)
    {
        return Err("native session Goal owner record is invalid or unsupported".into());
    }
    let digest = value["sha256"]
        .as_str()
        .filter(|digest| valid_hash(digest))
        .ok_or("native session Goal owner digest is invalid")?;
    let mut without_digest = value.clone();
    without_digest
        .as_object_mut()
        .expect("validated object")
        .remove("sha256");
    if hash::value(&without_digest) != digest {
        return Err("native session Goal owner digest mismatch".into());
    }
    Ok(())
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::claude_identity;

    #[test]
    fn claude_host_session_selector_wins_over_a_stale_export() {
        let current = Some("current-session".to_owned());
        let cleared = Some("cleared-session".to_owned());
        assert_eq!(
            claude_identity(current.clone(), cleared.clone()).as_deref(),
            Some("current-session")
        );
        assert_eq!(
            claude_identity(current.clone(), current.clone()).as_deref(),
            Some("current-session")
        );
        assert_eq!(
            claude_identity(None, cleared).as_deref(),
            Some("cleared-session"),
            "the export still serves processes without the host selector"
        );
        assert_eq!(
            claude_identity(current, None).as_deref(),
            Some("current-session")
        );
        assert_eq!(claude_identity(None, None), None);
    }
}

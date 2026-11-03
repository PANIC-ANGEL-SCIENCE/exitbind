//! Native-host session identity and its private, owner-bound Goal journal root.
//!
//! Host identifiers are correlation locators only. They are hashed before
//! entering paths or persisted records and never grant host authority.

pub(crate) use crate::project::portability::DirectoryAccess as Access;
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    env, fs,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};

pub(super) const OWNER_FILE: &str = "owner.json";
pub(super) const OWNER_MAX_BYTES: u64 = 512 * 1024;
/// The owner-record field of a session that adopted another session's
/// journal: that journal's session key.
const ADOPTED_FIELD: &str = "journalSessionKey";

thread_local! {
    // Only a callback whose existing child intent and Work owner were
    // verified may install this temporary, thread-local projection context.
    static CALLBACK_SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) host: &'static str,
    /// The key of the Goal journal this session reads and writes: its own,
    /// or the journal it explicitly adopted (`goal adopt`).
    pub(crate) key: String,
    /// The hashed host-reported identity of this session. A payload
    /// `session_id` is matched against this key, never against `key`.
    pub(crate) selector_key: String,
    pub(crate) root: PathBuf,
}

/// Restore the preceding projection context on every exit, including unwind.
/// The guard cannot move to another thread where a different slot would be
/// restored.
pub(crate) struct CallbackSessionGuard {
    previous: Option<Session>,
    _same_thread: PhantomData<Rc<()>>,
}

impl Drop for CallbackSessionGuard {
    fn drop(&mut self) {
        CALLBACK_SESSION.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

/// Use only an already owner-verified child session for one callback. An
/// ambient selector remains authoritative and must agree with that session.
pub(crate) fn scoped_callback_session(session: Session) -> Result<CallbackSessionGuard, String> {
    if let Some((host, identity)) = current_identity()? {
        if host != session.host || !session.reports(&identity) {
            return Err(
                "native selector conflicts with the verified child receiving session".into(),
            );
        }
    }
    install_callback_session(session)
}

fn install_callback_session(session: Session) -> Result<CallbackSessionGuard, String> {
    let previous = CALLBACK_SESSION.with(|slot| {
        let mut current = slot.borrow_mut();
        if current.as_ref().is_some_and(|existing| {
            existing.host != session.host || existing.selector_key != session.selector_key
        }) {
            return Err("nested child receiving sessions disagree".to_owned());
        }
        Ok(current.replace(session))
    })?;
    Ok(CallbackSessionGuard {
        previous,
        _same_thread: PhantomData,
    })
}

#[derive(Clone)]
pub(crate) struct Owner {
    pub(crate) project_id: Option<String>,
    pub(crate) identity_sha256: String,
}

/// The owner and Goal from the same bounded recovery acquisition.
pub(crate) struct RecoverySnapshot {
    pub(crate) owner: Owner,
    pub(crate) latest: Option<Value>,
    pub(crate) owner_config: Option<super::recovery::ConfigBinding>,
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
    current_with_access(Access::RequireWritable)
}

pub(crate) fn current_with_access(access: Access) -> Result<Option<Session>, String> {
    if let Some(session) = CALLBACK_SESSION.with(|slot| slot.borrow().clone()) {
        return Ok(Some(session));
    }
    match current_identity()? {
        Some((host, identity)) => session_with_access(host, Some(identity), access),
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
    if !valid_identity(&value) {
        return Err(format!(
            "{name} is invalid; native Goal lookup was declined"
        ));
    }
    Ok(Some(value))
}

fn valid_identity(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn session(host: &'static str, identity: Option<String>) -> Result<Option<Session>, String> {
    session_with_access(host, identity, Access::RequireWritable)
}

fn session_with_access(
    host: &'static str,
    identity: Option<String>,
    access: Access,
) -> Result<Option<Session>, String> {
    let Some(identity) = identity else {
        return Ok(None);
    };
    let own = Session::for_key(host, &session_key(host, &identity))?;
    // A session that adopted another session's journal reads and writes that
    // journal. An unreadable adoption record leaves the session on its own
    // root, where `owner()` reports the record's error.
    Ok(Some(match own.adopted_key(access) {
        Ok(Some(journal)) => Session {
            selector_key: own.selector_key,
            ..Session::for_key(host, &journal)?
        },
        _ => own,
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
    /// The session whose own journal has this key.
    pub(crate) fn for_key(host: &'static str, key: &str) -> Result<Session, String> {
        Ok(Session {
            host,
            key: key.to_owned(),
            selector_key: key.to_owned(),
            root: base_directory()?.join(format!("session-{key}")),
        })
    }

    /// Whether a host payload's `session_id` names this session.
    pub(crate) fn reports(&self, identity: &str) -> bool {
        session_key(self.host, identity) == self.selector_key
    }

    /// Whether this session reads a journal it adopted.
    pub(crate) fn adopted(&self) -> bool {
        self.key != self.selector_key
    }

    pub(super) fn owner_value(&self, access: Access) -> Result<Option<Value>, String> {
        if !self.ensure_root_with_access(false, access)? {
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
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "native session Goal owner record is malformed".to_owned())
    }

    /// The journal key this session's own owner record adopted, if any.
    fn adopted_key(&self, access: Access) -> Result<Option<String>, String> {
        let Some(value) = self.owner_value(access)? else {
            return Ok(None);
        };
        if value["protocol"] == 2 {
            super::owner_correction::validate_marker_envelope(self, &value)?;
            return Ok(None);
        }
        validate_owner_v1(&value, &self.key, self.host)?;
        Ok(value[ADOPTED_FIELD].as_str().map(str::to_owned))
    }

    /// Record in this session's own root that it reads the journal `journal`
    /// of the same owner. Refuses when this session has any owner record.
    pub(crate) fn write_adoption(&self, owner: &Owner, journal: &str) -> Result<(), String> {
        if self.adopted() || !valid_hash(journal) || journal == self.key {
            return Err("native session Goal adoption target is invalid".into());
        }
        self.ensure_root(true)?;
        let mut value = json!({
            "kind":"native_session_goal_owner",
            "protocol":1,
            "host":self.host,
            "sessionKey":self.key,
            "ownerProjectId":owner.project_id,
            "ownerIdentitySha256":owner.identity_sha256,
            ADOPTED_FIELD:journal,
        });
        value["sha256"] = json!(hash::value(&value));
        write_owner(&self.root, &value)
    }

    pub(crate) fn ensure_root(&self, create: bool) -> Result<bool, String> {
        self.ensure_root_with_access(create, Access::RequireWritable)
    }

    fn ensure_root_with_access(&self, create: bool, access: Access) -> Result<bool, String> {
        let base = self
            .root
            .parent()
            .ok_or("native session Goal root has no parent")?;
        if create {
            crate::project::portability::create_private_directory(base)?;
        } else if !directory_exists(base)? {
            return Ok(false);
        } else {
            crate::project::portability::verify_private_directory_with_access(base, access)?;
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
        crate::project::portability::verify_private_directory_with_access(&self.root, access)?;
        let state = self
            .root
            .join(crate::project::layout_types::state_namespace());
        if directory_exists(&state)? {
            crate::project::portability::verify_private_directory_with_access(&state, access)?;
        }
        Ok(true)
    }

    pub(crate) fn owner(&self) -> Result<Option<Owner>, String> {
        self.owner_with_access(Access::RequireWritable)
    }

    pub(crate) fn recovery_snapshot(
        &self,
        access: Access,
    ) -> Result<Option<RecoverySnapshot>, String> {
        super::owner_correction::recovery_snapshot(self, access)
    }

    pub(crate) fn owner_with_access(&self, access: Access) -> Result<Option<Owner>, String> {
        let Some(value) = self.owner_value(access)? else {
            return Ok(None);
        };
        if value["protocol"] == 2 {
            return super::owner_correction::resolve_owner(self, &value, access)
                .map(|resolved| Some(resolved.owner));
        }
        validate_owner_v1(&value, &self.key, self.host)?;
        // The journal an adoption names is never itself an adoption, and an
        // adoption record that did not resolve names no journal to read.
        if value.get(ADOPTED_FIELD).is_some() {
            return Err("native session Goal owner record names another session's journal that cannot be read".into());
        }
        super::owner_correction::validate_legacy_history(self, &value, access)?;
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
        write_owner(&self.root, &value)?;
        Ok(owner)
    }
}

fn write_owner(root: &Path, value: &Value) -> Result<(), String> {
    let mut file =
        crate::project::path::secure_create_new(root, OWNER_FILE, "native session Goal owner")?;
    use std::io::Write;
    file.write_all(value.to_string().as_bytes())
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

pub(crate) fn scope(loaded: &Loaded, establish_owner: bool) -> Result<Scope, String> {
    scope_inner(loaded, establish_owner, Access::RequireWritable)
}

pub(crate) fn scope_readonly(loaded: &Loaded) -> Result<Scope, String> {
    scope_with_access(loaded, Access::ReadExisting)
}

pub(crate) fn scope_with_access(loaded: &Loaded, access: Access) -> Result<Scope, String> {
    scope_inner(loaded, false, access)
}

fn scope_inner(loaded: &Loaded, establish_owner: bool, access: Access) -> Result<Scope, String> {
    let Some(session) = current_with_access(access)? else {
        return Ok(project_scope(loaded));
    };
    scope_for_with_access(loaded, session, establish_owner, access)
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

/// Resolve a native hook's explicit host and payload identity read-only. The
/// locator is never authority: callers must verify the existing owner too.
pub(crate) fn host_session_readonly(host: &str, identity: &str) -> Result<Option<Session>, String> {
    if !valid_identity(identity) {
        return Err("native session identity is invalid".into());
    }
    match host {
        "claude" => session_with_access("claude", Some(identity.to_owned()), Access::ReadExisting),
        "codex" => session_with_access("codex", Some(identity.to_owned()), Access::ReadExisting),
        _ => Err("native session host is unsupported".into()),
    }
}

pub(crate) fn scope_for(
    loaded: &Loaded,
    session: Session,
    establish_owner: bool,
) -> Result<Scope, String> {
    scope_for_with_access(loaded, session, establish_owner, Access::RequireWritable)
}

fn scope_for_with_access(
    loaded: &Loaded,
    session: Session,
    establish_owner: bool,
    access: Access,
) -> Result<Scope, String> {
    let owner = if establish_owner {
        Some(session.create_owner(loaded)?)
    } else {
        session.owner_with_access(access)?
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

pub(super) fn validate_owner_v1(value: &Value, key: &str, host: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("native session Goal owner record is malformed")?;
    let adopted = match value.get(ADOPTED_FIELD) {
        None => false,
        Some(journal) => journal
            .as_str()
            .filter(|journal| valid_hash(journal) && *journal != key)
            .map(|_| true)
            .ok_or("native session Goal owner record is invalid or unsupported")?,
    };
    if object.len() != 7 + usize::from(adopted)
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

/// Recheck native Goal authority under the Goal journal lock, including for
/// writers that selected their owner before the lock was acquired.
pub(super) fn require_mutation_owner(root: &Path, loaded: &Loaded) -> Result<(), String> {
    if root == loaded.state_root {
        let native_root = base_directory().ok().is_some_and(|base| {
            root.parent() == Some(base.as_path())
                && root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_prefix("session-"))
                    .is_some_and(valid_hash)
        });
        if !native_root {
            return Ok(());
        }
    }
    require_recorded_owner(root, &owner_for(loaded)?)
}

pub(super) fn require_recorded_owner(root: &Path, expected: &Owner) -> Result<(), String> {
    let key = root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("session-"))
        .filter(|key| valid_hash(key))
        .ok_or("native Goal journal root is invalid")?;
    if root.parent() != Some(base_directory()?.as_path()) {
        return Err("native Goal journal root is outside the session store".into());
    }
    let probe = Session::for_key("codex", key)?;
    let value = probe
        .owner_value(Access::RequireWritable)?
        .ok_or("native Goal owner is unavailable under the journal lock")?;
    let host = match value["host"].as_str() {
        Some("codex") => "codex",
        Some("claude") => "claude",
        _ => return Err("native Goal owner host is invalid".into()),
    };
    let session = Session::for_key(host, key)?;
    let (owner, pending) = if value["protocol"] == 2 {
        let resolved =
            super::owner_correction::resolve_owner(&session, &value, Access::RequireWritable)?;
        (resolved.owner, resolved.pending)
    } else {
        (
            session.owner()?.ok_or("native Goal owner is unavailable")?,
            false,
        )
    };
    if pending {
        return Err(
            "native Goal owner correction is pending; retry or abort the exact correction".into(),
        );
    }
    if owner.identity_sha256 != expected.identity_sha256 || owner.project_id != expected.project_id
    {
        return Err(
            "native Goal owner no longer matches the selected owner under the journal lock".into(),
        );
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
    use super::{claude_identity, install_callback_session, Session, CALLBACK_SESSION};

    #[test]
    fn callback_session_scope_restores_after_nested_return_and_unwind() {
        let session = Session {
            host: "claude",
            key: "journal".into(),
            selector_key: "selector".into(),
            root: "/fixture".into(),
        };
        assert!(CALLBACK_SESSION.with(|slot| slot.borrow().is_none()));
        let outer = install_callback_session(session.clone()).unwrap();
        {
            let _inner = install_callback_session(session.clone()).unwrap();
            assert_eq!(super::current().unwrap().unwrap().selector_key, "selector");
            let mut wrong = session.clone();
            wrong.selector_key = "other".into();
            assert!(install_callback_session(wrong).is_err());
        }
        assert_eq!(super::current().unwrap().unwrap().selector_key, "selector");
        drop(outer);
        assert!(CALLBACK_SESSION.with(|slot| slot.borrow().is_none()));
        let unwind = std::panic::catch_unwind(|| {
            let _guard = install_callback_session(session).unwrap();
            panic!("synthetic callback failure");
        });
        assert!(unwind.is_err());
        assert!(CALLBACK_SESSION.with(|slot| slot.borrow().is_none()));
    }

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

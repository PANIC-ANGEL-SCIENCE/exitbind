//! Bounded, append-only ledger storage.
//!
//! This module owns filesystem mechanics only.  Run orchestration stays in
//! `mod.rs`, while event shape and state transitions stay in `state.rs`.
use super::state as run_state;
use crate::{config::Loaded, evidence::hash};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub(crate) struct LedgerPath {
    pub(crate) path: PathBuf,
    pub(crate) lock: PathBuf,
    pub(crate) root: PathBuf,
    pub(crate) expected: PathBuf,
}

pub(crate) struct SupersessionClaim {
    pub(crate) value: Value,
    created_inode: Option<(u64, u64)>,
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum AppendFailure {
    None,
    StageWrite,
    StageFlush,
    Commit,
}

pub(crate) fn load(
    loaded: &Loaded,
    requested: &str,
) -> Result<(LedgerPath, Vec<Value>, String), String> {
    let path = ledger_path(&loaded.state_root, requested, false)?;
    load_at(loaded, &path)
}

pub(crate) fn load_at(
    _loaded: &Loaded,
    ledger: &LedgerPath,
) -> Result<(LedgerPath, Vec<Value>, String), String> {
    let mut file = open_read_nofollow(&ledger.path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || fs::canonicalize(&ledger.path).map_err(|error| error.to_string())? != ledger.expected
    {
        return Err("ledger path changed while opening".into());
    }

    let mut source = String::new();
    file.read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut confirmed = String::new();
    file.read_to_string(&mut confirmed)
        .map_err(|error| error.to_string())?;
    let current = fs::symlink_metadata(&ledger.path).map_err(|error| error.to_string())?;
    if source != confirmed
        || current.file_type().is_symlink()
        || !current.is_file()
        || inode(&current) != inode(&metadata)
    {
        return Err("ledger changed while reading".into());
    }
    let mut events = Vec::new();
    let mut lines: Vec<&str> = source.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            return Err(format!("invalid run ledger line {}: empty line", index + 1));
        }
        let event: Value = serde_json::from_str(line)
            .map_err(|error| format!("invalid run ledger line {}: {error}", index + 1))?;
        run_state::validate_event(&event, events.last(), index + 1)?;
        events.push(event);
    }
    run_state::reduce(&events)?;
    Ok((ledger.clone(), events, source))
}

pub(crate) fn ledger_path(
    root: &Path,
    requested: &str,
    allow_missing: bool,
) -> Result<LedgerPath, String> {
    let portable = requested.replace('\\', "/");
    if requested.trim().is_empty()
        || requested.contains('\0')
        || Path::new(requested).is_absolute()
        || portable.starts_with('/')
        || portable.contains(":/")
        || portable == ".."
        || portable.starts_with("../")
    {
        return Err("ledger path must be a non-empty string".into());
    }

    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let path = root.join(requested);
    let parent = path.parent().ok_or("ledger parent does not exist")?;
    let real_parent = fs::canonicalize(parent).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("ledger parent does not exist: {requested}")
        } else {
            error.to_string()
        }
    })?;
    if !real_parent.starts_with(&root) {
        return Err(format!("path escapes project root: {requested}"));
    }
    let expected = real_parent.join(path.file_name().ok_or("invalid ledger path")?);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!("ledger must not be a symlink: {requested}"));
            }
            if !metadata.is_file() {
                return Err(format!("ledger must be a regular file: {requested}"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("ledger does not exist: {requested}"));
        }
        Err(error) => return Err(error.to_string()),
    }

    let relative = expected
        .strip_prefix(&root)
        .map_err(|_| format!("path escapes project root: {requested}"))?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let namespace = crate::project::layout_types::state_namespace();
    let runs_prefix = format!("{namespace}/runs/");
    let lock = if relative.starts_with(&runs_prefix) {
        root.join(namespace)
            .join("locks")
            .join(format!("run-v1-{}.lock", hash::text(&relative)))
    } else {
        PathBuf::from(format!("{}.lock", expected.display()))
    };
    Ok(LedgerPath {
        lock,
        path: expected.clone(),
        root,
        expected,
    })
}

pub(crate) fn append(
    ledger: &LedgerPath,
    event: &Value,
    create: bool,
    expected: &str,
) -> Result<(), String> {
    #[cfg(test)]
    {
        append_with_failure(ledger, event, create, expected, AppendFailure::None)
    }
    #[cfg(not(test))]
    {
        append_with_failure(ledger, event, create, expected)
    }
}

fn append_with_failure(
    ledger: &LedgerPath,
    event: &Value,
    create: bool,
    expected: &str,
    #[cfg(test)] failure: AppendFailure,
) -> Result<(), String> {
    let line = serde_json::to_string(event).map_err(|error| error.to_string())?;
    let mut source = String::new();
    let opened_metadata = if create {
        match fs::symlink_metadata(&ledger.path) {
            Ok(_) => return Err("ledger already exists; run was not started".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        }
    } else {
        let mut file = open_read_nofollow(&ledger.path).map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        verify_opened(ledger, &metadata)?;
        file.read_to_string(&mut source)
            .map_err(|error| error.to_string())?;
        if source != expected {
            return Err("ledger changed before append".into());
        }
        Some(metadata)
    };

    let staged = stage_path(&ledger.path);
    let result = (|| {
        let mut file = open_stage(&staged)?;
        let bytes = if create {
            format!("{line}\n").into_bytes()
        } else {
            format!("{source}{line}\n").into_bytes()
        };
        #[cfg(test)]
        if matches!(failure, AppendFailure::StageWrite) {
            return Err("injected ledger stage write failure".into());
        }
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        #[cfg(test)]
        if matches!(failure, AppendFailure::StageFlush) {
            return Err("injected ledger stage flush failure".into());
        }
        file.flush().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);

        if let Some(metadata) = &opened_metadata {
            verify_opened(ledger, metadata)?;
            let mut confirmed =
                open_read_nofollow(&ledger.path).map_err(|error| error.to_string())?;
            let mut current_source = String::new();
            confirmed
                .read_to_string(&mut current_source)
                .map_err(|error| error.to_string())?;
            if current_source != expected {
                return Err("ledger changed before commit".into());
            }
            #[cfg(test)]
            if matches!(failure, AppendFailure::Commit) {
                return Err("injected ledger commit precondition failure".into());
            }
            fs::rename(&staged, &ledger.path).map_err(|error| error.to_string())?;
        } else {
            fs::hard_link(&staged, &ledger.path).map_err(|error| error.to_string())?;
            let current = fs::symlink_metadata(&ledger.path).map_err(|error| error.to_string())?;
            if current.file_type().is_symlink() || !current.is_file() {
                let _ = fs::remove_file(&ledger.path);
                return Err("ledger changed while committing".into());
            }
        }
        Ok(())
    })();
    let _ = fs::remove_file(&staged);
    result
}

fn verify_opened(ledger: &LedgerPath, opened: &fs::Metadata) -> Result<(), String> {
    if !opened.is_file() {
        return Err("ledger must be a regular file".into());
    }
    let canonical = fs::canonicalize(&ledger.path).map_err(|error| error.to_string())?;
    if !canonical.starts_with(&ledger.root) || canonical != ledger.expected {
        return Err("ledger path changed while opening".into());
    }
    let current = fs::symlink_metadata(&ledger.path).map_err(|error| error.to_string())?;
    if current.file_type().is_symlink() || !current.is_file() || inode(&current) != inode(opened) {
        return Err("ledger changed while opening".into());
    }
    Ok(())
}

fn stage_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    path.with_file_name(format!(
        ".{}.append-{}-{nonce}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ledger"),
        std::process::id()
    ))
}

fn open_stage(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|error| error.to_string())
}

pub(crate) fn with_lock<T, F: FnOnce() -> Result<T, String>>(
    ledger: &LedgerPath,
    action: F,
) -> Result<T, String> {
    crate::project::managed_files::ensure_state_directory(
        &ledger.root,
        ledger.lock.parent().ok_or("run lock has no parent")?,
    )?;
    let mut handle = None;
    for attempt in 0..2 {
        match open_lock(&ledger.lock) {
            Ok(file) => {
                handle = Some(file);
                break;
            }
            Err(error)
                if attempt == 0
                    && error.kind() == std::io::ErrorKind::AlreadyExists
                    && remove_stale(&ledger.lock) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err("run ledger is busy; no mutation was made".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    }

    let mut lock = handle.ok_or("run ledger is busy; no mutation was made")?;
    let owned = lock.metadata().map_err(|error| error.to_string())?;
    lock.write_all(
        format!(
            "{{\"pid\":{},\"createdAt\":\"{}\"}}",
            std::process::id(),
            now()
        )
        .as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    lock.flush().map_err(|error| error.to_string())?;

    let result = action();
    drop(lock);
    if let Ok(metadata) = fs::symlink_metadata(&ledger.lock) {
        if inode(&metadata) == inode(&owned) {
            let _ = fs::remove_file(&ledger.lock);
        }
    }
    result
}

pub(crate) fn predecessor(loaded: &Loaded, start: &Value) -> Result<(), String> {
    let Some(link) = start.get("supersedes") else {
        return Ok(());
    };
    let predecessor = ledger_path(
        &loaded.state_root,
        link["ledgerPath"]
            .as_str()
            .ok_or("invalid superseded predecessor path")?,
        false,
    )?;
    let (_, events, source) = load_at(loaded, &predecessor)?;
    let head = events.last().ok_or("superseded predecessor is empty")?;
    if hash::text(&source) != link["ledgerSha256"]
        || events[0]["runId"] != link["runId"]
        || head["eventSha256"] != link["headEventSha256"]
        || events[0]["configSha256"] != link["configSha256"]
    {
        return Err("superseded predecessor provenance mismatch".into());
    }
    Ok(())
}

pub(crate) fn claim_path(ledger: &LedgerPath) -> PathBuf {
    PathBuf::from(format!("{}.supersede", ledger.path.display()))
}

/// Read and validate an existing supersession claim without requiring a
/// caller to know the successor request that created it.
pub(crate) fn valid_claim(path: &Path) -> Result<Option<Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("supersession claim is not a regular file".into());
    }
    let mut file = open_read_nofollow(path).map_err(|error| error.to_string())?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if !opened.is_file() || inode(&opened) != inode(&metadata) {
        return Err("supersession claim changed while opening".into());
    }
    let mut source = String::new();
    file.read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut confirmed = String::new();
    file.read_to_string(&mut confirmed)
        .map_err(|error| error.to_string())?;
    let current = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if source != confirmed || current.file_type().is_symlink() || inode(&current) != inode(&opened)
    {
        return Err("supersession claim changed while reading".into());
    }
    let value: Value =
        serde_json::from_str(&source).map_err(|_| "invalid supersession claim".to_string())?;
    validate_claim(&value)?;
    Ok(Some(value))
}

pub(crate) fn obtain_claim(path: &Path, wanted: &Value) -> Result<SupersessionClaim, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("supersession claim is not a regular file".into());
            }
            let mut file = open_read_nofollow(path).map_err(|error| error.to_string())?;
            let opened = file.metadata().map_err(|error| error.to_string())?;
            if !opened.is_file() || inode(&opened) != inode(&metadata) {
                return Err("supersession claim changed while opening".into());
            }
            let mut source = String::new();
            file.read_to_string(&mut source)
                .map_err(|error| error.to_string())?;
            file.seek(SeekFrom::Start(0))
                .map_err(|error| error.to_string())?;
            let mut confirmed = String::new();
            file.read_to_string(&mut confirmed)
                .map_err(|error| error.to_string())?;
            let current = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            if source != confirmed
                || current.file_type().is_symlink()
                || inode(&current) != inode(&opened)
            {
                return Err("supersession claim changed while reading".into());
            }
            let value: Value = serde_json::from_str(&source)
                .map_err(|_| "invalid supersession claim".to_string())?;
            validate_claim(&value)?;
            for key in [
                "oldLedgerPath",
                "oldLedgerSha256",
                "oldRunId",
                "oldHeadEventSha256",
                "oldConfigSha256",
                "newLedgerPath",
                "workflow",
                "goalSha256",
                "configSha256",
            ] {
                if value[key] != wanted[key] {
                    return Err("a different successor is already claimed".into());
                }
            }
            Ok(SupersessionClaim {
                value,
                created_inode: None,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let serialized = serde_json::to_string(wanted).map_err(|error| error.to_string())?;
            let mut file = open_nofollow(path, true, false).map_err(|error| {
                if error.contains("already exists") {
                    "a different successor is already claimed".into()
                } else {
                    error
                }
            })?;
            let owned = file.metadata().map_err(|error| error.to_string())?;
            if let Err(error) = file.write_all(serialized.as_bytes()) {
                drop(file);
                remove_owned(path, &owned);
                return Err(error.to_string());
            }
            if let Err(error) = file.flush() {
                drop(file);
                remove_owned(path, &owned);
                return Err(error.to_string());
            }
            let current = match fs::symlink_metadata(path) {
                Ok(current) => current,
                Err(error) => {
                    drop(file);
                    remove_owned(path, &owned);
                    return Err(error.to_string());
                }
            };
            if current.file_type().is_symlink() || inode(&current) != inode(&owned) {
                drop(file);
                remove_owned(path, &owned);
                return Err("supersession claim changed while writing".into());
            }
            Ok(SupersessionClaim {
                value: wanted.clone(),
                created_inode: Some(inode(&owned)),
            })
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn rollback_claim(path: &Path, claim: &SupersessionClaim) {
    let Some(expected_inode) = claim.created_inode else {
        return;
    };
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || inode(&metadata) != expected_inode
    {
        return;
    }
    let Ok(mut file) = open_read_nofollow(path) else {
        return;
    };
    let mut source = String::new();
    if file.read_to_string(&mut source).is_err()
        || serde_json::from_str::<Value>(&source).ok().as_ref() != Some(&claim.value)
    {
        return;
    }
    drop(file);
    if let Ok(current) = fs::symlink_metadata(path) {
        if !current.file_type().is_symlink() && inode(&current) == expected_inode {
            let _ = fs::remove_file(path);
        }
    }
}

fn remove_owned(path: &Path, owned: &fs::Metadata) {
    if let Ok(current) = fs::symlink_metadata(path) {
        if !current.file_type().is_symlink() && inode(&current) == inode(owned) {
            let _ = fs::remove_file(path);
        }
    }
}

fn validate_claim(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("invalid supersession claim")?;
    let fields = [
        "version",
        "oldLedgerPath",
        "oldLedgerSha256",
        "oldRunId",
        "oldHeadEventSha256",
        "oldConfigSha256",
        "newLedgerPath",
        "workflow",
        "goalSha256",
        "configSha256",
        "newRunId",
        "timestamp",
    ];
    if object.len() != fields.len()
        || fields.iter().any(|key| !object.contains_key(*key))
        || value["version"] != 1
        || [
            "oldLedgerSha256",
            "oldRunId",
            "oldHeadEventSha256",
            "oldConfigSha256",
            "goalSha256",
            "configSha256",
            "newRunId",
        ]
        .iter()
        .any(|key| !sha(value[key].as_str()))
        || chrono::DateTime::parse_from_rfc3339(value["timestamp"].as_str().unwrap_or("")).is_err()
    {
        return Err("invalid supersession claim".into());
    }
    Ok(())
}

fn open_nofollow(path: &Path, create: bool, append: bool) -> Result<File, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        let flags = libc::O_NOFOLLOW
            | if append { libc::O_APPEND } else { 0 }
            | if create {
                libc::O_CREAT | libc::O_EXCL
            } else {
                0
            };
        options.read(true).write(true).custom_flags(flags);
        if create {
            options.mode(0o600);
        }
        options.open(path).map_err(|error| {
            if create && error.kind() == std::io::ErrorKind::AlreadyExists {
                "ledger already exists; run was not started".into()
            } else {
                error.to_string()
            }
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, create, append);
        Err("run mutation requires O_NOFOLLOW support".into())
    }
}

fn open_read_nofollow(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().read(true).open(path)
    }
}

fn open_lock(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "O_NOFOLLOW",
        ))
    }
}

fn remove_stale(path: &Path) -> bool {
    let Ok(initial) = fs::symlink_metadata(path) else {
        return true;
    };
    if initial.file_type().is_symlink() || !initial.is_file() {
        return false;
    }
    let Ok(mut file) = open_read_nofollow(path) else {
        return false;
    };
    let Ok(opened) = file.metadata() else {
        return false;
    };
    if inode(&opened) != inode(&initial) {
        return false;
    }
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return false;
    }
    let Ok(meta) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let Some(pid) = meta["pid"].as_u64() else {
        return false;
    };
    let valid_meta = meta.as_object().is_some_and(|object| {
        object.len() == 2 && object.contains_key("pid") && object.contains_key("createdAt")
    }) && pid > 0
        && libc::pid_t::try_from(pid).is_ok()
        && meta["createdAt"]
            .as_str()
            .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok());
    if !valid_meta || !pid_is_verifiably_absent(pid as libc::pid_t) {
        return false;
    }
    match fs::symlink_metadata(path) {
        Ok(current) if inode(&current) == inode(&initial) && current.is_file() => {
            fs::remove_file(path).is_ok()
        }
        _ => false,
    }
}

#[cfg(unix)]
fn pid_is_verifiably_absent(pid: libc::pid_t) -> bool {
    (unsafe { libc::kill(pid, 0) }) == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn pid_is_verifiably_absent(_: libc::pid_t) -> bool {
    false
}

#[cfg(unix)]
fn inode(metadata: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn inode(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.len(), 0)
}

fn sha(value: Option<&str>) -> bool {
    value.is_some_and(|text| {
        text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (PathBuf, LedgerPath, String) {
        let root = std::env::temp_dir().join(format!(
            "exitbind-run-ledger-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("runs.jsonl");
        let source = "{\"version\":8,\"kind\":\"run\",\"action\":\"start\"}\n".to_owned();
        fs::write(&path, &source).unwrap();
        let expected = fs::canonicalize(&path).unwrap();
        let ledger = LedgerPath {
            path: expected.clone(),
            lock: root.join("runs.lock"),
            root: fs::canonicalize(&root).unwrap(),
            expected,
        };
        (root, ledger, source)
    }

    #[test]
    fn staged_failures_preserve_ledger_bytes_and_remove_stage_files() {
        for failure in [
            AppendFailure::StageWrite,
            AppendFailure::StageFlush,
            AppendFailure::Commit,
        ] {
            let (root, ledger, source) = fixture();
            let result = append_with_failure(
                &ledger,
                &json!({"version": 8, "kind": "run", "action": "check"}),
                false,
                &source,
                failure,
            );
            assert!(result.is_err());
            assert_eq!(fs::read(&ledger.path).unwrap(), source.as_bytes());
            assert!(fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .all(|name| !name.to_str().is_some_and(|name| name.contains(".append-"))));
            fs::remove_dir_all(root).unwrap();
        }
    }
}

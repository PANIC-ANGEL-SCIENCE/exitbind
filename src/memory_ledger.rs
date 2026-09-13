use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::config::Loaded;

use super::{apply_event, validate_event};

#[derive(Debug)]
pub(crate) struct LedgerSnapshot {
    pub(crate) path: PathBuf,
    pub(crate) real_root: PathBuf,
    pub(crate) expected_real_path: PathBuf,
    pub(crate) exists: bool,
    identity: Option<FileIdentity>,
    pub(crate) source: String,
    pub(crate) events: Vec<Value>,
    pub(crate) items: BTreeMap<String, Value>,
    pub(crate) last_event_sha256: Option<String>,
    pub(crate) last_timestamp: Option<String>,
}

#[derive(Debug)]
struct ResolvedPath {
    path: PathBuf,
    real_root: PathBuf,
    expected_real_path: PathBuf,
    exists: bool,
    identity: Option<FileIdentity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

#[cfg(unix)]
fn identity(metadata: &Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Some(FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn identity(_: &Metadata) -> Option<FileIdentity> {
    None
}

pub(crate) fn stable_text(path: &Path, label: &str) -> Result<String, String> {
    let bytes = stable_bytes(path, label)?;
    String::from_utf8(bytes).map_err(|e| format!("{label}: {e}"))
}

pub(crate) fn stable_bytes(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let first = fs::symlink_metadata(path).map_err(|e| format!("{label}: {e}"))?;
    if !first.is_file() {
        return Err(format!("{label} must be a regular file"));
    }
    let before = identity(&first);
    let bytes = fs::read(path).map_err(|e| format!("{label}: {e}"))?;
    let after = fs::symlink_metadata(path).map_err(|e| format!("{label}: {e}"))?;
    if !after.is_file() || identity(&after) != before {
        return Err(format!("{label} changed while reading"));
    }
    let confirmation = fs::read(path).map_err(|e| format!("{label}: {e}"))?;
    if bytes != confirmation {
        return Err(format!("{label} changed while reading"));
    }
    Ok(bytes)
}

pub(crate) fn confined_target(
    root: &Path,
    requested: &str,
    label: &str,
) -> Result<PathBuf, String> {
    if requested.trim().is_empty() || requested.contains('\0') {
        return Err(format!("{label} path must be a non-empty string"));
    }
    let root = absolute(root)?;
    let candidate = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        root.join(requested)
    };
    if !contained(&root, &candidate) {
        return Err(format!("path escapes project root: {label}"));
    }
    let relative = candidate
        .strip_prefix(&root)
        .map_err(|_| format!("path escapes project root: {label}"))?;
    let mut normalized = root.clone();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(value) => normalized.push(value),
            std::path::Component::ParentDir => {
                if normalized == root {
                    return Err(format!("path escapes project root: {label}"));
                }
                normalized.pop();
            }
            _ => return Err(format!("path escapes project root: {label}")),
        }
    }
    Ok(normalized)
}

pub(crate) fn path_exists_without_symlinks(
    root: &Path,
    requested: &str,
    label: &str,
) -> Result<bool, String> {
    let candidate = confined_target(root, requested, label)?;
    let lexical_root = absolute(root)?;
    let real_root = fs::canonicalize(root)
        .map_err(|_| format!("project root does not exist: {}", root.display()))?;
    let relative = candidate
        .strip_prefix(&lexical_root)
        .map_err(|_| format!("path escapes project root: {label}"))?;
    if relative.as_os_str().is_empty() {
        return Err(format!("{label} path must name a project file"));
    }
    let components = relative.components().collect::<Vec<_>>();
    let mut current = real_root;
    for (index, component) in components.iter().enumerate() {
        let next = current.join(component.as_os_str());
        match fs::symlink_metadata(&next) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!("{label} path must not contain symlinks"));
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Ok(false);
                }
                if index + 1 == components.len() {
                    return Ok(true);
                }
                current = next;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("{label}: {error}")),
        }
    }
    Ok(false)
}

pub(crate) fn ensure_receipt_parent(
    root: &Path,
    requested: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let target = confined_target(root, requested, "receipt")?;
    if path_exists_without_symlinks(root, requested, "receipt")? {
        return Err("receipt already exists; refusing to overwrite".into());
    }
    let parent = target
        .parent()
        .ok_or("receipt parent must be a directory")?
        .to_path_buf();
    let lexical_root = absolute(root)?;
    if parent != lexical_root {
        let parent_name = relative_project_path(root, &parent)?;
        if !path_exists_without_symlinks(root, &parent_name, "receipt parent")? {
            return Err("receipt parent does not exist".into());
        }
    }
    let metadata = fs::symlink_metadata(&parent).map_err(|e| format!("receipt parent: {e}"))?;
    if !metadata.is_dir() {
        return Err("receipt parent must be a directory".into());
    }
    Ok((target, parent))
}

pub(crate) fn append_event(ledger: &LedgerSnapshot, event: &Value) -> Result<(), String> {
    let line = format!(
        "{}\n",
        serde_json::to_string(event).map_err(|e| e.to_string())?
    );
    #[cfg(not(unix))]
    {
        let _ = (ledger, line);
        return Err("ledger mutation requires O_NOFOLLOW support".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .append(true)
            .custom_flags(o_nofollow());
        if ledger.exists {
            options.mode(0o600);
        } else {
            options.create_new(true).mode(0o600);
        }
        let mut handle = match options.open(&ledger.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && !ledger.exists => {
                return Err("ledger already exists; proposal was not appended".into())
            }
            Err(error) => return Err(error.to_string()),
        };
        let metadata = handle.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("ledger must be a regular file".into());
        }
        if let Some(expected) = ledger.identity {
            if identity(&metadata) != Some(expected) {
                return Err("ledger changed while opening".into());
            }
        }
        let opened_path = fs::canonicalize(&ledger.path).map_err(|e| e.to_string())?;
        if !contained(&ledger.real_root, &opened_path) || opened_path != ledger.expected_real_path {
            return Err("ledger path changed while opening".into());
        }
        let mut observed = String::new();
        handle
            .read_to_string(&mut observed)
            .map_err(|e| e.to_string())?;
        if observed != ledger.source {
            return Err("ledger changed before append".into());
        }
        if !ledger.exists {
            handle
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        handle
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        handle.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(unix)]
fn o_nofollow() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        0x20000
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        0x100
    }
}

pub(crate) fn read_ledger(
    loaded: &Loaded,
    requested: &str,
    allow_missing: bool,
) -> Result<LedgerSnapshot, String> {
    let resolved = resolve_ledger_path(&loaded.state_root, requested, allow_missing)?;
    if !resolved.exists {
        return Ok(LedgerSnapshot {
            path: resolved.path,
            real_root: resolved.real_root,
            expected_real_path: resolved.expected_real_path,
            exists: false,
            identity: resolved.identity,
            source: String::new(),
            events: Vec::new(),
            items: BTreeMap::new(),
            last_event_sha256: None,
            last_timestamp: None,
        });
    }
    let source = stable_text(&resolved.path, "ledger")?;
    let mut events = Vec::new();
    let mut items = BTreeMap::new();
    let mut previous_hash = None;
    let mut previous_timestamp = None;
    let mut lines = source.split('\n').map(str::to_owned).collect::<Vec<_>>();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            return Err(format!(
                "invalid memory ledger line {}: empty line",
                index + 1
            ));
        }
        let event: Value = serde_json::from_str(line)
            .map_err(|e| format!("invalid memory ledger line {}: {e}", index + 1))?;
        validate_event(
            &event,
            &loaded.product_root,
            &loaded.control_root,
            index + 1,
            previous_hash.as_deref(),
            previous_timestamp.as_deref(),
            &items,
        )?;
        apply_event(&mut items, &event)?;
        previous_hash = event["eventSha256"].as_str().map(str::to_owned);
        previous_timestamp = event["timestamp"].as_str().map(str::to_owned);
        events.push(event);
    }
    Ok(LedgerSnapshot {
        path: resolved.path,
        real_root: resolved.real_root,
        expected_real_path: resolved.expected_real_path,
        exists: true,
        identity: resolved.identity,
        source,
        events,
        items,
        last_event_sha256: previous_hash,
        last_timestamp: previous_timestamp,
    })
}

fn resolve_ledger_path(
    root: &Path,
    requested: &str,
    allow_missing: bool,
) -> Result<ResolvedPath, String> {
    if requested.trim().is_empty() || requested.contains('\0') {
        return Err("ledger path must be a non-empty string".into());
    }
    let candidate = confined_target(root, requested, "ledger").map_err(|error| {
        if error.starts_with("path escapes project root") {
            format!("path escapes project root: {requested}")
        } else {
            error
        }
    })?;
    let real_root = fs::canonicalize(root)
        .map_err(|_| format!("project root does not exist: {}", root.display()))?;
    let parent = candidate
        .parent()
        .ok_or_else(|| format!("ledger parent does not exist: {requested}"))?;
    let real_parent = fs::canonicalize(parent).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("ledger parent does not exist: {requested}")
        } else {
            e.to_string()
        }
    })?;
    if !contained(&real_root, &real_parent) {
        return Err(format!("path escapes project root: {requested}"));
    }
    let file_name = candidate
        .file_name()
        .ok_or_else(|| format!("ledger path must name a file: {requested}"))?;
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!("ledger must not be a symlink: {requested}"));
            }
            if !metadata.is_file() {
                return Err(format!("ledger must be a regular file: {requested}"));
            }
            Ok(ResolvedPath {
                expected_real_path: real_parent.join(file_name),
                path: candidate,
                real_root,
                exists: true,
                identity: identity(&metadata),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {
            Ok(ResolvedPath {
                expected_real_path: real_parent.join(file_name),
                path: candidate,
                real_root,
                exists: false,
                identity: None,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("ledger does not exist: {requested}"))
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn project_file(root: &Path, requested: &str, label: &str) -> Result<PathBuf, String> {
    let candidate = confined_target(root, requested, label)?;
    let real_root = fs::canonicalize(root)
        .map_err(|_| format!("project root does not exist: {}", root.display()))?;
    let real = fs::canonicalize(candidate).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("declared file does not exist: {requested}")
        } else {
            e.to_string()
        }
    })?;
    if !contained(&real_root, &real) {
        return Err(format!("path escapes project root: {requested}"));
    }
    let metadata = fs::symlink_metadata(&real).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err(format!("declared path is not a regular file: {requested}"));
    }
    Ok(real)
}

pub(crate) fn relative_project_path(root: &Path, path: &Path) -> Result<String, String> {
    let value = path
        .strip_prefix(absolute(root)?)
        .map_err(|_| "path escapes project root".to_owned())?
        .to_str()
        .ok_or("project-relative path is not valid UTF-8")?
        .replace('\\', "/");
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.starts_with("../")
        || Path::new(&value).is_absolute()
    {
        return Err("path escapes project root".into());
    }
    Ok(value)
}

fn contained(root: &Path, candidate: &Path) -> bool {
    candidate.strip_prefix(root).is_ok()
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|error| format!("current directory is unavailable: {error}"))
    }
}

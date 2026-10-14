//! Bounded durable storage and integrity checks for pre-return attempts.

use crate::config::Loaded;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 1_048_576;

pub(super) fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file =
        crate::project::managed_files::open_state_file(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn record_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut record = value.clone();
    record
        .as_object_mut()
        .ok_or("pre-return journal record must be an object")?
        .remove("recordSha256");
    let digest = crate::evidence::hash::value(&record);
    record
        .as_object_mut()
        .ok_or("pre-return journal record must be an object")?
        .insert("recordSha256".into(), json!(digest));
    serde_json::to_vec(&record).map_err(|e| e.to_string())
}

pub(super) fn directory_path(loaded: &Loaded) -> PathBuf {
    let root = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace());
    root.join("pre-return")
}

pub(super) fn ensure_directory(loaded: &Loaded) -> Result<PathBuf, String> {
    let directory = directory_path(loaded);
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, &directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::symlink_metadata(&directory)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o777
            != 0o700
        {
            return Err("pre-return journal directory permissions must be 0700".into());
        }
    }
    Ok(directory)
}

pub(super) fn record_path(
    loaded: &Loaded,
    attempt_id: &str,
    suffix: &str,
) -> Result<PathBuf, String> {
    if attempt_id.len() != 64 || !attempt_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("pre-return attempt id is invalid".into());
    }
    let directory = directory_path(loaded);
    for candidate in [
        directory.as_path(),
        directory
            .parent()
            .ok_or("pre-return journal path has no namespace")?,
    ] {
        match fs::symlink_metadata(candidate) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err("pre-return journal parent is not a regular directory".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(directory.join(format!("pre-return-{attempt_id}.{suffix}.json")))
}

pub(super) fn read_record(path: &Path) -> Result<Value, String> {
    let directory = path
        .parent()
        .ok_or("pre-return record has no parent directory")?;
    let directory_meta = fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
    if directory_meta.file_type().is_symlink() || !directory_meta.is_dir() {
        return Err("pre-return journal directory is not a regular directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if directory_meta.permissions().mode() & 0o777 != 0o700 {
            return Err("pre-return journal directory permissions are not private".into());
        }
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("pre-return record is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o777 != 0o600 {
            return Err("pre-return record permissions are not private".into());
        }
    }
    let mut open = fs::OpenOptions::new();
    open.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.custom_flags(libc::O_NOFOLLOW);
    }
    let file = open.open(path).map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || opened.len() > MAX_BYTES {
        return Err("pre-return record is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("pre-return record exceeds its size limit".into());
    }
    let record: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("pre-return record is invalid: {e}"))?;
    let expected = record["recordSha256"]
        .as_str()
        .ok_or("pre-return record digest is missing")?;
    let mut unsigned = record.clone();
    unsigned
        .as_object_mut()
        .ok_or("pre-return record is invalid")?
        .remove("recordSha256");
    if crate::evidence::hash::value(&unsigned) != expected {
        return Err("pre-return record digest does not match its contents".into());
    }
    Ok(record)
}

pub(super) fn install_captured_logs(
    capture: &crate::run::capture::CapturedCheck,
) -> Result<(), String> {
    let stdout_temp = capture
        .stdout_temp
        .as_ref()
        .ok_or("stdout capture temp is missing")?;
    let stdout_final = capture
        .stdout_final
        .as_ref()
        .ok_or("stdout capture destination is missing")?;
    let stderr_temp = capture
        .stderr_temp
        .as_ref()
        .ok_or("stderr capture temp is missing")?;
    let stderr_final = capture
        .stderr_final
        .as_ref()
        .ok_or("stderr capture destination is missing")?;
    let installed_stdout = match crate::run::capture::install_capture(stdout_temp, stdout_final) {
        Ok(installed) => installed,
        Err(error) => {
            let cleanup = crate::run::capture::cleanup_capture(capture);
            return Err([Some(error), cleanup.err()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("; "));
        }
    };
    match crate::run::capture::install_capture(stderr_temp, stderr_final) {
        Ok(installed_stderr) => match crate::run::capture::cleanup_capture(capture) {
            Ok(()) => Ok(()),
            Err(error) => {
                let rollback = crate::run::capture::rollback_installed(
                    capture,
                    installed_stdout,
                    installed_stderr,
                );
                Err([Some(error), rollback.err()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("; "))
            }
        },
        Err(error) => {
            let rollback =
                crate::run::capture::rollback_installed(capture, installed_stdout, false);
            let cleanup = crate::run::capture::cleanup_capture(capture);
            Err([Some(error), rollback.err(), cleanup.err()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("; "))
        }
    }
}

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const PREFIX: &str = "hld_";

pub(super) struct Held {
    pub(super) reference: String,
    pub(super) sha256: String,
    pub(super) bytes: usize,
    pub(super) created: bool,
}

fn directory(loaded: &Loaded) -> PathBuf {
    loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("held")
}

fn checked_directory(loaded: &Loaded) -> Result<PathBuf, String> {
    let path = directory(loaded);
    let info = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if info.file_type().is_symlink() || !info.is_dir() {
        return Err("held path must be a regular directory".into());
    }
    #[cfg(unix)]
    if info.permissions().mode() & 0o777 != 0o700 {
        return Err("held directory permissions are not private".into());
    }
    Ok(path)
}

fn ensure_directory(loaded: &Loaded) -> Result<PathBuf, String> {
    let root = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace());
    let directory = root.join("held");
    for path in [&root, &directory] {
        match fs::symlink_metadata(path) {
            Ok(info) if info.file_type().is_symlink() => {
                return Err(format!(
                    "held path must not be a symlink: {}",
                    path.display()
                ))
            }
            Ok(info) if !info.is_dir() => {
                return Err(format!("held path must be a directory: {}", path.display()))
            }
            #[cfg(unix)]
            Ok(info)
                if path.as_path() == directory.as_path()
                    && info.permissions().mode() & 0o777 != 0o700 =>
            {
                return Err("held directory permissions are not private".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                builder.mode(0o700);
                builder.create(path).map_err(|error| error.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(directory)
}

fn parts(reference: &str) -> Result<(&str, &str, &str), String> {
    let value = reference
        .strip_prefix(PREFIX)
        .ok_or("held reference is invalid")?;
    let mut fields = value.split('_');
    let work = fields.next().ok_or("held reference is invalid")?;
    let assignment = fields.next().ok_or("held reference is invalid")?;
    let bytes = fields.next().ok_or("held reference is invalid")?;
    if fields.next().is_some()
        || work.len() != 64
        || assignment.len() != 64
        || bytes.len() != 64
        || ![work, assignment, bytes]
            .into_iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("held reference is invalid".into());
    }
    Ok((work, assignment, bytes))
}

fn reference(work: &str, assignment: &str, bytes: &[u8]) -> Result<(String, String), String> {
    let work = work
        .strip_prefix("smw_")
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("work handle is invalid")?;
    let assignment_sha256 = hash::text(assignment);
    let bytes_sha256 = hash::bytes(bytes);
    Ok((
        format!("{PREFIX}{work}_{assignment_sha256}_{bytes_sha256}"),
        bytes_sha256,
    ))
}

fn file(directory: &Path, reference: &str) -> Result<PathBuf, String> {
    let _ = parts(reference)?;
    Ok(directory.join(format!("{reference}.bin")))
}

pub(super) fn store(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    bytes: &[u8],
) -> Result<Held, String> {
    let directory = ensure_directory(loaded)?;
    let (reference, sha256) = reference(work, assignment, bytes)?;
    let path = file(&directory, &reference)?;
    let created = match fs::symlink_metadata(&path) {
        Ok(info) if info.file_type().is_symlink() || !info.is_file() => {
            return Err("held artifact is not a regular file".into())
        }
        Ok(_) => {
            let existing = read_checked(&path, &sha256)?;
            if existing != bytes {
                return Err("held artifact changed for the same reference".into());
            }
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&path).map_err(|error| error.to_string())?;
            file.write_all(bytes).map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            true
        }
        Err(error) => return Err(error.to_string()),
    };
    Ok(Held {
        reference,
        sha256,
        bytes: bytes.len(),
        created,
    })
}

fn read_checked(path: &Path, expected_sha256: &str) -> Result<Vec<u8>, String> {
    let info = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if info.file_type().is_symlink() || !info.is_file() {
        return Err("held artifact is not a regular file".into());
    }
    #[cfg(unix)]
    if info.permissions().mode() & 0o777 != 0o600 {
        return Err("held artifact permissions are not private".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if hash::bytes(&bytes) != expected_sha256 {
        return Err("held artifact changed or has an invalid hash".into());
    }
    Ok(bytes)
}

pub(super) fn read(
    loaded: &Loaded,
    reference: &str,
    work: &str,
    assignment: &str,
) -> Result<Vec<u8>, String> {
    let (work_part, assignment_part, bytes_part) = parts(reference)?;
    let expected_work = work.strip_prefix("smw_").ok_or("work handle is invalid")?;
    if work_part != expected_work || assignment_part != hash::text(assignment) {
        return Err("held artifact is stale for the current assignment".into());
    }
    let directory = checked_directory(loaded)?;
    let path = file(&directory, reference)?;
    read_checked(&path, bytes_part)
}

pub(super) fn existing(
    loaded: &Loaded,
    reference: &str,
    work: &str,
    assignment: &str,
) -> Result<Held, String> {
    let (work_part, assignment_part, bytes_part) = parts(reference)?;
    let expected_work = work.strip_prefix("smw_").ok_or("work handle is invalid")?;
    if work_part != expected_work || assignment_part != hash::text(assignment) {
        return Err("held artifact is stale for the current assignment".into());
    }
    let bytes = read(loaded, reference, work, assignment)?;
    Ok(Held {
        reference: reference.to_owned(),
        sha256: bytes_part.to_owned(),
        bytes: bytes.len(),
        created: false,
    })
}

pub(super) fn remove(loaded: &Loaded, reference: &str) -> Result<(), String> {
    let path = file(&checked_directory(loaded)?, reference)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn describe(reference: &str, bytes: &[u8]) -> Value {
    json!({
        "reference": reference,
        "sha256": hash::bytes(bytes),
        "bytes": bytes.len(),
        "action": "resubmit_after_replan_or_evidence",
    })
}

pub(super) fn response(held: &Held) -> Value {
    json!({
        "reference": held.reference,
        "sha256": held.sha256,
        "bytes": held.bytes,
        "action": "resubmit_after_replan_or_evidence",
    })
}

pub(super) fn discover(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
) -> Result<Vec<Value>, String> {
    let candidate = directory(loaded);
    if matches!(
        fs::symlink_metadata(candidate),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    ) {
        return Ok(Vec::new());
    }
    let directory = checked_directory(loaded)?;
    let entries = fs::read_dir(directory).map_err(|error| error.to_string())?;
    let work_part = work.strip_prefix("smw_").ok_or("work handle is invalid")?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let info = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if info.file_type().is_symlink() || !info.is_file() {
            return Err("held directory contains a non-regular entry".into());
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            return Err("held artifact filename is not valid UTF-8".into());
        };
        let Some(reference) = name.strip_suffix(".bin") else {
            continue;
        };
        let Ok((candidate_work, assignment_sha256, bytes_sha256)) = parts(reference) else {
            continue;
        };
        if candidate_work != work_part || assignment_sha256 != hash::text(assignment) {
            continue;
        }
        let bytes = read_checked(&path, bytes_sha256)?;
        found.push(describe(reference, &bytes));
    }
    found.sort_by(|left, right| left["reference"].as_str().cmp(&right["reference"].as_str()));
    Ok(found)
}

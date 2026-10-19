//! Immutable, bounded records in existing confined StateRoot storage.
use crate::{config::Loaded, evidence::hash, project::path};
use serde_json::Value;
use std::io::Write;

pub(super) const MAX_BYTES: u64 = 256 * 1024;
pub(super) fn name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "shipping target must be 1..64 ASCII letters, digits, hyphens or underscores".into(),
        );
    }
    Ok(())
}
pub(super) fn relative(id: &str, leaf: &str) -> Result<String, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid shipping identity".into());
    }
    name(leaf)?;
    Ok(format!(
        "{}/shipping/{id}/{leaf}",
        crate::project::layout_types::state_namespace()
    ))
}
pub(super) fn read(loaded: &Loaded, id: &str, leaf: &str) -> Result<Option<Vec<u8>>, String> {
    match path::secure_bytes_observation_single_link_bounded(
        &loaded.state_root,
        &relative(id, leaf)?,
        "shipping record",
        MAX_BYTES,
    ) {
        path::SecureBytesResult::Bytes(bytes) => Ok(Some(bytes)),
        path::SecureBytesResult::Absent(_) => Ok(None),
        path::SecureBytesResult::Unsafe(e) | path::SecureBytesResult::Unreadable(e) => Err(e),
        #[cfg(not(unix))]
        path::SecureBytesResult::Unsupported(e) => Err(e),
    }
}
pub(super) fn get(loaded: &Loaded, id: &str, leaf: &str) -> Result<Option<Value>, String> {
    read(loaded, id, leaf)?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|e| format!("partial or invalid shipping record; hold: {e}"))
        })
        .transpose()
}
pub(super) fn create(loaded: &Loaded, id: &str, leaf: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("shipping record exceeds 256 KiB".into());
    }
    #[cfg(unix)]
    {
        let destination = loaded.state_root.join(relative(id, leaf)?);
        let parent = destination
            .parent()
            .ok_or("shipping destination has no parent")?;
        crate::project::managed_files::ensure_state_directory(&loaded.state_root, parent)?;
        let mut file =
            path::secure_create_new(&loaded.state_root, &relative(id, leaf)?, "shipping record")?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        Err("shipping requires no-follow Unix storage".into())
    }
}
pub(super) fn put(loaded: &Loaded, id: &str, leaf: &str, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if let Some(prior) = read(loaded, id, leaf)? {
        if prior == bytes {
            return Ok(());
        }
        return Err("shipping record collision; existing evidence preserved".into());
    }
    create(loaded, id, leaf, &bytes)
}
pub(super) fn packet(loaded: &Loaded, id: &str) -> Result<Value, String> {
    let value = get(loaded, id, "packet")?.ok_or("shipping packet unavailable")?;
    if hash::value(&value) != id
        || value["format"] != "accepted-local-ship-v1"
        || value["version"] != 1
    {
        return Err("stale or altered shipping packet".into());
    }
    let target = value["authority"]["target"]
        .as_str()
        .ok_or("packet target missing")?;
    name(target)?;
    if value["authority"]["kind"] != "owner_selected_local_staging"
        || value["authority"]["externalEffects"] != false
        || value["authority"]["actions"] != serde_json::json!(["stage_accepted_artifact"])
        || !value["source"]["artifacts"]
            .as_array()
            .is_some_and(|items| items.contains(&value["artifact"]))
    {
        return Err(
            "shipping packet does not bind an accepted artifact and local-only authority".into(),
        );
    }
    Ok(value)
}

/// Bounded discovery also preserves holds written by the initial packet-only
/// prototype. Record contents are still read through the no-follow helpers.
pub(super) fn identities(loaded: &Loaded) -> Result<Vec<String>, String> {
    let mut directory = loaded.state_root.clone();
    for part in [crate::project::layout_types::state_namespace(), "shipping"] {
        directory.push(part);
        match std::fs::symlink_metadata(&directory) {
            Ok(info) if info.is_dir() && !info.file_type().is_symlink() => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            _ => return Err("shipping history directory is unsafe or unavailable; hold".into()),
        }
    }
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        if ids.len() == 256 {
            return Err("shipping history exceeds bounded hold readback; hold".into());
        }
        let name = entry.map_err(|e| e.to_string())?.file_name();
        let id = name.to_str().ok_or("shipping identity is not UTF-8")?;
        relative(id, "packet")?;
        ids.push(id.to_owned());
    }
    ids.sort();
    Ok(ids)
}

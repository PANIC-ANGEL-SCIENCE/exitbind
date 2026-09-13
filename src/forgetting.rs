use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{config::Loaded, hash, memory};

const TERMINAL_STATES: [&str; 3] = ["rejected", "revoked", "expired"];

pub fn attest(
    loaded: &Loaded,
    actor: &str,
    ledger_name: &str,
    receipt_name: &str,
) -> Result<Value, String> {
    if loaded.source.is_empty() {
        return Err("loaded configuration is required".into());
    }
    let snapshot = memory::snapshot(loaded, ledger_name)?;
    if snapshot.items.len() != 1 {
        return Err("memory attestation requires a ledger with exactly one item".into());
    }
    let item = snapshot
        .items
        .values()
        .next()
        .ok_or("memory attestation requires one item")?;
    let state = item["state"].as_str().ok_or("invalid memory item state")?;
    if !TERMINAL_STATES.contains(&state) {
        return Err(format!("memory item is not terminal: {state}"));
    }

    let agent = loaded.config["agents"]
        .get(actor)
        .ok_or_else(|| format!("unknown agent '{actor}'"))?;
    let scope = item["scope"].as_str().ok_or("invalid memory item scope")?;
    if !agent["memoryForget"]
        .as_array()
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(scope)))
    {
        return Err(format!(
            "agent is not authorized for memoryForget scope '{scope}'"
        ));
    }

    let source_name = item["source"]["path"]
        .as_str()
        .ok_or("invalid memory source")?;
    let source_target =
        memory::confined_target(&loaded.product_root, source_name, "recorded source")?;
    let receipt_target = memory::confined_target(&loaded.state_root, receipt_name, "receipt")?;
    if source_target == receipt_target {
        return Err("receipt path must not equal the recorded source path".into());
    }

    let config_hash = hash::text(&loaded.source);
    for event in &snapshot.events {
        if event["configSha256"] != config_hash {
            return Err("configuration changed since memory event".into());
        }
        assert_event_profile_current(loaded, event)?;
    }

    if memory::path_exists_without_symlinks(&loaded.product_root, source_name, "recorded source")? {
        return Err("recorded memory source still exists".into());
    }
    let (receipt_path, parent) = memory::ensure_receipt_parent(&loaded.state_root, receipt_name)?;
    let targets = [receipt_path.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let profile_name = agent["profile"]
        .as_str()
        .ok_or_else(|| format!("agent '{actor}' has no profile"))?;
    let profile_path = profile_file(loaded, profile_name)?;
    let profile_text = memory::stable_text(&profile_path, "actor profile")?;
    let actor_profile = json!({
        "path": relative_project_path(&loaded.control_root, &profile_path)?,
        "sha256": hash::text(&profile_text),
    });
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut receipt = json!({
        "version": 1,
        "kind": "memory-forgetting",
        "producer": crate::producer::evidence(),
        "itemId": item["itemId"],
        "scope": scope,
        "terminalState": state,
        "memoryLedgerSha256": hash::text(&snapshot.source),
        "actor": actor,
        "actorProfile": actor_profile,
        "observed": "source-absent",
        "timestamp": timestamp,
        "limitations": ["local observation only; this does not prove secure erasure, backup deletion, remote deletion, or model forgetting"],
    });
    receipt["receiptSha256"] = json!(hash::value(&receipt));
    create_receipt(&loaded.state_root, &receipt_path, &parent, &receipt)?;
    Ok(receipt)
}

fn assert_event_profile_current(loaded: &Loaded, event: &Value) -> Result<(), String> {
    let actor = event["actor"].as_str().ok_or("invalid memory actor")?;
    let configured = loaded.config["agents"]
        .get(actor)
        .ok_or_else(|| format!("memory actor configuration changed: {actor}"))?;
    let profile_name = configured["profile"]
        .as_str()
        .ok_or_else(|| format!("memory actor configuration changed: {actor}"))?;
    let profile_path = profile_file(loaded, profile_name)?;
    let current_path = relative_project_path(&loaded.control_root, &profile_path)?;
    let current_hash = hash::text(&memory::stable_text(&profile_path, "actor profile")?);
    if event["actorProfile"]["path"] != current_path
        || event["actorProfile"]["sha256"] != current_hash
    {
        return Err(format!("memory actor profile changed: {actor}"));
    }
    Ok(())
}

fn profile_file(loaded: &Loaded, requested: &str) -> Result<PathBuf, String> {
    let candidate = memory::confined_target(&loaded.control_root, requested, "actor profile")?;
    let root = fs::canonicalize(&loaded.control_root).map_err(|_| {
        format!(
            "project root does not exist: {}",
            loaded.control_root.display()
        )
    })?;
    let real = fs::canonicalize(candidate).map_err(|error| error.to_string())?;
    if !real.starts_with(root) {
        return Err("path escapes project root".into());
    }
    let metadata = fs::symlink_metadata(&real).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("actor profile must be a regular file".into());
    }
    Ok(real)
}

fn create_receipt(root: &Path, path: &Path, parent: &Path, receipt: &Value) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (root, path, parent, receipt);
        return Err("receipt creation requires O_NOFOLLOW support".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let expected_parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
        if !expected_parent.starts_with(fs::canonicalize(root).map_err(|error| error.to_string())?)
        {
            return Err("path escapes project root: receipt".into());
        }
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(o_nofollow());
        let mut handle = options.open(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "receipt already exists; refusing to overwrite".to_owned()
            } else {
                error.to_string()
            }
        })?;
        if !handle
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("receipt must be a regular file".into());
        }
        let opened = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let expected =
            expected_parent.join(path.file_name().ok_or("receipt path must name a file")?);
        if opened != expected {
            return Err("receipt path changed while opening".into());
        }
        handle
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        let bytes = format!(
            "{}\n",
            serde_json::to_string_pretty(receipt).map_err(|error| error.to_string())?
        );
        handle
            .write_all(bytes.as_bytes())
            .map_err(|error| error.to_string())?;
        handle.sync_all().map_err(|error| error.to_string())?;
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

fn relative_project_path(root: &Path, path: &Path) -> Result<String, String> {
    let value = path
        .strip_prefix(root)
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

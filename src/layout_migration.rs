//! Explicit migration from legacy distribution-owned profiles to Soulmate-owned profiles.

use crate::{config::Loaded, hash, managed_files};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

const LEGACY_PREFIX: &str = ".agents/profiles/";
const LEGACY_HARNESS: &str = "harness-manifest.json";

struct Operation {
    agents: Vec<String>,
    source: String,
    target: String,
    source_path: PathBuf,
    target_path: PathBuf,
    bytes: Vec<u8>,
}

struct DirectoryOperation {
    root_name: &'static str,
    relative: &'static str,
    path: PathBuf,
}

pub(crate) fn prepare_paths(loaded: &Loaded, apply: bool) -> Result<Value, String> {
    let mut directories = Vec::new();
    let desired = crate::project_layout::control_dirs()
        .into_iter()
        .map(|relative| ("control", &loaded.control_root, relative))
        .chain(
            crate::project_layout::state_dirs()
                .into_iter()
                .map(|relative| ("state", &loaded.state_root, relative)),
        );
    for (root_name, root, relative) in desired {
        if let Some(path) = missing_directory(root, relative)? {
            directories.push(DirectoryOperation {
                root_name,
                relative,
                path,
            });
        }
    }

    let canonical = crate::harness_manifest::canonical_path(loaded);
    let manifest = match fs::symlink_metadata(loaded.control_root.join(canonical)) {
        Ok(_) => {
            regular_path(
                &loaded.control_root,
                canonical,
                "canonical harness manifest",
            )?;
            None
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::symlink_metadata(loaded.control_root.join(LEGACY_HARNESS)) {
                Ok(_) => {
                    let source = regular_path(
                        &loaded.control_root,
                        LEGACY_HARNESS,
                        "legacy harness manifest",
                    )?;
                    let target = absent_target(&loaded.control_root, canonical)?;
                    let bytes = fs::read(&source).map_err(|error| error.to_string())?;
                    Some((source, target, bytes))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err(error) => return Err(error.to_string()),
    };
    let directory_report = || {
        directories
            .iter()
            .map(|operation| {
                json!({
                    "action": "create-directory",
                    "root": operation.root_name,
                    "path": operation.relative,
                })
            })
            .collect::<Vec<_>>()
    };
    let manifest_report = || {
        manifest.as_ref().map(|(_, _, bytes)| {
            json!({
                "action": "copy-retain-legacy",
                "source": LEGACY_HARNESS,
                "target": canonical,
                "sha256": hash::bytes(bytes),
            })
        })
    };
    let unchanged = directories.is_empty() && manifest.is_none();
    if !apply || unchanged {
        return Ok(json!({
            "mode": if apply { "apply" } else { "dry-run" },
            "status": if unchanged { "unchanged" } else { "ready" },
            "directories": directory_report(),
            "manifest": manifest_report(),
            "historicalEvidenceRewritten": false,
        }));
    }

    let control_targets = directories
        .iter()
        .filter(|operation| operation.root_name == "control")
        .map(|operation| operation.path.as_path())
        .chain(manifest.iter().map(|(_, target, _)| target.as_path()))
        .collect::<Vec<_>>();
    let state_targets = directories
        .iter()
        .filter(|operation| operation.root_name == "state")
        .map(|operation| operation.path.as_path())
        .collect::<Vec<_>>();
    crate::git_preflight::refuse_tracked_targets(&loaded.control_root, &control_targets)?;
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &state_targets)?;

    for operation in &directories {
        let root = if operation.root_name == "control" {
            &loaded.control_root
        } else {
            &loaded.state_root
        };
        managed_files::ensure_managed_directory(root, &operation.path)?;
    }
    if let Some((source, target, bytes)) = &manifest {
        if fs::read(regular_path(
            &loaded.control_root,
            LEGACY_HARNESS,
            "legacy harness manifest",
        )?)
        .map_err(|error| error.to_string())?
            != *bytes
        {
            return Err("legacy harness manifest changed during migration".into());
        }
        absent_target(&loaded.control_root, canonical)?;
        managed_files::write_exclusive(target, bytes)?;
        if fs::read(source).map_err(|error| error.to_string())? != *bytes {
            let _ = fs::remove_file(target);
            return Err("legacy harness manifest changed during migration".into());
        }
    }
    Ok(json!({
        "mode": "apply",
        "status": "applied",
        "directories": directory_report(),
        "manifest": manifest_report(),
        "historicalEvidenceRewritten": false,
    }))
}

pub(crate) fn run(loaded: &Loaded, apply: bool) -> Result<Value, String> {
    let mut grouped = BTreeMap::<String, (String, Vec<String>)>::new();
    let mut next = loaded.config.clone();
    for (name, agent) in &loaded.agents {
        let source = agent.profile.replace('\\', "/");
        let Some(suffix) = source.strip_prefix(LEGACY_PREFIX) else {
            continue;
        };
        validate_suffix(suffix)?;
        let target = format!("{}/{suffix}", crate::project_layout::CANONICAL_AGENTS_DIR);
        grouped
            .entry(source)
            .or_insert_with(|| (target.clone(), Vec::new()))
            .1
            .push(name.clone());
        next["agents"][name]["profile"] = json!(target);
    }
    let errors = crate::config::validate(&next);
    if !errors.is_empty() {
        return Err(format!(
            "migrated configuration would be invalid:\n- {}",
            errors.join("\n- ")
        ));
    }

    let mut operations = Vec::new();
    for (source, (target, agents)) in grouped {
        let source_path = existing_regular(&loaded.control_root, &source)?;
        let target_path = absent_target(&loaded.control_root, &target)?;
        let bytes = fs::read(&source_path).map_err(|error| error.to_string())?;
        operations.push(Operation {
            agents,
            source,
            target,
            source_path,
            target_path,
            bytes,
        });
    }

    let mut next_source = serde_json::to_string_pretty(&next).map_err(|error| error.to_string())?;
    next_source.push('\n');
    let report = || {
        operations
            .iter()
            .map(|operation| {
                json!({
                    "agents": operation.agents,
                    "source": operation.source,
                    "target": operation.target,
                    "sha256": hash::bytes(&operation.bytes),
                })
            })
            .collect::<Vec<_>>()
    };
    if operations.is_empty() {
        return Ok(json!({
            "mode": if apply { "apply" } else { "dry-run" },
            "status": "unchanged",
            "operations": [],
            "configSha256Before": hash::text(&loaded.source),
            "configSha256After": hash::text(&loaded.source),
        }));
    }
    if !apply {
        return Ok(json!({
            "mode": "dry-run",
            "status": "ready",
            "operations": report(),
            "configSha256Before": hash::text(&loaded.source),
            "configSha256After": hash::text(&next_source),
        }));
    }

    let mut targets = vec![loaded.path.as_path()];
    for operation in &operations {
        targets.push(operation.source_path.as_path());
        targets.push(operation.target_path.as_path());
    }
    crate::git_preflight::refuse_tracked_targets(&loaded.control_root, &targets)?;

    unchanged(loaded)?;
    for operation in &operations {
        let current = existing_regular(&loaded.control_root, &operation.source)?;
        if fs::read(current).map_err(|error| error.to_string())? != operation.bytes {
            return Err(format!(
                "legacy profile changed after planning: {}",
                operation.source
            ));
        }
        absent_target(&loaded.control_root, &operation.target)?;
    }

    let mut created = Vec::new();
    let result = (|| {
        for operation in &operations {
            managed_files::ensure_managed_directory(
                &loaded.control_root,
                operation
                    .target_path
                    .parent()
                    .ok_or("migration target has no parent")?,
            )?;
            managed_files::write_exclusive(&operation.target_path, &operation.bytes)?;
            created.push(operation.target_path.clone());
        }
        unchanged(loaded)?;
        for operation in &operations {
            if fs::read(existing_regular(&loaded.control_root, &operation.source)?)
                .map_err(|error| error.to_string())?
                != operation.bytes
            {
                return Err(format!(
                    "legacy profile changed during migration: {}",
                    operation.source
                ));
            }
        }

        let name = loaded
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("configuration path must name a UTF-8 file")?;
        let temporary = loaded.path.with_file_name(format!(
            ".{name}.exitbind-layout-{}.tmp",
            std::process::id()
        ));
        managed_files::write_exclusive(&temporary, next_source.as_bytes())?;
        fs::set_permissions(
            &temporary,
            fs::metadata(&loaded.path)
                .map_err(|error| error.to_string())?
                .permissions(),
        )
        .map_err(|error| error.to_string())?;
        unchanged(loaded)?;
        if let Err(error) = fs::rename(&temporary, &loaded.path) {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        Ok(())
    })();
    if let Err(error) = result {
        for path in created.iter().rev() {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }

    for operation in &operations {
        if fs::read(existing_regular(&loaded.control_root, &operation.source)?)
            .map_err(|error| error.to_string())?
            != operation.bytes
        {
            return Err(format!(
                "configuration migrated but legacy profile changed before cleanup: {}",
                operation.source
            ));
        }
        fs::remove_file(&operation.source_path).map_err(|error| {
            format!(
                "configuration migrated but legacy cleanup failed for {}: {error}",
                operation.source
            )
        })?;
    }
    prune_empty_legacy_directories(loaded, &operations);
    Ok(json!({
        "mode": "apply",
        "status": "applied",
        "operations": report(),
        "configSha256Before": hash::text(&loaded.source),
        "configSha256After": hash::text(&next_source),
    }))
}

fn validate_suffix(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.split('/').any(|component| component.is_empty())
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "legacy profile path is not portable: {LEGACY_PREFIX}{value}"
        ));
    }
    Ok(())
}

fn existing_regular(root: &Path, relative: &str) -> Result<PathBuf, String> {
    regular_path(root, relative, "legacy profile")
}

fn regular_path(root: &Path, relative: &str, label: &str) -> Result<PathBuf, String> {
    let path = checked_path(root, relative, true)?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!("{label} must be a regular file: {relative}"));
    }
    Ok(path)
}

fn missing_directory(root: &Path, relative: &'static str) -> Result<Option<PathBuf>, String> {
    let path = checked_path(root, relative, false)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() => Ok(None),
        Ok(_) => Err(format!(
            "migration directory must be a real directory: {relative}"
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(path)),
        Err(error) => Err(error.to_string()),
    }
}

fn absent_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = checked_path(root, relative, false)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => Err(format!("migration target already exists: {relative}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error.to_string()),
    }
}

fn checked_path(root: &Path, relative: &str, require_all: bool) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("migration path escapes ControlRoot: {relative}"));
    }
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "migration path must not contain symlinks: {relative}"
                    ));
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Err(format!("migration parent must be a directory: {relative}"));
                }
            }
            Err(error) if !require_all && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(current)
}

fn unchanged(loaded: &Loaded) -> Result<(), String> {
    if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source {
        return Err("configuration changed during layout migration".into());
    }
    Ok(())
}

fn prune_empty_legacy_directories(loaded: &Loaded, operations: &[Operation]) {
    let legacy_root = loaded.control_root.join(".agents/profiles");
    for operation in operations {
        let mut current = operation.source_path.parent();
        while let Some(directory) = current {
            if !directory.starts_with(&legacy_root) || fs::remove_dir(directory).is_err() {
                break;
            }
            if directory == legacy_root {
                break;
            }
            current = directory.parent();
        }
    }
}

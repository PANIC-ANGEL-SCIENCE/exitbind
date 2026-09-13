//! Shallow, no-follow discovery of configured project-local memory ledgers.

use crate::{config, memory, memory_policy};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) struct DiscoveredLedger {
    pub(crate) path: String,
    pub(crate) snapshot: memory::LedgerSnapshot,
}

pub(crate) fn discover(loaded: &config::Loaded) -> Result<Option<Vec<DiscoveredLedger>>, String> {
    let Some(policy) = memory_policy::get(&loaded.config) else {
        return Ok(None);
    };
    let directory = resolve_directory(&loaded.state_root, &policy.root)?;
    let Some(directory) = directory else {
        return Ok(Some(Vec::new()));
    };
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|error| format!("memory root: {error}"))? {
        let entry = entry.map_err(|error| format!("memory root entry: {error}"))?;
        let file_name = entry.file_name();
        if Path::new(&file_name)
            .extension()
            .and_then(|value| value.to_str())
            != Some("jsonl")
        {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("memory ledger {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "memory ledger must not be a symlink: {}",
                path.display()
            ));
        }
        if !metadata.is_file() {
            return Err(format!(
                "memory ledger must be a regular file: {}",
                path.display()
            ));
        }
        let name = file_name
            .to_str()
            .ok_or_else(|| format!("memory ledger filename is not UTF-8: {}", path.display()))?;
        entries.push(name.to_owned());
    }
    entries.sort();
    entries
        .into_iter()
        .map(|name| {
            let relative = config::rel(loaded.state_root.as_path(), &directory.join(name))?;
            let snapshot = memory::snapshot(loaded, &relative)?;
            Ok(DiscoveredLedger {
                path: relative,
                snapshot,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

fn resolve_directory(root: &Path, requested: &str) -> Result<Option<PathBuf>, String> {
    let real_root =
        fs::canonicalize(root).map_err(|error| format!("project root does not exist: {error}"))?;
    let mut current = real_root;
    for component in Path::new(requested).components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::CurDir) {
                continue;
            }
            return Err("memory.root must stay inside the project".into());
        };
        let next = current.join(name);
        match fs::symlink_metadata(&next) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "memory root must not contain symlinks: {}",
                    next.display()
                ))
            }
            Ok(metadata) if metadata.is_dir() => current = next,
            Ok(_) => {
                return Err(format!(
                    "memory root must be a directory: {}",
                    next.display()
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("memory root: {error}")),
        }
    }
    Ok(Some(current))
}

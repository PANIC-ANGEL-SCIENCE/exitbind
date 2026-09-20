//! Tested-input identity for v6 checked runs.
//!
//! Coverage: every regular file and symlink under the product root that Git
//! would consider (tracked plus untracked, non-ignored files) or, outside a Git
//! worktree, every file found by walking the product root. Exitbind state
//! directories and `.git` are excluded. Ignored files, files outside the
//! product root, the environment, remote services, and time are not covered;
//! evidence reuse never claims them.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const COVERAGE: &str = "product-root-files-v1";

const EXCLUDED_DIRS: [&str; 3] = [".git", ".exitbind", ".soulmate"];

/// Digest of the current tested inputs, or an error when they cannot be
/// established (which callers must treat as "not reusable").
pub(crate) fn fingerprint(loaded: &crate::config::Loaded) -> Result<String, String> {
    let root = &loaded.product_root;
    let mut paths = match crate::project::git_preflight::worktree_root(root)? {
        Some(_) => git_paths(root)?,
        None => walk_paths(root)?,
    };
    paths.retain(|path| !excluded(root, &loaded.state_root, path));
    paths.sort();
    paths.dedup();
    let mut entries = Vec::with_capacity(paths.len());
    for relative in paths {
        let text = relative
            .to_str()
            .ok_or("tested input paths must be UTF-8")?
            .to_owned();
        let full = root.join(&relative);
        let entry = match fs::symlink_metadata(&full) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                json!([text, "deleted"])
            }
            Err(error) => return Err(format!("tested input cannot be read: {error}")),
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = fs::read_link(&full).map_err(|error| error.to_string())?;
                let target = target
                    .to_str()
                    .ok_or("tested input symlink targets must be UTF-8")?;
                json!([text, "symlink", crate::evidence::hash::text(target)])
            }
            Ok(meta) if meta.is_file() => {
                json!([text, "file", crate::evidence::hash::file(&full)?])
            }
            Ok(_) => continue,
        };
        entries.push(entry);
    }
    Ok(crate::evidence::hash::value(
        &json!({"coverage": COVERAGE, "entries": Value::Array(entries)}),
    ))
}

fn excluded(root: &Path, state_root: &Path, relative: &Path) -> bool {
    let first = relative
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str());
    if first.is_some_and(|name| EXCLUDED_DIRS.contains(&name)) {
        return true;
    }
    state_root != root && root.join(relative).starts_with(state_root)
}

fn git_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            ".",
        ])
        .output()
        .map_err(|error| format!("tested inputs cannot be listed: {error}"))?;
    if !output.status.success() {
        return Err("tested inputs cannot be listed by Git".into());
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| {
            std::str::from_utf8(part)
                .map(PathBuf::from)
                .map_err(|_| "tested input paths must be UTF-8".to_owned())
        })
        .collect()
}

fn walk_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let entries = fs::read_dir(root.join(&relative))
            .map_err(|error| format!("tested inputs cannot be listed: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = relative.join(entry.file_name());
            if relative.as_os_str().is_empty()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| EXCLUDED_DIRS.contains(&name))
            {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_state_directories_do_not_hide_sources() {
        let root = Path::new("/p");
        assert!(excluded(root, root, Path::new(".exitbind/runs/a.jsonl")));
        assert!(excluded(root, root, Path::new(".git/HEAD")));
        assert!(!excluded(root, root, Path::new("src/.exitbind.rs")));
        assert!(!excluded(root, root, Path::new("checks/preserve.sh")));
    }
}

//! Bounded, content-free file result references for a direct native task.

use crate::evidence::hash;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};
use std::process::Command;

const MAX_PATH_LIST_BYTES: usize = 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) struct Snapshot {
    files: BTreeMap<String, Option<String>>,
}

pub(crate) fn capture(root: &Path) -> Result<Snapshot, &'static str> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(root)
        .output()
        .map_err(|_| "git_inventory_unavailable")?;
    if !output.status.success() || output.stdout.len() > MAX_PATH_LIST_BYTES {
        return Err("git_inventory_unavailable");
    }
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        if files.len() >= MAX_FILES {
            return Err("file_count_limit");
        }
        let path = std::str::from_utf8(raw).map_err(|_| "non_utf8_path")?;
        let relative = Path::new(path);
        if relative.is_absolute()
            || !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err("unsafe_inventory_path");
        }
        let absolute = root.join(relative);
        let metadata = match fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                files.insert(path.to_owned(), None);
                continue;
            }
            Err(_) => return Err("file_changed_during_capture"),
        };
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err("unsupported_or_large_file");
        }
        total = total.saturating_add(metadata.len());
        if total > MAX_TOTAL_BYTES {
            return Err("total_file_limit");
        }
        let bytes = fs::read(&absolute).map_err(|_| "file_changed_during_capture")?;
        if bytes.len() as u64 != metadata.len() {
            return Err("file_changed_during_capture");
        }
        if files
            .insert(path.to_owned(), Some(hash::bytes(&bytes)))
            .is_some()
        {
            return Err("duplicate_inventory_path");
        }
    }
    Ok(Snapshot { files })
}

pub(crate) fn result(
    before: Result<Snapshot, &'static str>,
    after: Result<Snapshot, &'static str>,
) -> Value {
    let (Ok(before), Ok(after)) = (before, after) else {
        return json!({
            "status": "gap",
            "scope": "git_tracked_and_unignored_files",
            "reason": "bounded_file_capture_unavailable",
        });
    };
    let mut changes = Vec::new();
    let paths = before
        .files
        .keys()
        .chain(after.files.keys())
        .collect::<BTreeSet<_>>();
    for path in paths {
        let old = before.files.get(path).and_then(Option::as_ref);
        let new = after.files.get(path).and_then(Option::as_ref);
        if old != new {
            changes.push(json!({"path": path, "beforeSha256": old, "afterSha256": new}));
        }
    }
    let reference =
        hash::value(&json!({"scope": "git_tracked_and_unignored_files", "changes": changes}));
    json!({
        "status": "observed",
        "scope": "git_tracked_and_unignored_files",
        "referenceSha256": reference,
        "changes": changes,
        "attribution": "before_after_only",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_result_references_new_file_bytes_without_accepting_the_task() {
        let root = std::env::temp_dir().join(format!(
            "exitbind-direct-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        let before = capture(&root);
        fs::write(root.join("result.txt"), b"direct recording works.\n").unwrap();
        let observed = result(before, capture(&root));
        assert_eq!(observed["status"], "observed");
        assert_eq!(observed["changes"][0]["path"], "result.txt");
        assert_eq!(
            observed["changes"][0]["afterSha256"],
            hash::bytes(b"direct recording works.\n")
        );
        assert!(observed.get("acceptance").is_none());
        fs::remove_dir_all(root).unwrap();
    }
}

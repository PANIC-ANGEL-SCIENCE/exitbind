//! The one writer for product files under a root: plan cards, task-card
//! snapshots, carried lesson sources and the stamped configuration copy.
//! Before a file is written its directory is added to the local Git exclude
//! (`info/exclude`, never a tracked file), and after it is written `git status`
//! must list nothing under that directory; otherwise a newly created file is
//! removed and the write refuses. A root whose Git marker Git cannot read
//! refuses before writing, since such a file could be listed. A root outside
//! any Git worktree is written as is.
//!
//! Files are created once through real directories and never over another
//! file: identical bytes reuse the existing file, and different bytes either
//! refuse (`write_once`) or take a fresh suffixed directory (`write_suffixed`,
//! the plan-card route).

mod git;

use crate::project::path::{self, SecureBytesResult};
use std::path::Path;
use std::time::{Duration, Instant};

/// Fresh suffixed directories tried for one slug.
const MAX_SUFFIX: usize = 32;
const GIT_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Written {
    Created,
    Reused,
}

/// Write `bytes` once at `<directory>/<name>` under `root`. An identical
/// existing file is reused; a different one refuses.
pub(crate) fn write_once(
    root: &Path,
    directory: &str,
    name: &str,
    bytes: &[u8],
    label: &str,
) -> Result<Written, String> {
    let relative = format!("{directory}/{name}");
    let git = hide(root, directory, label)?;
    match existing(root, &relative, label)? {
        Some(current) if current == bytes => {
            return finish(root, &git, directory, &relative, Written::Reused, label)
        }
        Some(_) => {
            return Err(format!(
                "{label} already exists with different bytes: {relative}"
            ))
        }
        None => {}
    }
    create(root, &relative, bytes, label)?;
    finish(root, &git, directory, &relative, Written::Created, label)
}

/// `write_once` for a root that is not the project root, such as a
/// local-mode state root: it belongs to no project worktree, so no Git
/// exclude is read or written, even when the directory happens to sit
/// inside an unrelated repository.
pub(crate) fn write_once_outside_project(
    root: &Path,
    directory: &str,
    name: &str,
    bytes: &[u8],
    label: &str,
) -> Result<Written, String> {
    let relative = format!("{directory}/{name}");
    match existing(root, &relative, label)? {
        Some(current) if current == bytes => Ok(Written::Reused),
        Some(_) => Err(format!(
            "{label} already exists with different bytes: {relative}"
        )),
        None => create(root, &relative, bytes, label).map(|()| Written::Created),
    }
}

/// Write `bytes` at `<directory>/<slug>/<file>`, then `<slug>-2`, `<slug>-3`,
/// ...: the first directory holding exactly these bytes is reused, the first
/// free one is created. Returns the root-relative path.
pub(crate) fn write_suffixed(
    root: &Path,
    directory: &str,
    slug: &str,
    file: &str,
    bytes: &[u8],
    label: &str,
) -> Result<(String, Written), String> {
    let git = hide(root, directory, label)?;
    let (relative, present) = place_suffixed(root, directory, slug, file, bytes, label)?;
    let written = if present {
        Written::Reused
    } else {
        create(root, &relative, bytes, label)?;
        Written::Created
    };
    finish(root, &git, directory, &relative, written, label).map(|written| (relative, written))
}

/// Read-only: where `write_suffixed` puts `bytes` and whether that file
/// already holds them.
pub(crate) fn place_suffixed(
    root: &Path,
    directory: &str,
    slug: &str,
    file: &str,
    bytes: &[u8],
    label: &str,
) -> Result<(String, bool), String> {
    for index in 1..=MAX_SUFFIX {
        let relative = format!("{directory}/{}/{file}", suffixed(slug, index));
        // A different file, a symlink or anything unreadable occupies the
        // name: it is never followed or replaced, the next name is tried.
        match path::secure_bytes_observation(root, &relative, label) {
            SecureBytesResult::Bytes(current) if current == bytes => return Ok((relative, true)),
            SecureBytesResult::Absent(_) => return Ok((relative, false)),
            _ => {}
        }
    }
    Err(format!(
        "no fresh {label} name is available under {directory}/{slug}"
    ))
}

/// The directory name for attempt `index`: the slug, then `slug-2`, `slug-3`,
/// ... kept within the 64-character Goal ID bound.
pub(crate) fn suffixed(slug: &str, index: usize) -> String {
    if index == 1 {
        return slug.to_owned();
    }
    let suffix = format!("-{index}");
    let keep = slug.len().min(64 - suffix.len());
    format!("{}{suffix}", &slug[..keep])
}

/// Read-only: refuse now, before any write, when `root` has a Git marker Git
/// cannot read, which would make the writer refuse later.
pub(crate) fn writable(root: &Path, label: &str) -> Result<(), String> {
    open(root, label).map(|_| ())
}

fn open(root: &Path, label: &str) -> Result<Option<git::Git>, String> {
    match git::Git::open(root, Instant::now() + GIT_DEADLINE) {
        Some(git) => Ok(Some(git)),
        None if root.ancestors().any(|dir| dir.join(".git").exists()) => Err(format!(
            "{label} was not written: the Git marker for {} cannot be read by git, so the file could be listed by Git",
            root.display()
        )),
        None => Ok(None),
    }
}

/// Add `directory` to the local Git exclude of the worktree holding `root`.
/// `None` when `root` is in no Git worktree.
fn hide(root: &Path, directory: &str, label: &str) -> Result<Option<git::Git>, String> {
    match open(root, label)? {
        Some(git) => {
            git.exclude(directory).ok_or_else(|| {
                format!(
                    "{label} was not written: {directory}/ could not be added to the local Git exclude of {}",
                    root.display()
                )
            })?;
            Ok(Some(git))
        }
        None => Ok(None),
    }
}

fn existing(root: &Path, relative: &str, label: &str) -> Result<Option<Vec<u8>>, String> {
    match path::secure_bytes_observation(root, relative, label) {
        SecureBytesResult::Bytes(bytes) => Ok(Some(bytes)),
        SecureBytesResult::Absent(_) => Ok(None),
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => Err(error),
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => Err(error),
    }
}

#[cfg(unix)]
fn create(root: &Path, relative: &str, bytes: &[u8], label: &str) -> Result<(), String> {
    use std::io::Write;
    let target = root.join(relative);
    if let Some(parent) = target.parent() {
        crate::project::managed_files::ensure_state_directory(root, parent)?;
    }
    crate::project::git_preflight::refuse_tracked_targets(root, &[target.as_path()])?;
    let mut file = path::secure_create_new(root, relative, label)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{label}: {error}"));
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(&target);
    }
    written
}

#[cfg(not(unix))]
fn create(_: &Path, _: &str, _: &[u8], label: &str) -> Result<(), String> {
    Err(format!("{label} requires a POSIX host"))
}

/// Prove Git lists nothing under `directory`; a file this call created is
/// removed again when it would be listed.
fn finish(
    root: &Path,
    git: &Option<git::Git>,
    directory: &str,
    relative: &str,
    written: Written,
    label: &str,
) -> Result<Written, String> {
    if git.as_ref().is_some_and(|git| !git.unlisted(directory)) {
        if written == Written::Created {
            let _ = std::fs::remove_file(root.join(relative));
        }
        return Err(format!(
            "{label} was not kept: git status lists {directory}/ although the local exclude names it"
        ));
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::suffixed;

    #[test]
    fn suffixed_names_stay_within_the_goal_id_bound() {
        assert_eq!(suffixed("r9", 1), "r9");
        assert_eq!(suffixed("r9", 2), "r9-2");
        let long = "a".repeat(64);
        assert_eq!(suffixed(&long, 12).len(), 64);
        assert!(suffixed(&long, 12).ends_with("-12"));
    }
}

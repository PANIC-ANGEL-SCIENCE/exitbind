//! The one temp root for integration tests. `temp(label)` creates a fresh
//! directory under the platform temp directory and returns a guard: when the
//! test ends normally the guard removes the directory; when the test fails
//! (the guard drops while its thread panics) it keeps the directory and names
//! it on the test's output, which libtest shows for the failing test.
//!
//! A test that needs a temp directory takes it from here; the temp-root scan in
//! `tests/temp_root_contract/temp_roots.rs` fails on any other acquisition.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// Every root name this process created, so one root is never "beside" another.
static CREATED: Mutex<BTreeSet<OsString>> = Mutex::new(BTreeSet::new());

/// The line a failing test prints for the directory it keeps.
pub const KEPT: &str = "kept test directory: ";

/// The line a passing test prints when its directory could not be removed.
pub const NOT_REMOVED: &str = "test directory not removed: ";

/// A test-owned directory, removed on drop unless the test is failing.
#[derive(Debug)]
pub struct TempRoot {
    path: PathBuf,
    /// Entries already beside the root when it was created (a stale sibling
    /// of an earlier process that had the same PID): not this test's.
    preexisting: BTreeSet<OsString>,
}

/// A new, empty directory `exitbind-<label>-<pid>-<sequence>` owned by the
/// calling test. Bind it to a named local (`let root = temp(..)`) so it lives
/// until the test ends; a temporary guard removes its directory at once.
#[must_use = "the directory is removed when the guard drops"]
pub fn temp(label: &str) -> TempRoot {
    temp_in(None, label)
}

/// `temp` under `parent` instead of the platform temp directory (`None`).
#[must_use = "the directory is removed when the guard drops"]
pub fn temp_in(parent: Option<&Path>, label: &str) -> TempRoot {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = parent.map_or_else(std::env::temp_dir, Path::to_path_buf);
    loop {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("exitbind-{label}-{}-{sequence}", std::process::id());
        if let Some(root) = create_root(&parent, &name) {
            return root;
        }
    }
}

/// The new directory `parent/name`, or `None` when that name is taken. The
/// entries already beside it are recorded, so a stale sibling an earlier
/// process with the same PID left never fails this test.
pub fn create_root(parent: &Path, name: &str) -> Option<TempRoot> {
    match std::fs::create_dir(parent.join(name)) {
        Ok(()) => {
            let path =
                std::fs::canonicalize(parent.join(name)).expect("canonicalize test directory");
            if let Some(name) = path.file_name() {
                CREATED
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(name.to_owned());
            }
            let preexisting = beside(&path, &BTreeSet::new())
                .into_iter()
                .filter_map(|entry| entry.file_name().map(OsStr::to_owned))
                .collect();
            Some(TempRoot { path, preexisting })
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
        Err(error) => panic!("create test directory: {error}"),
    }
}

impl TempRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("{KEPT}{}", self.path.display());
            return;
        }
        // Not an assertion (a late writer must not fail a passing test), but
        // always shown: written to the process's stderr, past libtest's
        // capture of a passing test's output.
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr(),
                    "{NOT_REMOVED}{} ({error})",
                    self.path.display()
                );
            }
        }
        let beside = beside(&self.path, &self.preexisting);
        assert!(
            beside.is_empty(),
            "a passing test left {beside:?} beside its temp root {}; create it inside the \
             root or take another root from temp()",
            self.path.display()
        );
    }
}

/// Entries named `<root name>-…` or `<root name>.…` next to `root` that no
/// guard of this process created and that were not there before the root:
/// what a test derived beside its root.
fn beside(root: &Path, preexisting: &BTreeSet<OsString>) -> Vec<PathBuf> {
    let (Some(parent), Some(name)) = (root.parent(), root.file_name().and_then(OsStr::to_str))
    else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let created = CREATED.lock().unwrap_or_else(PoisonError::into_inner);
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let entry_name = entry.file_name();
            let derived = entry_name
                .to_str()
                .and_then(|text| text.strip_prefix(name))
                .is_some_and(|rest| rest.starts_with('-') || rest.starts_with('.'));
            derived && !created.contains(&entry_name) && !preexisting.contains(&entry_name)
        })
        .map(|entry| entry.path())
        .collect()
}

impl Deref for TempRoot {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempRoot {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<OsStr> for TempRoot {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

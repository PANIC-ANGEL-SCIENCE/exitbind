//! The one temp root for `src/` test modules (the integration-test twin is
//! `tests/support/temp_root.rs`). `temp(label)` creates a fresh directory under
//! the platform temp directory and returns a guard: when the test ends
//! normally the guard removes the directory; when the test fails (the guard
//! drops while its thread panics) it keeps the directory and names it on the
//! test's output, which libtest shows for the failing test.

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
pub(crate) const KEPT: &str = "kept test directory: ";

/// The line a passing test prints when its directory could not be removed.
pub(crate) const NOT_REMOVED: &str = "test directory not removed: ";

/// A test-owned directory, removed on drop unless the test is failing.
#[derive(Debug)]
pub(crate) struct TempRoot {
    path: PathBuf,
    /// Entries already beside the root when it was created (a stale sibling
    /// of an earlier process that had the same PID): not this test's.
    preexisting: BTreeSet<OsString>,
}

/// A new, empty directory `exitbind-<label>-<pid>-<sequence>` owned by the
/// calling test. Bind it to a named local (`let root = temp(..)`) so it lives
/// until the test ends; a temporary guard removes its directory at once.
#[must_use = "the directory is removed when the guard drops"]
pub(crate) fn temp(label: &str) -> TempRoot {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("exitbind-{label}-{}-{sequence}", std::process::id());
        if let Some(root) = create_root(&std::env::temp_dir(), &name) {
            return root;
        }
    }
}

/// The new directory `parent/name`, or `None` when that name is taken. The
/// entries already beside it are recorded, so a stale sibling an earlier
/// process with the same PID left never fails this test.
pub(crate) fn create_root(parent: &Path, name: &str) -> Option<TempRoot> {
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
    /// Keep the directory past the guard, for a fixture that reports a
    /// failure by value (its caller fails the test) and names the directory.
    pub(crate) fn keep(self) -> PathBuf {
        let mut this = std::mem::ManuallyDrop::new(self);
        std::mem::take(&mut this.preexisting);
        std::mem::take(&mut this.path)
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

#[cfg(test)]
mod tests {
    use super::{create_root, temp, KEPT, NOT_REMOVED};
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    /// Set only by `a_failing_test_keeps_and_names_its_directory` for its child.
    const SEED: &str = "EXITBIND_SEEDED_UNIT_TEMP_ROOT_FAILURE";
    /// Set only by `a_root_that_cannot_be_removed_is_named` for its child.
    const UNREMOVABLE: &str = "EXITBIND_SEEDED_UNIT_UNREMOVABLE_TEMP_ROOT";

    /// A stale sibling older than the root does not fail a passing test.
    #[test]
    fn a_sibling_older_than_the_root_does_not_fail_the_passing_test() {
        let outer = temp("unit-temp-root-reused");
        let name = "exitbind-unit-reused-1-0";
        let stale = outer.join(format!("{name}-store"));
        fs::create_dir(&stale).unwrap();
        let root = create_root(&outer, name).expect("the name is free");
        drop(root);
        assert!(stale.is_dir());
        assert!(!outer.join(name).exists());
    }

    /// Passes; in the child run that sets `UNREMOVABLE` it leaves a
    /// read-only directory inside its root.
    #[test]
    fn seeded_unremovable_root() {
        if std::env::var_os(UNREMOVABLE).is_none() {
            return;
        }
        let root = temp("unit-unremovable");
        let locked = root.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("file"), b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_root_that_cannot_be_removed_is_named() {
        use std::os::unix::fs::PermissionsExt;
        let outer = temp("unit-unremovable-parent");
        // raw-spawn: this unit-test binary re-runs one of its own tests; no exitbind command or session state.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "test_root::tests::seeded_unremovable_root",
                "--exact",
                "--test-threads=1",
            ])
            .env(UNREMOVABLE, "1")
            .env("TMPDIR", &outer)
            .env("TMP", &outer)
            .env("TEMP", &outer)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        let left: Vec<PathBuf> = fs::read_dir(&outer)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        for path in &left {
            let _ = fs::set_permissions(path.join("locked"), fs::Permissions::from_mode(0o700));
        }
        if left.is_empty() {
            return; // a privileged user removes read-only directories
        }
        let named = stderr
            .lines()
            .find_map(|line| line.strip_prefix(NOT_REMOVED))
            .unwrap_or_else(|| panic!("the unremoved root is not named:\n{stderr}"));
        assert!(
            named.starts_with(left[0].to_str().expect("UTF-8 test path")),
            "{named}"
        );
    }

    #[test]
    fn a_passing_test_leaves_no_directory() {
        let kept: PathBuf = {
            let root = temp("unit-temp-root-passing");
            fs::create_dir_all(root.join("nested/deeper")).unwrap();
            fs::write(root.join("nested/file"), b"x").unwrap();
            root.to_path_buf()
        };
        assert!(!kept.exists(), "{}", kept.display());
    }

    /// Fails only in the child run that sets `SEED`; otherwise it passes at once.
    #[test]
    fn seeded_failing_test() {
        if std::env::var_os(SEED).is_none() {
            return;
        }
        let root = temp("unit-seeded-failure");
        fs::write(root.join("evidence"), b"kept").unwrap();
        panic!("seeded failure");
    }

    #[test]
    fn a_failing_test_keeps_and_names_its_directory() {
        let outer = temp("unit-seeded-failure-parent");
        // raw-spawn: this unit-test binary re-runs one of its own tests; no exitbind command or session state.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "test_root::tests::seeded_failing_test",
                "--exact",
                "--test-threads=1",
            ])
            .env(SEED, "1")
            .env("TMPDIR", &outer)
            .env("TMP", &outer)
            .env("TEMP", &outer)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!output.status.success(), "{stdout}");
        assert!(stdout.contains("1 failed"), "{stdout}");
        let named = stdout
            .lines()
            .find_map(|line| line.strip_prefix(KEPT))
            .unwrap_or_else(|| panic!("no kept directory named:\n{stdout}"));
        let kept = PathBuf::from(named.trim_end());
        assert_eq!(fs::read(kept.join("evidence")).unwrap(), b"kept");
        assert!(kept.starts_with(&*outer), "{}", kept.display());
    }
}

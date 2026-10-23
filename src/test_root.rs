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

/// A test-owned directory, removed on drop unless the test is failing.
#[derive(Debug)]
pub(crate) struct TempRoot {
    path: PathBuf,
}

/// A new, empty directory `exitbind-<label>-<pid>-<sequence>` owned by the
/// calling test. Bind it to a named local (`let root = temp(..)`) so it lives
/// until the test ends; a temporary guard removes its directory at once.
#[must_use = "the directory is removed when the guard drops"]
pub(crate) fn temp(label: &str) -> TempRoot {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "exitbind-{label}-{}-{sequence}",
            std::process::id()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                let path = std::fs::canonicalize(path).expect("canonicalize test directory");
                if let Some(name) = path.file_name() {
                    CREATED
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(name.to_owned());
                }
                return TempRoot { path };
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("create test directory: {error}"),
        }
    }
}

impl TempRoot {
    /// Keep the directory past the guard, for a fixture that reports a
    /// failure by value (its caller fails the test) and names the directory.
    pub(crate) fn keep(self) -> PathBuf {
        let mut this = std::mem::ManuallyDrop::new(self);
        std::mem::take(&mut this.path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("{KEPT}{}", self.path.display());
            return;
        }
        // Best effort: the temp-root count is the check for a removal failure.
        let _ = std::fs::remove_dir_all(&self.path);
        let beside = beside(&self.path);
        assert!(
            beside.is_empty(),
            "a passing test left {beside:?} beside its temp root {}; create it inside the \
             root or take another root from temp()",
            self.path.display()
        );
    }
}

/// Entries named `<root name>-…` or `<root name>.…` next to `root` that no
/// guard of this process created: what a test derived beside its root.
fn beside(root: &Path) -> Vec<PathBuf> {
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
            derived && !created.contains(&entry_name)
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
    use super::{temp, KEPT};
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    /// Set only by `a_failing_test_keeps_and_names_its_directory` for its child.
    const SEED: &str = "EXITBIND_SEEDED_UNIT_TEMP_ROOT_FAILURE";

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

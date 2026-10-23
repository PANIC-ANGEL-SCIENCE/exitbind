//! The session recovery record's lock. Every reader-modify-writer of the
//! record takes it here, so a refusal raised before the lock was taken names
//! this record, never the run ledger whose lock primitive it shares.

use super::hint::FILE;
use std::path::Path;

/// The refusal when the record's lock could not be taken.
pub(super) const BUSY: &str =
    "the session recovery record is busy or unavailable; no mutation was made";

/// Run `action` under the lock of the recovery record in the native session
/// directory `root`. An error raised before the lock was taken is the
/// record's busy refusal; an error from inside `action` passes through
/// unchanged. Decided by whether the lock was entered, never by its text.
pub(super) fn with_record_lock<T>(
    root: &Path,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let ledger = crate::run::ledger::ledger_path(root, FILE, true)?;
    let mut entered = false;
    let result = crate::run::ledger::with_lock(&ledger, || {
        entered = true;
        action()
    });
    if entered {
        result
    } else {
        result.map_err(|_| BUSY.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_names_the_session_record_and_an_inner_error_passes_through() {
        let root = crate::test_root::temp("record-lock");
        // A live holder: this process, so the lock is not stale.
        let lock = root.join(format!("{FILE}.lock"));
        let holder = format!(
            "{{\"pid\":{},\"createdAt\":\"2026-10-23T00:00:00Z\"}}",
            std::process::id()
        );
        std::fs::write(&lock, holder).unwrap();
        let mut ran = false;
        let busy = with_record_lock(&root, || {
            ran = true;
            Ok(())
        });
        assert!(!ran);
        assert_eq!(busy.unwrap_err(), BUSY);

        std::fs::remove_file(&lock).unwrap();
        let inner = with_record_lock::<()>(&root, || {
            Err("run ledger is busy; no mutation was made".into())
        });
        assert_eq!(
            inner.unwrap_err(),
            "run ledger is busy; no mutation was made",
            "an error from inside the lock is never renamed"
        );
        assert!(!lock.exists(), "the lock is released");
    }
}

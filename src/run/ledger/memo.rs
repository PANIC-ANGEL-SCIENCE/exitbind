//! Per-process memo of validated ledger reads.
//!
//! One command can read the same unchanged ledger hundreds of times: a native
//! session Goal revalidates every journey member, and each successor load
//! walks its predecessor. Each read still opens and reads the file; only the
//! per-event validation and full reduction are skipped when the exact bytes
//! at the same canonical path were already validated by this process. Any
//! appended or rewritten byte changes the key, so a mutation is always
//! revalidated in full. Nothing is persisted.
use crate::evidence::hash;
use serde_json::Value;
use std::{cell::RefCell, path::Path};

/// Bounded so a long-lived process never accumulates ledgers without limit.
const MAX_ENTRIES: usize = 32;

struct Entry {
    path: std::path::PathBuf,
    source_sha256: String,
    events: Vec<Value>,
}

thread_local! {
    static VALIDATED: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

/// Return the validated events for these exact bytes, or validate them with
/// `validate` and remember the result. Errors are never remembered.
pub(super) fn validated(
    path: &Path,
    source: &str,
    validate: impl FnOnce() -> Result<Vec<Value>, String>,
) -> Result<Vec<Value>, String> {
    let source_sha256 = hash::text(source);
    let hit = VALIDATED.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|entry| entry.path == path && entry.source_sha256 == source_sha256)
            .map(|entry| entry.events.clone())
    });
    if let Some(events) = hit {
        return Ok(events);
    }
    let events = validate()?;
    VALIDATED.with(|entries| {
        let mut entries = entries.borrow_mut();
        if entries.len() >= MAX_ENTRIES {
            entries.remove(0);
        }
        entries.push(Entry {
            path: path.to_path_buf(),
            source_sha256,
            events: events.clone(),
        });
    });
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn same_bytes_reuse_validation_and_changed_bytes_revalidate() {
        let path = Path::new("/memo-test/work.jsonl");
        let mut calls = 0;
        let first = validated(path, "a\n", || {
            calls += 1;
            Ok(vec![json!(1)])
        })
        .unwrap();
        let again = validated(path, "a\n", || {
            calls += 1;
            Ok(vec![json!(2)])
        })
        .unwrap();
        assert_eq!((first, again, calls), (vec![json!(1)], vec![json!(1)], 1));
        let changed = validated(path, "a\nb\n", || {
            calls += 1;
            Ok(vec![json!(3)])
        })
        .unwrap();
        assert_eq!((changed, calls), (vec![json!(3)], 2));
        let other_path = validated(Path::new("/memo-test/other.jsonl"), "a\n", || {
            calls += 1;
            Err("invalid".into())
        });
        assert!(other_path.is_err());
        // A failed validation is not remembered.
        let retried = validated(Path::new("/memo-test/other.jsonl"), "a\n", || {
            calls += 1;
            Ok(vec![json!(4)])
        })
        .unwrap();
        assert_eq!((retried, calls), (vec![json!(4)], 4));
    }
}

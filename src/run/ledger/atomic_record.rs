//! Atomic replacement of an existing private protocol record.

use super::{open_read_nofollow, open_stage, stage_path, verify_opened, LedgerPath};
use std::{
    fs,
    io::{Read, Write},
};

/// Atomically replace one already-opened private record while its owning
/// protocol holds the appropriate lock. The caller supplies the exact bytes it
/// observed; the same staged commit and identity checks used by append apply.
pub(crate) fn replace_exact(
    ledger: &LedgerPath,
    expected: &str,
    replacement: &str,
) -> Result<(), String> {
    let mut original = open_read_nofollow(&ledger.path).map_err(|error| error.to_string())?;
    let metadata = original.metadata().map_err(|error| error.to_string())?;
    verify_opened(ledger, &metadata)?;
    let mut source = String::new();
    original
        .read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    if source != expected {
        return Err("private record differs from its observed bytes".into());
    }
    let staged = stage_path(&ledger.path);
    let result = (|| {
        let mut file = open_stage(&staged)?;
        file.write_all(replacement.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        verify_opened(ledger, &metadata)?;
        let mut current = String::new();
        open_read_nofollow(&ledger.path)
            .map_err(|error| error.to_string())?
            .read_to_string(&mut current)
            .map_err(|error| error.to_string())?;
        if current != expected {
            return Err("private record differs before commit".into());
        }
        fs::rename(&staged, &ledger.path).map_err(|error| error.to_string())
    })();
    let _ = fs::remove_file(&staged);
    result
}

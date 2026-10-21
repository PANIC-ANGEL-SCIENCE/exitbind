//! The stamped configuration copy: the exact configuration bytes lesson
//! events were stamped with, kept once beside the lesson ledgers so the
//! "configuration changed since memory event" refusal can name a real file
//! for `memory revalidate --from-config`. One file per stamped configuration
//! hash; it is never a store or a journal and is never read as current
//! configuration.

use super::LedgerSnapshot;
use crate::{config::Loaded, evidence::hash};
use std::path::PathBuf;

const PREFIX: &str = "stamped-config-";

fn file_name(sha256: &str) -> Option<String> {
    let short = sha256.get(..16)?;
    short
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit())
        .then(|| format!("{PREFIX}{short}.json"))
}

/// The memory root, relative to the state root, when a memory policy exists.
fn memory_root(loaded: &Loaded) -> Option<String> {
    super::policy::get(&loaded.config).map(|policy| policy.root.trim_end_matches('/').to_owned())
}

/// The absolute path of the kept copy for `sha256` when it exists with
/// exactly those bytes.
pub(crate) fn find(loaded: &Loaded, sha256: &str) -> Option<PathBuf> {
    let relative = format!("{}/{}", memory_root(loaded)?, file_name(sha256)?);
    match crate::project::path::secure_bytes_observation(
        &loaded.state_root,
        &relative,
        "stamped configuration copy",
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) if hash::bytes(&bytes) == sha256 => {
            Some(loaded.state_root.join(relative))
        }
        _ => None,
    }
}

/// Whether `path` is this project's kept copy of the bytes it holds.
pub(crate) fn is_kept_copy(loaded: &Loaded, path: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let (Ok(given), Some(kept)) = (
        std::fs::canonicalize(path),
        find(loaded, &hash::bytes(&bytes)),
    ) else {
        return false;
    };
    std::fs::canonicalize(kept).is_ok_and(|kept| kept == given)
}

/// Keep a byte copy of `loaded`'s configuration beside the lesson ledgers.
/// An identical existing copy is kept; returns its absolute path.
pub(crate) fn keep(loaded: &Loaded) -> Result<PathBuf, String> {
    let root = memory_root(loaded).ok_or("stamped configuration copy needs a memory policy")?;
    let sha256 = hash::text(&loaded.source);
    let name = file_name(&sha256).ok_or("configuration hash is invalid")?;
    // Under a state root inside a Git worktree (portable mode) the copy is a
    // product file under the root: the one private writer hides it.
    crate::project::private_write::write_once(
        &loaded.state_root,
        &root,
        &name,
        loaded.source.as_bytes(),
        "stamped configuration copy",
    )?;
    Ok(loaded.state_root.join(format!("{root}/{name}")))
}

/// The typed refusal for lesson events stamped with `bound`: the kept copy
/// when it exists, and the exact `memory revalidate` route for this ledger.
pub(crate) fn changed(loaded: &Loaded, ledger: &LedgerSnapshot, bound: &str) -> String {
    let current = hash::text(&loaded.source);
    let copy = find(loaded, bound).map(|path| path.display().to_string());
    let ledger_path = crate::config::rel(&loaded.state_root, &ledger.path)
        .unwrap_or_else(|_| ledger.path.display().to_string());
    crate::drift::refusal(
        crate::drift::Class::MemoryConfiguration,
        bound,
        Some(&current),
        copy.as_deref(),
        crate::drift::Route::Revalidate {
            lead: loaded.lead().unwrap_or("lead"),
            ledger: &ledger_path,
            kept_copy: copy.as_deref(),
            config: &loaded.path,
        },
    )
}

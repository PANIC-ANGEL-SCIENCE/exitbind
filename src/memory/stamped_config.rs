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
    // Reusing an existing copy runs no Git.
    if let Some(existing) = find(loaded, &sha256) {
        return Ok(existing);
    }
    let name = file_name(&sha256).ok_or("configuration hash is invalid")?;
    let label = "stamped configuration copy";
    if same_root(&loaded.state_root, &loaded.product_root) {
        // Portable mode: the copy is a product file under the project root,
        // so the one private writer hides it from the project's Git.
        crate::project::private_write::write_once(
            &loaded.state_root,
            &root,
            &name,
            loaded.source.as_bytes(),
            label,
        )?;
    } else {
        // Local mode: the state root is no part of the project worktree; a
        // repository that happens to contain it is never touched.
        crate::project::private_write::write_once_outside_project(
            &loaded.state_root,
            &root,
            &name,
            loaded.source.as_bytes(),
            label,
        )?;
    }
    Ok(loaded.state_root.join(format!("{root}/{name}")))
}

fn same_root(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right || matches!((left.canonicalize(), right.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Called by the one configuration writer (`settings::atomic_write`) with
/// the bytes it is about to replace at `target`: when they are a
/// configuration with a memory policy and a memory ledger carries their
/// hash, keep them beside the ledgers first, so every product-driven change
/// leaves the copy the refusal names. Anything else is not a stamped
/// configuration and is left alone; a failure to keep refuses the write.
pub(crate) fn keep_replaced(target: &std::path::Path, replaced: &str) -> Result<(), String> {
    // Cheap first: most managed files are no configuration with a memory root.
    let stamped_shape = serde_json::from_str::<serde_json::Value>(replaced)
        .is_ok_and(|value| value["memory"]["root"].is_string());
    if !stamped_shape {
        return Ok(());
    }
    let Ok(at) = std::fs::canonicalize(target) else {
        return Ok(());
    };
    let Ok(previous) =
        crate::config::from_source(at, replaced.to_owned(), "replaced configuration")
    else {
        return Ok(());
    };
    let sha256 = hash::text(replaced);
    let Ok(Some(ledgers)) = super::discovery::discover(&previous) else {
        return Ok(());
    };
    let carried = ledgers.iter().any(|ledger| {
        ledger
            .snapshot
            .events
            .iter()
            .any(|event| event["configSha256"] == sha256.as_str())
    });
    if carried {
        keep(&previous).map_err(|error| {
            format!("stamped configuration copy: {error}; no configuration was written")
        })?;
    }
    Ok(())
}

/// The typed refusal for lesson events stamped with `bound`: the kept copy
/// when it exists, and one `memory revalidate` route that covers this ledger
/// and every other project-lesson ledger stamped with the same hash. With
/// the copy, only ledgers the route would revalidate join; without it (a hand
/// edit the writer never saw) the route keeps the previous-configuration slot
/// and still covers every ledger stamped with that hash. A specialist
/// ledger, or one refused for another reason, keeps its own route.
pub(crate) fn changed(loaded: &Loaded, ledger: &LedgerSnapshot, bound: &str) -> String {
    let others = same_copy_ledgers(loaded, ledger, bound);
    let mut group = vec![ledger];
    group.extend(others.iter());
    changed_ledgers(loaded, &group, bound)
}

/// The typed refusal naming exactly `ledgers` in its route.
pub(crate) fn changed_ledgers(loaded: &Loaded, ledgers: &[&LedgerSnapshot], bound: &str) -> String {
    let current = hash::text(&loaded.source);
    let copy = find(loaded, bound).map(|path| path.display().to_string());
    let mut paths = ledgers
        .iter()
        .map(|ledger| {
            crate::config::rel(&loaded.state_root, &ledger.path)
                .unwrap_or_else(|_| ledger.path.display().to_string())
        })
        .collect::<Vec<_>>();
    paths.sort();
    crate::drift::refusal(
        crate::drift::Class::MemoryConfiguration,
        bound,
        Some(&current),
        copy.as_deref(),
        crate::drift::Route::Revalidate {
            lead: loaded.lead().unwrap_or("lead"),
            ledgers: &paths,
            kept_copy: copy.as_deref(),
            config: &loaded.path,
        },
    )
}

fn is_project_lesson(ledger: &LedgerSnapshot) -> bool {
    ledger
        .items
        .values()
        .next()
        .is_some_and(|item| item["scope"] == super::lessons::SCOPE)
}

/// Other project-lesson ledgers whose history was stamped with `bound`; when
/// the kept copy exists, only those that pass every revalidation check
/// against it.
fn same_copy_ledgers(loaded: &Loaded, ledger: &LedgerSnapshot, bound: &str) -> Vec<LedgerSnapshot> {
    let Some(lead) = loaded.lead() else {
        return Vec::new();
    };
    if !is_project_lesson(ledger) {
        return Vec::new();
    }
    let previous = match find(loaded, bound) {
        Some(copy) => match copy
            .to_str()
            .and_then(|copy| crate::config::load_bytes_at(copy, &loaded.path).ok())
        {
            Some(previous) => Some(previous),
            None => return Vec::new(),
        },
        None => None,
    };
    let Ok(Some(discovered)) = super::discovery::discover(loaded) else {
        return Vec::new();
    };
    discovered
        .into_iter()
        .filter(|other| {
            other.snapshot.path != ledger.path
                && is_project_lesson(&other.snapshot)
                && matches!(super::state::history_drift(loaded, &other.snapshot),
                    Err(super::state::HistoryDrift::Configuration(other_bound)) if other_bound == bound)
                && previous.as_ref().map_or(true, |previous| {
                    super::revalidation::check(loaded, previous, lead, &other.path) == Ok(true)
                })
        })
        .map(|other| other.snapshot)
        .collect()
}

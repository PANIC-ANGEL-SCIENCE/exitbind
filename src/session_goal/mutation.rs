// Atomically appends and seals canonical Goal journal records.
// Requires the native mutation owner; see `src/session_goal.rs`.
//! Atomic mutation of the canonical Goal journal.

use super::{
    adoption, association, history_ledger, journal_form, journey, native, owner_correction,
    quarantine, raw_history, records_from_raw, recovery,
};
use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn sealed(mut value: Value, previous: Option<&Value>) -> Value {
    if let Some(previous) = previous {
        association::preserve_same_goal(previous, &mut value);
        journey::preserve_same_goal(previous, &mut value);
        recovery::preserve_same_goal(previous, &mut value);
        quarantine::preserve_same_goal(previous, &mut value);
        adoption::preserve_same_goal(previous, &mut value);
        owner_correction::preserve_same_goal(previous, &mut value);
    }
    value["previousEventSha256"] = previous
        .and_then(|record| record["eventSha256"].as_str())
        .map_or(Value::Null, |hash| json!(hash));
    value["eventSha256"] = json!(crate::evidence::hash::value(&value));
    value
}

/// Append a native Goal record while allowing a recovery request to locate an
/// earlier exact replay under the same journal lock. The ordinary mutation
/// path deliberately keeps its latest-record-only contract.
pub(crate) fn mutate_history<F>(
    root: &Path,
    loaded: &crate::config::Loaded,
    operation: F,
) -> Result<Value, String>
where
    F: FnOnce(&[Value]) -> Result<Value, String>,
{
    let ledger = history_ledger(root)?;
    crate::run::ledger::with_lock(&ledger, || {
        native::require_mutation_owner(root, loaded)?;
        let raw = raw_history(root)?;
        let history = records_from_raw(raw.as_deref())?;
        let record = operation(&history)?;
        if history.iter().any(|saved| saved == &record) {
            return Ok(record);
        }
        crate::run::ledger::append(
            &ledger,
            &journal_form::stored(&record, history.last()),
            raw.is_none(),
            raw.as_deref().unwrap_or_default(),
        )?;
        Ok(record)
    })
}

pub(crate) fn with_history<T, F>(root: &Path, operation: F) -> Result<T, String>
where
    F: FnOnce(&[Value]) -> Result<T, String>,
{
    let ledger = history_ledger(root)?;
    crate::run::ledger::with_lock(&ledger, || {
        let raw = raw_history(root)?;
        let history = records_from_raw(raw.as_deref())?;
        operation(&history)
    })
}

pub(crate) fn mutate<F>(
    root: &Path,
    loaded: &crate::config::Loaded,
    operation: F,
) -> Result<Value, String>
where
    F: FnOnce(Option<&Value>) -> Result<Value, String>,
{
    mutate_checked(
        root,
        || native::require_mutation_owner(root, loaded),
        operation,
    )
}

/// Context-only Work association may belong to another project; it must
/// retain the exact Goal owner observed before admission under the same lock.
pub(super) fn mutate_associated<F>(
    root: &Path,
    owner: &native::Owner,
    operation: F,
) -> Result<Value, String>
where
    F: FnOnce(Option<&Value>) -> Result<Value, String>,
{
    mutate_checked(
        root,
        || native::require_recorded_owner(root, owner),
        operation,
    )
}

fn mutate_checked<F, G>(root: &Path, guard: G, operation: F) -> Result<Value, String>
where
    F: FnOnce(Option<&Value>) -> Result<Value, String>,
    G: FnOnce() -> Result<(), String>,
{
    let ledger = history_ledger(root)?;
    crate::run::ledger::with_lock(&ledger, || {
        guard()?;
        let raw = raw_history(root)?;
        let history = records_from_raw(raw.as_deref())?;
        let previous = history.last();
        let record = operation(previous)?;
        if previous == Some(&record) {
            return Ok(record);
        }
        crate::run::ledger::append(
            &ledger,
            &journal_form::stored(&record, history.last()),
            raw.is_none(),
            raw.as_deref().unwrap_or_default(),
        )?;
        Ok(record)
    })
}

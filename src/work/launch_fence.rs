//! Serialize native launch admission with predecessor retirement.

use crate::{config::Loaded, run::ledger};

/// The existing Run lock protects only the admission and durable launch
/// observation, never the native process lifetime or a host callback.
pub(crate) fn with_live_work<T>(
    loaded: &Loaded,
    work: &str,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let relative = super::resolve(loaded, work)?;
    let selected = ledger::ledger_path(&loaded.state_root, &relative, false)?;
    ledger::with_lock(&selected, || {
        if ledger::valid_claim(&ledger::claim_path(&selected))?.is_some() {
            return Err(
                "Work is superseded; no new native launch or child authority was admitted".into(),
            );
        }
        action()
    })
}

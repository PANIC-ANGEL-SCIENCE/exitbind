//! Predicate-gated routing for a check whose observed result no longer tests
//! the current inputs. The caller keeps the actual pending assignment intact.

use crate::{config::Loaded, run};
use serde_json::Value;

pub(super) fn for_assignment(
    loaded: &Loaded,
    work: &str,
    snapshot: &run::RunSnapshot,
    target: &str,
    requirement_id: Option<&str>,
    role: &str,
) -> Result<Option<bool>, String> {
    if !matches!(role, "lead" | "reviewer")
        || (role == "reviewer"
            && snapshot
                .inspect_view()
                .get("quarantineVerification")
                .is_some())
    {
        return Ok(None);
    }
    let stale = snapshot.stale_ended_check(target, requirement_id)?;
    if stale.is_some() && !has_supersession_claim(loaded, work)? {
        Ok(stale)
    } else {
        Ok(None)
    }
}

fn has_supersession_claim(loaded: &Loaded, work: &str) -> Result<bool, String> {
    let ledger = super::resolve(loaded, work)?;
    let path = run::ledger::load(loaded, &ledger)?.0;
    Ok(run::ledger::valid_claim(&run::ledger::claim_path(&path))?.is_some())
}

pub(super) fn reviewer_outcomes(action: &Value) -> Option<Vec<&str>> {
    (action["action"] == "spawn"
        && action["role"] == "reviewer"
        && action.get("staleEndedCheck").is_some_and(Value::is_object))
    .then(|| vec!["rework", "blocked"])
}

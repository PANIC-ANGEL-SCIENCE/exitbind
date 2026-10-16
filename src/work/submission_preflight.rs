use crate::{config::Loaded, run};
use serde_json::Value;

/// Validate selected optional evidence and the ordinary next-state projection
/// before append. Caller bytes avoid imposing preview limits on normal results.
pub(super) fn preflight(
    loaded: &Loaded,
    work: &str,
    events: &[Value],
    source: &str,
    candidate_bytes: &[u8],
) -> Result<(), String> {
    if events.last().is_some_and(|event| {
        event["action"] == "submit" && event["role"] == "worker" && event["outcome"] == "completed"
    }) {
        crate::work::early_evidence::validate_submission(loaded, work, events, candidate_bytes)?;
    }
    let snapshot = run::RunSnapshot::from_events(loaded, events, source)?;
    let _ = super::next_snapshot::next_and_residual_from_snapshot(
        loaded, work, &snapshot, false, false,
    )?;
    Ok(())
}

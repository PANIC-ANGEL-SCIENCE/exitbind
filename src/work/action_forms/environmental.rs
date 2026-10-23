//! The Lead's environmental-attestation form on a failed observed check.
use super::*;

/// The `environmental` choice, offered only while a current check target's
/// latest result is a failed observed check and nothing else is pending. The
/// reducer re-validates the exact failed checks when the decision is recorded.
pub(super) fn choice(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
    next: &Value,
) -> Option<Value> {
    let failed_observed = next["packet"]["checkEvidence"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|target| target["status"] == "failed" && target["acquisition"] == "observed");
    if !failed_observed
        || !next["staleEndedCheck"].is_null()
        || next["packet"]["checkObservation"].is_object()
        || next["packet"]["pendingDisposition"].is_object()
    {
        return None;
    }
    let decision = crate::run::environmental_check::DECISION;
    let mut form = super::choice(
        loaded,
        crate::emitted::Route::new([
            "work",
            "disposition",
            work,
            assignment,
            "--current-binding",
            binding,
            "--decision",
            decision,
            "--reason",
            crate::emitted::slot("REASON").as_str(),
        ]),
        decision,
        &["REASON"],
        no_input(),
    );
    form["sideEffect"] = json!("records the Lead's reason that the failed check failed for an environmental cause; the failed check stays in the ledger and counts toward the repeated-failure limit, and the same target's check is pending again");
    Some(form)
}

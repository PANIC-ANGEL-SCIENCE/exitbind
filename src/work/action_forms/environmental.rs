//! The Lead's environmental-attestation form on a failed observed check, and
//! on a check observation that failed before the checker started.
use super::*;

/// The `environmental` choice. It is offered while a current check target's
/// latest result is a failed observed check, or while the current observation
/// records an ended failure before the checker started; nothing else may be
/// pending. The reducer re-validates the exact finding when the decision is
/// recorded.
pub(super) fn choice(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
    next: &Value,
    never_ran: bool,
) -> Option<Value> {
    if !next["staleEndedCheck"].is_null() {
        return None;
    }
    if never_ran {
        if next["packet"]["pendingDisposition"]["kind"] != "check_observation_failed" {
            return None;
        }
    } else {
        let failed_observed = next["packet"]["checkEvidence"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|target| target["status"] == "failed" && target["acquisition"] == "observed");
        if !failed_observed
            || next["packet"]["checkObservation"].is_object()
            || next["packet"]["pendingDisposition"].is_object()
        {
            return None;
        }
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
    form["sideEffect"] = json!(if never_ran {
        "records the Lead's reason that the check could not run here; no check result exists, the failed observation stays in the ledger, and the same check is pending again"
    } else {
        "records the Lead's reason that the failed check failed for an environmental cause; the failed check stays in the ledger and counts toward the repeated-failure limit, and the same target's check is pending again"
    });
    Some(form)
}

//! The Lead's environmental-attestation form on a failed observed check, on a
//! check observation that failed before the checker started, and on a timed-out
//! observation whose owned process group has since been observed ended.
use super::*;

/// Why the same check is owed a result without one being present.
pub(super) enum Source {
    /// A current check target's latest result is a failed observed check.
    FailedCheck,
    /// The observation ended before the checker started.
    NeverRan,
    /// The observation timed out and its recorded owned group is now empty.
    ClosedOwnedGroup,
}

/// The `environmental` choice. It is offered while a current check target's
/// latest result is a failed observed check, while the current observation
/// records an ended failure before the checker started, or while a timed-out
/// observation's owned group is observed with no process left; nothing else
/// may be pending. The reducer re-validates the exact finding when the
/// decision is recorded.
pub(super) fn choice(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
    next: &Value,
    source: Source,
) -> Option<Value> {
    if !next["staleEndedCheck"].is_null() {
        return None;
    }
    let side_effect = match source {
        Source::NeverRan => {
            if next["packet"]["pendingDisposition"]["kind"] != "check_observation_failed" {
                return None;
            }
            "records the Lead's reason that the check could not run here; no check result exists, the failed observation stays in the ledger, and the same check is pending again"
        }
        Source::ClosedOwnedGroup => {
            if !crate::run::owned_group::closed(
                &next["packet"]["checkObservation"],
                &next["packet"]["pendingDisposition"],
            ) {
                return None;
            }
            "records the Lead's reason that the timed-out check's owned processes have ended; no check result exists, the failed observation stays in the ledger, and the same check is pending again"
        }
        Source::FailedCheck => {
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
            "records the Lead's reason that the failed check failed for an environmental cause; the failed check stays in the ledger and counts toward the repeated-failure limit, and the same target's check is pending again"
        }
    };
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
    form["sideEffect"] = json!(side_effect);
    Some(form)
}

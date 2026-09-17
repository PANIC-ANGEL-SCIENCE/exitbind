//! Human-facing projection assembled from kernel facts.
//!
//! Wording remains in `run_presentation`; this module owns only the typed
//! model assembly boundary.

pub(crate) use crate::run_value::{
    HumanCheckState, HumanCheckTarget, HumanExplanation, HumanIdentity, HumanLeadDecision,
    HumanLeadState, HumanProtection, HumanRecord, HumanReviewer, HumanStatus, HumanWorker,
};

use crate::{run_exit, run_value};
use serde_json::Value;

pub(crate) fn human_status(state: &Value, artifact_current: bool) -> Result<HumanStatus, String> {
    let kernel = run_exit::reduce(state)?;
    run_value::human_status_from_kernel(state, artifact_current, &kernel)
}

pub(crate) fn human_explain(
    state: &Value,
    event_id: Option<&str>,
    artifact_current: bool,
) -> Result<HumanExplanation, String> {
    let status = human_status(state, artifact_current)?;
    let protection = event_id
        .map(|wanted| {
            status
                .protections
                .iter()
                .find(|event| event.event_sha256 == wanted)
                .cloned()
                .ok_or_else(|| {
                    "explanation target is not a protection event in this run".to_owned()
                })
        })
        .transpose()?;
    let guidance = if protection.is_some() {
        if matches!(state["version"].as_u64(), Some(4..=6)) {
            "observe the frozen check locally or report its host result for every current worker completion; repair or rework if needed, then request review and acceptance again; a passing result still requires authority"
        } else {
            "rerun the configured check in its host and report the actual result for every current worker completion; repair or rework if needed, then request review and acceptance again; a passing report still requires authority"
        }
    } else {
        "inspect the exact evidence references before choosing repair, rework, or acceptance"
    };
    Ok(HumanExplanation {
        status,
        protection,
        guidance,
    })
}

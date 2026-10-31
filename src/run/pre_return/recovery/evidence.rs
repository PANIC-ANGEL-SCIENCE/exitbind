//! Revalidate the exact product-owned proof used by a reported decision.
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

pub(crate) fn verify_saved(loaded: &Loaded, work: &str, evidence: &Value) -> Result<(), String> {
    if evidence["type"] != "owned_observer_v1" {
        return Err("pre-return recovery requires product-owned observer evidence".into());
    }
    let attempt = json!({"attemptId":evidence["attemptId"]});
    let current = super::observer_evidence(loaded, &attempt);
    if current.as_ref().is_ok_and(|value| value == evidence) {
        return Ok(());
    }
    let actual = current.as_ref().ok().map(hash::value);
    Err(crate::drift::refusal(
        crate::drift::Class::PreReturnRecoveryEvidence,
        &hash::value(evidence),
        actual.as_deref(),
        None,
        crate::drift::Route::WorkDetail {
            work,
            config: &loaded.path,
        },
    ))
}

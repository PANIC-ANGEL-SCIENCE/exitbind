// Admits native supersession starts and claims.
// Refuses invalid successor Work identities; see `src/run/mod.rs`.
//! Domain-separated admission for the existing supersession start and claim.

use crate::evidence::hash;
use serde_json::{json, Value};

pub(crate) fn intent(work: &str, start: &Value) -> Result<String, String> {
    if !crate::work::valid_work_handle(work) {
        return Err("native successor Work identity is invalid".into());
    }
    let predecessor = start
        .get("supersedes")
        .ok_or("native successor has no predecessor")?;
    crate::run::state::validate_supersession(predecessor, 1)?;
    Ok(hash::value(&json!({
        "domain": "exitbind.native-work-supersession-admission.v1",
        "work": work,
        "startIntentSha256": crate::run::start_event_intent_sha256(start)?,
        "supersedes": predecessor,
        "carryProtocol": start.get("carryProtocol"),
        "governorCarrySha256": start.get("governorCarrySha256"),
    })))
}

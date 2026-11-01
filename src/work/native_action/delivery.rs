//! Recheck the exact recipient instructions before admitting a native result.

use crate::{config::Loaded, work};
use serde_json::Value;

pub(super) fn submit_saved_checked(
    loaded: &Loaded,
    work_id: &str,
    assignment: &str,
    bytes: Vec<u8>,
    current_binding: Option<&str>,
    journal: &Value,
) -> Result<Value, String> {
    // Already admitted bytes may be recovered without promoting an old
    // recipient proof into a new Work return.
    if let Ok(replayed) = work::replay_recorded_native(loaded, work_id, assignment, journal, &bytes)
    {
        return Ok(replayed);
    }
    // Exact-match legacy journals keep their established replay behavior;
    // they never qualify for the new permit-transition bridge.
    if journal.get("recipientProofVersion").is_none() {
        return super::submit_saved(loaded, work_id, assignment, bytes, current_binding);
    }
    if journal["recipientProofVersion"] != 1 {
        return Err("native recipient delivery proof version is unsupported".into());
    }
    let expected = journal["recipientSha256"].as_str().ok_or(
        "native recipient delivery proof is unavailable; retained result has no automatic credit",
    )?;
    let current = work::current_action(loaded, work_id)?;
    if current["assignment"] != assignment
        || work::details::managed_recipient_fingerprint(loaded, &current)? != expected
    {
        return Err("native recipient rules, rights or guidance changed; retained result has no automatic credit".into());
    }
    super::submit_saved(loaded, work_id, assignment, bytes, current_binding)
}

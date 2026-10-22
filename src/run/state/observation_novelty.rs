//! Which observed check result is new information for the bounded governor.
//!
//! A start that carries `observationNoveltyProtocol` keys a passing observed
//! check on the checked subject as well as the frozen command and its status,
//! so a pass on a new subject (a new worker result) is new information and the
//! same pass on the same subject stays a duplicate. A failing status keeps the
//! subject-free key: the same failure on another result stays the same finding
//! and the governor's repeated-failure limit holds (R55 Lead decision on F7).
//! Historical starts without the marker keep the subject-free key they were
//! recorded with, and replay exactly as before.

use serde_json::{json, Value};

pub(crate) const FIELD: &str = "observationNoveltyProtocol";
pub(crate) const PROTOCOL: u64 = 1;

/// Mark a new start whose recovery protocol is enabled. A start without
/// recovery has no observed-check information to key and stays unmarked.
pub(crate) fn add_start_marker(start: &mut Value) {
    if start.get("recoveryProtocol").is_some() {
        start[FIELD] = json!(PROTOCOL);
    }
}

pub(super) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let Some(marker) = event.get(FIELD) else {
        return Ok(());
    };
    if marker.as_u64() != Some(PROTOCOL) || event.get("recoveryProtocol").is_none() {
        return Err(format!(
            "invalid run ledger line {line}: {FIELD} requires the recovery protocol"
        ));
    }
    Ok(())
}

/// The governor's one novelty key of an observed check. Target names,
/// artifacts, log contents, durations, timestamps and wrapper text are
/// deliberately absent; the subject is present only for a pass.
pub(super) fn key(state: &Value, event: &Value, status: &Value) -> String {
    let mut key = json!({
        "commandSha256": event["checkCommandSha256"],
        "status": status,
    });
    if state[FIELD] == PROTOCOL && passed(status) {
        key["subjectSha256"] = event["subjectSha256"].clone();
    }
    crate::evidence::hash::value(&key)
}

fn passed(status: &Value) -> bool {
    *status == json!({"exit": 0})
}

#[cfg(test)]
#[path = "observation_novelty_tests.rs"]
mod tests;

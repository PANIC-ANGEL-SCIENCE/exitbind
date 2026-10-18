//! Known-ended eligibility for the single verification follow-up.
use crate::run;
use serde_json::Value;

pub(super) fn validate_first_failure(events: &[Value], state: &Value) -> Result<(), String> {
    let start = events.first().ok_or("first verification ledger is empty")?;
    let marker = &start["quarantineVerification"];
    if start["version"] != 8 {
        return Err("follow-up requires a version-8 first verification".into());
    }
    run::quarantine::validate_first_failed_check(events, state, marker)?;
    Ok(())
}

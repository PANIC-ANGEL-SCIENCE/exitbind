//! Exact eligibility checks for the sole known-ended quarantine follow-up.
use super::*;

pub(super) fn validate_predecessor(loaded: &Loaded, marker: &Value) -> Result<(), String> {
    if marker["protocol"] != super::super::FOLLOWUP_PROTOCOL {
        return Ok(());
    }
    let link = &marker["followUp"];
    let (_, events, bytes) = crate::run::ledger::load(loaded, super::str_value(link, "ledger")?)?;
    let first_start = events.first().ok_or("first verification ledger is empty")?;
    let first_head = events
        .last()
        .ok_or("first verification ledger has no head")?;
    let state = crate::run::state::reduce(&events)?;
    let first_marker = &first_start["quarantineVerification"];
    if hash::bytes(bytes.as_bytes()) != link["ledgerSha256"]
        || first_start["version"] != 8
        || first_start["runId"] != link["runId"]
        || first_start["eventSha256"] != link["startEventSha256"]
        || first_head["eventSha256"] != link["headEventSha256"]
        || first_marker["protocol"] != super::super::PROTOCOL
        || first_marker.get("followUp").is_some()
        || first_marker["decisionSha256"] != link["decisionSha256"]
        || first_marker["predecessor"] != marker["predecessor"]
    {
        return Err("follow-up requires the exact completed first verification ledger".into());
    }
    let check = crate::run::quarantine::validate_first_failed_check(&events, &state, first_marker)?;
    let check_code = check["exitCode"]
        .as_i64()
        .or_else(|| check["result"]["code"].as_i64());
    if check["eventSha256"] != link["checkEventSha256"] || !check_code.is_some_and(|code| code != 0)
    {
        return Err("follow-up check event differs from the exact bound failed observation".into());
    }
    crate::run::quarantine::validate_custody_against_first_check(
        loaded,
        &link["custodyEvidence"],
        first_marker,
        check,
        check_code.ok_or("first verification check result is missing")?,
    )?;
    crate::run::artifact::assert_current(loaded, &state)?;
    let children = crate::session_goal::prepared_children_for_work(
        loaded,
        link["work"]
            .as_str()
            .ok_or("first verification Work is invalid")?,
    );
    if children["error"].is_string()
        || children.as_array().into_iter().flatten().any(|child| {
            !matches!(
                child["state"].as_str(),
                Some("recorded" | "failed" | "abandoned" | "superseded")
            ) || child["retainedResultBytes"]
                .as_u64()
                .is_some_and(|bytes| bytes > 0)
        })
    {
        return Err("follow-up is blocked by live, prepared, or retained child effects".into());
    }
    Ok(())
}

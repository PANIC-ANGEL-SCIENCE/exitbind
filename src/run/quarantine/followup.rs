//! Strict protocol step and identity for the one allowed known-ended follow-up.
use super::{exact, sha, work, FOLLOWUP_PROTOCOL, PROTOCOL};
use serde_json::{json, Value};

/// Admit a single observed nonzero check, optionally followed by the exact
/// configured Lead's normal blocked return. The complete ledger head remains
/// bound by the caller; this only permits the preserved adjacent terminal.
pub(crate) fn validate_first_failed_check<'a>(
    events: &'a [Value],
    state: &Value,
    marker: &Value,
) -> Result<&'a Value, String> {
    let start = events.first().ok_or("first verification ledger is empty")?;
    if marker["protocol"] != PROTOCOL
        || marker.get("followUp").is_some()
        || state["checks"]
            .as_array()
            .map_or(true, |checks| checks.len() != 1)
        || state["checkObservation"].is_object()
        || events
            .iter()
            .filter(|event| event["action"] == "check")
            .count()
            != 1
    {
        return Err("follow-up requires one completed first verification; unknown or repeated attempts refuse".into());
    }
    let check_sha = &state["checks"][0]["eventSha256"];
    let (index, check) = events
        .iter()
        .enumerate()
        .find(|(_, event)| event["action"] == "check" && &event["eventSha256"] == check_sha)
        .ok_or("first verification check event is missing")?;
    let code = check["exitCode"]
        .as_i64()
        .or_else(|| check["result"]["code"].as_i64());
    if check["acquisition"] != "observed"
        || check["origin"] != "local_report"
        || check["runId"] != start["runId"]
        || !code.is_some_and(|code| code != 0)
    {
        return Err("follow-up requires one observed nonzero known-ended check".into());
    }

    match events.get(index + 1..) {
        Some([]) => {}
        Some([blocked])
            if blocked["version"] == 8
                && blocked["runId"] == start["runId"]
                && blocked["action"] == "submit"
                && blocked["role"] == "lead"
                && blocked["agent"] == marker["leadOwner"]["agent"]
                && blocked["outcome"] == "blocked"
                && blocked["previousEventSha256"] == check["eventSha256"]
                && blocked.get("governorEvent").is_none()
                && state["status"] == "blocked" =>
        {
            // This is the only permitted event after the failed check.
        }
        _ => {
            return Err("follow-up permits only the exact check head or its configured Lead's immediate blocked return".into());
        }
    }
    Ok(check)
}

pub(super) fn validate_marker(marker: &Value, line: usize) -> Result<(), String> {
    if marker["protocol"] != FOLLOWUP_PROTOCOL {
        return Ok(());
    }
    let invalid = || format!("invalid run ledger line {line}: invalid quarantine verification");
    let step = &marker["followUp"];
    if !exact(
        step,
        &[
            "protocol",
            "work",
            "ledger",
            "runId",
            "startEventSha256",
            "headEventSha256",
            "ledgerSha256",
            "decisionSha256",
            "checkEventSha256",
            "custodyEvidence",
        ],
    ) || step["protocol"] != PROTOCOL
        || !work(&step["work"])
        || ![
            "runId",
            "startEventSha256",
            "headEventSha256",
            "ledgerSha256",
            "decisionSha256",
            "checkEventSha256",
        ]
        .iter()
        .all(|key| sha(&step[key]))
    {
        return Err(invalid());
    }
    let custody = &step["custodyEvidence"];
    if !exact(custody, &["root", "path", "sha256", "bytes"])
        || custody["root"] != "state"
        || !custody["path"]
            .as_str()
            .is_some_and(|path| !path.trim().is_empty())
        || !sha(&custody["sha256"])
        || custody["bytes"].as_u64().map_or(true, |bytes| bytes == 0)
    {
        return Err(invalid());
    }
    let handle = step["work"].as_str().ok_or_else(invalid)?;
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &handle[4..]);
    if step["ledger"].as_str() != Some(ledger.as_str())
        || step["work"] == marker["predecessor"]["work"]
        || step["work"] == marker["verification"]["isolationReview"]["work"]
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn identity(marker: &Value) -> Result<String, String> {
    super::validate_marker(marker, 0)?;
    Ok(crate::evidence::hash::value(&json!({
        "protocol": FOLLOWUP_PROTOCOL,
        "oldWork": marker["predecessor"]["work"],
        "startEventSha256": marker["predecessor"]["startEventSha256"],
        "admissionEventSha256": marker["predecessor"]["admissionEventSha256"],
        "firstVerificationWork": marker["followUp"]["work"],
        "firstVerificationStartEventSha256": marker["followUp"]["startEventSha256"],
        "firstVerificationCheckEventSha256": marker["followUp"]["checkEventSha256"],
        "firstVerificationFailedCheckEventSha256": marker["followUp"]["checkEventSha256"],
    })))
}

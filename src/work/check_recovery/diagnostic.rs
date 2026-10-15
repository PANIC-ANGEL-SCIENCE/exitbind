//! Immutable diagnosis is deliberately separate from recovery eligibility.
use super::*;

pub(super) fn inspect(
    loaded: &Loaded,
    work: &str,
    ledger_name: &str,
    events: &[Value],
    source: &str,
    state: &Value,
) -> Result<Option<Value>, String> {
    let observation = &state["checkObservation"];
    let facts = &observation["facts"];
    let Some(head) = events.last() else {
        return Ok(None);
    };
    if state["status"] != "blocked"
        || observation["state"] != "failed"
        || facts["termination"] != "unknown"
        || head["action"] != "submit"
        || head["outcome"] != "blocked"
    {
        return Ok(None);
    }
    if state["configSha256"] != crate::evidence::hash::text(&loaded.source)
        || std::fs::read(&loaded.path)
            .map_err(|e| format!("diagnostic configuration is unavailable: {e}"))?
            != loaded.source.as_bytes()
    {
        return Err(
            "historical diagnostic requires its exact frozen configuration identity".into(),
        );
    }
    let admission = events
        .iter()
        .find(|event| {
            event["eventSha256"] == observation["eventSha256"]
                && event["action"] == "check_observation"
        })
        .ok_or("unknown diagnostic has no validated admission identity")?;
    let failure = events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "check_observation_failed"
                && event["observation"]["admissionEventSha256"] == admission["eventSha256"]
                && event["observation"]["facts"] == *facts
        })
        .ok_or("unknown diagnostic has no validated durable failure identity")?;
    let current_inputs = match run::inputs::fingerprint(loaded) {
        Ok(current) => {
            json!({"state": if current == observation["binding"]["inputsSha256"] { "matches_admission" } else { "changed" }})
        }
        Err(_) => json!({"state": "unavailable"}),
    };
    let mut bounded_facts = serde_json::Map::new();
    for key in [
        "process",
        "processStarted",
        "groupEnded",
        "captureReadersEnded",
        "captureAvailability",
        "durationMs",
        "capture",
        "deadlineExceeded",
        "storageStage",
        "storage",
        "termination",
        "cleanupErrorCount",
        "remainingOwnedPathCount",
    ] {
        bounded_facts.insert(key.into(), facts[key].clone());
    }
    Ok(Some(json!({
        "work": work, "effect": "no-change", "checkRecorded": false,
        "recoverable": false, "status": "historical_unknown_termination",
        "originalOutcome": head["outcome"],
        "reference": {"ledger": ledger_name, "eventCount": events.len(), "ledgerSourceSha256": crate::evidence::hash::text(source),
            "admissionEventSha256": admission["eventSha256"], "failureEventSha256": failure["eventSha256"], "headEventSha256": head["eventSha256"]},
        "observation": bounded_facts,
        "executionIdentity": {"state": "unavailable", "meaning": "The admission did not retain a live kernel child identity; historical process candidates cannot prove termination."},
        "currentInputs": current_inputs,
        "meaning": "Inspection only. Preserve the blocked Work and unknown termination; this route supplies no recovery decision, execution, check credit or acceptance."
    })))
}

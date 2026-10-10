//! Explicit Lead-reported recovery of a known-ended, uncommitted observation.
//! This records failure, not observed check evidence or execution authority.
use super::*;

pub(crate) const ACTION: &str = "check_observation_recovered";
pub(crate) const MAX_DECISION_BYTES: usize = 32 * 1024;

fn exact(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

pub(crate) fn snapshot(state: &Value, source: &str) -> Value {
    let current = &state["checkObservation"];
    json!({"admissionEventSha256": current["eventSha256"],
        "ledgerSourceSha256": hash::text(source), "binding": current["binding"],
        "timeoutMs": current["timeoutMs"], "observer": current["producer"]})
}

/// Recovery has a canonical request binding of its own. Reconstruct the
/// admission and Lead assignment rather than trusting a submitted snapshot.
pub(crate) fn request_binding(
    work: &str,
    state: &Value,
    prefix_sha256: &Value,
) -> Result<String, String> {
    let lead = crate::run::assignment::pending(state)
        .into_iter()
        .find(|a| a["role"] == "lead")
        .ok_or("recovery requires the current Lead assignment")?;
    let mut admission = snapshot(state, "");
    admission["ledgerSourceSha256"] = prefix_sha256.clone();
    Ok(hash::value(&json!({"domain": "check_recovery_request_v1",
        "work": work, "lead": {"agent": lead["agent"],
            "assignment": crate::run::assignment::handle(work, &lead)?},
        "snapshot": admission})))
}

pub(crate) fn decision(value: &Value) -> Result<Value, String> {
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > MAX_DECISION_BYTES {
        return Err("recovery decision exceeds 32768 bytes".into());
    }
    if !exact(
        value,
        &[
            "version",
            "work",
            "agent",
            "approved",
            "reason",
            "currentBinding",
            "snapshot",
            "response",
            "responseSha256",
        ],
    ) || value["version"] != 1
        || value["approved"] != true
        || !crate::work::valid_work_handle(value["work"].as_str().unwrap_or(""))
        || !value["agent"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 128)
        || !value["reason"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.len() <= 1024)
        || !crate::run::state::check_observation::valid_facts(&response(value)?["observation"])
    {
        return Err("recovery requires a bounded, explicitly approved Lead decision".into());
    }
    let s = &value["snapshot"];
    if !exact(
        s,
        &[
            "admissionEventSha256",
            "ledgerSourceSha256",
            "binding",
            "timeoutMs",
            "observer",
        ],
    ) || ["admissionEventSha256", "ledgerSourceSha256"]
        .iter()
        .any(|k| !sha(&s[k]))
        || !sha(&value["currentBinding"])
        || !crate::producer::valid(&s["observer"])
    {
        return Err("invalid recovery admission identity".into());
    }
    let response = response(value)?;
    let facts = &response["observation"];
    if response["work"] != value["work"]
        || response["effect"] != "unknown"
        || response["checkRecorded"] != false
        || response["durableFailureRecorded"] != false
        || response["reason"]["code"] != "observation_failure_storage_unavailable"
        || facts["termination"] != "ended"
        || facts["groupEnded"] != true
        || facts["captureReadersEnded"] != true
        || facts["cleanupErrorCount"] != 0
        || facts["remainingOwnedPathCount"] != 0
        || facts["processStarted"] != true
        || facts["process"].is_null()
        || !facts["partialCaptures"]
            .as_array()
            .is_some_and(|p| !p.is_empty())
    {
        return Err(
            "recovery requires an exact retained known-ended storage-failure response".into(),
        );
    }
    let operation = hash::value(&json!({"identity": s["binding"],
        "ledgerSourceSha256": s["ledgerSourceSha256"]}));
    let mut streams = std::collections::BTreeSet::new();
    for partial in facts["partialCaptures"]
        .as_array()
        .ok_or("missing partial capture list")?
    {
        let path = partial["path"].as_str().ok_or("invalid partial path")?;
        if !path.starts_with(&format!(".exitbind/artifacts/check-partial-{operation}-"))
            || !streams.insert(partial["stream"].as_str().ok_or("invalid partial stream")?)
        {
            return Err("partial capture does not belong to the exact admitted observation".into());
        }
    }
    Ok(response)
}

fn sha(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn response(value: &Value) -> Result<Value, String> {
    let raw = value["response"]
        .as_str()
        .ok_or("recovery needs the complete saved response")?;
    if raw.len() > MAX_DECISION_BYTES || value["responseSha256"] != hash::text(raw) {
        return Err("retained recovery response bytes changed or exceed the bound".into());
    }
    serde_json::from_str(raw).map_err(|e| format!("invalid retained recovery response: {e}"))
}

pub(crate) fn validate(event: &Value) -> Result<(), String> {
    if event["version"] != 8
        || event["action"] != ACTION
        || !exact(
            &event["observation"],
            &["admissionEventSha256", "facts", "recovery"],
        )
    {
        return Err("invalid recovered observation event".into());
    }
    let d = &event["observation"]["recovery"];
    let response = decision(d)?;
    if event["observation"]["facts"] != response["observation"]
        || event["observation"]["admissionEventSha256"] != d["snapshot"]["admissionEventSha256"]
    {
        return Err("recovered failure differs from its retained response or admission".into());
    }
    Ok(())
}

pub(crate) fn validate_prefix(
    event: &Value,
    prefix: &str,
    ledger: &run_ledger::LedgerPath,
) -> Result<(), String> {
    let d = &event["observation"]["recovery"];
    if d["snapshot"]["ledgerSourceSha256"] != hash::text(&format!("{prefix}\n"))
        || d["work"] != canonical_work_for_ledger(ledger)?
    {
        return Err("recovery does not bind the exact retained ledger prefix and Work".into());
    }
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    validate(event)?;
    let current = &state["checkObservation"];
    let d = &event["observation"]["recovery"];
    let s = &d["snapshot"];
    let lead = crate::run::assignment::pending(state)
        .into_iter()
        .find(|a| a["role"] == "lead")
        .ok_or("recovery requires the current Lead assignment")?;
    if state["status"] != "running"
        || state["checkObservationProtocol"] != check_observation::PROTOCOL
        || current["state"] != "running"
        || state["pendingDisposition"].is_object()
        || event["previousEventSha256"] != current["eventSha256"]
        || s["admissionEventSha256"] != current["eventSha256"]
        || s["binding"] != current["binding"]
        || s["timeoutMs"] != current["timeoutMs"]
        || s["observer"] != current["producer"]
        || d["agent"] != lead["agent"]
        || d["currentBinding"]
            != request_binding(
                d["work"].as_str().ok_or("invalid recovery Work")?,
                state,
                &s["ledgerSourceSha256"],
            )?
    {
        return Err("recovery is not the exact current Lead/admission binding".into());
    }
    // Keep the original observer identity. No check or governor event is added.
    state["checkObservation"]["state"] = json!("failed");
    state["checkObservation"]["facts"] = event["observation"]["facts"].clone();
    state["checkObservation"]["recoveryEventSha256"] = event["eventSha256"].clone();
    state["pendingDisposition"] = json!({"owner": "lead", "triggerEventSha256": event["eventSha256"],
        "basisSha256": state["basis"]["sha256"], "kind": "check_observation_failed",
        "findingSha256s": [event["eventSha256"]]});
    Ok(())
}

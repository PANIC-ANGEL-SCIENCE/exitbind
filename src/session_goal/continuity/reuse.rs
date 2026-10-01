//! Explicit application of a retained finding, supported by a current observed
//! check. Retrieval and a caller's claim alone never establish applicability.

use super::*;

pub(super) fn apply(
    loaded: &Loaded,
    work: &str,
    previous: &Value,
    record: &mut Value,
    input: &Value,
) -> Result<(), String> {
    shape(
        input,
        &[
            "action",
            "expectedRevision",
            "bindingRevision",
            "operationId",
            "diagnosisRevision",
            "resultEventSha256",
            "conditionsSha256",
        ],
    )?;
    let operation = field(input, "operationId", 64)?;
    let revision = revision(input, "diagnosisRevision")?;
    let diagnosis = previous["continuation"]["diagnoses"]
        .as_array()
        .and_then(|xs| {
            xs.iter()
                .find(|x| x["operationId"] == operation && x["revision"] == revision)
        })
        .ok_or("reuse needs an existing diagnosis revision")?;
    if !matches!(
        diagnosis["class"].as_str(),
        Some("observed_failure" | "diagnosed_cause" | "verified_repair")
    ) {
        return Err("a hypothesis cannot establish finding reuse".into());
    }
    let ledger = verify_work(loaded, work)?;
    let old_event = diagnosis["evidenceEventSha256"]
        .as_str()
        .ok_or("reuse needs the finding's retained evidence")?;
    let source = run::inspect_event(loaded, &ledger, old_event)?;
    if !evidence_verified(&source) {
        return Err("finding evidence is unavailable".into());
    }
    let event = field(input, "resultEventSha256", 64)?;
    let applied = run::inspect_event(loaded, &ledger, event)?;
    let evidence = &applied["event"];
    let (inputs, conditions) = current_conditions(loaded)?;
    if applied["eventIndex"].as_u64() <= source["eventIndex"].as_u64()
        || evidence["action"] != "check"
        || evidence["acquisition"] != "observed"
        || evidence["result"]["kind"] != "exit"
        || evidence["result"]["code"] != 0
        || !evidence_verified(&applied)
        || !acquired_config_current(loaded, evidence)
        || evidence["inputsSha256"] != inputs
        || input["conditionsSha256"] != conditions
    {
        return Err("reuse needs a later current passing observed check".into());
    }
    let revision = record["revision"].clone();
    if record["continuation"].get("reuses").is_none() {
        record["continuation"]["reuses"] = json!([]);
    }
    let reuses = record["continuation"]["reuses"]
        .as_array_mut()
        .ok_or("reuse records malformed")?;
    if reuses.len() >= MAX_ITEMS
        || reuses.iter().any(|r| {
            r["operationId"] == operation
                && r["diagnosisRevision"] == input["diagnosisRevision"]
                && r["resultEventSha256"] == event
        })
    {
        return Err("reuse identity is duplicate or its bound is exceeded".into());
    }
    reuses.push(
        json!({"operationId":operation,"diagnosisRevision":input["diagnosisRevision"],
        "findingEventSha256":old_event,"resultEventSha256":event,"inputsSha256":inputs,
        "conditionsSha256":conditions,"revision":revision,
        "sourceClass":"lead_reported_application_with_observed_check"}),
    );
    Ok(())
}

pub(super) fn project(
    loaded: &Loaded,
    work: &str,
    continuation: &Value,
) -> Result<Vec<Value>, String> {
    let (inputs, conditions) = current_conditions(loaded)?;
    let ledger = verify_work(loaded, work)?;
    let mut result = Vec::new();
    for reuse in continuation["reuses"].as_array().into_iter().flatten() {
        let source = reuse["findingEventSha256"]
            .as_str()
            .and_then(|event| run::inspect_event(loaded, &ledger, event).ok());
        let applied = reuse["resultEventSha256"]
            .as_str()
            .and_then(|event| run::inspect_event(loaded, &ledger, event).ok());
        let applicable = reuse["inputsSha256"] == inputs
            && reuse["conditionsSha256"] == conditions
            && source.as_ref().is_some_and(evidence_verified)
            && applied.as_ref().is_some_and(|event| {
                evidence_verified(event)
                    && acquired_config_current(loaded, &event["event"])
                    && event["event"]["inputsSha256"] == inputs
            });
        result.push(json!({"relation":reuse,"applicableCurrent":applicable}));
    }
    Ok(result)
}

//! Validation of evidence identity against the current assignment and
//! carried-mutation lineage.

use serde_json::Value;

pub(crate) fn validate_governor_identity(state: &Value, event: &Value) -> Result<(), String> {
    if !crate::run::assignment::pending(state)
        .into_iter()
        .any(|item| {
            item["agent"] == event["agent"]
                && item["role"] == "worker"
                && item["role"] == event["role"]
                && item["stage"] == event["stage"]
                && item["attempt"] == event["attempt"]
        })
    {
        return Err("evidence is not bound to the current worker assignment".into());
    }
    if event["subjectSha256"] != state["subject"]["sha256"]
        || event["inputsSha256"] != event["governorEvent"]["inputSha256"]
        || event["governorEvent"]["evidence"]["inputSha256"] != event["inputsSha256"]
    {
        return Err("evidence is bound to a stale run identity or input".into());
    }
    let governor = &state["governor"];
    match event["governorEvent"]["identityTransition"].as_str() {
        None => return Ok(()),
        Some("carried_mutation_v1") => {}
        Some(_) => return Err("evidence identity transition is unsupported".into()),
    }
    let current = governor["currentMutation"]
        .as_object()
        .ok_or("evidence identity transition requires a current mutation")?;
    if current["subjectSha256"] == event["subjectSha256"] && current["attempt"] == event["attempt"]
    {
        return Err("evidence identity transition is unnecessary".into());
    }
    if current["carryLineage"] != true
        || event["governorEvent"]["previousSha256"] != current["eventSha256"]
        || event["governorEvent"]["lineageSha256"] != governor["lineageSha256"]
    {
        return Err("evidence identity transition is not bound to the current mutation".into());
    }
    let prior = current["attempt"].as_u64();
    let next = event["attempt"].as_u64();
    if prior
        .zip(next)
        .map_or(true, |(prior, next)| next != prior + 1)
    {
        return Err("evidence identity transition does not use the next attempt".into());
    }
    Ok(())
}

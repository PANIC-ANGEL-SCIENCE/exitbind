//! Frozen Work checks, bounded responses and exact committed replay.
use super::*;

pub(crate) fn check_with_timeout(
    loaded: &Loaded,
    work: &str,
    timeout_ms: Option<&str>,
) -> Result<Value, String> {
    crate::run::check_observation::timeout(timeout_ms)?;
    let ledger = resolve(loaded, work)?;
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let pending = retry_check_target(&snapshot)?;
    let observed = if let Some(pending) = pending {
        run::observe_check_for_requirement(
            loaded,
            &ledger,
            &pending.target_event_sha256,
            pending.requirement_id.as_deref(),
            timeout_ms,
        )?
    } else {
        crate::run::check_observation::committed_replay(loaded, &ledger, timeout_ms)?.ok_or("no current worker check is pending; inspect Work detail for a pending observation decision")?
    };
    let mut response = json!({"work": work});
    for field in ["event", "observationFailure", "checkRecorded", "recovered"] {
        if let Some(value) = observed.get(field) {
            response[field] = value.clone();
        }
    }
    if let Some(error) = observed.get("projectionError") {
        response["projectionError"] = error.clone();
    }
    if let Some(error) = observed.get("cleanupError") {
        response["cleanupError"] = error.clone();
    }
    match next_for(loaded, work, &ledger) {
        Ok(next) => response["next"] = next,
        Err(error) => response["projectionError"] = json!(error),
    }
    let bounded = bounded_mutation(&response, work, None, &ledger, config_path);
    if observed.get("observationFailure").is_some()
        || mutation_response::check_result_failed(&bounded["result"])
    {
        return Err(format!(
            "EXITBIND_JSON:{}",
            serde_json::to_string(&bounded).map_err(|error| error.to_string())?
        ));
    }
    Ok(bounded)
}

pub(super) fn unresolved(next: &Value) -> bool {
    let observation = &next["packet"]["checkObservation"];
    observation.is_object()
        && (observation["state"] == "running" || observation["facts"]["termination"] != "ended")
}

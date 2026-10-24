//! Frozen Work checks, bounded responses and exact committed replay.
use super::*;

pub(crate) fn check_with_timeout(
    loaded: &Loaded,
    work: &str,
    timeout_ms: Option<&str>,
) -> Result<Value, String> {
    crate::run::check_observation::timeout(timeout_ms)?;
    let ledger = resolve(loaded, work)?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let observation = snapshot.state()["checkObservation"].clone();
    if observation["state"] == "running" {
        // A live or unverifiable observer keeps the admission as it is. A
        // recorder known to have ended is closed through the ordinary failure
        // path, which raises the Lead's observation decision.
        if !crate::run::check_observation::observer_ended(&observation) {
            return Err(crate::run::check_observation::admitted_refusal(
                &observation,
                Some(work),
                &loaded.path,
            ));
        }
        let closed =
            crate::run::check_observation::close_dead_admission(loaded, &ledger, &observation)?;
        return finish(loaded, work, &ledger, closed);
    }
    if observation.is_object() {
        // A failed observation has no check result; only its Lead decision
        // re-pends the target, so this check starts nothing.
        return Err(crate::run::check_observation::admitted_refusal(
            &observation,
            Some(work),
            &loaded.path,
        ));
    }
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
        crate::run::check_observation::committed_replay(loaded, &ledger, timeout_ms)?.ok_or_else(|| {
            let detail = crate::emitted::command_line(&["work", "detail", work, "--view", "action"], Some(&loaded.path));
            format!("no current worker check is pending; inspect {detail} for a pending observation decision")
        })?
    };
    finish(loaded, work, &ledger, observed)
}

/// The bounded response of one check outcome, including the current next
/// action and the failure shape the caller reports.
fn finish(loaded: &Loaded, work: &str, ledger: &str, observed: Value) -> Result<Value, String> {
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
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
    match next_for(loaded, work, ledger) {
        Ok(next) => response["next"] = next,
        Err(error) => response["projectionError"] = json!(error),
    }
    let mut bounded = bounded_mutation(&response, work, None, ledger, config_path);
    super::refreshed_action::attach(loaded, work, &response["next"], &mut bounded);
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

/// A recorded observation that ended before the checker started. No check
/// result exists, so the same check is still owed.
pub(super) fn observation_never_ran(next: &Value) -> bool {
    let observation = &next["packet"]["checkObservation"];
    observation["state"] == "failed"
        && observation["facts"]["processStarted"] == false
        && observation["facts"]["termination"] == "ended"
}

/// A terminal blocked Work whose checker never produced a result: its
/// observation ended before the checker started, or no current check target
/// holds any result. Replacing such a check would claim a defective contract
/// from evidence that does not exist.
pub(super) fn blocked_check_never_ran(loaded: &Loaded, work: &str, next: &Value) -> bool {
    if next["packet"]["checkObservation"].is_object() {
        return observation_never_ran(next);
    }
    let Ok(ledger) = resolve(loaded, work) else {
        return false;
    };
    let Ok(snapshot) = run::RunSnapshot::capture(loaded, &ledger) else {
        return false;
    };
    let Ok(status) = snapshot.status_view() else {
        return false;
    };
    match status["checks"]["targets"].as_array() {
        Some(targets) => {
            targets.is_empty() || targets.iter().all(|target| target["status"] == "missing")
        }
        None => true,
    }
}

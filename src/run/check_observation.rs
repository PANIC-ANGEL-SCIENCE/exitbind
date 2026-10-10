//! Durable admission and failure facts inside the canonical run ledger.
//! Admission prevents concurrent execution; an unresolved effect never grants
//! another execution. Only an observed check supplies check evidence.
use super::*;

pub(crate) const PROTOCOL: u64 = 1;
pub(crate) const MAX_TIMEOUT_MS: u64 = 86_400_000;

/// New marked observations execute only the run's frozen configuration and
/// its still-current selected contract. Historical runs retain drift warnings.
pub(crate) fn assert_current(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if state["checkObservationProtocol"] != PROTOCOL {
        return Ok(());
    }
    if state["configSha256"] != hash::text(&loaded.source)
        || fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
    {
        return Err("check observation requires the frozen current configuration; inspect the Work and use authorized supersession for a changed configuration".into());
    }
    crate::project::architecture::assert_current(loaded)
}

pub(crate) fn timeout(value: Option<&str>) -> Result<u64, String> {
    let value = value
        .map(|v| parse_positive("--timeout-ms", v))
        .transpose()?
        .unwrap_or(DEFAULT_OBSERVE_TIMEOUT_MS);
    if value > MAX_TIMEOUT_MS {
        return Err(format!("--timeout-ms must be at most {MAX_TIMEOUT_MS}"));
    }
    Ok(value)
}

pub(crate) fn event(
    state: &Value,
    last: &Value,
    action: &str,
    details: Value,
) -> Result<Value, String> {
    Ok(run_state::make_event(json!({
        "version": 8, "kind": "run", "producer": crate::producer::evidence_for_version(8),
        "action": action, "runId": state["runId"], "observation": details,
        "previousEventSha256": last["eventSha256"], "timestamp": nondecreasing(&last["timestamp"])?
    })))
}

pub(super) fn admit(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    source: &mut String,
    state: &Value,
    events: &[Value],
    identity: Value,
    timeout_ms: u64,
) -> Result<Option<Value>, String> {
    if state["checkObservationProtocol"] != PROTOCOL {
        return Ok(None);
    }
    assert_current(loaded, state)?;
    let details = json!({"binding": identity, "timeoutMs": timeout_ms,
        "perStreamBytes": crate::run_value::MAX_CAPTURE_BYTES, "combinedBytes": 16 * 1_048_576,
        "state": "running"});
    if let Some(current) = state.get("checkObservation").filter(|v| v.is_object()) {
        if current["binding"] != details["binding"] || current["timeoutMs"] != timeout_ms {
            return Err("check observation binding or execution limit changed; inspect the current Work decision".into());
        }
        return Err("check observation is already admitted or failed; inspect Work detail and follow the Lead decision; no check was launched".into());
    }
    if fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source {
        return Err("configuration changed before observing; no check was launched".into());
    }
    let admission = event(
        state,
        events.last().ok_or("run ledger has no head")?,
        "check_observation",
        details,
    )?;
    let mut all = events.to_vec();
    all.push(admission.clone());
    run_state::reduce(&all)?;
    append(path, &admission, false, source)?;
    *source = load_at(loaded, path)?.2;
    Ok(Some(admission))
}

pub(super) fn fail(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    admission: Option<&Value>,
    facts: Value,
) -> Result<Value, String> {
    let Some(admission) = admission else {
        return Err("observation failed without recovery protocol".into());
    };
    with_lock(path, || {
        let (_, events, source) = load_at(loaded, path)?;
        // An acknowledged check always wins over a later storage/response error.
        if let Some(check) = events.iter().find(|e| {
            e["action"] == "check" && e["observationEventSha256"] == admission["eventSha256"]
        }) {
            return Ok(json!({"valid": true, "event": check, "recovered": true}));
        }
        let state = run_state::reduce(&events)?;
        let details = json!({"admissionEventSha256": admission["eventSha256"], "facts": facts});
        let failure = event(
            &state,
            events.last().ok_or("run ledger has no head")?,
            "check_observation_failed",
            details,
        )?;
        let mut all = events;
        all.push(failure.clone());
        run_state::reduce(&all)?;
        append(path, &failure, false, &source)?;
        Ok(json!({"valid": true, "observationFailure": failure, "checkRecorded": false}))
    })
}

pub(super) fn record_failure(
    loaded: &Loaded,
    ledger: &str,
    path: &run_ledger::LedgerPath,
    admission: Option<&Value>,
    facts: Value,
) -> Result<Value, String> {
    fail(loaded, path, admission, facts.clone()).map_err(|error| {
        let work = canonical_work_for_ledger(path).ok();
        let suffix = if let Some(work) = &work { vec!["work".into(), "detail".into(), work.clone(), "--json".into(), "--config".into()] }
            else { vec!["run".into(), "status".into(), ledger.into(), "--json".into(), "--config".into()] };
        let route = crate::work::response_recovery::bounded_argv(suffix, loaded.path.to_str(), 2048);
        format!("EXITBIND_JSON:{}", json!({
            "effect": "unknown", "checkRecorded": false, "durableFailureRecorded": false,
            "work": work, "reason": {"code": "observation_failure_storage_unavailable"},
            "observation": facts, "diagnostic": error.chars().filter(|c| !c.is_control()).take(512).collect::<String>(),
            "nextAction": {"type": "inspect", "safe": true, "command": route.argv,
                "sameConfigRequired": route.same_config, "sameExecutableRequired": route.same_executable},
            "recovery": "the admitted execution remains unresolved; inspect or stop, never retry"
        }))
    })
}

pub(crate) fn committed_replay(
    loaded: &Loaded,
    ledger: &str,
    timeout_ms: Option<&str>,
) -> Result<Option<Value>, String> {
    replay_for_operation(loaded, ledger, timeout_ms, None)
}

pub(super) fn replay_for_operation(
    loaded: &Loaded,
    ledger: &str,
    timeout_ms: Option<&str>,
    operation: Option<(&str, Option<&str>)>,
) -> Result<Option<Value>, String> {
    let timeout_ms = timeout(timeout_ms)?;
    let (path, events, _) = load(loaded, ledger)?;
    if events
        .first()
        .map_or(true, |start| start["checkObservationProtocol"] != PROTOCOL)
    {
        return Ok(None);
    }
    if claim_path(&path).exists() {
        return Err(
            "run has been superseded; committed replay cannot provide current authority".into(),
        );
    }
    let state = reduce_live(loaded, &events)?;
    if state["checkObservationProtocol"] != PROTOCOL || state["status"] != "running" {
        return Ok(None);
    }
    let Some(check) = events.iter().rev().find(|e| {
        e["action"] == "check"
            && e.get("observationEventSha256").is_some()
            && operation.map_or(true, |(target, requirement)| {
                e["targetEventSha256"] == target && e["requirementId"].as_str() == requirement
            })
    }) else {
        return Ok(None);
    };
    let Some(admission) = events
        .iter()
        .find(|e| e["eventSha256"] == check["observationEventSha256"])
    else {
        return Err("check admission is missing".into());
    };
    let binding = &admission["observation"]["binding"];
    if crate::run_value::validate_check_target(
        &state,
        binding["targetEventSha256"].as_str().unwrap_or(""),
    )
    .is_err()
    {
        return Ok(None);
    }
    assert_current(loaded, &state)?;
    let policy = active_check_policy(&state, binding["requirementId"].as_str())?;
    if binding["observerExecutableSha256"] != crate::producer::build_identity()["executableSha256"]
        || admission["producer"] != crate::producer::evidence_for_version(8)
        || binding["inputsSha256"] != live_inputs(&state)?
        || binding["configSha256"] != hash::text(&loaded.source)
        || binding["subjectSha256"] != state["subject"]["sha256"]
        || binding["checkCommandSha256"] != policy.command_sha256
        || admission["observation"]["timeoutMs"] != timeout_ms
        || fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
    {
        return Err("committed check replay refused changed binding, policy, configuration or execution limit".into());
    }
    crate::run::artifact::assert_current(loaded, &state)?;
    Ok(Some(
        json!({"valid": true, "event": check, "recovered": true}),
    ))
}

//! Durable admission and failure facts inside the canonical run ledger.
//! Admission prevents concurrent execution; an unresolved effect never grants
//! another execution. Only an observed check supplies check evidence.
use super::*;

pub(crate) const PROTOCOL: u64 = 1;
pub(crate) const MAX_TIMEOUT_MS: u64 = 86_400_000;

/// New marked observations execute only the run's frozen configuration and
/// its still-current selected contract. Historical runs retain drift warnings.
fn frozen_config_required(loaded: &Loaded, state: &Value) -> String {
    let current = std::fs::read_to_string(&loaded.path)
        .map(|source| crate::evidence::hash::text(&source))
        .ok();
    crate::drift::refusal(
        crate::drift::Class::CheckObservationConfiguration,
        state["configSha256"].as_str().unwrap_or_default(),
        current.as_deref(),
        None,
        crate::drift::Route::Supersede {
            ledger: None,
            config: Some(&loaded.path),
        },
    )
}

pub(crate) fn assert_current(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if state["checkObservationProtocol"] != PROTOCOL {
        return Ok(());
    }
    let path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not UTF-8")?;
    let current = crate::config::load(Some(path))?;
    if current.path != loaded.path
        || current.control_root != loaded.control_root
        || current.product_root != loaded.product_root
        || current.state_root != loaded.state_root
        || current.mode != loaded.mode
        || current.project_id != loaded.project_id
    {
        return Err(frozen_config_required(loaded, state));
    }
    crate::run::guidance_continuity::assert_config_current(&current, state)
        .map_err(|_| frozen_config_required(loaded, state))?;
    crate::project::architecture::assert_current(&current)?;
    crate::run::verification_preparation::assert_checker_sources_current(state)
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

/// The refusal of a second check while one observation is admitted: the
/// observation, whether it is still running or failed, and the route that
/// reads it. `work` is the Work the refused command named, when known.
pub(crate) fn admitted_refusal(
    observation: &Value,
    work: Option<&str>,
    config: &std::path::Path,
) -> String {
    let slot = crate::emitted::slot("WORK");
    let detail = crate::emitted::command_line(
        &["work", "detail", work.unwrap_or(&slot), "--view", "action"],
        Some(config),
    );
    let event = observation["eventSha256"].as_str().unwrap_or("unknown");
    let on = work.map_or_else(|| "this Work".to_owned(), |work| format!("Work {work}"));
    if observation["state"] == "running" {
        format!("check observation {event} is running on {on}: admitted, no end recorded yet; no check was launched. Read it with {detail}")
    } else {
        format!("check observation {event} on {on} failed without a check result; no check was launched. Read it and follow the Lead decision with {detail}")
    }
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
    // The recorder's identity is durably part of the admission: if this
    // process ends before recording the outcome, the next check can tell the
    // observer ended instead of refusing forever.
    let observer = crate::host::codex_exec::observer_identity();
    let details = json!({"binding": identity, "timeoutMs": timeout_ms,
        "perStreamBytes": crate::run_value::MAX_CAPTURE_BYTES, "combinedBytes": 16 * 1_048_576,
        "state": "running",
        "observer": {"pid": observer.pid, "startTimeTicks": observer.start_time_ticks}});
    if let Some(current) = state.get("checkObservation").filter(|v| v.is_object()) {
        if current["binding"] != details["binding"] || current["timeoutMs"] != timeout_ms {
            return Err("check observation binding or execution limit changed; inspect the current Work decision".into());
        }
        return Err(admitted_refusal(current, None, &loaded.path));
    }
    // Re-read and revalidate under the ledger lock immediately before the
    // durable admission. Marked advisory-only config edits are allowed; the
    // frozen influence projection and strict source checks still apply.
    assert_current(loaded, state)?;
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

/// Whether a running admission's recorder is known to have ended. A live or
/// unverifiable recorder is never closed, and an admission recorded by a
/// version that stored no observer stays open as it was.
pub(crate) fn observer_ended(observation: &Value) -> bool {
    let Some(pid) = observation["observer"]["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return false;
    };
    let identity = crate::host::codex_exec::ProcessIdentity {
        pid,
        process_group: None,
        start_time_ticks: observation["observer"]["startTimeTicks"].as_u64(),
    };
    crate::host::codex_exec::observer_liveness(&identity)
        == crate::host::codex_exec::ProcessLiveness::Ended
}

/// Close an admission whose recorder ended before recording the outcome. The
/// execution's end is unknown, so the failure facts are the conservative
/// unknown termination the existing quarantine decision already owns; the
/// normal failure path records it and raises the Lead's observation decision.
pub(crate) fn close_dead_admission(
    loaded: &Loaded,
    ledger: &str,
    admission: &Value,
) -> Result<Value, String> {
    let (path, _, _) = load(loaded, ledger)?;
    let facts = json!({
        "process": Value::Null,
        "processStarted": true,
        "groupEnded": false,
        "captureReadersEnded": false,
        "captureAvailability": "unavailable",
        "durationMs": 0,
        "capture": "disabled",
        "deadlineExceeded": false,
        "partialCaptures": [],
        "storageStage": "capture",
        "storage": "not_committed",
        "termination": "unknown",
        "diagnostic": "the process observing this check ended before recording its result; the execution's end is unknown",
        "cleanupErrorCount": 0,
        "remainingOwnedPathCount": 0
    });
    fail(loaded, &path, Some(admission), facts)
}

/// Where an unrecorded observation failure is read: the Work, or the run.
fn failure_read(work: Option<&str>, ledger: &str) -> crate::emitted::Route {
    match work {
        Some(work) => crate::emitted::Route::new(["work", "detail", work, "--json"]),
        None => crate::emitted::Route::new(["run", "status", ledger, "--json"]),
    }
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
        let route = failure_read(work.as_deref(), ledger).config(&loaded.path).within(2048);
        format!("EXITBIND_JSON:{}", json!({
            "effect": "unknown", "checkRecorded": false, "durableFailureRecorded": false,
            "work": work, "reason": {"code": "observation_failure_storage_unavailable"},
            "observation": facts, "diagnostic": crate::drift::plain(&error).chars().filter(|c| !c.is_control()).take(512).collect::<String>(),
            "nextAction": {"type": "inspect", "safe": true, "command": route.argv_value(),
                "sameConfigRequired": route.needs_config(), "sameExecutableRequired": route.needs_executable()},
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
    let attested = super::environmental_check::attested(&state);
    let Some(check) = events.iter().rev().find(|e| {
        e["action"] == "check"
            && e.get("observationEventSha256").is_some()
            && e["eventSha256"]
                .as_str()
                .map_or(true, |sha| !attested.contains(sha))
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
    if state.get("verificationPreparationProtocol").is_some() {
        let sources = crate::run::verification_preparation::current_checker_sources(
            loaded,
            &state,
            &policy.command,
        )?;
        let operation_id = binding["verificationOperationId"]
            .as_str()
            .ok_or("committed preparation replay has no operation identity")?;
        let preparation =
            crate::run::verification_preparation::planned(loaded, &state, operation_id, &sources)?
                .ok_or("committed preparation replay lost its selected recipe")?;
        if binding["verificationPreparationSha256"] != preparation.sha256()? {
            return Err(
                "committed check replay refused changed preparation recipe or checker sources"
                    .into(),
            );
        }
    } else if binding.get("verificationPreparationSha256").is_some()
        || binding.get("verificationOperationId").is_some()
    {
        return Err("committed check replay has preparation fields for an unselected run".into());
    }
    // The result may be returned only while the exact frozen influence remains
    // current, even if an advisory-only field changed after the first check.
    assert_current(loaded, &state)?;
    if binding["observerExecutableSha256"] != crate::producer::build_identity()["executableSha256"]
        || admission["producer"] != crate::producer::evidence_for_version(8)
        || binding["inputsSha256"] != live_inputs(&state)?
        || binding["configSha256"] != state["configSha256"]
        || binding["subjectSha256"] != state["subject"]["sha256"]
        || binding["checkCommandSha256"] != policy.command_sha256
        || admission["observation"]["timeoutMs"] != timeout_ms
    {
        return Err("committed check replay refused changed binding, policy, configuration or execution limit".into());
    }
    crate::run::artifact::assert_current(loaded, &state)?;
    super::continuation_effect::recover_observed_check(loaded, ledger, check)?;
    Ok(Some(
        json!({"valid": true, "event": check, "recovered": true}),
    ))
}

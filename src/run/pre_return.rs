//! Opt-in, source-bound diagnostic execution before a successful Worker return.
mod binding_diagnostic;
mod execution;
mod journal;
pub(crate) mod ownership;
mod predecessor;
pub(crate) mod recovery;
mod response;

use self::execution::run_or_recover;
use self::journal::{
    attempt_path, directory_path, ensure_directory, install_captured_logs, read_record,
    record_bytes, record_path, write_durable,
};
use self::response::{current_failure_response, failure_response, read_route, unavailable_refusal};
use super::submission::matching_mutation_grants;
use super::*;

#[allow(clippy::too_many_arguments)]
fn current_binding(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    events: &[Value],
    state: &Value,
    assignment: &Value,
    assignment_sha256: Option<&str>,
    artifact: &Value,
    policy: &crate::run_value::PreReturnPolicy,
) -> Result<Value, String> {
    let input_sha = live_inputs(state)?;
    let work = canonical_work_for_ledger(path).ok();
    let ledger = crate::project::path::inputs::within(&path.root, &path.path)
        .ok_or("ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let selector = work.clone().unwrap_or_else(|| ledger.clone());
    let grants = if state["governor"]["enabled"] == true {
        matching_mutation_grants(events, state, assignment, assignment_sha256)?
    } else {
        Vec::new()
    };
    let mut operation_id = crate::evidence::hash::value(&json!({
        "runId": state["runId"],
        "stage": assignment["stage"],
        "attempt": assignment["attempt"],
        "agent": assignment["agent"],
        "assignmentSha256": crate::evidence::hash::value(assignment),
        "resultSha256": artifact["sha256"],
        "policySha256": crate::evidence::hash::value(&policy.value()),
        "operation": "pre-return"
    }));
    let recovery = recovery::for_assignment(state, assignment)?;
    if let Some(recovery) = recovery {
        operation_id = crate::evidence::hash::value(&json!({
            "operation": "pre-return-recovery", "previousOperationId": operation_id,
            "reconciliationEventSha256": recovery["eventSha256"]
        }));
    }
    let preparation = crate::run::verification_preparation::planned(
        loaded,
        state,
        &operation_id,
        &policy.checker_sources,
    )?;
    if !grants.is_empty() && work.is_none() {
        return Err("governed permits require a canonical work ledger".into());
    }
    let mut binding = json!({
        "work": work, "ledger": ledger, "selector": selector, "runId": state["runId"],
        "stage": assignment["stage"], "attempt": assignment["attempt"], "agent": assignment["agent"],
        "role": assignment["role"], "assignmentSha256": crate::evidence::hash::value(assignment),
        "resultSha256": artifact["sha256"], "inputsSha256": input_sha,
        "subjectSha256": state["subject"]["sha256"], "configSha256": crate::evidence::hash::text(&loaded.source),
        "policySha256": crate::evidence::hash::value(&policy.value()),
        "grantSetSha256": crate::evidence::hash::value(&json!(grants)),
        "checkerExecutableSha256": crate::producer::build_identity()["executableSha256"],
        "producer": crate::producer::evidence_for_version(8)
    });
    if let Some(recovery) = recovery {
        binding["preReturnReconciliationSha256"] = recovery["eventSha256"].clone();
    }
    if let Some(preparation) = preparation {
        binding["verificationPreparation"] = preparation.identity()?;
        binding["verificationPreparationSha256"] = json!(preparation.sha256()?);
        binding["verificationOperationId"] = json!(operation_id);
    }
    Ok(binding)
}

fn assert_current_configuration(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if state["preReturnPolicy"].is_null() {
        return Ok(());
    }
    let current = fs::read_to_string(&loaded.path).map_err(|e| e.to_string())?;
    if state["configSha256"] != crate::evidence::hash::text(&loaded.source)
        || current != loaded.source
    {
        return Err(crate::drift::refusal(
            crate::drift::Class::PreReturnConfiguration,
            state["configSha256"].as_str().unwrap_or_default(),
            Some(&crate::evidence::hash::text(&current)),
            None,
            crate::drift::Route::Supersede {
                ledger: None,
                config: Some(&loaded.path),
            },
        ));
    }
    crate::project::architecture::assert_current(loaded)
        .map_err(|e| format!("pre-return architecture binding is stale: {e}"))?;
    let policy = crate::run_value::pre_return_policy_from_value(&state["preReturnPolicy"], 1)?;
    crate::run_value::validate_checker_sources(&policy.checker_sources, "pre-return")?;
    if let Some(warning) = super::input_drift_warning_for(loaded, state, None)? {
        return Err(crate::drift::refusal(
            crate::drift::Class::PreReturnInputs,
            warning["expectedInputsSha256"].as_str().unwrap_or_default(),
            warning["currentInputsSha256"].as_str(),
            None,
            crate::drift::Route::Supersede {
                ledger: None,
                config: Some(&loaded.path),
            },
        ));
    }
    Ok(())
}

type PreparedAdmission = (
    String,
    Value,
    bool,
    super::continuation_effect::Ticket,
    Option<String>,
);

fn latest_or_create(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    binding: Value,
    policy: &crate::run_value::PreReturnPolicy,
    state: &Value,
    own_repair: Option<&str>,
) -> Result<PreparedAdmission, String> {
    let attempt_id =
        crate::evidence::hash::value(&json!({"binding": binding, "limits": policy.value()}));
    predecessor::assert_no_unresolved(loaded, &binding, &attempt_id, state)?;
    let attempt_path = attempt_path(loaded, state, &attempt_id)?;
    if attempt_path.exists() {
        if state.get(ownership::FIELD).is_some()
            && !ownership::events(state, None)
                .iter()
                .any(|event| event["admission"]["attemptId"] == attempt_id)
        {
            return Err(unavailable_refusal(
                loaded,
                state,
                &binding,
                &attempt_id,
                "admission is not recorded in its owning ledger",
            ));
        }
        let attempt = predecessor::read_attempt(loaded, state, &attempt_id)?;
        if attempt["binding"] != binding || attempt["policy"] != policy.value() {
            return Err("pre-return attempt identity collision".into());
        }
        let result_path = record_path(loaded, &attempt_id, "result")?;
        if result_path.exists() {
            let result = read_record(&result_path)?;
            predecessor::validate_saved(loaded, &attempt, &result)?;
            execution::recover_saved_attempt(
                loaded,
                state,
                &attempt_id,
                &binding,
                policy,
                &result,
            )?;
            if result["state"] == "passed" {
                return Ok((attempt_id, result, true, None, None));
            }
            let mut replay = result;
            replay["replayed"] = json!(true);
            return Err(failure_response(loaded, &attempt_id, &replay, &attempt));
        }
        return Err(format!("pre-return attempt {attempt_id} has an uncertain outcome; inspect it and do not replay"));
    }
    let attempt = json!({"version":1,"kind":"pre_return_attempt","attemptId":attempt_id,"binding":binding,
        "policy":policy.value(),"state":"admitted","createdAt":super::now()});
    let bytes = record_bytes(&attempt)?;
    let ticket = super::continuation_effect::begin_check(
        loaded,
        state,
        "pre-return",
        &json!({"attemptId":attempt_id,"binding":binding,"policy":policy.value()}),
        &policy.command,
        binding["role"].as_str().unwrap_or("worker"),
        binding["agent"].as_str(),
    )?;
    ensure_directory(loaded)?;
    let recorded: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    predecessor::validate_admission(&recorded, &attempt_id)?;
    let admission_event = ownership::admit(loaded, path, state, &recorded, own_repair)?;
    if let Err(error) = write_durable(&attempt_path, &bytes) {
        return if admission_event.is_some() {
            Err(unavailable_refusal(
                loaded,
                state,
                &binding,
                &attempt_id,
                &format!("owning admission record could not be stored: {error}"),
            ))
        } else {
            Err(error)
        };
    }
    Ok((attempt_id, recorded, false, ticket, admission_event))
}

fn prepare_locked(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    agent: &str,
    expected: Option<&AssignmentIdentity>,
    assignment_sha256: Option<&str>,
    artifact_root: Option<&str>,
    artifact: &str,
) -> Result<Option<(String, Value, crate::run_value::PreReturnPolicy, Value)>, String> {
    let (_, events, _) = load_at(loaded, path)?;
    let state = reduce_live(loaded, &events)?;
    if state["status"] != "running" {
        return Err("run has already reached a terminal state; no mutation was made".into());
    }
    let Some(policy_value) = state.get("preReturnPolicy") else {
        return Ok(None);
    };
    let policy = crate::run_value::pre_return_policy_from_value(policy_value, 1)?;
    assert_current_configuration(loaded, &state)?;
    crate::run::artifact::assert_current(loaded, &state)?;
    let assignment = crate::run::assignment::pending(&state)
        .into_iter()
        .find(|item| item["agent"] == agent)
        .ok_or_else(|| format!("agent '{agent}' is not currently pending"))?;
    if assignment["role"] != "worker" {
        return Ok(None);
    }
    if let Some(expected) = expected {
        if !expected.matches(&assignment) {
            return Err("assignment changed before pre-return check; no mutation was made".into());
        }
    }
    let evidence = crate::run::artifact::evidence(loaded, artifact_root, artifact)?;
    let binding = current_binding(
        loaded,
        path,
        &events,
        &state,
        &assignment,
        assignment_sha256,
        &evidence,
        &policy,
    )?;
    Ok(Some((
        crate::evidence::hash::value(&assignment),
        binding,
        policy,
        evidence,
    )))
}

pub(super) fn enabled(loaded: &Loaded, ledger: &str) -> Result<bool, String> {
    let (_, events, _) = load(loaded, ledger)?;
    Ok(reduce_live(loaded, &events)?
        .get("preReturnPolicy")
        .is_some())
}

/// Called under the same ledger lock as the possible forward-repair event.
/// An interrupted check must be resolved before that event can be appended.
pub(super) fn guard_worker_before_effect(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    events: &[Value],
    state: &Value,
    expected: &AssignmentIdentity,
    assignment_handle: &str,
) -> Result<(), String> {
    let Some(policy_value) = state.get("preReturnPolicy") else {
        return Ok(());
    };
    let assignment = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| expected.matches(item))
        .ok_or("worker assignment changed before the pre-return guard")?;
    let work = canonical_work_for_ledger(path)?;
    if crate::run::assignment::handle(&work, &assignment)? != assignment_handle {
        return Err("worker assignment handle changed before the pre-return guard".into());
    }
    let ledger = path
        .path
        .strip_prefix(&path.root)
        .map_err(|_| "ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let identity = json!({"runId":state["runId"],"ledger":ledger,"stage":assignment["stage"],
        "attempt":assignment["attempt"],"agent":assignment["agent"],"role":assignment["role"]});
    let pending = predecessor::unresolved(loaded, &identity, state)?;
    let Some(reconciled) = recovery::for_assignment(state, &assignment)? else {
        if let Some(attempt) = pending.first() {
            return Err(format!(
                "pre-return attempt {} has an uncertain outcome; inspect it and do not replay",
                attempt["attemptId"]
            ));
        }
        return Ok(());
    };
    let snapshot = &reconciled["recovery"]["snapshot"];
    let predecessor = &snapshot["predecessor"]["attemptId"];
    if pending
        .iter()
        .any(|attempt| attempt["attemptId"] != *predecessor)
    {
        return Err(
            "the one reconciled pre-return retry is unfinished; no further effect is admitted"
                .into(),
        );
    }
    let policy = crate::run_value::pre_return_policy_from_value(policy_value, 1)?;
    let binding = current_binding(
        loaded,
        path,
        events,
        state,
        &assignment,
        Some(&crate::evidence::hash::text(assignment_handle)),
        &json!({"sha256":snapshot["held"]["sha256"]}),
        &policy,
    )?;
    let current_id =
        crate::evidence::hash::value(&json!({"binding":binding,"limits":policy.value()}));
    predecessor::assert_no_unresolved(loaded, &binding, &current_id, state)
}

pub(super) struct CallerBinding<'a> {
    pub(super) check: &'a dyn Fn() -> Result<(), String>,
    pub(super) own_repair: Option<&'a str>,
}

pub(super) fn prepare_for_assignment<F: FnOnce() -> Result<String, String>>(
    loaded: &Loaded,
    ledger: &str,
    agent: &str,
    expected: &AssignmentIdentity,
    assignment_handle_sha256: &str,
    artifact: F,
    caller: CallerBinding<'_>,
) -> Result<(String, String, Option<String>), String> {
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    let mut artifact = Some(artifact);
    let prepared = with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let (_, events, _) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        if !caller.own_repair.is_some_and(|sha| {
            events
                .last()
                .and_then(|event| event["eventSha256"].as_str())
                == Some(sha)
        }) {
            (caller.check)()?;
        }
        let produce = artifact
            .take()
            .ok_or("artifact producer was already consumed")?;
        let artifact_path = produce()?;
        if state.get("preReturnPolicy").is_none() {
            return Err("pre-return policy changed before check admission".into());
        }
        let (_attempt_id, binding, policy, _) = prepare_locked(
            loaded,
            &path,
            agent,
            Some(expected),
            Some(assignment_handle_sha256),
            Some("state"),
            &artifact_path,
        )?
        .ok_or("pre-return policy is not applicable to the current Worker completion")?;
        let state = reduce_live(loaded, &load_at(loaded, &path)?.1)?;
        if state["governor"]["enabled"] == true {
            if state["governor"]["state"] != "ready" {
                return Err(GOVERNOR_COMPLETION_REFUSAL.into());
            }
            let assignment = crate::run::assignment::pending(&state)
                .into_iter()
                .find(|item| item["agent"] == agent)
                .ok_or("worker assignment changed before pre-return check")?;
            matching_mutation_grants(
                &load_at(loaded, &path)?.1,
                &state,
                &assignment,
                Some(assignment_handle_sha256),
            )?;
        }
        let (attempt_id, attempt, replayed, ticket, admission_event) = latest_or_create(
            loaded,
            &path,
            binding.clone(),
            &policy,
            &state,
            caller.own_repair,
        )?;
        Ok((
            artifact_path,
            attempt_id,
            attempt,
            replayed,
            policy,
            state,
            ticket,
            admission_event,
        ))
    })?;
    let (artifact_path, attempt_id, attempt, replayed, policy, state, ticket, admission_event) =
        prepared;
    run_or_recover(
        loaded,
        &attempt_id,
        &attempt,
        &policy,
        &state,
        replayed,
        &ticket,
    )?;
    Ok((artifact_path, attempt_id, admission_event))
}

pub(super) fn prepare_direct(
    loaded: &Loaded,
    ledger: &str,
    agent: &str,
    outcome: &str,
    artifact_root: Option<&str>,
    artifact: &str,
) -> Result<Option<String>, String> {
    if outcome != "completed" {
        return Ok(None);
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    let prepared = with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let Some((_attempt_id, binding, policy, _)) =
            prepare_locked(loaded, &path, agent, None, None, artifact_root, artifact)?
        else {
            return Ok(None);
        };
        let (_, events, _) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        if state["governor"]["enabled"] == true {
            if state["governor"]["state"] != "ready" {
                return Err(GOVERNOR_COMPLETION_REFUSAL.into());
            }
            let assignment = crate::run::assignment::pending(&state)
                .into_iter()
                .find(|item| item["agent"] == agent)
                .ok_or("worker assignment changed before pre-return check")?;
            let grants = matching_mutation_grants(&events, &state, &assignment, None)?;
            if !grants.is_empty() {
                return Err(
                    "exact assignment identity is required to acknowledge mutation grants".into(),
                );
            }
        }
        let (attempt_id, attempt, replayed, ticket, _) =
            latest_or_create(loaded, &path, binding, &policy, &state, None)?;
        Ok(Some((attempt_id, attempt, replayed, policy, state, ticket)))
    })?;
    let Some((attempt_id, attempt, replayed, policy, state, ticket)) = prepared else {
        return Ok(None);
    };
    run_or_recover(
        loaded,
        &attempt_id,
        &attempt,
        &policy,
        &state,
        replayed,
        &ticket,
    )?;
    Ok(Some(attempt_id))
}

pub(super) fn assert_pass_current(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    state: &Value,
    assignment: &Value,
    assignment_sha256: Option<&str>,
    artifact: &Value,
    attempt_id: Option<&str>,
) -> Result<(), String> {
    let Some(policy_value) = state.get("preReturnPolicy") else {
        return Ok(());
    };
    let policy = crate::run_value::pre_return_policy_from_value(policy_value, 1)?;
    let events = state["events"]
        .as_array()
        .ok_or("current run events are unavailable")?;
    let binding = current_binding(
        loaded,
        path,
        events,
        state,
        assignment,
        assignment_sha256,
        artifact,
        &policy,
    )?;
    let id = crate::evidence::hash::value(&json!({"binding":binding,"limits":policy.value()}));
    predecessor::assert_no_unresolved(loaded, &binding, &id, state)?;
    if let Some(expected) = attempt_id.filter(|expected| *expected != id) {
        let saved = predecessor::read_attempt(loaded, state, expected)?;
        return Err(binding_diagnostic::mismatch(
            loaded, &saved, &binding, state, expected,
        ));
    }
    let result = read_record(&record_path(loaded, &id, "result")?)?;
    let attempt = predecessor::read_attempt(loaded, state, &id)?;
    predecessor::validate_saved(loaded, &attempt, &result)?;
    if attempt["binding"] != binding || attempt["policy"] != policy.value() {
        return Err("pre-return attempt evidence does not match the current result binding".into());
    }
    if result["state"] != "passed" || result["attemptId"] != id {
        return Err(
            "a current passing pre-return attempt is required before completed submission".into(),
        );
    }
    if let Err(error) = assert_current_configuration(loaded, state) {
        let category = binding_diagnostic::category(&error);
        return Err(format!(
            "{error}; changed components: {category}; inspect the saved attempt with {}",
            binding_diagnostic::readback(loaded, attempt_id, &attempt)
        ));
    }
    if let Err(error) = crate::run::artifact::assert_current(loaded, state) {
        return Err(format!(
            "{error}; changed components: artifact; inspect the saved attempt with {}",
            binding_diagnostic::readback(loaded, attempt_id, &attempt)
        ));
    }
    Ok(())
}

pub(crate) fn projection(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = load(loaded, &ledger)?;
    let state = reduce_live(loaded, &events)?;
    let Some(policy) = state.get("preReturnPolicy") else {
        return Ok(json!({"enabled":false,"policy":null,"attempt":null}));
    };
    let mut latest: Option<Value> = None;
    let attempts = if state.get(ownership::FIELD).is_some() {
        match predecessor::attempts(loaded, &state, None) {
            Ok(attempts) if state[ownership::FIELD] == ownership::PROTOCOL => {
                attempts.into_iter().last().into_iter().collect()
            }
            Ok(attempts) => attempts,
            Err(error) => {
                return Ok(json!({"enabled":true,"policy":policy,"attempt":null,
                    "unavailable":{"reason":error},"readRoute":null}));
            }
        }
    } else {
        predecessor::attempts(loaded, &state, None)?
    };
    for attempt in attempts {
        if attempt["binding"]["runId"] != state["runId"] || attempt["binding"]["ledger"] != ledger {
            continue;
        }
        let result_path = record_path(
            loaded,
            attempt["attemptId"].as_str().unwrap_or(""),
            "result",
        )?;
        let result = if result_path.exists() {
            Some(read_record(&result_path)?)
        } else {
            None
        };
        let attempt_time = attempt["createdAt"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .ok_or("pre-return attempt timestamp is invalid")?;
        let newer = match latest.as_ref() {
            None => true,
            Some(prior) => {
                let prior_time = prior["attempt"]["createdAt"]
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .ok_or("pre-return attempt timestamp is invalid")?;
                attempt_time > prior_time
            }
        };
        if newer {
            latest = Some(json!({"attempt":attempt,"result":result}));
        }
    }
    Ok(
        json!({"enabled":true,"policy":policy,"attempt":latest,"readRoute":latest.as_ref().map(|item|read_route(loaded,item["attempt"]["binding"]["selector"].as_str().unwrap_or(""),item["attempt"]["attemptId"].as_str().unwrap_or(""),None))}),
    )
}

pub(crate) fn read(loaded: &Loaded, work: &str, attempt_id: &str) -> Result<Value, String> {
    let path = selected_ledger(loaded, work)?;
    let (_, events, _) = load_at(loaded, &path)?;
    let state = reduce_live(loaded, &events)?;
    if state.get("preReturnPolicy").is_none() {
        return Err("Work has no pre-return policy".into());
    }
    let attempt = predecessor::read_attempt(loaded, &state, attempt_id)?;
    let result_path = record_path(loaded, attempt_id, "result")?;
    let result = if result_path.exists() {
        Some(read_record(&result_path)?)
    } else {
        None
    };
    let ledger_identity = path
        .path
        .strip_prefix(&path.root)
        .map_err(|_| "ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let expected_work = canonical_work_for_ledger(&path).ok();
    if attempt["binding"]["runId"] != state["runId"]
        || attempt["binding"]["ledger"] != ledger_identity
        || attempt["binding"]["work"] != expected_work.map(Value::from).unwrap_or(Value::Null)
        || attempt["binding"]["selector"] != work
    {
        return Err("pre-return attempt does not belong to this Work or ledger".into());
    }
    Ok(json!({"attempt":attempt,"result":result}))
}

fn selected_ledger(loaded: &Loaded, selector: &str) -> Result<run_ledger::LedgerPath, String> {
    if selector.starts_with("smw_") {
        let ledger = crate::work::resolve(loaded, selector)?;
        let (path, _, _) = load(loaded, &ledger)?;
        return Ok(path);
    }
    let path = ledger_path(&loaded.state_root, selector, false)?;
    load_at(loaded, &path)?;
    Ok(path)
}

pub(crate) fn read_stream(
    loaded: &Loaded,
    work: &str,
    attempt_id: &str,
    stream: &str,
) -> Result<Vec<u8>, String> {
    if !matches!(stream, "stdout" | "stderr") {
        return Err("--stream must be stdout or stderr".into());
    }
    let record = read(loaded, work, attempt_id)?;
    let reference = record["result"][stream]
        .as_object()
        .map(|_| record["result"][stream].clone())
        .ok_or("pre-return stream is unavailable")?;
    crate::run::artifact::read_exact(
        loaded,
        &reference,
        "pre-return capture",
        crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
    )
}

/// Bind a Lead's blocked report to one exact known-ended nonzero diagnostic.
/// Capture or execution uncertainty is never accepted as defective-policy evidence.
pub(crate) fn replacement_source_attempt(
    loaded: &Loaded,
    ledger: &run_ledger::LedgerPath,
    start: &Value,
    blocked_event: &Value,
) -> Result<String, String> {
    let frozen = start
        .get("preReturnPolicy")
        .ok_or("pre-return replacement requires a frozen pre-return policy")?;
    let report = crate::run::artifact::read_exact(
        loaded,
        &blocked_event["artifact"],
        "blocked-worker report",
        crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
    )?;
    let report = String::from_utf8(report)
        .map_err(|_| "blocked-worker report must be UTF-8 to bind a diagnostic attempt")?;
    let relative = ledger
        .path
        .strip_prefix(&ledger.root)
        .map_err(|_| "ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let (_, events, _) = run_ledger::load_at_unchecked(loaded, ledger)?;
    if events.first() != Some(start) || !events.iter().any(|event| event == blocked_event) {
        return Err("blocked pre-return source differs from its owning ledger".into());
    }
    let state = run_state::reduce(&events)?;
    let attempts = if state.get(ownership::FIELD).is_some() {
        ownership::events(&state, None)
            .into_iter()
            .filter(|event| {
                event["admission"]["attemptId"]
                    .as_str()
                    .is_some_and(|id| report.contains(id))
            })
            .map(|event| {
                let attempt = ownership::read_owned(loaded, &state, event)?;
                let id = event["admission"]["attemptId"]
                    .as_str()
                    .ok_or("missing pre-return admission identity")?;
                predecessor::validate_admission(&attempt, id)?;
                Ok(attempt)
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        predecessor::attempts(loaded, &state, None)?
    };
    let mut matching = Vec::new();
    for attempt in attempts {
        let Some(attempt_id) = attempt["attemptId"].as_str() else {
            return Err("retained pre-return attempt has no identity".into());
        };
        if attempt["binding"]["runId"] != start["runId"]
            || attempt["binding"]["ledger"] != relative
            || attempt["policy"] != *frozen
            || !report.contains(attempt_id)
        {
            continue;
        }
        let result = read_record(&record_path(loaded, attempt_id, "result")?)?;
        if result["attemptId"] != attempt_id
            || result["state"] != "failed"
            || result["failureKind"] != "nonzero_exit"
            || !result["exitCode"].as_i64().is_some_and(|code| code != 0)
            || result["bindingSha256"] != crate::evidence::hash::value(&attempt["binding"])
            || result["policySha256"] != crate::evidence::hash::value(frozen)
            || result["producer"]["producer"] != attempt["binding"]["producer"]
            || result["producer"]["executableSha256"]
                != attempt["binding"]["checkerExecutableSha256"]
        {
            continue;
        }
        for stream in ["stdout", "stderr"] {
            let reference = result
                .get(stream)
                .ok_or("known-ended pre-return result has incomplete capture")?;
            crate::run::artifact::read_exact(
                loaded,
                reference,
                "pre-return capture",
                crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
            )?;
        }
        matching.push(attempt_id.to_owned());
    }
    if matching.len() != 1 {
        return Err(
            "blocked-worker report must identify exactly one known-ended failed pre-return attempt"
                .into(),
        );
    }
    Ok(matching.remove(0))
}

pub(crate) fn replacement_ready(loaded: &Loaded, work: &str) -> Result<Option<String>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = load(loaded, &ledger)?;
    let state = reduce_live(loaded, &events)?;
    if state["status"] != "blocked" || state.get("preReturnPolicy").is_none() {
        return Ok(None);
    }
    let Some(blocked) = events.iter().rev().find(|event| {
        event["action"] == "submit" && event["role"] == "worker" && event["outcome"] == "blocked"
    }) else {
        return Ok(None);
    };
    replacement_source_attempt(
        loaded,
        &ledger_path(&loaded.state_root, &ledger, false)?,
        &events[0],
        blocked,
    )
    .map(Some)
}

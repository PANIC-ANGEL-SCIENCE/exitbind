//! Opt-in, source-bound diagnostic execution before a successful Worker return.
mod binding_diagnostic;
mod journal;

use self::journal::{
    directory_path, ensure_directory, install_captured_logs, read_record, record_bytes,
    record_path, write_durable,
};
use super::submission::matching_mutation_grants;
use super::*;
use std::time::{Duration, Instant};

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
    let ledger = path
        .path
        .strip_prefix(&path.root)
        .map_err(|_| "ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let selector = work.clone().unwrap_or_else(|| ledger.clone());
    let grants = if state["governor"]["enabled"] == true {
        matching_mutation_grants(events, state, assignment, assignment_sha256)?
    } else {
        Vec::new()
    };
    if !grants.is_empty() && work.is_none() {
        return Err("governed permits require a canonical work ledger".into());
    }
    Ok(json!({
        "work": work, "ledger": ledger, "selector": selector, "runId": state["runId"],
        "stage": assignment["stage"], "attempt": assignment["attempt"], "agent": assignment["agent"],
        "role": assignment["role"], "assignmentSha256": crate::evidence::hash::value(assignment),
        "resultSha256": artifact["sha256"], "inputsSha256": input_sha,
        "subjectSha256": state["subject"]["sha256"], "configSha256": crate::evidence::hash::text(&loaded.source),
        "policySha256": crate::evidence::hash::value(&policy.value()),
        "grantSetSha256": crate::evidence::hash::value(&json!(grants)),
        "checkerExecutableSha256": crate::producer::build_identity()["executableSha256"],
        "producer": crate::producer::evidence_for_version(8)
    }))
}

fn assert_current_configuration(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if state["preReturnPolicy"].is_null() {
        return Ok(());
    }
    if state["configSha256"] != crate::evidence::hash::text(&loaded.source)
        || fs::read_to_string(&loaded.path).map_err(|e| e.to_string())? != loaded.source
    {
        return Err("pre-return check requires the frozen current configuration; inspect and use authorized supersession for a changed configuration".into());
    }
    crate::project::architecture::assert_current(loaded)
        .map_err(|e| format!("pre-return architecture binding is stale: {e}"))?;
    let policy = crate::run_value::pre_return_policy_from_value(&state["preReturnPolicy"], 1)?;
    for source in policy.checker_sources {
        let argument_path = source.argument_path.as_deref().unwrap_or(&source.path);
        let path = std::path::Path::new(argument_path);
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("pre-return checker source is unavailable: {e}"))?;
        if canonical
            .to_str()
            .ok_or("pre-return checker path is not valid UTF-8")?
            != source.path
            || crate::evidence::hash::file(&canonical)? != source.sha256
        {
            return Err("pre-return checker source changed; use authorized supersession to bind the changed checker".into());
        }
    }
    if let Some(warning) = super::input_drift_warning_for(loaded, state, None)? {
        return Err(format!(
            "pre-return check requires current product inputs; use authorized supersession after input drift: {}",
            warning["currentInputsSha256"].as_str().unwrap_or("unavailable")
        ));
    }
    Ok(())
}

fn latest_or_create(
    loaded: &Loaded,
    binding: Value,
    policy: &crate::run_value::PreReturnPolicy,
) -> Result<(String, Value, bool), String> {
    let attempt_id =
        crate::evidence::hash::value(&json!({"binding": binding, "limits": policy.value()}));
    let attempt_path = record_path(loaded, &attempt_id, "attempt")?;
    if attempt_path.exists() {
        let attempt = read_record(&attempt_path)?;
        if attempt["binding"] != binding || attempt["policy"] != policy.value() {
            return Err("pre-return attempt identity collision".into());
        }
        let result_path = record_path(loaded, &attempt_id, "result")?;
        if result_path.exists() {
            let result = read_record(&result_path)?;
            if result["attemptId"] != attempt_id
                || result["bindingSha256"] != crate::evidence::hash::value(&binding)
                || result["policySha256"] != crate::evidence::hash::value(&policy.value())
            {
                return Err("pre-return result identity mismatch".into());
            }
            if result["state"] == "passed" {
                return Ok((attempt_id, result, true));
            }
            let mut replay = result;
            replay["replayed"] = json!(true);
            return Err(failure_response(loaded, &attempt_id, &replay));
        }
        return Err(format!("pre-return attempt {attempt_id} has an uncertain outcome; inspect it and do not replay"));
    }
    let attempt = json!({"version":1,"kind":"pre_return_attempt","attemptId":attempt_id,"binding":binding,
        "policy":policy.value(),"state":"admitted","createdAt":super::now()});
    let bytes = record_bytes(&attempt)?;
    ensure_directory(loaded)?;
    write_durable(&attempt_path, &bytes)?;
    Ok((attempt_id, attempt, false))
}

fn failure_response(loaded: &Loaded, attempt_id: &str, result: &Value) -> String {
    let work = read_record(&record_path(loaded, attempt_id, "attempt").unwrap_or_default())
        .ok()
        .and_then(|a| a["binding"]["selector"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let read = json!({
        "metadata": read_route(loaded, &work, attempt_id, None),
        "stdoutBytes": read_route(loaded, &work, attempt_id, Some("stdout")),
        "stderrBytes": read_route(loaded, &work, attempt_id, Some("stderr")),
    });
    format!(
        "EXITBIND_JSON:{}",
        json!({"effect":"no_change","preReturn":"refused","attemptId":attempt_id,
        "failureKind":result["failureKind"],"exitCode":result["exitCode"],"stdout":result["stdout"],"stderr":result["stderr"],
        "readRoutes":read,"nextActions":["repair within the current assignment and valid grant","report blocked to the Lead if repair is unavailable"],
        "finalCheckCredited":false,"automaticRework":false,"replayed":result["replayed"]==true})
    )
}

fn read_route(loaded: &Loaded, work: &str, attempt_id: &str, stream: Option<&str>) -> Value {
    let mut argv = vec![
        "run".into(),
        "pre-return".into(),
        "read".into(),
        work.into(),
        attempt_id.into(),
    ];
    if let Some(stream) = stream {
        argv.extend(["--stream".into(), stream.into()]);
    }
    argv.push("--config".into());
    let route = crate::work::response_recovery::bounded_argv(argv, loaded.path.to_str(), 2048);
    json!({"argv":route.argv,"sameConfigRequired":route.same_config,"sameExecutableRequired":route.same_executable})
}

fn run_or_recover(
    loaded: &Loaded,
    attempt_id: &str,
    attempt: &Value,
    policy: &crate::run_value::PreReturnPolicy,
    replayed: bool,
) -> Result<Value, String> {
    if replayed {
        return Ok(attempt.clone());
    }
    let request = crate::run::capture::ObservationRequest {
        command: policy.command.clone(),
        working_dir: loaded.product_root.clone(),
        artifact_dir: loaded
            .state_root
            .join(crate::project::layout_types::state_namespace())
            .join("artifacts"),
        state_root: loaded.state_root.clone(),
        deadline: Instant::now()
            .checked_add(Duration::from_millis(policy.timeout_ms))
            .unwrap_or_else(Instant::now),
        timeout_ms: policy.timeout_ms,
        operation_id: attempt_id.to_owned(),
        capture: crate::run::capture::CapturePolicy {
            per_stream_bytes: policy.capture_limit_bytes,
            combined_bytes: policy.capture_limit_bytes.saturating_mul(2),
        },
        retain_partial: true,
    };
    let result_path = record_path(loaded, attempt_id, "result")?;
    let result = match crate::run::capture::capture_observed_command(&request) {
        Ok(capture) if capture.group_ended => {
            let code = capture.status.code();
            if let Err(error) = install_captured_logs(&capture) {
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain",
                    "failureKind":"storage_failure","diagnostic":{"diagnostic":error,"storageStage":"capture_install"},
                    "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                    "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            } else {
                let state = if code == Some(0) { "passed" } else { "failed" };
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":state,
                "failureKind":if code.is_some() { if code==Some(0) { Value::Null } else { json!("nonzero_exit") } } else { json!("terminated") },
                "exitCode":code,"durationMs":capture.duration_ms,"stdout":capture.stdout_artifact,"stderr":capture.stderr_artifact,
                "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            }
        }
        Ok(capture) => {
            if let Err(error) = install_captured_logs(&capture) {
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain",
                    "failureKind":"storage_failure","diagnostic":{"diagnostic":error,"storageStage":"capture_install"},
                    "exitCode":capture.status.code(),"durationMs":capture.duration_ms,
                    "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                    "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            } else {
                json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain","failureKind":"descendants_remain_or_unknown",
            "exitCode":capture.status.code(),"durationMs":capture.duration_ms,"stdout":capture.stdout_artifact,"stderr":capture.stderr_artifact,
            "producer":request_producer(),"bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
            "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
            }
        }
        Err(error) => {
            let facts = error.facts();
            let diagnostic = facts["diagnostic"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase();
            let failure_kind = if facts["deadlineExceeded"] == true {
                "timeout"
            } else if diagnostic.contains("could not be launched") {
                "launch_failure"
            } else if diagnostic.contains("could not be created")
                || diagnostic.contains("artifact")
                || diagnostic.contains("directory")
                || diagnostic.contains("stored")
            {
                "storage_failure"
            } else if facts["capture"] == "incomplete" {
                "capture_failure"
            } else if facts["storageStage"] == "capture" {
                "storage_failure"
            } else {
                "execution_uncertain"
            };
            json!({"version":1,"kind":"pre_return_result","attemptId":attempt_id,"state":"uncertain",
                "failureKind":failure_kind,"diagnostic":facts,"producer":request_producer(),
                "bindingSha256":crate::evidence::hash::value(&attempt["binding"]),
                "policySha256":crate::evidence::hash::value(&attempt["policy"]),"completedAt":super::now()})
        }
    };
    let bytes = record_bytes(&result)?;
    write_durable(&result_path, &bytes).map_err(|e| {
        format!("pre-return result storage failed; attempt remains unresolved: {e}")
    })?;
    if result["state"] != "passed" {
        return Err(failure_response(loaded, attempt_id, &result));
    }
    Ok(result)
}

fn request_producer() -> Value {
    json!({"executableSha256":crate::producer::build_identity()["executableSha256"],"producer":crate::producer::evidence_for_version(8)})
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

pub(super) fn prepare_for_assignment<F: FnOnce() -> Result<String, String>>(
    loaded: &Loaded,
    ledger: &str,
    agent: &str,
    expected: &AssignmentIdentity,
    assignment_handle_sha256: &str,
    artifact: F,
) -> Result<(String, String), String> {
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    let mut artifact = Some(artifact);
    let prepared = with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let (_, events, _) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
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
        let (attempt_id, attempt, replayed) = latest_or_create(loaded, binding.clone(), &policy)?;
        Ok((artifact_path, attempt_id, attempt, replayed, policy))
    })?;
    let (artifact_path, attempt_id, attempt, replayed, policy) = prepared;
    run_or_recover(loaded, &attempt_id, &attempt, &policy, replayed)?;
    Ok((artifact_path, attempt_id))
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
        let (attempt_id, attempt, replayed) = latest_or_create(loaded, binding, &policy)?;
        Ok(Some((attempt_id, attempt, replayed, policy)))
    })?;
    let Some((attempt_id, attempt, replayed, policy)) = prepared else {
        return Ok(None);
    };
    run_or_recover(loaded, &attempt_id, &attempt, &policy, replayed)?;
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
    if let Some(expected) = attempt_id.filter(|expected| *expected != id) {
        let saved = read_record(&record_path(loaded, expected, "attempt")?)?;
        return Err(binding_diagnostic::mismatch(
            loaded, &saved, &binding, state, expected,
        ));
    }
    let result = read_record(&record_path(loaded, &id, "result")?)?;
    let attempt = read_record(&record_path(loaded, &id, "attempt")?)?;
    if attempt["binding"] != binding || attempt["policy"] != policy.value() {
        return Err("pre-return attempt evidence does not match the current result binding".into());
    }
    if result["state"] != "passed" || result["attemptId"] != id {
        return Err(
            "a current passing pre-return attempt is required before completed submission".into(),
        );
    }
    if result["bindingSha256"] != crate::evidence::hash::value(&binding)
        || result["policySha256"] != crate::evidence::hash::value(&policy.value())
        || result["producer"]["executableSha256"]
            != crate::producer::build_identity()["executableSha256"]
    {
        return Err(
            "pre-return result metadata does not match the current binding or producer".into(),
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
    for stream in ["stdout", "stderr"] {
        let reference = result
            .get(stream)
            .ok_or("passing pre-return result has an incomplete capture")?;
        crate::run::artifact::read_exact(
            loaded,
            reference,
            "pre-return capture",
            crate::run_value::PRE_RETURN_CAPTURE_MAX_BYTES,
        )?;
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
    let dir = directory_path(loaded);
    if !dir.exists() {
        return Ok(json!({"enabled":true,"policy":policy,"attempt":null}));
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("pre-return journal filename is not valid UTF-8")?;
        if !name.starts_with("pre-return-") || !name.ends_with(".attempt.json") {
            continue;
        }
        let attempt = read_record(&entry.path())?;
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
    let attempt = read_record(&record_path(loaded, attempt_id, "attempt")?)?;
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
    let directory = directory_path(loaded);
    if !directory.exists() {
        return Err("no retained pre-return diagnostic exists for this blocked report".into());
    }
    let mut matching = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("pre-return journal filename is not valid UTF-8")?;
        if !name.starts_with("pre-return-") || !name.ends_with(".attempt.json") {
            continue;
        }
        let attempt = read_record(&entry.path())?;
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

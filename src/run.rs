//! Run orchestration for bounded agent workflows.
//!
//! Ledger storage and locking live in `run_ledger`; event validation and state
//! transitions live in `run_state`. This module coordinates those boundaries
//! and checks live configuration/artifact evidence before mutation.

use crate::{
    config::{self, Loaded},
    envelope, hash, run_error, run_ledger, run_state,
};
pub use run_error::DriftError;
use run_ledger::{
    append, claim_path, ledger_path, load, load_at, obtain_claim, predecessor, rollback_claim,
    with_lock,
};
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_OBSERVE_TIMEOUT_MS: u64 = 1_800_000;
const OBSERVE_POLL_MS: u64 = 10;
const OBSERVE_TERMINATION_GRACE_MS: u64 = 250;

pub fn start(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    ledger: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
) -> Result<Value, String> {
    start_with_policy(
        loaded,
        workflow,
        goal,
        ledger,
        boundary,
        harness_receipt,
        None,
        None,
        None,
        None,
        None,
    )
}

/// Start a run with the optional, caller-reported deterministic check policy.
/// Supplying no policy preserves the v1/v2 start API and persisted shape.
#[allow(clippy::too_many_arguments)]
pub fn start_with_policy(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    ledger: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
    check_command: Option<&str>,
    proof_origin: Option<&str>,
    preserve_requirement: Option<&str>,
    preservation_check_command: Option<&str>,
    preservation_proof_origin: Option<&str>,
) -> Result<Value, String> {
    if workflow.trim().is_empty() {
        return Err("workflow is required".into());
    }
    if goal.trim().is_empty() {
        return Err("--goal requires a non-empty value".into());
    }
    let plan = crate::boundary_manifest::apply(
        loaded,
        selected_plan(envelope::plan(loaded, workflow, goal)?)?,
        boundary,
    )?;
    let check_policy = crate::run_value::policy_from_cli(check_command, proof_origin)?;
    let preservation = crate::run_value::preservation_from_cli(
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
    )?;
    if check_policy.is_some() && !has_worker_stage(&plan) {
        return Err("checked run requires a workflow with at least one worker stage".into());
    }
    if preservation.is_some() && !has_worker_stage(&plan) {
        return Err("preservation requires a workflow with at least one worker stage".into());
    }
    if preservation.is_some() && check_policy.is_none() {
        return Err("preservation requirements require a checked run with --check-command".into());
    }
    if preservation.is_some() && !crate::producer::exitbind_surface() {
        return Err("preservation requirements require Exitbind v6 runs".into());
    }
    if let Some(receipt) = harness_receipt {
        crate::receipt::for_run(loaded, receipt, &plan)?;
    }
    let timestamp = now();
    let config_sha = hash::text(&loaded.source);
    let run_id = hash::value(&json!({
        "workflow": workflow,
        "configSha256": config_sha,
        "goal": goal,
        "timestamp": timestamp
    }));
    let path = ledger_path(&loaded.state_root, ledger, true)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let event = with_lock(&path, || {
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let reference = if let Some(receipt) = harness_receipt {
            if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source
            {
                return Err("configuration changed while starting; reload configuration".into());
            }
            Some(crate::receipt::for_run(loaded, receipt, &plan)?)
        } else {
            None
        };
        let version = if crate::producer::exitbind_surface() {
            6
        } else if check_policy.is_some() {
            4
        } else if reference.is_some() {
            2
        } else {
            1
        };
        let mut value = json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "start",
            "runId": run_id,
            "workflow": workflow,
            "goal": goal,
            "configSha256": config_sha,
            "plan": plan,
            "previousEventSha256": null,
            "timestamp": timestamp
        });
        if let Some(reference) = reference {
            value["harnessReceipt"] = reference;
        }
        if let Some(policy) = &check_policy {
            value["checkPolicy"] = policy.value();
        }
        if let Some(preservation) = &preservation {
            value["preservation"] = preservation.value();
        }
        if version >= 5 {
            value["subject"] = subject(goal, &value["plan"], &config_sha, &run_id);
        }
        let event = run_state::make_event(value);
        append(&path, &event, true, "")?;
        Ok(event)
    })?;
    result(&[event])
}

pub fn next(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    assert_no_drift(loaded, &state)?;
    crate::run_artifact::assert_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    Ok(json!({
        "valid": true,
        "runId": state["runId"],
        "status": state["status"],
        "workflow": state["workflow"],
        "currentStage": if state["status"] == "running" { state["currentStage"].clone() } else { Value::Null },
        "attempt": if state["status"] == "running" { state["attempt"].clone() } else { Value::Null },
        "assignments": crate::run_assignment::pending(&state)
        ,"progress": crate::run_progress::project(&state)
    }))
}

pub fn submit(
    loaded: &Loaded,
    agent: &str,
    ledger: &str,
    outcome: &str,
    artifact: &str,
    artifact_root: Option<&str>,
) -> Result<Value, String> {
    if agent.trim().is_empty() {
        return Err("agent is required".into());
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let artifact = artifact.to_owned();
    submit_locked(loaded, &path, agent, outcome, None, artifact_root, || {
        Ok(artifact)
    })
}

/// The exact assignment selected by the work façade before it reads the
/// agent's result.  It is kept private to the crate so raw protocol identity
/// never becomes part of the façade surface.
#[derive(Debug, Clone)]
pub(crate) struct AssignmentIdentity {
    pub(crate) stage: u64,
    pub(crate) attempt: u64,
    pub(crate) agent: String,
    pub(crate) role: String,
}

impl AssignmentIdentity {
    pub(crate) fn from_action(action: &Value) -> Result<Self, String> {
        let packet = action
            .get("packet")
            .ok_or("current work action has no assignment packet")?;
        Ok(Self {
            stage: packet["stage"]
                .as_u64()
                .ok_or("current work action has invalid stage")?,
            attempt: packet["attempt"]
                .as_u64()
                .ok_or("current work action has invalid attempt")?,
            agent: packet["agent"]
                .as_str()
                .ok_or("current work action has no agent")?
                .to_owned(),
            role: packet["role"]
                .as_str()
                .ok_or("current work action has no role")?
                .to_owned(),
        })
    }

    fn matches(&self, assignment: &Value) -> bool {
        assignment["stage"].as_u64() == Some(self.stage)
            && assignment["attempt"].as_u64() == Some(self.attempt)
            && assignment["agent"].as_str() == Some(&self.agent)
            && assignment["role"].as_str() == Some(&self.role)
    }
}

/// Submit a work result after revalidating the façade's exact assignment
/// under the ledger lock.  The artifact is produced only after that check.
pub(crate) fn submit_for_assignment<F>(
    loaded: &Loaded,
    ledger: &str,
    expected: AssignmentIdentity,
    outcome: &str,
    artifact: F,
) -> Result<Value, String>
where
    F: FnOnce() -> Result<String, String>,
{
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let agent = expected.agent.clone();
    submit_locked(
        loaded,
        &path,
        &agent,
        outcome,
        Some(&expected),
        Some("state"),
        artifact,
    )
}

fn submit_locked<F>(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    agent: &str,
    outcome: &str,
    expected: Option<&AssignmentIdentity>,
    artifact_root: Option<&str>,
    artifact: F,
) -> Result<Value, String>
where
    F: FnOnce() -> Result<String, String>,
{
    let targets = [path.path.as_path(), path.lock.as_path()];
    with_lock(path, || {
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        crate::run_artifact::assert_current(loaded, &state)?;
        let assignment = crate::run_assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == agent)
            .ok_or_else(|| format!("agent '{agent}' is not currently pending"))?;
        if let Some(expected) = expected {
            if !expected.matches(&assignment) {
                return Err(
                    "assignment changed while work result was being read; no mutation was made"
                        .into(),
                );
            }
        }
        let artifact = artifact()?;
        let artifact_value = crate::run_artifact::evidence(loaded, artifact_root, &artifact)?;
        let last = events.last().ok_or("run ledger has no head event")?;
        // Preserve the ledger's historical event version. Optional fields
        // must not silently upgrade a v3 checked run on its next submission.
        let version = state["version"]
            .as_u64()
            .ok_or("run state is missing version")?;
        if assignment["role"] == "lead" && outcome == "accepted" {
            let assessment = crate::run_exit::reduce(&state)?.assessment;
            if assessment.is_blocked() {
                if assessment.has_refusal_evidence() {
                    let timestamp = nondecreasing(&last["timestamp"])?;
                    let protection = crate::run_value::protection_event(
                        &state,
                        agent,
                        &assignment["stage"],
                        &assignment["attempt"],
                        last,
                        &timestamp,
                    )?;
                    let protection = run_state::make_event(protection);
                    let mut protected = events.clone();
                    protected.push(protection.clone());
                    run_state::reduce(&protected)?;
                    append(path, &protection, false, &source)?;
                    return Err(format!(
                        "acceptance refused: configured check evidence is {}",
                        assessment.reason().unwrap_or("blocked")
                    ));
                }
                return Err(
                    "acceptance refused: checked worker completion or result is not observed"
                        .into(),
                );
            }
        }
        let mut event_value = json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "submit",
            "runId": state["runId"],
            "stage": assignment["stage"],
            "attempt": assignment["attempt"],
            "agent": agent,
            "role": assignment["role"],
            "outcome": outcome,
            "artifact": artifact_value,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        });
        if version >= 6
            && matches!(
                (assignment["role"].as_str(), outcome),
                (Some("reviewer"), "approved") | (Some("lead"), "accepted")
            )
        {
            event_value["inputsSha256"] = live_inputs(&state)?;
        }
        if version >= 5 {
            event_value["subjectSha256"] =
                if assignment["role"] == "worker" && outcome == "completed" {
                    crate::run_state::subject_for_submission(&state, &assignment, &artifact_value)
                        ["sha256"]
                        .clone()
                } else {
                    state["subject"]["sha256"].clone()
                };
        }
        let event = run_state::make_event(event_value);
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(path, &event, false, &source)?;
        Ok(json!({
            "valid": true,
            "event": event,
            "status": next_state["status"],
            "currentStage": if next_state["status"] == "running" { next_state["currentStage"].clone() } else { Value::Null },
            "attempt": if next_state["status"] == "running" { next_state["attempt"].clone() } else { Value::Null },
            "assignments": crate::run_assignment::pending(&next_state)
        }))
    })
}

/// Record a caller-supplied deterministic check result for the exact current
/// worker completion.  This function never executes `check_command`.
pub fn record_check(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    check_command: &str,
    exit_code: &str,
    duration_ms: Option<&str>,
) -> Result<Value, String> {
    record_check_for_requirement(
        loaded,
        ledger,
        target,
        None,
        check_command,
        exit_code,
        duration_ms,
    )
}

/// Record a caller-supplied result for either the normal deterministic check or
/// a named preservation requirement on the exact current worker completion.
pub fn record_check_for_requirement(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    requirement_id: Option<&str>,
    check_command: &str,
    exit_code: &str,
    duration_ms: Option<&str>,
) -> Result<Value, String> {
    if check_command.trim().is_empty() {
        return Err("--check-command requires a non-empty value".into());
    }
    if check_command.contains('\0') {
        return Err("--check-command must not contain NUL bytes".into());
    }
    let exit_code = parse_nonnegative("--exit-code", exit_code)?;
    let duration_ms = duration_ms
        .map(|value| parse_nonnegative("--duration-ms", value))
        .transpose()?;
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        if state["status"] != "running" {
            return Err("run has already reached a terminal state; no mutation was made".into());
        }
        assert_no_drift(loaded, &state)?;
        crate::run_artifact::assert_current(loaded, &state)?;
        let policy = active_check_policy(&state, requirement_id)?;
        if policy.command != check_command
            || policy.command_sha256 != crate::hash::text(check_command)
        {
            return Err("check command does not match the configured run policy".into());
        }
        crate::run_value::validate_check_target(&state, target)?;
        let last = events.last().ok_or("run ledger has no event head")?;
        let version = state["version"].as_u64().unwrap_or(3);
        let mut value = if version >= 4 {
            json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "check",
            "runId": state["runId"],
            "targetEventSha256": target,
            "checkCommand": check_command,
            "checkCommandSha256": crate::hash::text(check_command),
            "origin": policy.origin.as_str(),
            "acquisition": "reported",
            "result": {"kind": "exit", "code": exit_code},
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
            })
        } else {
            json!({
                "version": 3,
                "kind": "run",
                "producer": crate::producer::evidence_for_version(3),
                "action": "check",
                "runId": state["runId"],
                "targetEventSha256": target,
                "checkCommand": check_command,
                "checkCommandSha256": crate::hash::text(check_command),
                "origin": policy.origin.as_str(),
                "exitCode": exit_code,
                "previousEventSha256": last["eventSha256"],
                "timestamp": nondecreasing(&last["timestamp"])?
            })
        };
        if version >= 5 {
            value["subjectSha256"] = state["subject"]["sha256"].clone();
        }
        if version >= 6 {
            value["inputsSha256"] = live_inputs(&state)?;
            if let Some(id) = requirement_id {
                value["requirementId"] = json!(id);
            }
        }
        value["durationMs"] = duration_ms.map_or(Value::Null, |duration| json!(duration));
        let event = run_state::make_event(value);
        let mut all = events.clone();
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({
            "valid": true,
            "event": event,
            "runId": next_state["runId"],
            "status": next_state["status"],
            "checks": crate::run_value::status(&next_state, Some(true))?["checks"]
        }))
    })
}

/// Execute the frozen policy command and bind its locally observed result to
/// the exact current worker completion.  No caller-supplied command/result
/// fields are accepted.
pub fn observe_check(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    timeout_ms: Option<&str>,
) -> Result<Value, String> {
    observe_check_for_requirement(loaded, ledger, target, None, timeout_ms)
}

pub fn observe_check_for_requirement(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    requirement_id: Option<&str>,
    timeout_ms: Option<&str>,
) -> Result<Value, String> {
    let timeout_ms = timeout_ms
        .map(|value| parse_positive("--timeout-ms", value))
        .transpose()?
        .unwrap_or(DEFAULT_OBSERVE_TIMEOUT_MS);
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let (source, policy, inputs) = with_lock(&path, || {
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        if state["version"].as_u64().unwrap_or(0) < 4 {
            return Err("observe-check requires a newly created checked run".into());
        }
        if state["status"] != "running" {
            return Err("run has already reached a terminal state; no mutation was made".into());
        }
        assert_no_drift(loaded, &state)?;
        crate::run_artifact::assert_current(loaded, &state)?;
        predecessor(loaded, &events[0])?;
        let policy = active_check_policy(&state, requirement_id)?;
        crate::run_value::validate_check_target(&state, target)?;
        let inputs = if state["version"].as_u64() >= Some(6) {
            Some(live_inputs(&state)?)
        } else {
            None
        };
        Ok((source, policy, inputs))
    })?;

    let (status, duration_ms) = run_observed_command(loaded, &policy.command, timeout_ms)?;
    let result = observed_result(&status)?;

    with_lock(&path, || {
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let (_, current_events, current_source) = load_at(loaded, &path)?;
        let current_state = reduce_live(loaded, &current_events)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        if current_source != source {
            return Err("run ledger changed while observing; no mutation was made".into());
        }
        if current_state["version"].as_u64().unwrap_or(0) < 4
            || current_state["status"] != "running"
        {
            return Err("run changed while observing; no mutation was made".into());
        }
        assert_no_drift(loaded, &current_state)?;
        crate::run_artifact::assert_current(loaded, &current_state)?;
        predecessor(loaded, &current_events[0])?;
        let current_policy = active_check_policy(&current_state, requirement_id)?;
        if current_policy != policy {
            return Err("check policy changed while observing; no mutation was made".into());
        }
        crate::run_value::validate_check_target(&current_state, target)?;
        let last = current_events
            .last()
            .ok_or("run ledger has no event head")?;
        let mut event_value = json!({
            "version": current_state["version"],
            "kind": "run",
            "producer": crate::producer::evidence_for_version(current_state["version"].as_u64().unwrap_or(4)),
            "action": "check",
            "runId": current_state["runId"],
            "targetEventSha256": target,
            "checkCommand": policy.command,
            "checkCommandSha256": policy.command_sha256,
            "origin": policy.origin.as_str(),
            "acquisition": "observed",
            "result": result,
            "durationMs": duration_ms,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        });
        if current_state["version"].as_u64() >= Some(5) {
            event_value["subjectSha256"] = current_state["subject"]["sha256"].clone();
        }
        if let Some(inputs) = &inputs {
            if current_state.get("inputsSha256") != Some(inputs) {
                return Err(
                    "tested inputs changed while the check ran; no evidence was recorded".into(),
                );
            }
            event_value["inputsSha256"] = inputs.clone();
            if let Some(id) = requirement_id {
                event_value["requirementId"] = json!(id);
            }
        }
        let event = run_state::make_event(event_value);
        let mut all = current_events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &current_source)?;
        Ok(json!({
            "valid": true,
            "event": event,
            "runId": next_state["runId"],
            "status": next_state["status"],
            "checks": crate::run_value::status(&next_state, Some(true))?["checks"]
        }))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveCheckPolicy {
    command: String,
    command_sha256: String,
    origin: crate::run_value::ProofOrigin,
}

fn active_check_policy(
    state: &Value,
    requirement_id: Option<&str>,
) -> Result<ActiveCheckPolicy, String> {
    if let Some(id) = requirement_id {
        if state["version"].as_u64() < Some(6) {
            return Err("preservation checks require an Exitbind v6 run".into());
        }
        let preservation = state
            .get("preservation")
            .ok_or("run has no configured preservation policy")?;
        let preservation = crate::run_value::preservation_from_value(preservation, 0)
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        let requirement = preservation
            .requirement(id)
            .ok_or("preservation requirement is not configured")?;
        Ok(ActiveCheckPolicy {
            command: requirement.command.clone(),
            command_sha256: requirement.command_sha256.clone(),
            origin: requirement.origin,
        })
    } else {
        let policy = state
            .get("checkPolicy")
            .ok_or("run has no configured check policy")?;
        let policy = crate::run_value::policy_from_value(policy, 0)
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        Ok(ActiveCheckPolicy {
            command: policy.command,
            command_sha256: policy.command_sha256,
            origin: policy.origin,
        })
    }
}

fn run_observed_command(
    loaded: &Loaded,
    command_text: &str,
    timeout_ms: u64,
) -> Result<(ExitStatus, u64), String> {
    #[cfg(not(unix))]
    {
        let _ = (loaded, command_text, timeout_ms);
        return Err("observe-check requires a POSIX host".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let child_stderr = child_stderr_stdio()?;
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(command_text)
            .arg("soulmate-observe-check")
            .current_dir(&loaded.product_root)
            .stdout(child_stderr)
            .stderr(Stdio::inherit())
            .process_group(0);
        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|error| format!("check command could not be launched: {error}"))?;
        let timeout = Duration::from_millis(timeout_ms);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok((status, elapsed_ms(started))),
                Ok(None) if started.elapsed() >= timeout => {
                    let cleanup = terminate_process_group(&mut child);
                    return Err(match cleanup {
                        Ok(()) => format!(
                            "check observation timed out after {timeout_ms} ms; no mutation was made"
                        ),
                        Err(cleanup) => format!(
                            "check observation timed out after {timeout_ms} ms; cleanup failed: {cleanup}; no mutation was made"
                        ),
                    });
                }
                Ok(None) => {
                    let remaining = timeout.saturating_sub(started.elapsed());
                    thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
                }
                Err(error) => {
                    let cleanup = terminate_process_group(&mut child);
                    return Err(match cleanup {
                        Ok(()) => format!(
                            "check command could not be observed: {error}; no mutation was made"
                        ),
                        Err(cleanup) => format!(
                            "check command could not be observed: {error}; cleanup failed: {cleanup}; no mutation was made"
                        ),
                    });
                }
            }
        }
    }
}

#[cfg(unix)]
fn terminate_process_group(child: &mut Child) -> Result<(), String> {
    let process_group = match i32::try_from(child.id()) {
        Ok(process_group) => Some(process_group),
        Err(_) => None,
    };
    let mut failures = Vec::new();
    if process_group.is_none() {
        failures.push("check process identifier is outside the POSIX range".to_owned());
    }
    if let Some(process_group) = process_group {
        if let Err(error) = signal_process_group(process_group, libc::SIGTERM) {
            failures.push(format!("TERM cleanup failed: {error}"));
        }
    }

    let cleanup_started = Instant::now();
    let grace_started = Instant::now();
    let grace = Duration::from_millis(OBSERVE_TERMINATION_GRACE_MS);
    let cleanup_deadline = cleanup_started + grace.saturating_add(grace);
    let mut group_present = process_group.is_some();
    let mut leader_reaped = false;
    while grace_started.elapsed() < grace && Instant::now() < cleanup_deadline {
        match child.try_wait() {
            Ok(Some(_)) => leader_reaped = true,
            Ok(None) => {}
            Err(error) => failures.push(format!("check process could not be inspected: {error}")),
        }
        if let Some(process_group) = process_group {
            match process_group_exists(process_group) {
                Ok(false) => {
                    group_present = false;
                    break;
                }
                Ok(true) => {}
                Err(error) => failures.push(error),
            }
        }
        let remaining = grace.saturating_sub(grace_started.elapsed());
        if !remaining.is_zero() {
            thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
        }
    }
    if group_present {
        if let Some(process_group) = process_group {
            if let Err(error) = signal_process_group(process_group, libc::SIGKILL) {
                failures.push(format!("KILL cleanup failed: {error}"));
            }
        }
    }

    while !leader_reaped && Instant::now() < cleanup_deadline {
        match child.try_wait() {
            Ok(Some(_)) => leader_reaped = true,
            Ok(None) => {}
            Err(error) => {
                failures.push(format!("check process could not be reaped: {error}"));
                break;
            }
        }
        let remaining = cleanup_deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
        }
    }
    if !leader_reaped {
        failures.push("check process could not be reaped before cleanup deadline".to_owned());
    }

    while group_present && Instant::now() < cleanup_deadline {
        if let Some(process_group) = process_group {
            match process_group_exists(process_group) {
                Ok(false) => group_present = false,
                Ok(true) => {}
                Err(error) => failures.push(error),
            }
        } else {
            break;
        }
        if group_present {
            let remaining = cleanup_deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
            }
        }
    }
    if group_present {
        failures.push("check process group remained after forced termination".into());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        failures.sort();
        failures.dedup();
        Err(failures.join("; "))
    }
}

#[cfg(unix)]
fn signal_process_group(process_group: i32, signal: i32) -> Result<(), String> {
    if unsafe { libc::kill(-process_group, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!(
            "check process group could not be terminated: {error}"
        ))
    }
}

#[cfg(unix)]
fn process_group_exists(process_group: i32) -> Result<bool, String> {
    if unsafe { libc::kill(-process_group, 0) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(format!(
            "check process group could not be inspected: {error}"
        )),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        process_group_exists, terminate_process_group, Duration, Instant,
        OBSERVE_TERMINATION_GRACE_MS,
    };
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::{fs, thread};

    #[test]
    fn cleanup_reports_reap_failure_after_still_killing_the_group() {
        let marker =
            std::env::temp_dir().join(format!("soulmate-cleanup-{}.ready", std::process::id()));
        let _ = fs::remove_file(&marker);
        let command = format!(
            "sh -c 'trap : TERM HUP; echo ready > {}; while :; do :; done' & exit 0",
            marker.display()
        );
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .process_group(0)
            .spawn()
            .expect("cleanup fixture should launch");
        let child_pid = i32::try_from(child.id()).expect("fixture pid should fit POSIX");
        let ready_started = Instant::now();
        while !marker.exists() && ready_started.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(1));
        }
        let waited = unsafe { libc::waitpid(child_pid, std::ptr::null_mut(), 0) };
        let group_before = process_group_exists(child_pid).unwrap_or(false);

        let started = Instant::now();
        let failure = terminate_process_group(&mut child).expect_err("external reap is an error");
        let group_after = process_group_exists(child_pid);
        let ready = marker.exists();
        let _ = fs::remove_file(marker);
        assert_eq!(waited, child_pid);
        assert!(ready, "cleanup fixture did not become ready");
        assert!(
            group_before,
            "cleanup fixture process group was not present"
        );
        assert!(
            matches!(group_after, Ok(false)),
            "cleanup left the process group behind: {group_after:?}"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(OBSERVE_TERMINATION_GRACE_MS),
            "cleanup skipped its bounded TERM grace"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cleanup exceeded its bounded budget"
        );
        assert!(failure.contains("could not be reaped"), "{failure}");
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn observed_result(status: &std::process::ExitStatus) -> Result<Value, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Ok(json!({"kind": "signal", "signal": signal}));
        }
    }
    status
        .code()
        .map(|code| json!({"kind": "exit", "code": code as u64}))
        .ok_or_else(|| "check command ended indeterminately; no mutation was made".into())
}

fn child_stderr_stdio() -> Result<Stdio, String> {
    #[cfg(unix)]
    {
        use std::os::unix::io::{FromRawFd, RawFd};
        let fd: RawFd = unsafe { libc::dup(libc::STDERR_FILENO) };
        if fd < 0 {
            return Err(format!(
                "check command stderr could not be connected: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(unsafe { Stdio::from(std::fs::File::from_raw_fd(fd)) })
    }
    #[cfg(not(unix))]
    {
        Err("observe-check requires a POSIX host".into())
    }
}

fn parse_nonnegative(option: &str, value: &str) -> Result<u64, String> {
    if value.trim().is_empty() || value.starts_with('+') || value.starts_with('-') {
        return Err(format!("{option} must be a non-negative integer"));
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{option} must be a non-negative integer"))
}

fn parse_positive(option: &str, value: &str) -> Result<u64, String> {
    let parsed = parse_nonnegative(option, value)
        .map_err(|_| format!("{option} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{option} must be a positive integer"));
    }
    Ok(parsed)
}

pub fn inspect(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    predecessor(loaded, &events[0])?;
    let mut result = json!({
        "valid": true,
        "runId": state["runId"],
        "workflow": state["workflow"],
        "status": state["status"],
        "currentStage": if state["status"] == "running" { state["currentStage"].clone() } else { Value::Null },
        "attempt": if state["status"] == "running" { state["attempt"].clone() } else { Value::Null },
        "events": events,
        "submissions": state["submissions"]
    });
    if state.get("checkPolicy").is_some() {
        result["assignments"] = state["assignments"].clone();
    }
    if state["version"].as_u64() >= Some(6) {
        result["subject"] = state["subject"].clone();
        result["inputsSha256"] = state.get("inputsSha256").cloned().unwrap_or(Value::Null);
    }
    Ok(result)
}

/// Read-only current-state view with artifact/config revalidation.
pub fn status(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    assert_no_drift(loaded, &state)?;
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_value::status(&state, Some(artifact_current))
}

/// Read-only explanation for a run or one of its factual protection events.
pub fn explain(loaded: &Loaded, ledger: &str, event_id: Option<&str>) -> Result<Value, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    assert_no_drift(loaded, &state)?;
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_value::explain_with_artifact(&state, event_id, artifact_current)
}

/// Build the typed read-only projection used by the default human status view.
pub(crate) fn human_status(
    loaded: &Loaded,
    ledger: &str,
) -> Result<crate::run_human::HumanStatus, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    assert_no_drift(loaded, &state)?;
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_human::human_status(&state, artifact_current)
}

/// Build the typed read-only projection used by the default human explanation view.
pub(crate) fn human_explain(
    loaded: &Loaded,
    ledger: &str,
    event_id: Option<&str>,
) -> Result<crate::run_human::HumanExplanation, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    assert_no_drift(loaded, &state)?;
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_human::human_explain(&state, event_id, artifact_current)
}

/// Generate a local redacted report from explicitly selected ledgers.  The
/// report only contains hashes, counts, bounded statuses, and observed command
/// durations; it never returns goals, commands, prompts, profiles, paths, or
/// artifact bytes.
pub fn report(loaded: &Loaded, ledgers: &[&str]) -> Result<Value, String> {
    let mut states = Vec::with_capacity(ledgers.len());
    for ledger in ledgers {
        let (_, events, _) = load(loaded, ledger)?;
        let state = reduce_live(loaded, &events)?;
        predecessor(loaded, &events[0])?;
        states.push(state);
    }
    crate::run_value::aggregate(&states)
}

pub fn report_markdown(report: &Value) -> String {
    crate::run_value::markdown(report)
}

/// Start a fresh bounded run while atomically claiming one running or blocked predecessor.
pub fn supersede(
    loaded: &Loaded,
    old_ledger: &str,
    workflow: &str,
    goal: &str,
    new_ledger: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
) -> Result<Value, String> {
    supersede_with_policy(
        loaded,
        old_ledger,
        workflow,
        goal,
        new_ledger,
        boundary,
        harness_receipt,
        None,
        None,
        None,
        None,
        None,
    )
}

/// Supersede a predecessor while carrying its checked policy forward by
/// default.  Explicit policy arguments are available to the checked CLI
/// surface, but an omitted policy cannot silently downgrade a checked run.
#[allow(clippy::too_many_arguments)]
pub fn supersede_with_policy(
    loaded: &Loaded,
    old_ledger: &str,
    workflow: &str,
    goal: &str,
    new_ledger: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
    check_command: Option<&str>,
    proof_origin: Option<&str>,
    preserve_requirement: Option<&str>,
    preservation_check_command: Option<&str>,
    preservation_proof_origin: Option<&str>,
) -> Result<Value, String> {
    if workflow.trim().is_empty() {
        return Err("workflow is required".into());
    }
    if goal.trim().is_empty() {
        return Err("--goal requires a non-empty value".into());
    }
    if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source {
        return Err("configuration changed while superseding; reload configuration".into());
    }
    let old = ledger_path(&loaded.state_root, old_ledger, false)?;
    let new = ledger_path(&loaded.state_root, new_ledger, true)?;
    let targets = [
        old.path.as_path(),
        old.lock.as_path(),
        new.path.as_path(),
        new.lock.as_path(),
    ];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&old, || {
        crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let (_, old_events, old_source) = load_at(loaded, &old)?;
        let old_state = run_state::reduce(&old_events)?;
        if !matches!(old_state["status"].as_str(), Some("running" | "blocked")) {
            return Err("only a running or blocked run can be superseded".into());
        }
        if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source {
            return Err("configuration changed while superseding; reload configuration".into());
        }
        let plan = crate::boundary_manifest::apply(
            loaded,
            selected_plan(envelope::plan(loaded, workflow, goal)?)?,
            boundary,
        )?;
        let old_policy = old_state
            .get("checkPolicy")
            .map(|policy| crate::run_value::policy_from_value(policy, 0))
            .transpose()
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        let requested_policy = crate::run_value::policy_from_cli(check_command, proof_origin)?;
        let old_preservation = old_state
            .get("preservation")
            .map(|value| crate::run_value::preservation_from_value(value, 0))
            .transpose()
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        let requested_preservation = crate::run_value::preservation_from_cli(
            preserve_requirement,
            preservation_check_command,
            preservation_proof_origin,
        )?;
        let check_policy: Option<crate::run_value::CheckPolicy> = match (
            old_policy,
            requested_policy,
        ) {
            (Some(old), None) => Some(old),
            (None, requested) => requested,
            (Some(old), Some(requested)) => {
                if old != requested {
                    return Err(
                        "successor check policy differs from predecessor; choose an explicit new checked run"
                            .into(),
                    );
                }
                Some(old)
            }
        };
        let preservation = match (old_preservation, requested_preservation) {
            (Some(old), None) => Some(old),
            (None, requested) => requested,
            (Some(old), Some(requested)) => {
                if old != requested {
                    return Err(
                        "successor preservation policy differs from predecessor; choose an explicit new checked run"
                            .into(),
                    );
                }
                Some(old)
            }
        };
        if check_policy.is_some() && !has_worker_stage(&plan) {
            return Err(
                "checked successor requires a workflow with at least one worker stage".into(),
            );
        }
        if preservation.is_some() && check_policy.is_none() {
            return Err(
                "preservation successor requires a checked run with --check-command".into(),
            );
        }
        if preservation.is_some() && !has_worker_stage(&plan) {
            return Err(
                "preservation successor requires a workflow with at least one worker stage".into(),
            );
        }
        if preservation.is_some() && !crate::producer::exitbind_surface() {
            return Err("preservation requirements require Exitbind v6 runs".into());
        }
        let old_rel = config::rel(&loaded.state_root, &old.expected)?;
        let old_sha = hash::text(&old_source);
        let head = old_events
            .last()
            .and_then(|event| event["eventSha256"].as_str())
            .ok_or("old ledger has no event head")?
            .to_owned();
        let config_sha = hash::text(&loaded.source);
        let timestamp = now();
        let run_id = hash::value(&json!({
            "workflow": workflow,
            "configSha256": config_sha,
            "goal": goal,
            "timestamp": timestamp
        }));
        let harness_reference = harness_receipt
            .map(|receipt| crate::receipt::for_run(loaded, receipt, &plan))
            .transpose()?;
        let wanted_claim = json!({
            "version": 1,
            "oldLedgerPath": old_rel,
            "oldLedgerSha256": old_sha,
            "oldRunId": old_state["runId"],
            "oldHeadEventSha256": head,
            "oldConfigSha256": old_state["configSha256"],
            "newLedgerPath": config::rel(&loaded.state_root, &new.expected)?,
            "workflow": workflow,
            "goalSha256": hash::text(goal),
            "configSha256": config_sha,
            "newRunId": run_id,
            "timestamp": timestamp
        });
        let claim_path = claim_path(&old);
        if new.path.exists() && !claim_path.exists() {
            return Err("successor ledger exists without a matching predecessor claim".into());
        }
        let claim = obtain_claim(&claim_path, &wanted_claim)?;
        let supersedes = json!({
            "ledgerPath": claim.value["oldLedgerPath"],
            "ledgerSha256": claim.value["oldLedgerSha256"],
            "runId": claim.value["oldRunId"],
            "headEventSha256": claim.value["oldHeadEventSha256"],
            "configSha256": claim.value["oldConfigSha256"]
        });
        let version = if crate::producer::exitbind_surface() {
            6
        } else if check_policy.is_some() {
            4
        } else if harness_reference.is_some() {
            2
        } else {
            1
        };
        let mut event_value = json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "start",
            "runId": claim.value["newRunId"],
            "workflow": workflow,
            "goal": goal,
            "configSha256": config_sha,
            "plan": plan,
            "previousEventSha256": null,
            "timestamp": claim.value["timestamp"],
            "supersedes": supersedes
        });
        if let Some(reference) = harness_reference {
            event_value["harnessReceipt"] = reference;
        }
        if let Some(policy) = &check_policy {
            event_value["checkPolicy"] = policy.value();
        }
        if let Some(preservation) = &preservation {
            event_value["preservation"] = preservation.value();
        }
        if version >= 5 {
            event_value["subject"] = subject(goal, &event_value["plan"], &config_sha, &run_id);
        }
        let event = run_state::make_event(event_value);
        let successor = if new.path.exists() {
            match load_at(loaded, &new) {
                Ok((_, existing, _)) if existing.first() == Some(&event) => Ok(()),
                Ok(_) => Err("successor ledger already exists with different provenance".into()),
                Err(error) => Err(error),
            }
        } else {
            append(&new, &event, true, "")
        };
        if let Err(error) = successor {
            rollback_claim(&claim_path, &claim);
            return Err(error);
        }
        result(&[event])
    })
}

/// Reduce a ledger and, for v6 runs, attach the live tested-input identity so
/// evidence is only current when it was taken on the files present now.
pub(crate) fn reduce_live(loaded: &Loaded, events: &[Value]) -> Result<Value, String> {
    let mut state = run_state::reduce(events)?;
    if state["version"].as_u64() >= Some(6) && state["status"] == "running" {
        if let Ok(inputs) = crate::run_inputs::fingerprint(loaded) {
            state["inputsSha256"] = json!(inputs);
        }
    }
    Ok(state)
}

fn live_inputs(state: &Value) -> Result<Value, String> {
    state
        .get("inputsSha256")
        .filter(|value| value.is_string())
        .cloned()
        .ok_or_else(|| "tested inputs cannot be established; no evidence was recorded".into())
}

fn result(events: &[Value]) -> Result<Value, String> {
    let state = run_state::reduce(events)?;
    Ok(json!({
        "valid": true,
        "runId": state["runId"],
        "status": state["status"],
        "currentStage": state["currentStage"],
        "attempt": state["attempt"],
        "assignments": crate::run_assignment::pending(&state)
        ,"progress": crate::run_progress::project(&state)
    }))
}

fn subject(goal: &str, plan: &Value, config_sha: &str, run_id: &str) -> Value {
    let goal_sha = hash::text(goal);
    let plan_sha = hash::value(plan);
    let basis = json!({
        "version": 1,
        "runId": run_id,
        "goalSha256": goal_sha,
        "planSha256": plan_sha,
        "configSha256": config_sha,
        "attempt": 0,
        "previousSubjectSha256": Value::Null,
        "workerArtifactSha256": Value::Null,
        "transitionSha256": Value::Null,
    });
    let sha = hash::value(&basis);
    let mut value = basis;
    value["sha256"] = json!(sha);
    value
}

fn has_worker_stage(plan: &Value) -> bool {
    plan["stages"].as_array().is_some_and(|stages| {
        stages.iter().any(|stage| {
            stage["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == "worker"))
        })
    })
}

fn artifact_current(loaded: &Loaded, state: &Value) -> Result<bool, String> {
    match crate::run_artifact::assert_current(loaded, state) {
        Ok(()) => Ok(true),
        Err(error) if error.starts_with("artifact drift detected:") => Ok(false),
        Err(error) => Err(error),
    }
}

fn selected_plan(mut plan: Value) -> Result<Value, String> {
    let object = plan
        .as_object_mut()
        .ok_or("workflow plan must be an object")?;
    object.remove("goal");
    object.remove("notice");
    Ok(plan)
}

fn assert_no_drift(loaded: &Loaded, state: &Value) -> Result<(), String> {
    crate::boundary_manifest::assert_current(loaded, &state["plan"])?;
    let expected = state["configSha256"].as_str().unwrap_or("");
    let current = fs::read_to_string(&loaded.path)
        .map_err(|error| format!("configuration cannot be read: {error}"))?;
    let current_sha = hash::text(&current);
    if current_sha != expected {
        return Err(run_error::machine_drift(DriftError::config(
            expected.to_owned(),
            current_sha,
        )));
    }

    let mut seen = std::collections::BTreeSet::new();
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("validated run state has no plan stages")?;
    for stage in stages {
        let agents = stage["agents"]
            .as_array()
            .ok_or("validated run stage has no agents")?;
        for selected in agents {
            let name = selected["name"].as_str().unwrap_or("");
            if !seen.insert(name) {
                continue;
            }
            let configured = loaded
                .agent(name)
                .ok_or_else(|| format!("profile selection changed: agent '{name}' is missing"))?;
            let path = config::file(&loaded.control_root, &configured.profile)
                .map_err(|error| format!("profile cannot be read for '{name}': {error}"))?;
            let profile_sha = hash::text(
                &fs::read_to_string(&path)
                    .map_err(|error| format!("profile cannot be read for '{name}': {error}"))?,
            );
            if config::rel(&loaded.control_root, &path)? != selected["profile"]
                || profile_sha != selected["profileSha256"]
            {
                return Err(run_error::machine_drift(DriftError::profile(
                    name.to_owned(),
                    selected["profileSha256"].as_str().unwrap_or("").to_owned(),
                    profile_sha,
                )));
            }
            if let Some(references) = selected.get("memoryReferences") {
                let expected = hash::value(references);
                let current = match crate::memory_selection::resolve(loaded, name) {
                    Ok(current) => {
                        let current = Value::Array(current);
                        if &current == references {
                            continue;
                        }
                        hash::value(&current)
                    }
                    Err(error) => hash::text(&format!("memory evidence unavailable: {error}")),
                };
                return Err(run_error::machine_drift(DriftError::memory(
                    name.to_owned(),
                    expected,
                    current,
                )));
            }
        }
    }
    if let Some(reference) = state.get("harnessReceipt") {
        crate::receipt::assert_current(loaded, reference, &state["plan"])?;
    }
    Ok(())
}

pub(crate) fn assert_current_for_receipt(loaded: &Loaded, state: &Value) -> Result<(), String> {
    assert_no_drift(loaded, state)?;
    crate::run_artifact::assert_current(loaded, state)
}

fn nondecreasing(previous: &Value) -> Result<String, String> {
    let candidate = now();
    let candidate_time = chrono::DateTime::parse_from_rfc3339(&candidate)
        .map_err(|_| "generated run timestamp is invalid".to_owned())?;
    let previous = previous
        .as_str()
        .ok_or("previous run timestamp is invalid")?;
    let previous_time = chrono::DateTime::parse_from_rfc3339(previous)
        .map_err(|_| "previous run timestamp is invalid".to_owned())?;
    if candidate_time < previous_time {
        Ok(previous.to_owned())
    } else {
        Ok(candidate)
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

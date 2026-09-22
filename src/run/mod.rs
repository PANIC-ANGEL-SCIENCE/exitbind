//! Run orchestration for bounded agent workflows.
//!
//! Ledger storage and locking live in `ledger`; event validation and state
//! transitions live in `state`. This module coordinates those boundaries
//! and checks live configuration/artifact evidence before mutation.

pub(crate) mod artifact;
pub(crate) mod assignment;
pub(crate) mod error;
pub(crate) mod inputs;
pub(crate) mod ledger;
pub(crate) mod state;

pub use self::error::DriftError;
use self::ledger::{
    append, claim_path, ledger_path, load, load_at, obtain_claim, predecessor, rollback_claim,
    with_lock,
};
use self::{error as run_error, ledger as run_ledger, state as run_state};
use crate::{
    config::{self, Loaded},
    envelope,
    evidence::hash,
};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_OBSERVE_TIMEOUT_MS: u64 = 1_800_000;
const OBSERVE_POLL_MS: u64 = 10;
const OBSERVE_TERMINATION_GRACE_MS: u64 = 250;

fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

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
    basis: Option<&str>,
    review_policy: Option<&str>,
) -> Result<Value, String> {
    if workflow.trim().is_empty() {
        return Err("workflow is required".into());
    }
    if goal.trim().is_empty() {
        return Err("--goal requires a non-empty value".into());
    }
    let plan = crate::config::boundary::apply(
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
    let extension = extension_from_cli(basis, review_policy)?;
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
        crate::evidence::receipt::for_run(loaded, receipt, &plan)?;
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
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let event = with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let reference = if let Some(receipt) = harness_receipt {
            if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source
            {
                return Err("configuration changed while starting; reload configuration".into());
            }
            Some(crate::evidence::receipt::for_run(loaded, receipt, &plan)?)
        } else {
            None
        };
        let version = if crate::producer::exitbind_surface() {
            // A run that authorizes provider-quota fallback records fallback
            // provenance, which only the v7 submit shape can carry.
            8
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
        if let Some(extension) = &extension {
            value["basisProtocol"] = json!(crate::kernel::basis::PROTOCOL_VERSION);
            if let Some(basis) = &extension.basis {
                value["basis"] = basis.value();
            }
            value["reviewPolicy"] = extension.review.value();
        }
        if version >= 5 {
            value["subject"] = subject(
                goal,
                &value["plan"],
                &config_sha,
                &run_id,
                extension
                    .as_ref()
                    .and_then(|x| x.basis.as_ref())
                    .map(|x| x.sha256.as_str()),
            );
        }
        if version >= 6 {
            value["governor"] = json!({
                "version": crate::context::GOVERNOR_VERSION,
                "budget": crate::context::HARD_ITERATION_BUDGET,
                "noInformationLimit": crate::context::NO_INFORMATION_LIMIT,
                "postReplanLimit": crate::context::POST_REPLAN_LIMIT,
                "grantProtocol": crate::context::GRANT_PROTOCOL_VERSION,
            });
        }
        let event = run_state::make_event(value);
        append(&path, &event, true, "")?;
        Ok(event)
    })?;
    result(&[event])
}

/// Record the owner-controlled review choice for the current marked run.
/// This is a ledger transition, so a later choice is explicitly chained to
/// the prior decision and cannot be inferred from a missing review.
pub fn review_policy(
    loaded: &Loaded,
    agent: &str,
    ledger: &str,
    decision: &str,
    reason: Option<&str>,
) -> Result<Value, String> {
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        if state["status"] != "running" || state.get("basisProtocol").is_none() {
            return Err("review policy changes require a marked running run".into());
        }
        let previous = state["reviewPolicy"]["sha256"]
            .as_str()
            .ok_or("run has no current review policy")?;
        let review = crate::kernel::basis::review_value_with_previous(
            decision,
            reason.unwrap_or("owner decision supplied through the explicit run interface"),
            Some(previous),
        )?;
        let last = events.last().ok_or("run ledger has no event head")?;
        let event = run_state::make_event(json!({
            "version": 8,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(8),
            "action": "review_policy",
            "runId": state["runId"],
            "stage": state["currentStage"],
            "attempt": state["attempt"],
            "agent": agent,
            "role": "lead",
            "basisProtocol": crate::kernel::basis::PROTOCOL_VERSION,
            "basisSha256": state["basis"]["sha256"],
            "previousDecisionSha256": previous,
            "reviewPolicy": review.value(),
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = reduce_live(loaded, &all)?;
        append(&path, &event, false, &source)?;
        Ok(
            json!({"valid":true,"event":event,"status":next_state["status"],"assignments":crate::run::assignment::pending(&next_state)}),
        )
    })
}

pub fn next(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    RunSnapshot::capture(loaded, ledger)?.next_view(loaded)
}

/// Accept only a result identity that is present at the head of a governed,
/// accepted run ledger.  Goal closure is a separate Lead decision, so this
/// helper supplies evidence identity without allowing arbitrary strings.
pub(crate) fn current_result_ref(loaded: &Loaded, reference: &str) -> Result<bool, String> {
    let runs = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("runs");
    let entries = match fs::read_dir(runs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    let requested_name = reference
        .strip_prefix("smw_")
        .map(|token| format!("work-{token}.jsonl"));
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("run ledger path is not valid UTF-8")?;
        if let Some(requested_name) = &requested_name {
            if name != requested_name {
                continue;
            }
        }
        let relative = format!(
            "{}/runs/{name}",
            crate::project::layout_types::state_namespace()
        );
        let (_, events, _) = match load(loaded, &relative) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(last) = events.last() else { continue };
        if requested_name.is_none() && last["eventSha256"].as_str() != Some(reference) {
            continue;
        }
        let state = run_state::reduce(&events)?;
        let tested_inputs_current = if state["version"].as_u64() >= Some(6) {
            state["inputsSha256"].as_str().is_some_and(|expected| {
                crate::run::inputs::fingerprint(loaded).is_ok_and(|current| current == expected)
            })
        } else {
            true
        };
        let current = assert_no_drift(loaded, &state).is_ok()
            && artifact_current(loaded, &state).unwrap_or(false)
            && tested_inputs_current;
        return Ok(current
            && state["status"] == "accepted"
            && last["action"] == "submit"
            && last["role"] == "lead"
            && last["outcome"] == "accepted");
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
pub fn submit(
    loaded: &Loaded,
    agent: &str,
    ledger: &str,
    outcome: &str,
    artifact: &str,
    artifact_root: Option<&str>,
    reason: Option<&str>,
    disposition: Option<&str>,
) -> Result<Value, String> {
    if agent.trim().is_empty() {
        return Err("agent is required".into());
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let artifact = artifact.to_owned();
    submit_locked(
        loaded,
        &path,
        agent,
        outcome,
        reason,
        None,
        None,
        disposition,
        SubmitSubject {
            root: artifact_root,
            artifact: || Ok(artifact),
        },
    )
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
    pub(crate) basis_sha256: Option<String>,
    pub(crate) review_decision_sha256: Option<String>,
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
            basis_sha256: packet["basisSha256"].as_str().map(str::to_owned),
            review_decision_sha256: packet["reviewDecisionSha256"].as_str().map(str::to_owned),
        })
    }

    fn matches(&self, assignment: &Value) -> bool {
        assignment["stage"].as_u64() == Some(self.stage)
            && assignment["attempt"].as_u64() == Some(self.attempt)
            && assignment["agent"].as_str() == Some(&self.agent)
            && assignment["role"].as_str() == Some(&self.role)
            && assignment["basisSha256"].as_str() == self.basis_sha256.as_deref()
            && assignment["reviewDecisionSha256"].as_str() == self.review_decision_sha256.as_deref()
    }
}

/// Submit a work result after revalidating the façade's exact assignment
/// under the ledger lock.  The artifact is produced only after that check.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_for_assignment<F>(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    outcome: &str,
    reason: Option<&str>,
    disposition: Option<&str>,
    artifact: F,
) -> Result<Value, String>
where
    F: FnOnce() -> Result<String, String>,
{
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let agent = expected.agent.clone();
    let assignment_sha256 = crate::evidence::hash::text(assignment_handle);
    submit_locked(
        loaded,
        &path,
        &agent,
        outcome,
        reason,
        Some(&expected),
        Some(&assignment_sha256),
        disposition,
        SubmitSubject {
            root: Some("state"),
            artifact,
        },
    )
}

/// Atomically consume one cooperative mutation unit for the exact pending
/// assignment before a caller changes product files.  The permission is not
/// a permit detached from state: it is a hashed run-ledger event whose
/// governor payload is replayed by `state::reduce` under the same lock.
pub(crate) fn permit_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    work: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    operation: &str,
) -> Result<Value, String> {
    if operation.trim().is_empty() || operation.len() > 120 || operation.contains('\0') {
        return Err("governor operation must be non-empty and at most 120 bytes".into());
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let canonical_work = canonical_work_for_ledger(&path)?;
    if canonical_work != work {
        return Err("work identifier is not bound to the canonical ledger".into());
    }
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        if state["governor"]["enabled"] != true {
            return Err("cooperative governor is unavailable on this run".into());
        }
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or_else(|| format!("agent '{}' is not currently pending", expected.agent))?;
        if assignment["role"] != "worker" || expected.role != "worker" {
            return Err(
                "only the exact current worker assignment may receive a mutation grant".into(),
            );
        }
        if crate::run::assignment::handle(work, &assignment)? != assignment_handle {
            return Err("assignment handle is not bound to this run's current worker".into());
        }
        if !expected.matches(&assignment) {
            return Err(
                "assignment changed while governor permission was requested; no mutation was made"
                    .into(),
            );
        }
        let inputs = live_inputs(&state)?;
        let last = events.last().ok_or("run ledger has no event head")?;
        let previous_governor = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let lineage = state["governor"]["lineageSha256"]
            .as_str()
            .map_or_else(|| state["subject"]["sha256"].clone(), |value| json!(value));
        let checkpoint = state["governor"]["spent"]
            .as_u64()
            .unwrap_or_default()
            .saturating_add(1);
        let terminal_refusal = state["governor"]["state"] == "evidence_required";
        let mut governor_payload = json!({
            "action": if terminal_refusal { "blocked" } else { "mutation" },
            "runId": state["runId"],
            "subjectSha256": state["subject"]["sha256"],
            "attempt": state["attempt"],
            "checkpoint": checkpoint,
            "inputSha256": inputs.clone(),
        });
        if terminal_refusal {
            governor_payload["reason"] = json!("new exact evidence is required");
        } else {
            governor_payload["lineageSha256"] = lineage;
            governor_payload["carryLineage"] = json!(true);
            governor_payload["unit"] = json!(format!("{}-mutation", expected.role));
            governor_payload["operation"] = json!(operation);
            governor_payload["newEvidenceSha256"] = Value::Null;
        }
        let governor_event = crate::context::event(previous_governor.as_ref(), governor_payload);
        let version = state["version"]
            .as_u64()
            .ok_or("run state is missing version")?;
        let event_value = json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern",
            "runId": state["runId"],
            "stage": assignment["stage"],
            "attempt": assignment["attempt"],
            "agent": assignment["agent"],
            "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": inputs,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": operation,
            "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        });
        let event = run_state::make_event(event_value);
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        if terminal_refusal {
            return Err("governor durably blocked; exact evidence is required".into());
        }
        Ok(json!({
            "valid": true,
            "allowed": true,
            "event": event,
            "runId": next_state["runId"],
            "governor": next_state["governor"],
        }))
    })
}

fn canonical_work_for_ledger(path: &run_ledger::LedgerPath) -> Result<String, String> {
    let relative = path
        .path
        .strip_prefix(&path.root)
        .map_err(|_| "ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    let prefix = format!(
        "{}/runs/work-",
        crate::project::layout_types::state_namespace()
    );
    let token = relative
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(".jsonl"))
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .ok_or("governed permits require a canonical work ledger")?;
    Ok(format!("smw_{token}"))
}

/// Record a material governor re-plan for the exact pending assignment.
#[allow(clippy::too_many_arguments)]
pub(crate) fn replan_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    hypothesis: Option<&str>,
    evidence_request: Option<&str>,
    scope_decision: Option<&str>,
    blocker: Option<&str>,
) -> Result<Value, String> {
    let semantic = [hypothesis, evidence_request, scope_decision, blocker]
        .into_iter()
        .flatten()
        .any(|value| !value.trim().is_empty());
    if !semantic {
        return Err(
            "re-plan requires a material hypothesis, evidence request, scope decision, or blocker"
                .into(),
        );
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        if state["governor"]["enabled"] != true {
            return Err("cooperative governor is unavailable on this run".into());
        }
        if state["governor"]["state"] != "replan_required" {
            return Err("material re-plan is not currently required".into());
        }
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == "worker")
            .ok_or("no pending worker assignment is available for re-plan")?;
        if !expected.matches(&assignment) {
            return Err(
                "assignment changed while re-plan was requested; no mutation was made".into(),
            );
        }
        let inputs = live_inputs(&state)?;
        let last = events.last().ok_or("run has no event head")?;
        let previous_governor = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let governor_event = crate::context::event(
            previous_governor.as_ref(),
            json!({
                "action": "replan",
                "runId": state["runId"],
                "subjectSha256": state["subject"]["sha256"],
                "attempt": state["attempt"],
                "checkpoint": state["governor"]["spent"],
                "inputSha256": inputs,
                "hypothesis": hypothesis,
                "evidenceRequest": evidence_request,
                "scopeDecision": scope_decision,
                "blocker": blocker,
            }),
        );
        let version = state["version"]
            .as_u64()
            .ok_or("run state has no version")?;
        let event = run_state::make_event(json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern",
            "runId": state["runId"],
            "stage": assignment["stage"],
            "attempt": assignment["attempt"],
            "agent": assignment["agent"],
            "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": live_inputs(&state)?,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": "replan",
            "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({"valid": true, "event": event, "governor": next_state["governor"]}))
    })
}

/// Bind one exact product/state artifact as new governor evidence under the
/// same assignment lock used by mutation permission and completion.
pub(crate) fn evidence_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    artifact_root: Option<&str>,
    artifact_path: &str,
) -> Result<Value, String> {
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        if state["governor"]["enabled"] != true {
            return Err("cooperative governor is unavailable on this run".into());
        }
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or("assignment is not currently pending")?;
        if !expected.matches(&assignment) {
            return Err("assignment changed while evidence was requested".into());
        }
        let mut evidence = crate::run::artifact::evidence(loaded, artifact_root, artifact_path)?;
        evidence["subjectSha256"] = state["subject"]["sha256"].clone();
        evidence["attempt"] = state["attempt"].clone();
        evidence["inputSha256"] = live_inputs(&state)?;
        if let Some(previous) = state["governor"]["evidence"].as_array().and_then(|items| {
            items.iter().find(|item| {
                item["root"] == evidence["root"]
                    && item["path"] == evidence["path"]
                    && item["sha256"] == evidence["sha256"]
                    && item["subjectSha256"] == evidence["subjectSha256"]
                    && item["attempt"] == evidence["attempt"]
                    && item["inputSha256"] == evidence["inputSha256"]
            })
        }) {
            let event = events.iter().rev().find(|event| {
                event["governorEvent"]["action"] == "evidence"
                    && event["governorEvent"]["evidence"] == *previous
            });
            if let Some(event) = event {
                return Ok(
                    json!({"valid": true, "idempotent": true, "event": event, "governor": state["governor"]}),
                );
            }
        }
        if state["governor"]["evidence"]
            .as_array()
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item["root"] == evidence["root"]
                        && item["path"] == evidence["path"]
                        && item["sha256"] != evidence["sha256"]
                })
            })
        {
            return Err("evidence artifact changed for the same path".into());
        }
        let last = events.last().ok_or("run has no event head")?;
        let previous_governor = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let governor_event = crate::context::event(
            previous_governor.as_ref(),
            json!({
                "action": "evidence",
                "runId": state["runId"],
                "subjectSha256": state["subject"]["sha256"],
                "attempt": state["attempt"],
                "inputSha256": evidence["inputSha256"],
                "evidence": evidence,
            }),
        );
        let version = state["version"]
            .as_u64()
            .ok_or("run state has no version")?;
        let event = run_state::make_event(json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern",
            "runId": state["runId"],
            "stage": assignment["stage"],
            "attempt": assignment["attempt"],
            "agent": assignment["agent"],
            "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": live_inputs(&state)?,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": "evidence",
            "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({"valid": true, "event": event, "governor": next_state["governor"]}))
    })
}

pub(crate) fn sensor_request_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
) -> Result<Value, String> {
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or("assignment is not currently pending")?;
        if !expected.matches(&assignment) {
            return Err("assignment changed while sensor request was requested".into());
        }
        let request = crate::context::sensor_request(&state["governor"])?;
        if state["governor"]["currentSensorRequest"]["requestDigest"] == request["requestDigest"] {
            if let Some(event) = events.iter().rev().find(|event| {
                event["governorEvent"]["action"] == "sensor_request"
                    && event["governorEvent"]["requestDigest"] == request["requestDigest"]
            }) {
                return Ok(
                    json!({"valid": true, "idempotent": true, "event": event, "governor": state["governor"]}),
                );
            }
        }
        let last = events.last().ok_or("run has no event head")?;
        let previous = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let governor_event = crate::context::event(previous.as_ref(), request);
        let version = state["version"]
            .as_u64()
            .ok_or("run state has no version")?;
        let event = run_state::make_event(json!({
            "version": version, "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern", "runId": state["runId"],
            "stage": assignment["stage"], "attempt": assignment["attempt"],
            "agent": assignment["agent"], "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": live_inputs(&state)?,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": "sensor_request", "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({"valid": true, "event": event, "governor": next_state["governor"]}))
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sensor_result_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    assessment: &str,
    confidence: Option<&str>,
    input_digest: &str,
    identity_source: &str,
) -> Result<Value, String> {
    if !matches!(
        assessment,
        "unavailable" | "low_information" | "replan" | "evidence" | "block" | "ready"
    ) {
        return Err("sensor assessment is outside the bounded schema".into());
    }
    if input_digest.len() != 64
        || !input_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("sensor input-digest must be lowercase SHA-256".into());
    }
    if identity_source != "host-reported" {
        return Err("sensor identity must remain host-reported".into());
    }
    let confidence = confidence
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|_| "sensor confidence is invalid".to_owned())
        })
        .transpose()?;
    if confidence.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err("sensor confidence must be finite and within 0..=1".into());
    }
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        assert_no_drift(loaded, &state)?;
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or("assignment is not currently pending")?;
        if !expected.matches(&assignment) {
            return Err("assignment changed while sensor result was requested".into());
        }
        let request = state["governor"]["currentSensorRequest"]
            .as_object()
            .ok_or("sensor result has no current request")?;
        let current = state["governor"]["currentMutation"]
            .as_object()
            .ok_or("sensor result has no current mutation")?;
        if let Some(previous) = events.iter().rev().find(|event| {
            event["governorEvent"]["action"] == "sensor"
                && event["governorEvent"]["requestDigest"] == request["requestDigest"]
        }) {
            let prior = &previous["governorEvent"];
            let expected_confidence = confidence.map_or(Value::Null, |value| json!(value));
            if prior["assessment"] == assessment
                && prior["inputDigest"] == input_digest
                && prior["confidence"] == expected_confidence
            {
                return Ok(
                    json!({"valid": true, "idempotent": true, "event": previous, "governor": state["governor"]}),
                );
            }
            return Err("conflicting duplicate sensor result refused".into());
        }
        let last = events.last().ok_or("run has no event head")?;
        let previous = state["governor"]["headSha256"]
            .as_str()
            .map(|head| json!({"eventSha256": head}));
        let mut payload = json!({
            "action": "sensor", "sensorVersion": crate::context::SENSOR_VERSION,
            "runId": state["runId"], "subjectSha256": state["subject"]["sha256"],
            "attempt": state["attempt"], "checkpoint": current["checkpoint"],
            "inputSha256": current["inputSha256"], "inputDigest": input_digest,
            "requestDigest": request["requestDigest"], "assessment": assessment,
            "identitySource": identity_source,
        });
        if let Some(confidence) = confidence {
            payload["confidence"] = json!(confidence);
        } else {
            payload["confidence"] = Value::Null;
        }
        let governor_event = crate::context::event(previous.as_ref(), payload);
        let version = state["version"]
            .as_u64()
            .ok_or("run state has no version")?;
        let event = run_state::make_event(json!({
            "version": version, "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern", "runId": state["runId"],
            "stage": assignment["stage"], "attempt": assignment["attempt"],
            "agent": assignment["agent"], "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": live_inputs(&state)?,
            "assignmentSha256": crate::evidence::hash::text(assignment_handle),
            "operation": "sensor", "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        }));
        let mut all = events;
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({"valid": true, "event": event, "governor": next_state["governor"]}))
    })
}

/// The submitted subject: an artifact root and a producer deferred until the
/// assignment has been revalidated under the ledger lock.
struct SubmitSubject<'r, F> {
    root: Option<&'r str>,
    artifact: F,
}

fn matching_mutation_grants(
    events: &[Value],
    state: &Value,
    assignment: &Value,
    assignment_sha256: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    let Some(subject) = state["subject"]["sha256"].as_str() else {
        return Err("run subject is not available for grant matching".into());
    };
    let Some(_result_input) = state["inputsSha256"].as_str() else {
        return Err("tested input identity is not available for grant matching".into());
    };
    let lineage = state["governor"]["lineageSha256"].as_str();
    let mut acknowledged = std::collections::BTreeSet::new();
    for event in events {
        if let Some(grants) = event["governorEvent"]["grantEventSha256s"].as_array() {
            acknowledged.extend(grants.iter().filter_map(Value::as_str));
        }
    }
    let mut result = Vec::new();
    for event in events {
        let nested = &event["governorEvent"];
        if event["action"] != "govern"
            || event["runId"] != state["runId"]
            || event["subjectSha256"] != subject
            || event["stage"] != assignment["stage"]
            || event["attempt"] != assignment["attempt"]
            || event["agent"] != assignment["agent"]
            || event["role"] != assignment["role"]
            || assignment_sha256.map_or(false, |hash| event["assignmentSha256"] != hash)
            || nested["action"] != "mutation"
            || nested["runId"] != state["runId"]
            || nested["subjectSha256"] != subject
            || nested["attempt"] != state["attempt"]
            || nested["inputSha256"] != event["inputsSha256"]
            || nested["carryLineage"] != true
            || lineage.map_or(true, |value| nested["lineageSha256"] != value)
        {
            continue;
        }
        let Some(hash) = event["eventSha256"].as_str() else {
            return Err("mutation grant has no outer event identity".into());
        };
        if !acknowledged.contains(hash) {
            let source = nested["inputSha256"]
                .as_str()
                .ok_or("mutation grant has no source input identity")?;
            result.push((hash.to_owned(), source.to_owned()));
        }
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn submit_locked<F>(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    agent: &str,
    outcome: &str,
    reason: Option<&str>,
    expected: Option<&AssignmentIdentity>,
    assignment_sha256: Option<&str>,
    disposition: Option<&str>,
    subject: SubmitSubject<'_, F>,
) -> Result<Value, String>
where
    F: FnOnce() -> Result<String, String>,
{
    let SubmitSubject { root, artifact } = subject;
    let artifact_root = root;
    let targets = [path.path.as_path(), path.lock.as_path()];
    with_lock(path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, path)?;
        let state = reduce_live(loaded, &events)?;
        if state["governor"]["enabled"] == true && state["governor"]["state"] == "blocked" {
            return Err("governor is durably blocked; work result refused".into());
        }
        assert_no_drift(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        if agent == "lead"
            && outcome == "accepted"
            && state["reviewPolicy"]["decision"] == "required"
            && state["plan"]["stages"].as_array().is_some_and(|stages| {
                stages.iter().any(|stage| {
                    stage["stage"] == state["currentStage"]
                        && stage["agents"].as_array().is_some_and(|agents| {
                            agents
                                .iter()
                                .any(|candidate| candidate["role"] == "reviewer")
                        })
                })
            })
        {
            return Err("canonical acceptance requires reviewer approval".into());
        }
        let assignment = crate::run::assignment::pending(&state)
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
        let mut grant_hashes = Vec::new();
        if state["governor"]["enabled"] == true
            && assignment["role"] == "worker"
            && outcome == "completed"
        {
            if state["governor"]["state"] != "ready" {
                return Err("governor requires re-plan or evidence before work completion".into());
            }
            grant_hashes =
                matching_mutation_grants(&events, &state, &assignment, assignment_sha256)?;
            if assignment_sha256.is_none() && !grant_hashes.is_empty() {
                return Err(
                    "exact assignment identity is required to acknowledge mutation grants".into(),
                );
            }
        }
        let artifact = artifact()?;
        let artifact_value = crate::run::artifact::evidence(loaded, artifact_root, &artifact)?;
        let last = events.last().ok_or("run ledger has no head event")?;
        // Preserve the ledger's historical event version. Optional fields
        // must not silently upgrade a v3 checked run on its next submission.
        let version = state["version"]
            .as_u64()
            .ok_or("run state is missing version")?;
        let result_inputs = if version >= 6 {
            Some(live_inputs(&state)?)
        } else {
            None
        };
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
        if version >= 6 {
            if let Some(assignment_sha256) = assignment_sha256 {
                event_value["assignmentSha256"] = json!(assignment_sha256);
            }
        }
        if version >= 6
            && matches!(
                (assignment["role"].as_str(), outcome),
                (Some("reviewer"), "approved")
                    | (Some("lead"), "accepted")
                    | (Some("worker"), "completed")
            )
        {
            event_value["inputsSha256"] = result_inputs.clone().unwrap();
        }
        if version >= 5 {
            event_value["subjectSha256"] =
                if assignment["role"] == "worker" && outcome == "completed" {
                    crate::run::state::subject_for_submission(&state, &assignment, &artifact_value)
                        ["sha256"]
                        .clone()
                } else {
                    state["subject"]["sha256"].clone()
                };
        }
        if version >= 8 && state.get("basisProtocol").is_some() {
            event_value["basisSha256"] = state["basis"]["sha256"].clone();
            event_value["reviewDecisionSha256"] = state["reviewPolicy"]["sha256"].clone();
        }
        if outcome == "disposition" {
            let raw = disposition
                .or(reason)
                .ok_or("a disposition submission requires --disposition JSON")?;
            let parsed: Value = serde_json::from_str(raw)
                .map_err(|error| format!("disposition is not valid JSON: {error}"))?;
            let disposition = crate::kernel::basis::parse_disposition(&parsed, "disposition")?;
            event_value["disposition"] = disposition.value();
        }
        if let Some(fallback) = fallback_provenance(&assignment, outcome, reason, version)? {
            event_value["fallback"] = fallback;
        }
        if state["governor"]["enabled"] == true
            && assignment["role"] == "worker"
            && outcome == "completed"
        {
            let previous_governor = state["governor"]["headSha256"]
                .as_str()
                .map(|head| json!({"eventSha256": head}));
            let lineage = state["governor"]["lineageSha256"]
                .as_str()
                .map_or_else(|| state["subject"]["sha256"].clone(), |value| json!(value));
            let input_sha = result_inputs
                .clone()
                .unwrap_or_else(|| json!(crate::evidence::hash::text("unbound")));
            // Pre-mutation permits and completed submissions share one
            // canonical governor lineage.  Counting submissions alone would
            // replay a post-permit completion at checkpoint 1 and make the
            // ledger unreducible (or silently reset the bound).
            let explicit = !grant_hashes.is_empty();
            let (grant_event_sha256s, source_inputs_sha256s): (Vec<_>, Vec<_>) =
                grant_hashes.iter().cloned().unzip();
            let mutation = json!({
                "action": "mutation",
                "runId": state["runId"],
                "subjectSha256": state["subject"]["sha256"],
                "attempt": state["attempt"],
                "checkpoint": if grant_hashes.is_empty() {
                    json!(state["governor"]["spent"].as_u64().unwrap_or_default().saturating_add(1))
                } else {
                    state["governor"]["spent"].clone()
                },
                "inputSha256": input_sha,
                "lineageSha256": lineage,
                "carryLineage": true,
                "unit": "worker",
                "operation": "completed_submission",
                "newEvidenceSha256": artifact_value["sha256"],
                "authorizationMode": if explicit { "explicit" } else { "implicit" },
                "grantEventSha256s": grant_event_sha256s,
                "sourceInputsSha256s": source_inputs_sha256s,
            });
            event_value["governorEvent"] =
                crate::context::event(previous_governor.as_ref(), mutation);
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
            "assignments": crate::run::assignment::pending(&next_state)
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
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        if state["status"] != "running" {
            return Err("run has already reached a terminal state; no mutation was made".into());
        }
        assert_no_drift(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        let policy = active_check_policy(&state, requirement_id)?;
        if policy.command != check_command
            || policy.command_sha256 != crate::evidence::hash::text(check_command)
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
            "checkCommandSha256": crate::evidence::hash::text(check_command),
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
            "checkCommandSha256": crate::evidence::hash::text(check_command),
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
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    let (source, policy, inputs, identity, capture_logs) = with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
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
        crate::run::artifact::assert_current(loaded, &state)?;
        predecessor(loaded, &events[0])?;
        let policy = active_check_policy(&state, requirement_id)?;
        crate::run_value::validate_check_target(&state, target)?;
        let inputs = if state["version"].as_u64() >= Some(6) {
            Some(live_inputs(&state)?)
        } else {
            None
        };
        Ok((
            source,
            policy.clone(),
            inputs.clone(),
            json!({
                "runId": state["runId"],
                "targetEventSha256": target,
                "subjectSha256": state["subject"]["sha256"],
                "inputsSha256": inputs,
                "checkCommandSha256": policy.command_sha256,
            }),
            state["version"] == 8,
        ))
    })?;

    let capture = match if capture_logs {
        capture_observed_command(loaded, &policy.command, timeout_ms, &identity)
    } else {
        run_observed_command(loaded, &policy.command, timeout_ms).map(CapturedCheck::without_logs)
    } {
        Ok(capture) => capture,
        Err(error) => return Err(error),
    };

    let result = observed_result(&capture.status)?;
    let committed = with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
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
        crate::run::artifact::assert_current(loaded, &current_state)?;
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
            "durationMs": capture.duration_ms,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        });
        if let (Some(stdout), Some(stderr)) = (&capture.stdout_artifact, &capture.stderr_artifact) {
            event_value["stdout"] = stdout.clone();
            event_value["stderr"] = stderr.clone();
        }
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
        let installed_stdout = match (&capture.stdout_temp, &capture.stdout_final) {
            (Some(temp), Some(final_path)) => install_capture(temp, final_path)?,
            _ => false,
        };
        let installed_stderr = match (&capture.stderr_temp, &capture.stderr_final) {
            (Some(temp), Some(final_path)) => match install_capture(temp, final_path) {
                Ok(installed) => installed,
                Err(error) => {
                    if installed_stdout {
                        if let Some(final_path) = &capture.stdout_final {
                            let _ = fs::remove_file(final_path);
                        }
                    }
                    return Err(error);
                }
            },
            _ => false,
        };
        let append_result = append(&path, &event, false, &current_source);
        if let Err(error) = append_result {
            if installed_stdout {
                if let Some(final_path) = &capture.stdout_final {
                    let _ = fs::remove_file(final_path);
                }
            }
            if installed_stderr {
                if let Some(final_path) = &capture.stderr_final {
                    let _ = fs::remove_file(final_path);
                }
            }
            return Err(error);
        }
        Ok(json!({
            "valid": true,
            "event": event,
            "runId": next_state["runId"],
            "status": next_state["status"],
            "checks": crate::run_value::status(&next_state, Some(true))?["checks"]
        }))
    });
    if let Some(path) = &capture.stdout_temp {
        let _ = fs::remove_file(path);
    }
    if let Some(path) = &capture.stderr_temp {
        let _ = fs::remove_file(path);
    }
    committed
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

struct CapturedCheck {
    status: ExitStatus,
    duration_ms: u64,
    stdout_temp: Option<PathBuf>,
    stderr_temp: Option<PathBuf>,
    stdout_final: Option<PathBuf>,
    stderr_final: Option<PathBuf>,
    stdout_artifact: Option<Value>,
    stderr_artifact: Option<Value>,
}

impl CapturedCheck {
    fn without_logs((status, duration_ms): (ExitStatus, u64)) -> Self {
        Self {
            status,
            duration_ms,
            stdout_temp: None,
            stderr_temp: None,
            stdout_final: None,
            stderr_final: None,
            stdout_artifact: None,
            stderr_artifact: None,
        }
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
            .arg("exitbind-observe-check")
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

fn capture_observed_command(
    loaded: &Loaded,
    command_text: &str,
    timeout_ms: u64,
    identity: &Value,
) -> Result<CapturedCheck, String> {
    let artifact_dir = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace())
        .join("artifacts");
    crate::project::managed_files::ensure_managed_directory(&loaded.state_root, &artifact_dir)?;
    let log_id = hash::value(identity);
    let stdout_final = artifact_dir.join(format!("check-log-{log_id}-stdout.raw"));
    let stderr_final = artifact_dir.join(format!("check-log-{log_id}-stderr.raw"));
    let nonce = format!("{}-{}", std::process::id(), timestamp_nanos());
    let stdout_temp = artifact_dir.join(format!(".check-log-{log_id}-{nonce}-stdout.tmp"));
    let stderr_temp = artifact_dir.join(format!(".check-log-{log_id}-{nonce}-stderr.tmp"));
    let result = capture_to_files(loaded, command_text, timeout_ms, &stdout_temp, &stderr_temp);
    let (status, duration_ms) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&stdout_temp);
            let _ = fs::remove_file(&stderr_temp);
            return Err(error);
        }
    };
    let stdout = match fs::read(&stdout_temp) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = fs::remove_file(&stdout_temp);
            let _ = fs::remove_file(&stderr_temp);
            return Err(error.to_string());
        }
    };
    let stderr = match fs::read(&stderr_temp) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = fs::remove_file(&stdout_temp);
            let _ = fs::remove_file(&stderr_temp);
            return Err(error.to_string());
        }
    };
    let stdout_artifact = match artifact_value(loaded, &stdout_final, &stdout) {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&stdout_temp);
            let _ = fs::remove_file(&stderr_temp);
            return Err(error);
        }
    };
    let stderr_artifact = match artifact_value(loaded, &stderr_final, &stderr) {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&stdout_temp);
            let _ = fs::remove_file(&stderr_temp);
            return Err(error);
        }
    };
    Ok(CapturedCheck {
        status,
        duration_ms,
        stdout_temp: Some(stdout_temp),
        stderr_temp: Some(stderr_temp),
        stdout_final: Some(stdout_final),
        stderr_final: Some(stderr_final),
        stdout_artifact: Some(stdout_artifact),
        stderr_artifact: Some(stderr_artifact),
    })
}

fn capture_to_files(
    loaded: &Loaded,
    command_text: &str,
    timeout_ms: u64,
    stdout_temp: &PathBuf,
    stderr_temp: &PathBuf,
) -> Result<(ExitStatus, u64), String> {
    #[cfg(not(unix))]
    {
        let _ = (loaded, command_text, timeout_ms, stdout_temp, stderr_temp);
        return Err("observe-check requires a POSIX host".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let stdout_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stdout_temp)
            .map_err(|error| format!("stdout capture could not be created: {error}"))?;
        let stderr_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stderr_temp)
        {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_file(stdout_temp);
                return Err(format!("stderr capture could not be created: {error}"));
            }
        };
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(command_text)
            .arg("exitbind-observe-check")
            .current_dir(&loaded.product_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let started = Instant::now();
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(stdout_temp);
                let _ = fs::remove_file(stderr_temp);
                return Err(format!("check command could not be launched: {error}"));
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = terminate_process_group(&mut child);
                let _ = fs::remove_file(stdout_temp);
                let _ = fs::remove_file(stderr_temp);
                return Err("check stdout pipe was not available; no mutation was made".into());
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                let _ = terminate_process_group(&mut child);
                let _ = fs::remove_file(stdout_temp);
                let _ = fs::remove_file(stderr_temp);
                return Err("check stderr pipe was not available; no mutation was made".into());
            }
        };
        let (sender, receiver) = mpsc::channel();
        let stdout_thread = {
            let sender = sender.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    drain_capture_stream(stdout, stdout_file, "stdout")
                }))
                .map_err(|_| "stdout capture reader panicked".to_owned())
                .and_then(|result| result);
                let _ = sender.send(result);
            })
        };
        let stderr_thread = {
            let sender = sender.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    drain_capture_stream(stderr, stderr_file, "stderr")
                }))
                .map_err(|_| "stderr capture reader panicked".to_owned())
                .and_then(|result| result);
                let _ = sender.send(result);
            })
        };
        drop(sender);
        let timeout = Duration::from_millis(timeout_ms);
        let deadline = started
            .checked_add(timeout)
            .unwrap_or_else(|| Instant::now() + timeout);
        let mut stream_results = Vec::new();
        let mut stream_error = None;
        let mut status = None;
        let mut timed_out = false;
        loop {
            while let Ok(result) = receiver.try_recv() {
                if let Err(error) = &result {
                    stream_error = Some(error.clone());
                }
                stream_results.push(result);
            }
            if stream_error.is_some() || (status.is_some() && stream_results.len() == 2) {
                break;
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(exit_status)) => status = Some(exit_status),
                    Ok(None) => {}
                    Err(error) => {
                        stream_error =
                            Some(format!("check command could not be observed: {error}"));
                        break;
                    }
                }
            }
            if stream_results.len() == 2 {
                if status.is_some() {
                    break;
                }
                if Instant::now() >= deadline {
                    timed_out = true;
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
                continue;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(Duration::from_millis(OBSERVE_POLL_MS));
            match receiver.recv_timeout(wait) {
                Ok(result) => {
                    if let Err(error) = &result {
                        stream_error = Some(error.clone());
                    }
                    stream_results.push(result);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stream_error = Some(format!(
                        "capture reader stopped unexpectedly ({} stream result(s))",
                        stream_results.len()
                    ));
                }
            }
        }

        let cleanup = if timed_out || stream_error.is_some() {
            // A failed reader can leave a descendant holding the other pipe
            // even after the shell leader has exited; always inspect and
            // terminate the process group on this path.
            terminate_process_group(&mut child)
        } else {
            Ok(())
        };
        while stream_results.len() < 2 {
            match receiver.recv_timeout(Duration::from_millis(OBSERVE_TERMINATION_GRACE_MS)) {
                Ok(result) => {
                    if let Err(error) = &result {
                        stream_error = Some(error.clone());
                    }
                    stream_results.push(result);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    stream_error = Some("capture readers did not finish after cleanup".into());
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stream_error = Some(format!(
                        "capture reader stopped unexpectedly ({} stream result(s))",
                        stream_results.len()
                    ));
                    break;
                }
            }
        }
        if stream_results.len() == 2 {
            let stdout_panicked = stdout_thread.join().is_err();
            let stderr_panicked = stderr_thread.join().is_err();
            if stdout_panicked || stderr_panicked {
                stream_error = Some("capture reader panicked".into());
            }
        } else {
            // A reader which did not report by the bounded cleanup deadline is
            // deliberately detached here: joining it could turn a failed
            // capture into an unbounded wait. Its pathname is removed below.
            drop(stdout_thread);
            drop(stderr_thread);
        }
        if let Some(error) = stream_error {
            let _ = fs::remove_file(stdout_temp);
            let _ = fs::remove_file(stderr_temp);
            return Err(format!("{error}; no mutation was made"));
        }
        if let Err(error) = cleanup {
            let _ = fs::remove_file(stdout_temp);
            let _ = fs::remove_file(stderr_temp);
            return Err(format!(
                "check process cleanup failed: {error}; no mutation was made"
            ));
        }
        if timed_out {
            let _ = fs::remove_file(stdout_temp);
            let _ = fs::remove_file(stderr_temp);
            return Err(format!(
                "check observation timed out after {timeout_ms} ms; no mutation was made"
            ));
        }
        let status = status.ok_or("check command ended without a status")?;
        if stream_results.iter().any(Result::is_err) {
            let _ = fs::remove_file(stdout_temp);
            let _ = fs::remove_file(stderr_temp);
            return Err("check capture failed; no mutation was made".into());
        }
        Ok((status, elapsed_ms(started)))
    }
}

fn drain_capture_stream<R: Read>(
    mut reader: R,
    mut file: File,
    stream: &str,
) -> Result<(), String> {
    let mut total = 0usize;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("{stream} capture read failed: {error}"))?;
        if count == 0 {
            file.flush()
                .map_err(|error| format!("{stream} capture flush failed: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("{stream} capture sync failed: {error}"))?;
            return Ok(());
        }
        if (total + count) as u64 > crate::run_value::MAX_CAPTURE_BYTES {
            return Err(format!(
                "{stream} capture exceeded the 1048576-byte stream limit"
            ));
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("{stream} capture write failed: {error}"))?;
        total += count;
    }
}

fn artifact_value(loaded: &Loaded, path: &std::path::Path, bytes: &[u8]) -> Result<Value, String> {
    let relative = crate::config::rel(&loaded.state_root, path)?;
    Ok(json!({
        "root": "state",
        "path": relative,
        "sha256": hash::bytes(bytes),
        "bytes": bytes.len(),
    }))
}

fn install_capture(temp: &std::path::Path, final_path: &std::path::Path) -> Result<bool, String> {
    match fs::symlink_metadata(final_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("captured log artifact already exists with invalid type".into());
            }
            let existing = fs::read(final_path).map_err(|error| error.to_string())?;
            let proposed = fs::read(temp).map_err(|error| error.to_string())?;
            if existing != proposed {
                return Err("captured log artifact already exists with different bytes".into());
            }
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(temp, final_path).map_err(|error| error.to_string())?;
            Ok(true)
        }
        Err(error) => Err(error.to_string()),
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
        canonical_work_for_ledger, process_group_exists, terminate_process_group, Duration,
        Instant, OBSERVE_TERMINATION_GRACE_MS,
    };
    use crate::run::ledger::LedgerPath;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::{fs, thread};

    #[test]
    fn canonical_work_binding_rejects_cross_run_ledger_pairs() {
        let root = PathBuf::from("/fixture");
        let token_a = "a".repeat(64);
        let token_b = "b".repeat(64);
        let path = root.join(format!(
            "{}/runs/work-{token_a}.jsonl",
            crate::project::layout_types::state_namespace()
        ));
        let ledger = LedgerPath {
            path: path.clone(),
            lock: root.join(".exitbind/locks/permit.lock"),
            root,
            expected: path,
        };
        assert_eq!(
            canonical_work_for_ledger(&ledger).unwrap(),
            format!("smw_{token_a}")
        );
        assert_ne!(
            canonical_work_for_ledger(&ledger).unwrap(),
            format!("smw_{token_b}")
        );
    }

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
    Ok(RunSnapshot::capture(loaded, ledger)?.inspect_view())
}

/// Read-only current-state view with artifact/config revalidation.
pub fn status(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    RunSnapshot::capture(loaded, ledger)?.status_view()
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

/// The terminal block this run offers a reader, if any. Read-only, and
/// separate from the status projection so the display surfaces can share one
/// rule with the work facade.
pub(crate) fn terminal_display(loaded: &Loaded, ledger: &str) -> Option<&'static str> {
    let snapshot = RunSnapshot::capture(loaded, ledger).ok()?;
    crate::work::packet::terminal_display(&snapshot.inspect_view(), || {
        crate::run::inputs::fingerprint(loaded).ok()
    })
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
    basis: Option<&str>,
    review_policy: Option<&str>,
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
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&old, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        let (_, old_events, old_source) = load_at(loaded, &old)?;
        let old_state = run_state::reduce(&old_events)?;
        if !matches!(old_state["status"].as_str(), Some("running" | "blocked")) {
            return Err("only a running or blocked run can be superseded".into());
        }
        if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source {
            return Err("configuration changed while superseding; reload configuration".into());
        }
        let plan = crate::config::boundary::apply(
            loaded,
            selected_plan(envelope::plan(loaded, workflow, goal)?)?,
            boundary,
        )?;
        let extension = extension_from_cli(basis, review_policy)?;
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
        if old_state["governor"]["enabled"] == true
            && old_state["subject"]["goalSha256"] == hash::text(goal)
        {
            return Err(
                "same-goal supersession is refused until governor lineage carry is persisted"
                    .into(),
            );
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
            .map(|receipt| crate::evidence::receipt::for_run(loaded, receipt, &plan))
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
            8
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
        if let Some(extension) = &extension {
            event_value["basisProtocol"] = json!(crate::kernel::basis::PROTOCOL_VERSION);
            if let Some(basis) = &extension.basis {
                event_value["basis"] = basis.value();
            }
            event_value["reviewPolicy"] = extension.review.value();
        }
        if version >= 5 {
            event_value["subject"] = subject(
                goal,
                &event_value["plan"],
                &config_sha,
                &run_id,
                extension
                    .as_ref()
                    .and_then(|x| x.basis.as_ref())
                    .map(|x| x.sha256.as_str()),
            );
        }
        if version >= 6 {
            event_value["governor"] = json!({
                "version": crate::context::GOVERNOR_VERSION,
                "budget": crate::context::HARD_ITERATION_BUDGET,
                "noInformationLimit": crate::context::NO_INFORMATION_LIMIT,
                "postReplanLimit": crate::context::POST_REPLAN_LIMIT,
            });
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
    Ok(reduce_with_inputs(loaded, events)?.0)
}

/// Which tested-input identity the facts of a reduced state describe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputContext {
    /// Pre-v6 run: evidence was never bound to tested inputs.
    NotBound,
    /// Running v6 run: the inputs present now.
    Current(String),
    /// Running v6 run whose current inputs could not be established; nothing
    /// that depends on them is current, and no stored digest substitutes.
    Unavailable(String),
    /// Terminal v6 run: identities are the recorded historical ones and say
    /// nothing about the files present now.
    Historical,
}

fn reduce_with_inputs(loaded: &Loaded, events: &[Value]) -> Result<(Value, InputContext), String> {
    let mut state = run_state::reduce(events)?;
    if state["version"].as_u64() < Some(6) {
        return Ok((state, InputContext::NotBound));
    }
    if state["status"] != "running" {
        return Ok((state, InputContext::Historical));
    }
    let context = match crate::run::inputs::fingerprint(loaded) {
        Ok(inputs) => {
            state["inputsSha256"] = json!(inputs);
            InputContext::Current(inputs)
        }
        Err(error) => {
            if let Some(object) = state.as_object_mut() {
                object.remove("inputsSha256");
            }
            InputContext::Unavailable(error)
        }
    };
    Ok((state, context))
}

/// One validated ledger revision reduced once, with the input context
/// established once for it. Every view of a single request derives from the
/// same snapshot, so next actions, evidence, and explanations cannot mix
/// revisions. It is not a permit: mutations revalidate under the ledger lock.
pub(crate) struct RunSnapshot {
    events: Vec<Value>,
    state: Value,
    inputs: InputContext,
    ledger_sha256: String,
    drift: Result<(), String>,
    artifact_current: Result<bool, String>,
}

impl RunSnapshot {
    pub(crate) fn capture(loaded: &Loaded, ledger: &str) -> Result<Self, String> {
        let (_, events, source) = load(loaded, ledger)?;
        let (state, inputs) = reduce_with_inputs(loaded, &events)?;
        predecessor(loaded, &events[0])?;
        let drift = assert_no_drift(loaded, &state);
        let artifact_current = if drift.is_ok() {
            artifact_current(loaded, &state)
        } else {
            Ok(false)
        };
        Ok(Self {
            events,
            state,
            inputs,
            ledger_sha256: hash::bytes(source.as_bytes()),
            drift,
            artifact_current,
        })
    }

    pub(crate) fn status(&self) -> &Value {
        &self.state["status"]
    }

    pub(crate) fn inputs(&self) -> &InputContext {
        &self.inputs
    }

    pub(crate) fn inspect_view(&self) -> Value {
        let state = &self.state;
        let mut result = json!({
            "valid": true,
            "runId": state["runId"],
            "workflow": state["workflow"],
            "status": state["status"],
            "currentStage": if state["status"] == "running" { state["currentStage"].clone() } else { Value::Null },
            "attempt": if state["status"] == "running" { state["attempt"].clone() } else { Value::Null },
            "events": self.events,
            "submissions": state["submissions"]
        });
        result["ledgerSha256"] = json!(self.ledger_sha256.clone());
        if state.get("checkPolicy").is_some() {
            result["assignments"] = state["assignments"].clone();
        }
        if state["version"].as_u64() >= Some(6) {
            result["subject"] = state["subject"].clone();
            result["inputsSha256"] = match &self.inputs {
                InputContext::Current(inputs) => json!(inputs),
                _ => Value::Null,
            };
        }
        if state.get("governor").is_some() {
            result["governor"] = state["governor"].clone();
        }
        result
    }

    pub(crate) fn status_view(&self) -> Result<Value, String> {
        self.drift.clone()?;
        crate::run_value::status(&self.state, Some(self.artifact_current.clone()?))
    }

    pub(crate) fn next_view(&self, loaded: &Loaded) -> Result<Value, String> {
        self.drift.clone()?;
        crate::run::artifact::assert_current(loaded, &self.state)?;
        let state = &self.state;
        Ok(json!({
            "valid": true,
            "runId": state["runId"],
            "status": state["status"],
            "workflow": state["workflow"],
            "currentStage": if state["status"] == "running" { state["currentStage"].clone() } else { Value::Null },
            "attempt": if state["status"] == "running" { state["attempt"].clone() } else { Value::Null },
            "assignments": crate::run::assignment::pending(state),
            "progress": crate::run_progress::project(state)
        }))
    }
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
        "assignments": crate::run::assignment::pending(&state)
        ,"progress": crate::run_progress::project(&state)
    }))
}

fn subject(
    goal: &str,
    plan: &Value,
    config_sha: &str,
    run_id: &str,
    basis_sha256: Option<&str>,
) -> Value {
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
    let mut value = basis;
    if let Some(basis_sha256) = basis_sha256 {
        value["basisSha256"] = json!(basis_sha256);
    }
    let sha = hash::value(&value);
    value["sha256"] = json!(sha);
    value
}

struct ProtocolExtension {
    basis: Option<crate::kernel::basis::Basis>,
    review: crate::kernel::basis::ReviewDecision,
}

fn extension_from_cli(
    basis: Option<&str>,
    review_policy: Option<&str>,
) -> Result<Option<ProtocolExtension>, String> {
    if basis.is_none() && review_policy.is_none() {
        return Ok(None);
    }
    let review_policy = review_policy
        .ok_or("--basis requires an explicit --review-policy required|omitted decision")?;
    let basis = basis
        .map(|value| crate::kernel::basis::parse_basis_text(value, "basis"))
        .transpose()?;
    let review = crate::kernel::basis::review_value(
        review_policy,
        "owner decision supplied through the explicit run interface",
    )?;
    Ok(Some(ProtocolExtension { basis, review }))
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

/// Whether any selected stage agent carries an authorized alternate binding.
#[allow(dead_code)]
fn has_fallback_runtime(plan: &Value) -> bool {
    plan["stages"].as_array().is_some_and(|stages| {
        stages.iter().any(|stage| {
            stage["agents"].as_array().is_some_and(|agents| {
                agents
                    .iter()
                    .any(|agent| agent.get("fallbackRuntime").is_some())
            })
        })
    })
}

/// Build the fallback provenance a submit event carries, if this submission is
/// an operational unavailability or the substitution that answers one.
///
/// The reason is a bounded code chosen by the caller, never parsed from vendor
/// prose, and the binding is recorded as `host-reported`: it is what the caller
/// declared it would run, not something Exitbind observed a provider execute.
fn fallback_provenance(
    assignment: &Value,
    outcome: &str,
    reason: Option<&str>,
    version: u64,
) -> Result<Option<Value>, String> {
    if outcome == "unavailable" {
        let reason = reason.ok_or("--outcome unavailable requires --reason")?;
        if !crate::run::state::FALLBACK_REASONS.contains(&reason) {
            return Err(format!(
                "--reason must be one of: {}",
                crate::run::state::FALLBACK_REASONS.join(", ")
            ));
        }
        // Only a v7 run can carry the reason; an unauthorized run still records
        // the operational failure, it just has no bounded code to attach.
        if version < 7 {
            return Ok(None);
        }
        // The binding that could not execute, so the ledger answers which
        // execution identity was unavailable and not merely that one was.
        return Ok(Some(json!({
            "reason": reason,
            "runtime": crate::run::assignment::binding(&assignment["runtime"]),
            "identitySource": "host-reported",
        })));
    }
    if let Some(substitution) = assignment["substitution"].as_object() {
        // The alternate binding that actually produced this verdict, under the
        // same reviewer contract the primary carried.
        return Ok(Some(json!({
            "reason": substitution["reason"],
            "runtime": substitution["runtime"],
            "identitySource": "host-reported",
            "substituted": true,
        })));
    }
    Ok(None)
}

fn artifact_current(loaded: &Loaded, state: &Value) -> Result<bool, String> {
    match crate::run::artifact::assert_current(loaded, state) {
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
    crate::config::boundary::assert_current(loaded, &state["plan"])?;
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
            assert_selected_agent(loaded, selected)?;
            if let Some(references) = selected.get("memoryReferences") {
                let expected = hash::value(references);
                let current = match crate::memory::selection::resolve(loaded, name) {
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
        crate::evidence::receipt::assert_current(loaded, reference, &state["plan"])?;
    }
    Ok(())
}

/// A selected agent's profile bytes and requested runtime must still match the
/// configuration that produced the plan.  The authorized alternate binding is
/// part of that runtime, so it drifts with the contract it belongs to.
fn assert_selected_agent(loaded: &Loaded, selected: &Value) -> Result<(), String> {
    let name = selected["name"].as_str().unwrap_or("");
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
    if selected
        .get("runtime")
        .is_some_and(|runtime| runtime != &configured.runtime_value())
    {
        return Err(run_error::machine_drift(DriftError::profile(
            name.to_owned(),
            hash::value(&selected["runtime"]),
            hash::value(&configured.runtime_value()),
        )));
    }
    Ok(())
}

pub(crate) fn assert_current_for_receipt(loaded: &Loaded, state: &Value) -> Result<(), String> {
    assert_no_drift(loaded, state)?;
    crate::run::artifact::assert_current(loaded, state)
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

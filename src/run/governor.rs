use super::*;

pub(crate) struct AssignmentIdentity {
    pub(crate) stage: u64,
    pub(crate) attempt: u64,
    pub(crate) agent: String,
    pub(crate) role: String,
    pub(crate) basis_sha256: Option<String>,
    pub(crate) review_decision_sha256: Option<String>,
}

pub(crate) struct RecordedProtection {
    pub(crate) event: Value,
    pub(crate) ledger_sha256: String,
}

pub(crate) const REQUEST_ID_MAX_BYTES: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmissionRefusal {
    GovernorReplanOrEvidence,
}

pub(super) const GOVERNOR_COMPLETION_REFUSAL: &str =
    "governor requires re-plan or evidence before work completion";

pub(crate) fn submission_refusal(error: &str) -> Option<SubmissionRefusal> {
    (error == GOVERNOR_COMPLETION_REFUSAL).then_some(SubmissionRefusal::GovernorReplanOrEvidence)
}

/// Reconstruct the durable identity of a cooperative mutation request from
/// fields that are present in the ledger event.  Keeping this derivation in
/// the run layer lets both the writer and historical reducer reject a
/// rehashed or colliding request instead of trusting a SHA-shaped string.
#[allow(clippy::too_many_arguments)]
pub(crate) fn governor_request_digest(
    run_id: &str,
    stage: u64,
    attempt: u64,
    agent: &str,
    role: &str,
    subject_sha256: &str,
    inputs_sha256: &str,
    assignment_sha256: &str,
    operation: &str,
    request_id: &str,
) -> String {
    hash::value(&json!({
        "runId": run_id,
        "stage": stage,
        "attempt": attempt,
        "agent": agent,
        "role": role,
        "subjectSha256": subject_sha256,
        "inputsSha256": inputs_sha256,
        "assignmentSha256": assignment_sha256,
        "operation": operation,
        "requestId": request_id,
    }))
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

    pub(crate) fn matches(&self, assignment: &Value) -> bool {
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
pub(crate) fn permit_for_assignment(
    loaded: &Loaded,
    ledger: &str,
    work: &str,
    assignment_handle: &str,
    expected: AssignmentIdentity,
    operation: &str,
    request_id: Option<&str>,
) -> Result<Value, String> {
    if operation.trim().is_empty() || operation.len() > 120 || operation.contains('\0') {
        return Err("governor operation must be non-empty and at most 120 bytes".into());
    }
    if request_id.is_some_and(|value| {
        value.trim().is_empty() || value.len() > REQUEST_ID_MAX_BYTES || value.contains('\0')
    }) {
        return Err(format!(
            "governor request-id must be non-empty and at most {REQUEST_ID_MAX_BYTES} bytes"
        ));
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
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
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
        let assignment_sha256 = crate::evidence::hash::text(assignment_handle);
        let request_digest = request_id.map(|request_id| {
            governor_request_digest(
                state["runId"].as_str().unwrap_or_default(),
                assignment["stage"].as_u64().unwrap_or_default(),
                assignment["attempt"].as_u64().unwrap_or_default(),
                assignment["agent"].as_str().unwrap_or_default(),
                assignment["role"].as_str().unwrap_or_default(),
                state["subject"]["sha256"].as_str().unwrap_or_default(),
                inputs.as_str().unwrap_or_default(),
                assignment_sha256.as_str(),
                operation,
                request_id,
            )
        });
        if let (Some(request_id), Some(request_digest)) = (request_id, request_digest.as_deref()) {
            if let Some(previous) = events
                .iter()
                .rev()
                .find(|event| event["action"] == "govern" && event["requestId"] == request_id)
            {
                let same_request = previous["requestDigest"] == request_digest
                    && previous["operation"] == operation
                    && previous["runId"] == state["runId"]
                    && previous["stage"] == assignment["stage"]
                    && previous["attempt"] == assignment["attempt"]
                    && previous["agent"] == assignment["agent"]
                    && previous["role"] == assignment["role"]
                    && previous["subjectSha256"] == state["subject"]["sha256"]
                    && previous["inputsSha256"] == inputs
                    && previous["assignmentSha256"] == assignment_sha256
                    && previous["governorEvent"]["requestId"] == request_id
                    && previous["governorEvent"]["requestDigest"] == request_digest;
                if !same_request {
                    return Err(
                        "request-id conflict: it is already bound to a different governor request"
                            .into(),
                    );
                }
                if previous["governorEvent"]["action"] == "blocked" {
                    return Err("governor durably blocked; exact evidence is required".into());
                }
                return Ok(json!({
                    "valid": true,
                    "allowed": previous["governorEvent"]["action"] == "mutation",
                    "idempotent": true,
                    "event": previous,
                    "runId": state["runId"],
                    "governor": state["governor"],
                }));
            }
        }
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
        if let (Some(request_id), Some(request_digest)) = (request_id, request_digest.as_deref()) {
            governor_payload["requestId"] = json!(request_id);
            governor_payload["requestDigest"] = json!(request_digest);
        }
        let governor_event = crate::context::event(previous_governor.as_ref(), governor_payload);
        let version = state["version"]
            .as_u64()
            .ok_or("run state is missing version")?;
        let mut event_value = json!({
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
            "assignmentSha256": assignment_sha256,
            "operation": operation,
            "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?,
        });
        if let (Some(request_id), Some(request_digest)) = (request_id, request_digest.as_deref()) {
            event_value["requestId"] = json!(request_id);
            event_value["requestDigest"] = json!(request_digest);
        }
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

pub(super) fn canonical_work_for_ledger(path: &run_ledger::LedgerPath) -> Result<String, String> {
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
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
        crate::run::artifact::assert_current(loaded, &state)?;
        if state["governor"]["enabled"] != true {
            return Err("cooperative governor is unavailable on this run".into());
        }
        if state["governor"]["state"] != "replan_required" {
            return Err("material re-plan is not currently required".into());
        }
        super::check_observation::assert_current(loaded, &state)?;
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
                "identityTransition": "carried_mutation_v1",
                "assignmentPacketSha256": crate::evidence::hash::value(&assignment),
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
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
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
        let identity_changed = state["governor"]["currentMutation"].is_object()
            && (state["governor"]["subjectSha256"] != state["subject"]["sha256"]
                || state["governor"]["attempt"] != state["attempt"]);
        let mut governor_payload = json!({
            "action": "evidence",
            "runId": state["runId"],
            "subjectSha256": state["subject"]["sha256"],
            "attempt": state["attempt"],
            "inputSha256": evidence["inputSha256"],
            "evidence": evidence,
        });
        if identity_changed {
            state["governor"]["currentMutation"]
                .as_object()
                .ok_or("evidence identity transition requires a current mutation")?;
            governor_payload["identityTransition"] = json!("carried_mutation_v1");
            governor_payload["lineageSha256"] = state["governor"]["lineageSha256"].clone();
        }
        let governor_event = crate::context::event(previous_governor.as_ref(), governor_payload);
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
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
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
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
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

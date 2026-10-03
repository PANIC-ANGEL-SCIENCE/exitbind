use super::*;

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
    work: Option<String>,
    inputs: InputContext,
    ledger_sha256: String,
    drift: Result<(), String>,
    artifact_drift: Option<Value>,
    artifact_current: Result<bool, String>,
}

impl RunSnapshot {
    pub(crate) fn capture(loaded: &Loaded, ledger: &str) -> Result<Self, String> {
        let (path, events, source) =
            load_at(loaded, &ledger_path(&loaded.state_root, ledger, false)?)?;
        let mut snapshot = Self::from_events(loaded, &events, &source)?;
        snapshot.work = canonical_work_for_ledger(&path).ok();
        Ok(snapshot)
    }

    pub(crate) fn from_events(
        loaded: &Loaded,
        events: &[Value],
        source: &str,
    ) -> Result<Self, String> {
        if events.is_empty() {
            return Err("run ledger has no events".into());
        }
        let (state, inputs) = reduce_with_inputs(loaded, events)?;
        predecessor(loaded, &events[0])?;
        let drift = view_drift(loaded, &state);
        if let Err(error) = &drift {
            if drift_value(error).is_none() {
                return Err(error.clone());
            }
        }
        let artifact_drift = crate::run::artifact::drift_warning(loaded, &state)?;
        let artifact_current = artifact_current(loaded, &state);
        Ok(Self {
            events: events.to_vec(),
            state,
            work: None,
            inputs,
            ledger_sha256: hash::bytes(source.as_bytes()),
            drift,
            artifact_drift,
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
            "work": self.work,
            "status": state["status"],
            "currentStage": if state["status"] == "running" { state["currentStage"].clone() } else { Value::Null },
            "attempt": if state["status"] == "running" { state["attempt"].clone() } else { Value::Null },
            "events": self.events,
            "submissions": state["submissions"]
        });
        result["warnings"] = self
            .drift
            .as_ref()
            .err()
            .and_then(|error| drift_value(error))
            .map_or_else(
                || {
                    self.artifact_drift
                        .clone()
                        .map_or_else(|| json!([]), |warning| json!([warning]))
                },
                |warning| json!([warning]),
            );
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
        let mut status =
            crate::run_value::status(&self.state, Some(self.artifact_current.clone()?))?;
        status["warnings"] = self
            .drift
            .as_ref()
            .err()
            .and_then(|error| drift_value(error))
            .map_or_else(
                || {
                    self.artifact_drift
                        .clone()
                        .map_or_else(|| json!([]), |warning| json!([warning]))
                },
                |warning| json!([warning]),
            );
        Ok(status)
    }

    pub(crate) fn next_view(&self, loaded: &Loaded) -> Result<Value, String> {
        assert_exact_receipt(loaded, &self.state)?;
        let state = &self.state;
        let next = json!({
            "valid": true,
            "runId": state["runId"],
            "status": state["status"],
            "workflow": state["workflow"],
            "work": self.work,
            "configSha256": state["configSha256"],
            "projectIdentity": crate::host::assignment_context::project_identity(loaded)?,
            "currentStage": if state["status"] == "running" {
                state["currentStage"].clone()
            } else {
                Value::Null
            },
            "attempt": if state["status"] == "running" {
                state["attempt"].clone()
            } else {
                Value::Null
            },
            "assignments": crate::run::assignment::pending(state),
            "progress": crate::run_progress::project_with_artifact(
                state,
                Some(self.artifact_current.clone()?),
            ),
            "warnings": self
                .drift
                .as_ref()
                .err()
                .and_then(|error| drift_value(error))
                .map_or_else(
                    || self.artifact_drift.clone().map_or_else(|| json!([]), |warning| json!([warning])),
                    |warning| json!([warning]),
                )
        });
        Ok(next)
    }
}

pub(super) fn live_inputs(state: &Value) -> Result<Value, String> {
    state
        .get("inputsSha256")
        .filter(|value| value.is_string())
        .cloned()
        .ok_or_else(|| "tested inputs cannot be established; no evidence was recorded".into())
}

pub(crate) fn result(events: &[Value]) -> Result<Value, String> {
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

pub(crate) fn subject(
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

pub(super) struct ProtocolExtension {
    pub(super) basis: Option<crate::kernel::basis::Basis>,
    pub(super) review: crate::kernel::basis::ReviewDecision,
}

pub(super) fn extension_from_cli(
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

pub(super) fn has_worker_stage(plan: &Value) -> bool {
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
pub(super) fn has_fallback_runtime(plan: &Value) -> bool {
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
pub(super) fn fallback_provenance(
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

pub(super) fn artifact_current(loaded: &Loaded, state: &Value) -> Result<bool, String> {
    match crate::run::artifact::assert_current(loaded, state) {
        Ok(()) => Ok(true),
        Err(error) if error.starts_with("artifact drift detected:") => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn selected_plan(mut plan: Value) -> Result<Value, String> {
    let object = plan
        .as_object_mut()
        .ok_or("workflow plan must be an object")?;
    object.remove("goal");
    object.remove("notice");
    Ok(plan)
}

pub(crate) fn assert_no_drift(loaded: &Loaded, state: &Value) -> Result<(), String> {
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
                let current = match crate::memory::selection::resolve_for_task(
                    loaded,
                    name,
                    state["goal"].as_str(),
                ) {
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

pub(crate) fn drift_value(error: &str) -> Option<Value> {
    error
        .strip_prefix(run_error::DRIFT_PREFIX)
        .or_else(|| error.strip_prefix(run_error::LEGACY_DRIFT_PREFIX))
        .and_then(|value| serde_json::from_str(value).ok())
}

pub(crate) fn drift_warning(loaded: &Loaded, state: &Value) -> Result<Option<Value>, String> {
    match view_drift(loaded, state) {
        Ok(()) => Ok(None),
        Err(error) => drift_value(&error).map(Some).ok_or(error),
    }
}

pub(super) fn view_drift(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if state["version"] == 2 {
        if let Some(reference) = state.get("harnessReceipt") {
            if crate::evidence::receipt::is_missing_historical_reference(loaded, reference)? {
                let expected = reference["sha256"].as_str().unwrap_or_default().to_owned();
                return Err(run_error::machine_drift(DriftError::harness_receipt(
                    expected,
                    String::new(),
                )));
            }
        }
    }
    assert_exact_receipt(loaded, state)?;
    assert_no_drift(loaded, state)
}

pub(super) fn assert_exact_receipt(loaded: &Loaded, state: &Value) -> Result<(), String> {
    if let Some(reference) = state.get("harnessReceipt") {
        crate::evidence::receipt::assert_exact_reference(loaded, reference)?;
    }
    Ok(())
}

pub(super) fn action_drift_warning(
    loaded: &Loaded,
    state: &Value,
) -> Result<Option<Value>, String> {
    assert_exact_receipt(loaded, state)?;
    if let Some(warning) = drift_warning(loaded, state)? {
        return Ok(Some(warning));
    }
    input_drift_warning(loaded, state)
}

pub(super) fn input_drift_warning(loaded: &Loaded, state: &Value) -> Result<Option<Value>, String> {
    input_drift_warning_for(loaded, state, None)
}

pub(crate) fn input_drift_warning_for(
    loaded: &Loaded,
    state: &Value,
    expected_override: Option<&str>,
) -> Result<Option<Value>, String> {
    if state["version"].as_u64() < Some(6) || state["status"] != "running" {
        return Ok(None);
    }
    let Some(expected) = expected_override.or_else(|| state["inputsSha256"].as_str()) else {
        return Ok(None);
    };
    let current = crate::run::inputs::fingerprint(loaded)?;
    if current == expected {
        return Ok(None);
    }
    Ok(Some(json!({
        "error": "tested inputs drift detected after run start",
        "classification": "input_drift",
        "expectedInputsSha256": expected,
        "currentInputsSha256": current,
    })))
}

pub(crate) fn warn_drift(warning: Option<&Value>) {
    if let Some(classification) = warning.and_then(|value| value["classification"].as_str()) {
        eprintln!("warning: {classification} detected; continuing with the recorded run");
    }
}

/// A selected agent's profile bytes and requested runtime must still match the
/// configuration that produced the plan.  The authorized alternate binding is
/// part of that runtime, so it drifts with the contract it belongs to.
pub(super) fn assert_selected_agent(loaded: &Loaded, selected: &Value) -> Result<(), String> {
    let name = selected["name"].as_str().unwrap_or("");
    let expected_profile = selected["profileSha256"].as_str().unwrap_or("").to_owned();
    let unavailable = || {
        run_error::machine_drift(DriftError::profile(
            name.to_owned(),
            expected_profile.clone(),
            String::new(),
        ))
    };
    let configured = loaded.agent(name).ok_or_else(unavailable)?;
    let path =
        config::file(&loaded.control_root, &configured.profile).map_err(|_| unavailable())?;
    let profile_sha = hash::text(&fs::read_to_string(&path).map_err(|_| unavailable())?);
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
    let drift = action_drift_warning(loaded, state)?;
    warn_drift(drift.as_ref());
    crate::run::artifact::assert_current(loaded, state)
}

pub(super) fn nondecreasing(previous: &Value) -> Result<String, String> {
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

use super::*;

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
            value["recoveryProtocol"] = json!(crate::run::state::RECOVERY_PROTOCOL_VERSION);
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
                "replanBinding": "assignment_packet_v1",
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
        let drift = action_drift_warning(loaded, &state)?;
        warn_drift(drift.as_ref());
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

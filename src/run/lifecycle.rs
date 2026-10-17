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
    start_with_policy_and_pre_return(
        loaded,
        workflow,
        goal,
        ledger,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_with_policy_and_pre_return(
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
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
) -> Result<Value, String> {
    start_with_association(
        loaded,
        workflow,
        goal,
        ledger,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
        None,
        None,
    )
}

/// Only the continuation owner can supply this verified, non-deserializable
/// association; the ordinary Run start interface cannot attach a marker.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start_with_continuation(
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
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
    association: &crate::project::continuation::control::VerifiedAssociation,
) -> Result<Value, String> {
    start_with_association(
        loaded,
        workflow,
        goal,
        ledger,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
        Some(association),
        None,
    )
}

/// Start a run using verification authority that was constructed by the
/// quarantine admission protocol. The ordinary Run interface cannot attach
/// this marker.
pub(crate) fn start_with_quarantine_verification(
    loaded: &Loaded,
    verified: &super::quarantine::admission::Verified,
) -> Result<Value, String> {
    start_with_association(
        loaded,
        verified.workflow(),
        verified.goal(),
        verified.ledger(),
        None,
        None,
        Some(verified.command()),
        Some("local_report"),
        None,
        None,
        None,
        Some(verified.basis()),
        Some("required"),
        None,
        None,
        None,
        None,
        Some(verified),
    )
}

#[allow(clippy::too_many_arguments)]
fn start_with_association(
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
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
    association: Option<&crate::project::continuation::control::VerifiedAssociation>,
    quarantine: Option<&super::quarantine::admission::Verified>,
) -> Result<Value, String> {
    if workflow.trim().is_empty() {
        return Err("workflow is required".into());
    }
    if goal.trim().is_empty() {
        return Err("--goal requires a non-empty value".into());
    }
    if let Some(association) = association {
        crate::project::continuation::runtime::validate_start_association(
            loaded,
            association,
            ledger,
        )?;
    }
    let mut plan = crate::config::boundary::apply(
        loaded,
        selected_plan(envelope::plan(loaded, workflow, goal)?)?,
        boundary,
    )?;
    let check_policy = crate::run_value::policy_from_cli(check_command, proof_origin)?;
    let pre_return_policy = crate::run_value::PreReturnPolicy::from_cli(
        loaded,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
    )?;
    crate::project::architecture::bind_plan(loaded, &mut plan)?;
    let preservation = crate::run_value::preservation_from_cli(
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
    )?;
    let verification_checker_sources = if loaded.verification_preparation.is_some() {
        Some(crate::run_value::verification_checker_sources(
            loaded,
            check_policy.as_ref(),
            preservation.as_ref(),
        )?)
    } else {
        None
    };
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
    if pre_return_policy.is_some() && check_policy.is_none() {
        return Err("--pre-return-command requires --check-command".into());
    }
    if pre_return_policy.is_some() && !has_worker_stage(&plan) {
        return Err("pre-return policy requires a workflow with at least one worker stage".into());
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
        if let Some(verified) = quarantine {
            if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source
                || hash::text(&loaded.source) != verified.marker()["verification"]["configSha256"]
                || crate::run::inputs::fingerprint(loaded)?
                    != verified.marker()["verification"]["inputsSha256"]
            {
                return Err(
                    "quarantine verification inputs or configuration changed before start".into(),
                );
            }
        }
        let continuation = association
            .map(|association| {
                crate::project::continuation::runtime::validate_start_association(
                    loaded,
                    association,
                    ledger,
                )
            })
            .transpose()?;
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
        if let Some(marker) = continuation {
            value["projectContinuation"] = marker;
            crate::run::continuation_protocol::validate_start(&value, 1)?;
        }
        if let Some(reference) = reference {
            value["harnessReceipt"] = reference;
        }
        if let Some(policy) = &check_policy {
            value["checkPolicy"] = policy.value();
            if loaded.verification_preparation.is_some() {
                crate::run::verification_preparation::add_start_fields(
                    &mut value,
                    Some(verification_checker_sources.as_deref().unwrap_or_default()),
                );
            }
        }
        if let Some(policy) = &pre_return_policy {
            value["preReturnPolicy"] = policy.value();
        }
        if let Some(preservation) = &preservation {
            value["preservation"] = preservation.value();
        }
        if let Some(extension) = &extension {
            value["basisProtocol"] = json!(crate::kernel::basis::PROTOCOL_VERSION);
            if check_policy.is_some() && quarantine.is_none() {
                value["recoveryProtocol"] = json!(crate::run::state::RECOVERY_PROTOCOL_VERSION);
            }
            if check_policy.is_some() {
                value["checkObservationProtocol"] = json!(crate::run::check_observation::PROTOCOL);
            }
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
        if let Some(verified) = quarantine {
            value["quarantineVerification"] = verified.marker().clone();
            crate::run::quarantine::validate_start(&value, 1)?;
        }
        if version >= 6 && quarantine.is_none() {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_intent_sha256_with_pre_return(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
    check_command: Option<&str>,
    proof_origin: Option<&str>,
    preserve_requirement: Option<&str>,
    preservation_check_command: Option<&str>,
    preservation_proof_origin: Option<&str>,
    basis: Option<&str>,
    review_policy: Option<&str>,
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
) -> Result<String, String> {
    start_intent_with_association(
        loaded,
        workflow,
        goal,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
        None,
    )
}

/// Preview binds the exact association without reading or reserving its claim.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start_intent_sha256_with_continuation(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
    check_command: Option<&str>,
    proof_origin: Option<&str>,
    preserve_requirement: Option<&str>,
    preservation_check_command: Option<&str>,
    preservation_proof_origin: Option<&str>,
    basis: Option<&str>,
    review_policy: Option<&str>,
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
    association: &crate::project::continuation::control::VerifiedAssociation,
) -> Result<String, String> {
    start_intent_with_association(
        loaded,
        workflow,
        goal,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
        Some(association),
    )
}

#[allow(clippy::too_many_arguments)]
fn start_intent_with_association(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
    check_command: Option<&str>,
    proof_origin: Option<&str>,
    preserve_requirement: Option<&str>,
    preservation_check_command: Option<&str>,
    preservation_proof_origin: Option<&str>,
    basis: Option<&str>,
    review_policy: Option<&str>,
    pre_return_command: Option<&str>,
    pre_return_timeout_ms: Option<&str>,
    pre_return_capture_bytes: Option<&str>,
    association: Option<&crate::project::continuation::control::VerifiedAssociation>,
) -> Result<String, String> {
    if workflow.trim().is_empty() {
        return Err("workflow is required".into());
    }
    if goal.trim().is_empty() {
        return Err("--goal requires a non-empty value".into());
    }
    let mut plan = crate::config::boundary::apply(
        loaded,
        selected_plan(envelope::plan(loaded, workflow, goal)?)?,
        boundary,
    )?;
    let check_policy = crate::run_value::policy_from_cli(check_command, proof_origin)?;
    let pre_return_policy = crate::run_value::PreReturnPolicy::from_cli(
        loaded,
        pre_return_command,
        pre_return_timeout_ms,
        pre_return_capture_bytes,
    )?;
    crate::project::architecture::bind_plan(loaded, &mut plan)?;
    let preservation = crate::run_value::preservation_from_cli(
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
    )?;
    let verification_checker_sources = if loaded.verification_preparation.is_some() {
        Some(crate::run_value::verification_checker_sources(
            loaded,
            check_policy.as_ref(),
            preservation.as_ref(),
        )?)
    } else {
        None
    };
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
    if pre_return_policy.is_some() && check_policy.is_none() {
        return Err("--pre-return-command requires --check-command".into());
    }
    if pre_return_policy.is_some() && !has_worker_stage(&plan) {
        return Err("pre-return policy requires a workflow with at least one worker stage".into());
    }
    if preservation.is_some() && !crate::producer::exitbind_surface() {
        return Err("preservation requirements require Exitbind v6 runs".into());
    }
    let harness_reference = harness_receipt
        .map(|receipt| crate::evidence::receipt::for_run(loaded, receipt, &plan))
        .transpose()?;
    let version = if crate::producer::exitbind_surface() {
        8
    } else if check_policy.is_some() {
        4
    } else if harness_reference.is_some() {
        2
    } else {
        1
    };
    let mut value = json!({
        "version": version,
        "kind": "run",
        "producer": crate::producer::evidence_for_version(version),
        "action": "start",
        "workflow": workflow,
        "goal": goal,
        "configSha256": hash::text(&loaded.source),
        "plan": plan,
    });
    if let Some(association) = association {
        value["projectContinuation"] =
            serde_json::to_value(association.association()).map_err(|error| error.to_string())?;
        crate::run::continuation_protocol::validate_start(&value, 1)?;
    }
    if let Some(reference) = harness_reference {
        value["harnessReceipt"] = reference;
    }
    if let Some(policy) = &check_policy {
        value["checkPolicy"] = policy.value();
        if loaded.verification_preparation.is_some() {
            crate::run::verification_preparation::add_start_fields(
                &mut value,
                Some(verification_checker_sources.as_deref().unwrap_or_default()),
            );
        }
    }
    if let Some(policy) = &pre_return_policy {
        value["preReturnPolicy"] = policy.value();
    }
    if let Some(preservation) = &preservation {
        value["preservation"] = preservation.value();
    }
    if let Some(extension) = &extension {
        value["basisProtocol"] = json!(crate::kernel::basis::PROTOCOL_VERSION);
        if check_policy.is_some() {
            value["recoveryProtocol"] = json!(crate::run::state::RECOVERY_PROTOCOL_VERSION);
            value["checkObservationProtocol"] = json!(crate::run::check_observation::PROTOCOL);
        }
        if let Some(basis) = &extension.basis {
            value["basis"] = basis.value();
        }
        value["reviewPolicy"] = extension.review.value();
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
    Ok(hash::value(&value))
}

/// Hash the same stable field projection from a validated start event.
pub(crate) fn start_event_intent_sha256(event: &Value) -> Result<String, String> {
    const FIELDS: &[&str] = &[
        "version",
        "kind",
        "producer",
        "action",
        "workflow",
        "goal",
        "configSha256",
        "plan",
        "harnessReceipt",
        "checkPolicy",
        "preReturnPolicy",
        "preservation",
        "basisProtocol",
        "recoveryProtocol",
        "checkObservationProtocol",
        "basis",
        "reviewPolicy",
        "governor",
        "projectContinuation",
        "quarantineVerification",
    ];
    if event["action"] != "start" {
        return Err("Work ledger does not begin with a start event".into());
    }
    let mut projection = serde_json::Map::new();
    for field in FIELDS
        .iter()
        .chain(crate::run::verification_preparation::EVENT_FIELDS.iter())
    {
        if let Some(value) = event.get(*field) {
            projection.insert((*field).to_owned(), value.clone());
        }
    }
    Ok(hash::value(&Value::Object(projection)))
}

pub(crate) fn quarantine_start_intent_sha256(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    command: &str,
    basis: &str,
    marker: &Value,
) -> Result<String, String> {
    let mut plan = crate::config::boundary::apply(
        loaded,
        selected_plan(envelope::plan(loaded, workflow, goal)?)?,
        None,
    )?;
    crate::project::architecture::bind_plan(loaded, &mut plan)?;
    let policy = crate::run_value::policy_from_cli(Some(command), Some("local_report"))?
        .ok_or("quarantine verification check policy is missing")?;
    let extension = extension_from_cli(Some(basis), Some("required"))?
        .ok_or("quarantine verification basis is missing")?;
    let mut value = json!({
        "version":8,"kind":"run","producer":crate::producer::evidence_for_version(8),
        "action":"start","workflow":workflow,"goal":goal,
        "configSha256":hash::text(&loaded.source),"plan":plan,
        "checkPolicy":policy.value(),"basisProtocol":crate::kernel::basis::PROTOCOL_VERSION,
        "checkObservationProtocol":crate::run::check_observation::PROTOCOL,
        "basis":extension.basis.ok_or("quarantine verification basis is missing")?.value(),
        "reviewPolicy":extension.review.value(),"quarantineVerification":marker
    });
    let verification_checker_sources = if loaded.verification_preparation.is_some() {
        Some(crate::run_value::verification_checker_sources(
            loaded,
            Some(&policy),
            None,
        )?)
    } else {
        None
    };
    if loaded.verification_preparation.is_some() {
        crate::run::verification_preparation::add_start_fields(
            &mut value,
            Some(verification_checker_sources.as_deref().unwrap_or_default()),
        );
    }
    start_event_intent_sha256(&value)
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
        crate::run::quarantine::refuse_mutation(&state, "review-policy change")?;
        crate::project::continuation::runtime::guard_review_policy(loaded, &state, decision)?;
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
        let ticket = super::continuation_effect::begin_event(
            loaded,
            &state,
            crate::project::continuation::control::Kind::Effect,
            &event,
        )?;
        append(&path, &event, false, &source)?;
        super::continuation_effect::complete(loaded, &ticket, true, true)?;
        Ok(
            json!({"valid":true,"event":event,"status":next_state["status"],"assignments":crate::run::assignment::pending(&next_state)}),
        )
    })
}

pub fn next(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    RunSnapshot::capture(loaded, ledger)?.next_view(loaded)
}

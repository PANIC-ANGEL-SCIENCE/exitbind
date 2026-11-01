use super::*;

mod check_replacement;
mod read;
mod successor_protocol;
pub use self::check_replacement::{replace_check_contract, replace_pre_return_contract};
pub use self::read::{explain, inspect, report, status};
pub(crate) use self::read::{human_explain, human_status, terminal_display};
use self::successor_protocol::attach_successor_work;

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
    supersede_with_policy_and_recovery(
        loaded,
        old_ledger,
        workflow,
        goal,
        new_ledger,
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
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn supersede_with_owner_recovery(
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
    owner_recovery: Option<&str>,
) -> Result<Value, String> {
    supersede_with_policy_and_recovery(
        loaded,
        old_ledger,
        workflow,
        goal,
        new_ledger,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        owner_recovery,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn supersede_with_policy_and_recovery(
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
    owner_recovery: Option<&str>,
    expected_head_event_sha256: Option<&str>,
    check_replacement: Option<Value>,
    pre_return_policy_override: Option<crate::run_value::PreReturnPolicy>,
) -> Result<Value, String> {
    supersede_with_hook(
        SupersedeRequest {
            loaded,
            old_ledger,
            workflow,
            goal,
            new_ledger,
            boundary,
            harness_receipt,
            check_command,
            proof_origin,
            preserve_requirement,
            preservation_check_command,
            preservation_proof_origin,
            basis,
            review_policy,
            owner_recovery,
            expected_head_event_sha256,
            check_replacement,
            pre_return_policy_override,
        },
        None,
    )
}

type BeforeClaim<'a> = dyn FnMut(&Value, &Value, bool, bool) -> Result<String, String> + 'a;

/// Native admission is performed under the predecessor lock before its claim.
pub(crate) fn supersede_with_native_admission(
    loaded: &Loaded,
    old_ledger: &str,
    workflow: &str,
    goal: &str,
    new_ledger: &str,
    before_claim: &mut BeforeClaim<'_>,
) -> Result<Value, String> {
    supersede_with_hook(
        SupersedeRequest {
            loaded,
            old_ledger,
            workflow,
            goal,
            new_ledger,
            boundary: None,
            harness_receipt: None,
            check_command: None,
            proof_origin: None,
            preserve_requirement: None,
            preservation_check_command: None,
            preservation_proof_origin: None,
            basis: None,
            review_policy: None,
            owner_recovery: None,
            expected_head_event_sha256: None,
            check_replacement: None,
            pre_return_policy_override: None,
        },
        Some(before_claim),
    )
}

struct SupersedeRequest<'a> {
    loaded: &'a Loaded,
    old_ledger: &'a str,
    workflow: &'a str,
    goal: &'a str,
    new_ledger: &'a str,
    boundary: Option<&'a str>,
    harness_receipt: Option<&'a str>,
    check_command: Option<&'a str>,
    proof_origin: Option<&'a str>,
    preserve_requirement: Option<&'a str>,
    preservation_check_command: Option<&'a str>,
    preservation_proof_origin: Option<&'a str>,
    basis: Option<&'a str>,
    review_policy: Option<&'a str>,
    owner_recovery: Option<&'a str>,
    expected_head_event_sha256: Option<&'a str>,
    check_replacement: Option<Value>,
    pre_return_policy_override: Option<crate::run_value::PreReturnPolicy>,
}

fn supersede_with_hook(
    request: SupersedeRequest<'_>,
    before_claim: Option<&mut BeforeClaim<'_>>,
) -> Result<Value, String> {
    let SupersedeRequest {
        loaded,
        old_ledger,
        workflow,
        goal,
        new_ledger,
        boundary,
        harness_receipt,
        check_command,
        proof_origin,
        preserve_requirement,
        preservation_check_command,
        preservation_proof_origin,
        basis,
        review_policy,
        owner_recovery,
        expected_head_event_sha256,
        mut check_replacement,
        pre_return_policy_override,
    } = request;
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
        crate::run::quarantine::refuse_mutation(&old_state, "supersession or replacement")?;
        if check_replacement.is_some()
            && old_events
                .last()
                .and_then(|event| event["eventSha256"].as_str())
                != expected_head_event_sha256
        {
            return Err(
                "predecessor changed after replacement preview; refresh current Work detail".into(),
            );
        }
        if check_replacement.is_some()
            && (old_events[0]["workflow"] != workflow || old_events[0]["goal"] != goal)
        {
            return Err(
                "check replacement must preserve the predecessor workflow and human Goal".into(),
            );
        }
        if crate::run::state::check_observation::unresolved(&old_state) {
            return Err("supersession refused unresolved check effects; no successor may retry uncertain execution".into());
        }
        if !matches!(old_state["status"].as_str(), Some("running" | "blocked")) {
            return Err("only a running or blocked run can be superseded".into());
        }
        if fs::read_to_string(&loaded.path).map_err(|error| error.to_string())? != loaded.source {
            return Err("configuration changed while superseding; reload configuration".into());
        }
        let plan = if check_replacement.is_some() {
            crate::run::check_replacement::frozen_plan(loaded, &old_events[0])?
        } else if before_claim.is_some() {
            crate::run::check_replacement::frozen_plan_for(
                loaded,
                &old_events[0],
                "native Work supersession",
            )?
        } else {
            let mut plan = crate::config::boundary::apply(
                loaded,
                selected_plan(envelope::plan(loaded, workflow, goal)?)?,
                boundary,
            )?;
            crate::project::architecture::bind_plan(loaded, &mut plan)?;
            plan
        };
        let extension = extension_from_cli(basis, review_policy)?;
        if owner_recovery.is_some() && extension.is_some() {
            return Err("owner recovery must preserve the predecessor basis and review decision; omit --basis and --review-policy".into());
        }
        let old_policy = old_state
            .get("checkPolicy")
            .map(|policy| crate::run_value::policy_from_value(policy, 0))
            .transpose()
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        let requested_policy = crate::run_value::policy_from_cli(check_command, proof_origin)?;
        let mut pre_return_policy = old_state.get("preReturnPolicy").cloned();
        if let Some(policy) = &pre_return_policy {
            crate::run_value::pre_return_policy_from_value(policy, 0)
                .map_err(|error| error.replacen("line 0", "state", 1))?;
        }
        if let Some(replacement) = check_replacement.as_mut() {
            check_replacement::bind_marker(
                loaded,
                &old,
                &old_events,
                &old_state,
                replacement,
                requested_policy.as_ref(),
                &mut pre_return_policy,
                pre_return_policy_override.as_ref(),
            )?;
        }
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
                if old != requested && check_replacement.is_none() {
                    return Err(
                        "successor check policy differs from predecessor; choose an explicit new checked run"
                            .into(),
                    );
                }
                Some(if check_replacement.is_some() {
                    requested
                } else {
                    old
                })
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
        let verification_checker_sources = if loaded.verification_preparation.is_some() {
            Some(crate::run_value::verification_checker_sources(
                loaded,
                check_policy.as_ref(),
                preservation.as_ref(),
            )?)
        } else {
            None
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
        let carry_accounting = if let Some(recovery_path) = owner_recovery {
            if old_state["governor"]["enabled"] != true {
                return Err("owner recovery requires a governed predecessor".into());
            }
            let old_rel = config::rel(&loaded.state_root, &old.expected)?;
            Some(crate::run::carry::make_recovery(
                loaded,
                &old_state,
                &old_rel,
                &old_source,
                &old_events,
                goal,
                recovery_path,
            )?)
        } else if old_state["governor"]["enabled"] == true
            && old_state["subject"]["goalSha256"] == hash::text(goal)
        {
            let old_rel = config::rel(&loaded.state_root, &old.expected)?;
            Some(crate::run::carry::make(
                &old_state,
                &old_rel,
                &old_source,
                &old_events,
                goal,
            )?)
        } else {
            None
        };
        let old_rel = config::rel(&loaded.state_root, &old.expected)?;
        let new_rel = config::rel(&loaded.state_root, &new.expected)?;
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
            "newLedgerPath": new_rel.clone(),
            "workflow": workflow,
            "goalSha256": hash::text(goal),
            "configSha256": config_sha,
            "newRunId": run_id,
            "timestamp": timestamp
        });
        let carry_protocol = carry_accounting
            .as_ref()
            .map(|carry| {
                carry["protocol"]
                    .as_u64()
                    .ok_or("governor carry has no supported protocol")
            })
            .transpose()?;
        let carry_sha = carry_accounting.as_ref().map(crate::evidence::hash::value);
        let mut wanted_claim = wanted_claim;
        if old_state
            .get(crate::run::guidance_continuity::INPUT_CONTRACT_FIELD)
            .is_some()
        {
            wanted_claim[crate::run::guidance_continuity::INPUT_CONTRACT_FIELD] =
                old_state[crate::run::guidance_continuity::INPUT_CONTRACT_FIELD].clone();
        }
        if let Some(replacement) = &check_replacement {
            wanted_claim["checkReplacement"] = replacement.clone();
        }
        if let Some(policy) = &pre_return_policy {
            wanted_claim["preReturnPolicy"] = policy.clone();
        }
        if check_policy.is_some() && loaded.verification_preparation.is_some() {
            crate::run::verification_preparation::add_start_fields(
                &mut wanted_claim,
                Some(verification_checker_sources.as_deref().unwrap_or_default()),
            );
        }
        if let (Some(protocol), Some(sha)) = (carry_protocol, carry_sha.as_ref()) {
            wanted_claim["carryProtocol"] = json!(protocol);
            wanted_claim["governorCarrySha256"] = json!(sha);
        }
        let claim_path = claim_path(&old);
        if new.path.exists() && !claim_path.exists() {
            return Err("successor ledger exists without a matching predecessor claim".into());
        }
        if !claim_path.exists() && old_state.get("projectContinuation").is_some() {
            // Fresh associated retirement is a new controlled effect. Check
            // the existing Work's live mandate before persisting the durable
            // claim; a matching saved claim below remains replayable after a
            // later stop or closure.
            crate::project::continuation::runtime::guard(loaded, &old_state)?;
            let predecessor = crate::work::locator_for_ledger(&old_rel)
                .ok_or("associated predecessor is not a canonical Work")?;
            crate::project::continuation::runtime::ensure_goal_linked_for_retirement(
                loaded,
                &old_state,
                &predecessor,
                old_events.first().ok_or("predecessor start is missing")?,
            )?;
        }
        // Both the pre-claim admission and committed start use this builder.
        let build_event = |claim_value: &Value| -> Result<Value, String> {
            let successor_run_id = claim_value["newRunId"]
                .as_str()
                .ok_or("supersession claim has no successor run id")?;
            let supersedes = json!({
                "ledgerPath": claim_value["oldLedgerPath"],
                "ledgerSha256": claim_value["oldLedgerSha256"],
                "runId": claim_value["oldRunId"],
                "headEventSha256": claim_value["oldHeadEventSha256"],
                "configSha256": claim_value["oldConfigSha256"]
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
                "runId": successor_run_id,
                "workflow": workflow,
                "goal": goal,
                "configSha256": config_sha,
                "plan": plan,
                "previousEventSha256": null,
                "timestamp": claim_value["timestamp"],
                "supersedes": supersedes
            });
            if let (Some(carry), Some(protocol), Some(sha)) =
                (carry_accounting.clone(), carry_protocol, carry_sha.clone())
            {
                event_value["carryProtocol"] = json!(protocol);
                event_value["governorCarry"] = carry;
                event_value["governorCarrySha256"] = json!(sha);
            }
            if let Some(reference) = harness_reference.clone() {
                event_value["harnessReceipt"] = reference;
            }
            if let Some(policy) = &check_policy {
                event_value["checkPolicy"] = policy.value();
                if loaded.verification_preparation.is_some() {
                    crate::run::verification_preparation::add_start_fields(
                        &mut event_value,
                        Some(verification_checker_sources.as_deref().unwrap_or_default()),
                    );
                }
            }
            if let Some(policy) = &pre_return_policy {
                event_value["preReturnPolicy"] = policy.clone();
            }
            if let Some(replacement) = &check_replacement {
                event_value["checkReplacement"] = replacement.clone();
            }
            if let Some(preservation) = &preservation {
                event_value["preservation"] = preservation.value();
            }
            if let Some(extension) = &extension {
                event_value["basisProtocol"] = json!(crate::kernel::basis::PROTOCOL_VERSION);
                if check_policy.is_some() {
                    event_value["recoveryProtocol"] =
                        json!(crate::run::state::RECOVERY_PROTOCOL_VERSION);
                    crate::run::state::observation_novelty::add_start_marker(&mut event_value);
                    event_value["checkObservationProtocol"] =
                        json!(crate::run::check_observation::PROTOCOL);
                }
                if let Some(basis) = &extension.basis {
                    event_value["basis"] = basis.value();
                }
                event_value["reviewPolicy"] = extension.review.value();
            }
            if version >= 5 {
                let basis_sha = extension
                    .as_ref()
                    .and_then(|x| x.basis.as_ref())
                    .map(|x| x.sha256.as_str())
                    .or_else(|| carry_protocol.and_then(|_| old_state["basis"]["sha256"].as_str()))
                    .or_else(|| {
                        check_replacement
                            .as_ref()
                            .and_then(|_| old_state["basis"]["sha256"].as_str())
                    });
                event_value["subject"] = subject(
                    goal,
                    &event_value["plan"],
                    &config_sha,
                    successor_run_id,
                    basis_sha,
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
            if carry_protocol.is_some() {
                event_value["governor"] = old_events[0]["governor"].clone();
                if old_state.get("basisProtocol").is_some() {
                    event_value["basisProtocol"] = old_state["basisProtocol"].clone();
                    if event_value.get("basis").is_none() {
                        if let Some(basis) = old_state.get("basis") {
                            event_value["basis"] = basis.clone();
                        }
                    }
                }
            }
            if extension.is_none() && (carry_protocol.is_some() || check_replacement.is_some()) {
                if let Some(review) = old_state.get("reviewPolicy") {
                    event_value["reviewPolicy"] = review.clone();
                }
                if let Some(protocol) = old_state.get("recoveryProtocol") {
                    event_value["recoveryProtocol"] = protocol.clone();
                }
                if let Some(protocol) = old_state.get("observationNoveltyProtocol") {
                    event_value["observationNoveltyProtocol"] = protocol.clone();
                }
                if let Some(protocol) = old_state.get("checkObservationProtocol") {
                    event_value["checkObservationProtocol"] = protocol.clone();
                }
            }
            // Mark before reserving so replay rebuilds the same first event.
            if version == 8 {
                crate::run::guidance_continuity::add_start_marker(&mut event_value, loaded);
                if crate::run::guidance_continuity::enabled(&old_state)
                    && old_state
                        .get(crate::run::guidance_continuity::INPUT_CONTRACT_FIELD)
                        .is_none()
                {
                    event_value
                        .as_object_mut()
                        .unwrap()
                        .remove(crate::run::guidance_continuity::INPUT_CONTRACT_FIELD);
                }
            }
            Ok(event_value)
        };
        let expected_admission = if let Some(callback) = before_claim {
            let existing = crate::run::ledger::valid_claim(&claim_path)?;
            if let Some(value) = &existing {
                crate::run::ledger::assert_claim_request(value, &wanted_claim)?;
            }
            let mut prospective = build_event(existing.as_ref().unwrap_or(&wanted_claim))?;
            if let Some(association) = old_state.get("projectContinuation") {
                prospective["projectContinuation"] = association.clone();
            }
            Some(callback(
                &prospective,
                &wanted_claim,
                existing.is_some(),
                new.path.exists(),
            )?)
        } else {
            None
        };
        let claim = obtain_claim(&claim_path, &wanted_claim)?;
        let mut event_value = build_event(&claim.value)?;
        let ticket = crate::project::continuation::runtime::prepare_successor(
            loaded,
            &old_state,
            &mut event_value,
            &new_rel,
        )?;
        if let Some(expected) = expected_admission {
            let work = crate::work::locator_for_ledger(&new_rel)
                .ok_or("native successor destination is not a canonical Work")?;
            if crate::run::native_successor_intent(&work, &event_value)? != expected {
                rollback_claim(&claim_path, &claim);
                return Err(
                    "successor start differs from its native admission; repeat the exact request"
                        .into(),
                );
            }
        }
        let event = run_state::make_event(event_value);
        if new.path.exists() {
            let (_, existing, _) = load_at(loaded, &new)?;
            successor_protocol::assert_same_request(&existing, &event)?;
            return Ok(successor_protocol::attach_goal_linkage(
                loaded,
                &new_rel,
                attach_successor_work(
                    loaded,
                    claim.value["newLedgerPath"].as_str().unwrap_or_default(),
                    result(&existing)?,
                ),
            ));
        }
        if let Err(error) = append(&new, &event, true, "") {
            rollback_claim(&claim_path, &claim);
            return Err(error);
        }
        super::continuation_effect::complete(loaded, &ticket, true, true)?;
        Ok(successor_protocol::attach_goal_linkage(
            loaded,
            &new_rel,
            attach_successor_work(loaded, &new_rel, result(&[event])?),
        ))
    })
}

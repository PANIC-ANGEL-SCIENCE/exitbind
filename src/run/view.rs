use super::*;

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
    let drift = drift_warning(loaded, &state)?;
    warn_drift(drift.as_ref());
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
    let drift = drift_warning(loaded, &state)?;
    warn_drift(drift.as_ref());
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
    let drift = drift_warning(loaded, &state)?;
    warn_drift(drift.as_ref());
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
    let mut warnings = Vec::new();
    for ledger in ledgers {
        let (_, events, _) = load(loaded, ledger)?;
        let state = reduce_live(loaded, &events)?;
        if let Some(warning) = drift_warning(loaded, &state)? {
            warn_drift(Some(&warning));
            warnings.push(json!({"ledger": ledger, "warning": warning}));
        }
        predecessor(loaded, &events[0])?;
        states.push(state);
    }
    let mut report = crate::run_value::aggregate(&states)?;
    report["warnings"] = json!(warnings);
    Ok(report)
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
        let successor_run_id = claim.value["newRunId"]
            .as_str()
            .ok_or("supersession claim has no successor run id")?;
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
            "runId": successor_run_id,
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
                successor_run_id,
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
        if new.path.exists() {
            let (_, existing, _) = load_at(loaded, &new)?;
            let request_fields = [
                "runId",
                "workflow",
                "goal",
                "configSha256",
                "plan",
                "checkPolicy",
                "preservation",
                "basisProtocol",
                "basis",
                "reviewPolicy",
                "governor",
                "supersedes",
            ];
            let mismatched = existing.first().map(|current| {
                request_fields
                    .into_iter()
                    .filter(|field| current.get(*field) != event.get(*field))
                    .collect::<Vec<_>>()
            });
            let same_subject = existing.first().is_some_and(|current| {
                match (current.get("subject"), event.get("subject")) {
                    (Some(Value::Object(current)), Some(Value::Object(requested))) => current
                        .iter()
                        .filter(|(key, _)| key.as_str() != "sha256")
                        .all(|(key, value)| requested.get(key) == Some(value)),
                    (None, None) => true,
                    _ => false,
                }
            });
            let same_request = mismatched.as_ref().is_some_and(Vec::is_empty) && same_subject;
            if same_request {
                return result(&existing);
            }
            let mut fields = mismatched.unwrap_or_default();
            if !same_subject {
                fields.push("subject");
            }
            return Err(format!(
                "successor ledger already exists with different provenance (fields: {})",
                fields.join(", ")
            ));
        }
        if let Err(error) = append(&new, &event, true, "") {
            rollback_claim(&claim_path, &claim);
            return Err(error);
        }
        result(&[event])
    })
}

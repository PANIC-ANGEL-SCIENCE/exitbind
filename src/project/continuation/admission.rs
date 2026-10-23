//! Exact task claims are reserved before starting an ordinary Work.
use super::*;
use crate::session_goal::requirements::{self, ContractScope, PrepareRequest};

pub(super) fn plan_digest(plan: &Plan) -> Result<String, String> {
    let mut value = serde_json::to_value(plan).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("plan must be an object")?
        .remove("id");
    Ok(hash::value(&value))
}

fn read_plan(loaded: &Loaded, source: &str, spec: &Spec) -> Result<Plan, String> {
    model::validate_path(source)?;
    let lead = loaded
        .lead()
        .and_then(|name| loaded.agent(name))
        .ok_or("a configured Lead is required")?;
    if !lead.observe.iter().any(|root| contains(root, source)) {
        return Err("plan is outside the configured Lead read boundary".into());
    }
    let bytes = crate::project::path::secure_bytes_observation_bounded(
        &loaded.product_root,
        source,
        "project continuation plan",
        32 * 1024,
    );
    let bytes = match bytes {
        crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
        _ => return Err("plan must be a bounded readable regular project file".into()),
    };
    let plan: Plan =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid continuation plan: {e}"))?;
    plan.validate(spec)?;
    Ok(plan)
}

pub(super) fn contains(root: &str, path: &str) -> bool {
    let root = root
        .strip_suffix("/**")
        .unwrap_or(root)
        .trim_end_matches('/');
    root == "**"
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(super) fn validate_recipients(loaded: &Loaded, spec: &Spec, plan: &Plan) -> Result<(), String> {
    let workflow = loaded
        .workflows
        .get(&plan.workflow)
        .ok_or("plan workflow is not configured")?;
    if workflow.workers.is_empty()
        || workflow.reviewers.is_empty()
        || workflow
            .workers
            .iter()
            .any(|worker| workflow.reviewers.contains(worker))
    {
        return Err(
            "continuation needs actual worker and separate configured reviewer roles".into(),
        );
    }
    let lead = loaded
        .lead()
        .ok_or("continuation requires a configured Lead")?;
    for (role, names) in [
        ("lead", vec![lead.to_string()]),
        ("worker", workflow.workers.clone()),
        ("reviewer", workflow.reviewers.clone()),
        ("adviser", workflow.advisers.clone()),
    ] {
        if !names.is_empty() && !spec.mandate.roles.iter().any(|allowed| allowed == role) {
            return Err(format!("mandate does not permit the selected {role} role"));
        }
        for name in names {
            let agent = loaded
                .agent(&name)
                .ok_or("selected recipient is unavailable")?;
            if agent
                .commands
                .iter()
                .any(|command| !spec.mandate.commands.contains(command))
            {
                return Err(format!(
                    "configured {name} commands exceed the selected mandate"
                ));
            }
            if agent
                .write
                .iter()
                .any(|path| !plan.write.iter().any(|root| contains(root, path)))
            {
                return Err(format!("configured {name} writes exceed the selected task boundary; narrow configuration before selection"));
            }
        }
    }
    Ok(())
}

pub(super) fn goal(plan: &Plan) -> String {
    format!(
        "{}\nOutcome: {}\nWhy next: {}\nAcceptance: {}\nPreserve: {}",
        plan.title,
        plan.outcome,
        plan.rationale,
        plan.acceptance.join("; "),
        plan.preservation.join("; ")
    )
}

pub(super) fn preservation(plan: &Plan) -> String {
    format!("project-preservation:{}", plan.preservation.join("; "))
}

fn work(record: &Record, plan: &Plan) -> Result<String, String> {
    let token = hash::value(&json!({"project":record.project_identity,
        "mandate":record.specification.mandate.id,"task":plan.task_id}));
    Ok(format!("smw_{token}"))
}

fn prepared(record: &Record, plan: &Plan) -> Result<control::VerifiedAssociation, String> {
    let work = work(record, plan)?;
    let policy = control::Policy {
        project: &record.project_identity,
        specification_sha256: &record.specification_sha256,
        mandate: &record.specification.mandate,
    };
    control::prepare_association(&policy, &plan.task_id, &work, &work)
        .map_err(|reason| format!("invalid selected association: {reason:?}"))
}

fn intent(loaded: &Loaded, record: &Record, plan: &Plan, basis: &str) -> Result<String, String> {
    let timeout = plan
        .pre_return
        .as_ref()
        .map(|policy| policy.timeout_ms.to_string());
    let capture = plan
        .pre_return
        .as_ref()
        .map(|policy| policy.capture_limit_bytes.to_string());
    crate::run::start_intent_sha256_with_continuation(
        loaded,
        &plan.workflow,
        &goal(plan),
        None,
        None,
        Some(&plan.check_command),
        None,
        Some(&preservation(plan)),
        Some(&plan.preservation_check_command),
        None,
        Some(basis),
        Some("required"),
        plan.pre_return
            .as_ref()
            .map(|policy| policy.command.as_str()),
        timeout.as_deref(),
        capture.as_deref(),
        &prepared(record, plan)?,
    )
}

fn check_capacity(loaded: &Loaded, record: &Record, plan: &Plan) -> Result<(), String> {
    let policy = &record.specification.mandate;
    if policy.mode == Mode::Propose {
        return Err("propose-only mandate creates no Work".into());
    }
    let maximum = if policy.mode == Mode::Cycle {
        1
    } else {
        policy.max_works
    };
    if record.claims.len() as u64 >= maximum {
        return Err("aggregate Work admission budget exhausted; claims and failed reservations are retained".into());
    }
    if record.account.works_spent() >= maximum
        || record.account.actions_spent() >= policy.max_actions
    {
        return Err(
            "aggregate actual admission/action budget exhausted; failed attempts stay spent".into(),
        );
    }
    for dependency in &plan.dependencies {
        let claim = record
            .claims
            .iter()
            .find(|c| &c.task_id == dependency)
            .ok_or("dependent task has no prior claim")?;
        let status = work_status(loaded, claim)?;
        if status["status"] != "accepted"
            || status["warnings"]
                .as_array()
                .map_or(true, |v| !v.is_empty())
            || status["currentEvidence"]["state"] != "current"
        {
            return Err("dependent task requires current accepted Work evidence, not a checkbox or child acknowledgement".into());
        }
    }
    Ok(())
}

fn read_record(loaded: &Loaded, source: &str) -> Result<(Spec, String, String, Record), String> {
    let (spec, digest) = sources::read(loaded, source)?;
    let relative = journal::relative(loaded, &spec.mandate.id)?;
    let (record, _) = journal::read(loaded, &relative)?;
    let record = record.ok_or("select the owner's explicit bounded mandate before admission")?;
    if record.specification_path != source || record.specification_sha256 != digest {
        return Err(
            "source or mandate differs from its selected revision; inspect/replan before admission"
                .into(),
        );
    }
    current(loaded, &record)?;
    Ok((spec, digest, relative, record))
}

fn replay(
    loaded: &Loaded,
    record: &Record,
    plan: &Plan,
    digest: &str,
) -> Result<Option<Value>, String> {
    let Some(claim) = record
        .claims
        .iter()
        .find(|claim| claim.task_id == plan.task_id)
    else {
        return Ok(None);
    };
    if claim.plan_sha256 != digest {
        return Err("stable task identity is already claimed with different plan bytes; do not rename it to retry uncertainty".into());
    }
    let actual = work_status(loaded, claim);
    Ok(Some(
        json!({"version":1,"state":if actual.is_ok(){"existing_work"}else{"reserved_unresolved"},
        "effect":"no-change","work":claim.work,"taskId":claim.task_id,"actualWork":actual.as_ref().ok(),
        "unresolved":actual.err(),"detail":{"command":work_argv(loaded,&claim.work).full_argv()?},
        "replay":"No Work or native child was started. Inspect this exact claim; a missing/unknown start is held and is never retried under a new name."}),
    ))
}

pub(crate) fn admit(
    loaded: &Loaded,
    source: &str,
    plan_path: &str,
    expected: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    let prepare = |plan: &Plan| {
        requirements::prepare(PrepareRequest {
            root: &loaded.state_root,
            loaded,
            goal_id: &plan.goal_id,
            ids: &plan.requirements.join(","),
            checker: &plan.checker,
            command: &plan.check_command,
            basis: None,
            integration: plan.integration,
            scope: ContractScope::Project,
        })
    };
    let (spec, source_digest, relative, record) = read_record(loaded, source)?;
    let plan = read_plan(loaded, plan_path, &spec)?;
    let digest = plan_digest(&plan)?;
    if let Some(response) = replay(loaded, &record, &plan, &digest)? {
        return Ok(response);
    }
    validate_recipients(loaded, &spec, &plan)?;
    check_capacity(loaded, &record, &plan)?;
    let coverage = prepare(&plan)?;
    let start_intent = intent(loaded, &record, &plan, &coverage.basis)?;
    // Preview runs the same pure admission decision without persisting its
    // proposed account. Parallel/unknown owners therefore hold before apply.
    runtime::reserve_admission(
        loaded,
        &record,
        &plan,
        &work(&record, &plan)?,
        &start_intent,
    )?;
    let binding = hash::value(&json!({"head":record.event_sha256,"source":source_digest,
        "plan":digest,"configuration":hash::text(&loaded.source),"startIntent":start_intent}));
    let mut response = json!({"version":1,"state":"preview","readOnly":true,"effect":"no-change",
        "currentBinding":binding,"plan":plan,"startIntentSha256":start_intent,
        "apply":{"command":argv(loaded,&["admit",source,"--proposal",plan_path,"--current-binding",&binding,"--apply"]).full_argv()?},
        "nativeExecution":"Admission creates ordinary Work only; consume its complete current native assignment and action forms through the existing host."});
    if !apply {
        return Ok(response);
    }
    if expected != Some(binding.as_str()) {
        return Err("plan admission preview is stale; no Work was started".into());
    }
    let selected = journal::prepare(loaded, &relative)?;
    let reserved = ledger::with_lock(&selected, || {
        let (fresh_spec, fresh_digest, _, fresh) = read_record(loaded, source)?;
        let current_plan = read_plan(loaded, plan_path, &fresh_spec)?;
        if plan_digest(&current_plan)? != digest {
            return Err("plan changed before admission".into());
        }
        if let Some(existing) = replay(loaded, &fresh, &current_plan, &digest)? {
            response = existing;
            return Ok(None);
        }
        if fresh.event_sha256 != record.event_sha256 || fresh_digest != source_digest {
            return Err("another planner changed the mandate before admission".into());
        }
        check_capacity(loaded, &fresh, &current_plan)?;
        validate_recipients(loaded, &fresh_spec, &current_plan)?;
        let coverage = prepare(&current_plan)?;
        if intent(loaded, &fresh, &current_plan, &coverage.basis)? != start_intent {
            return Err("Goal/check/profile/selected experience changed before admission".into());
        }
        let classified = crate::work::classify(Some(loaded), &loaded.product_root, true, true);
        if classified["availability"]["managedNamespace"] != true {
            return Err("governed Work activation is unavailable".into());
        }
        let work = work(&fresh, &plan)?;
        let token = work.strip_prefix("smw_").ok_or("invalid planned Work")?;
        let ledger_relative = format!(
            "{}/runs/work-{token}.jsonl",
            crate::project::layout_types::state_namespace()
        );
        // A pre-existing unclaimed Work is conflicting state, not replay.
        if std::fs::symlink_metadata(loaded.state_root.join(&ledger_relative)).is_ok() {
            return Err(
                "deterministic Work exists without this claim; inspect without adoption or retry"
                    .into(),
            );
        }
        let (verified, account, ticket) =
            runtime::reserve_admission(loaded, &fresh, &plan, &work, &start_intent)?;
        let claim = journal::Claim {
            task_id: plan.task_id.clone(),
            plan_sha256: digest.clone(),
            plan: plan.clone(),
            work: work.clone(),
            start_intent_sha256: start_intent.clone(),
            checker_sha256: requirements::checker_input(
                loaded,
                &plan.checker,
                &plan.check_command,
            )?
            .sha256,
            admitted_at: Utc::now().to_rfc3339(),
            association: verified.association().clone(),
        };
        let (_, raw) = journal::read(loaded, &relative)?;
        let mut next = fresh.clone();
        next.claims.push(claim);
        next.account = account;
        journal::append(&selected, next, Some(&fresh), &raw)?;
        Ok(Some((work, ledger_relative, verified, ticket, coverage)))
    })?;
    let Some((work, ledger_relative, verified, ticket, coverage)) = reserved else {
        return Ok(response);
    };
    // The reservation is durable and the mandate lock is released. Never
    // wait for a Work lock while holding that account lock.
    let timeout = plan
        .pre_return
        .as_ref()
        .map(|policy| policy.timeout_ms.to_string());
    let capture = plan
        .pre_return
        .as_ref()
        .map(|policy| policy.capture_limit_bytes.to_string());
    let started = crate::run::start_with_continuation(
        loaded,
        &plan.workflow,
        &goal(&plan),
        &ledger_relative,
        None,
        None,
        Some(&plan.check_command),
        None,
        Some(&preservation(&plan)),
        Some(&plan.preservation_check_command),
        None,
        Some(&coverage.basis),
        Some("required"),
        plan.pre_return
            .as_ref()
            .map(|policy| policy.command.as_str()),
        timeout.as_deref(),
        capture.as_deref(),
        &verified,
    );
    let ticket = Some(ticket);
    response["readOnly"] = json!(false);
    response["effect"] = json!("claim_recorded");
    response["work"] = json!(work);
    match started {
        Err(error) => {
            let accounted = runtime::complete(loaded, &ticket, false, false);
            response["state"] = json!("reserved_unresolved");
            response["unresolved"] = json!(error);
            response["recovery"]=json!("Inspect this exact Work; the reservation remains spent. Never repeat start or mint another task ID.");
            if let Err(error) = accounted {
                response["accountingRecovery"] = json!(error);
            }
        }
        Ok(_) => {
            if let Err(error) = runtime::complete(loaded, &ticket, true, true) {
                response["accountingRecovery"] = json!(error);
            }
            response["state"] = json!("work_started");
            let linked = (|| {
                let (_, events, _) = ledger::load(loaded, &ledger_relative)?;
                let start = events.first().ok_or("Work start unavailable")?["eventSha256"]
                    .as_str()
                    .ok_or("Work start hash unavailable")?;
                requirements::write_scope_companion(loaded, &work, start, &coverage)?;
                requirements::assign(
                    &loaded.state_root,
                    loaded,
                    &plan.goal_id,
                    &plan.requirements.join(","),
                    &work,
                    false,
                )?;
                crate::work::set_focus(loaded, &work)?;
                Ok::<(), String>(())
            })();
            if let Err(error) = linked {
                response["linkageRecovery"] = json!(error)
            }
            response["detail"] = json!({"command":work_argv(loaded,&work).full_argv()?});
        }
    }
    Ok(response)
}

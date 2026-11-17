// Previews Lead replacement of a defective frozen check.
// Requires exactly one replacement target; see `src/work/mod.rs`.
//! Lead-owned, previewable replacement of an explicitly defective frozen check.

use super::*;

pub(crate) struct Options<'a> {
    pub(crate) check_command: Option<&'a str>,
    pub(crate) pre_return_command: Option<&'a str>,
    pub(crate) pre_return_timeout_ms: Option<&'a str>,
    pub(crate) pre_return_capture_bytes: Option<&'a str>,
    pub(crate) reason: &'a str,
    pub(crate) classification: &'a str,
    pub(crate) current_binding: Option<&'a str>,
    pub(crate) successor_scope: Option<&'a str>,
    pub(crate) scope_decision_sha256: Option<&'a str>,
    pub(crate) recover_partial: bool,
    pub(crate) apply: bool,
}

pub(crate) fn replace(loaded: &Loaded, work: &str, options: Options<'_>) -> Result<Value, String> {
    let standalone = match options.successor_scope {
        None => false,
        Some("standalone") => true,
        Some(_) => return Err("--successor-scope must be standalone".into()),
    };
    if !standalone && options.scope_decision_sha256.is_some() {
        return Err("--scope-decision-sha256 requires --successor-scope standalone".into());
    }
    if options.recover_partial && (!standalone || !options.apply) {
        return Err("--recover-partial requires --successor-scope standalone and --apply".into());
    }
    if standalone && options.apply != options.scope_decision_sha256.is_some() {
        return Err("standalone apply requires the scope decision SHA-256 from its preview".into());
    }
    let replacing_pre_return = options.pre_return_command.is_some();
    if options.check_command.is_some() == replacing_pre_return
        || (!replacing_pre_return
            && (options.pre_return_timeout_ms.is_some()
                || options.pre_return_capture_bytes.is_some()))
    {
        return Err(
            "choose exactly one replacement target: --check-command or --pre-return-command".into(),
        );
    }
    let binding = options
        .current_binding
        .filter(|value| !value.trim().is_empty())
        .ok_or("work replace-check requires --current-binding from current Work detail")?;
    details::ensure_current_config(loaded)?;
    let ledger = resolve(loaded, work)?;
    let (_, events, _) = run::ledger::load(loaded, &ledger)?;
    let state = run::state::reduce(&events)?;
    let start = events
        .first()
        .ok_or("predecessor Work has no start event")?;
    if state["status"] == "accepted" {
        return Err("accepted predecessor cannot be replaced".into());
    }
    if !matches!(state["status"].as_str(), Some("running" | "blocked")) {
        return Err("only a running or blocked predecessor can be continued".into());
    }
    if start["configSha256"] != crate::evidence::hash::text(&loaded.source) {
        return Err("predecessor configuration is stale; check replacement requires the current project configuration".into());
    }
    let _frozen_plan = crate::run::check_replacement::frozen_plan(loaded, start)?;
    let next = next_for(loaded, work, &ledger)?;
    if standalone && super::check::unresolved(&next) {
        return Err("an unresolved check observation blocks standalone replacement".into());
    }
    if next["current"]["binding"] != binding {
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!(
            "current action form binding is stale; {route} and follow current.details.grouped"
        ));
    }
    let (lead_agent, lead_native) = if next["role"] == "lead" && next["assignment"].is_string() {
        (
            next["agent"]
                .as_str()
                .ok_or("current Lead identity is missing")?,
            lead_native_for_agent(start, next["agent"].as_str().unwrap_or_default())?,
        )
    } else if next["action"] == "done" && state["status"] == "blocked" {
        // The route itself is an explicit sibling Lead decision bound to the
        // current terminal Work view; it does not create a submission.
        start_lead_owner(start)?
    } else {
        return Err("check replacement requires a current Lead decision or blocked Work with a configured Lead owner".into());
    };
    if !standalone
        && crate::session_goal::association::durable_standalone_scope(
            loaded,
            work,
            crate::session_goal::native::Access::RequireWritable,
        )?
    {
        return Err(
            "a standalone predecessor requires an explicit standalone successor preview".into(),
        );
    }
    let standalone_observation = if standalone {
        match crate::session_goal::association::classify_replacement_scope(loaded, work) {
            crate::session_goal::association::ReplacementScope::ValidatedCurrentCallerUnbound(
                observation,
            ) => Some(observation),
            crate::session_goal::association::ReplacementScope::Bound => {
                return Err(
                    "an existing native or local Goal relation cannot select standalone scope"
                        .into(),
                );
            }
            crate::session_goal::association::ReplacementScope::Invalid => {
                return Err("native Goal/session/project association is malformed or foreign; check replacement is blocked".into());
            }
            crate::session_goal::association::ReplacementScope::Unavailable(reason) => {
                return Err(format!("standalone scope unavailable: {reason}"));
            }
        }
    } else {
        None
    };
    if standalone && next_for(loaded, work, &ledger)?["current"]["binding"] != binding {
        return Err(
            "the current Work binding changed during scope observation; refresh before replacement"
                .into(),
        );
    }
    if !standalone {
        match crate::session_goal::association::context_for_work(loaded, work) {
            crate::session_goal::association::WorkContext::Owner(record)
            | crate::session_goal::association::WorkContext::Session(record) => {
                crate::session_goal::journey::require_resolved_replacement_admissions(&record)?;
            }
            crate::session_goal::association::WorkContext::Invalid => {
                return Err("native Goal/session/project association is malformed or foreign; check replacement is blocked".into());
            }
            crate::session_goal::association::WorkContext::Unavailable => {
                return Err("expected native Goal association or admission is unresolved; check replacement is blocked".into());
            }
            _ => {}
        }
    }
    let blocking = crate::session_goal::blocking_child(loaded, work)
        .map_err(|_| "prepared child state is unreadable; check replacement is blocked")?;
    if let Some(child) = blocking {
        return Err(format!(
            "check replacement is blocked by a live, prepared, or retained child result: {child}"
        ));
    }
    if options.classification != "defective-contract" {
        return Err("--classification must explicitly be defective-contract".into());
    }
    if options.reason.trim().is_empty()
        || options.reason.len() > 1024
        || options.reason.contains('\0')
    {
        return Err("--reason must be non-empty and at most 1024 bytes".into());
    }
    let old_policy = start
        .get("checkPolicy")
        .ok_or("predecessor has no frozen check policy")?;
    let proof_origin = old_policy["origin"].as_str().unwrap_or("local_report");
    let old_check_policy = crate::run_value::policy_from_value(old_policy, 0)?;
    let new_check_policy = options
        .check_command
        .map(|command| crate::run_value::policy_from_cli(Some(command), Some(proof_origin)))
        .transpose()?
        .flatten();
    let old_pre_return_policy = start
        .get("preReturnPolicy")
        .map(|policy| crate::run_value::pre_return_policy_from_value(policy, 0))
        .transpose()?;
    let new_pre_return_policy = if replacing_pre_return {
        if old_pre_return_policy.is_none() {
            return Err("pre-return policy replacement requires an opted-in predecessor".into());
        }
        Some(
            crate::run_value::PreReturnPolicy::from_cli(
                loaded,
                options.pre_return_command,
                options.pre_return_timeout_ms,
                options.pre_return_capture_bytes,
            )?
            .ok_or("successor pre-return policy is required")?,
        )
    } else {
        None
    };
    if let Some(new_policy) = &new_check_policy {
        if old_check_policy == *new_policy {
            return Err("replacement check policy must differ from the frozen predecessor".into());
        }
    }
    if let (Some(old_policy), Some(new_policy)) = (&old_pre_return_policy, &new_pre_return_policy) {
        if old_policy == new_policy {
            return Err(
                "replacement pre-return policy must differ from the frozen predecessor".into(),
            );
        }
    }
    let (source_kind, source_event_sha256) = events
        .iter()
        .rev()
        .find_map(|event| {
            let failed_check = !replacing_pre_return
                && event["action"] == "check"
                && (event["result"]["signal"].is_number()
                    || event["result"]["code"]
                        .as_i64()
                        .is_some_and(|code| code != 0)
                    || event["exitCode"].as_i64().is_some_and(|code| code != 0));
            let blocked_worker = event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "blocked";
            if failed_check {
                Some(("failed_check", event["eventSha256"].as_str()?))
            } else if blocked_worker {
                Some(("blocked_worker", event["eventSha256"].as_str()?))
            } else {
                None
            }
        })
        .ok_or("predecessor contains no exact failed-check or blocked-worker source evidence")?;
    let source = events
        .iter()
        .find(|event| event["eventSha256"] == source_event_sha256)
        .ok_or("source event is not present in the predecessor Work")?;
    let source_valid = match source_kind {
        "failed_check" => {
            source["action"] == "check"
                && (source["result"]["signal"].is_number()
                    || source["result"]["code"]
                        .as_i64()
                        .is_some_and(|code| code != 0)
                    || source["exitCode"].as_i64().is_some_and(|code| code != 0))
        }
        "blocked_worker" => {
            source["action"] == "submit"
                && source["role"] == "worker"
                && source["outcome"] == "blocked"
        }
        _ => false,
    };
    if !source_valid {
        return Err("source event must be a failed check or a blocked worker submission".into());
    }
    let diagnostic_attempt_id = if replacing_pre_return {
        Some(crate::run::pre_return::replacement_source_attempt(
            loaded,
            &crate::run::ledger::ledger_path(&loaded.state_root, &ledger, false)?,
            start,
            source,
        )?)
    } else {
        None
    };
    let workflow = start["workflow"]
        .as_str()
        .ok_or("predecessor workflow is missing")?;
    let goal = start["goal"]
        .as_str()
        .ok_or("predecessor Goal is missing")?;
    let replacement_id = if let Some(policy) = &new_pre_return_policy {
        crate::evidence::hash::value(&policy.value())
    } else {
        new_check_policy
            .as_ref()
            .ok_or("successor check policy is required")?
            .command_sha256
            .clone()
    };
    let generated_ledger = format!(
        "{}/runs/work-{}.jsonl",
        crate::project::layout_types::state_namespace(),
        crate::evidence::hash::text(&format!(
            "check-replacement-v1\n{}\n{}\n{}\n{}",
            start["runId"].as_str().unwrap_or_default(),
            events
                .last()
                .and_then(|event| event["eventSha256"].as_str())
                .unwrap_or_default(),
            replacement_id,
            options.reason
        ))
    );
    let new_ledger = generated_ledger.as_str();
    let mut source_binding = json!({"kind":source_kind,"eventSha256":source_event_sha256});
    if let Some(attempt_id) = &diagnostic_attempt_id {
        source_binding["attemptId"] = json!(attempt_id);
    }
    let (old_replacement_policy, new_replacement_policy) = if let Some(new_policy) =
        &new_pre_return_policy
    {
        (
            json!({"policySha256":crate::evidence::hash::value(&old_pre_return_policy.as_ref().unwrap().value())}),
            json!({"policySha256":crate::evidence::hash::value(&new_policy.value())}),
        )
    } else {
        let new_policy = new_check_policy
            .as_ref()
            .ok_or("successor check policy is required")?;
        (
            json!({"commandSha256":old_check_policy.command_sha256,"origin":old_check_policy.origin.as_str()}),
            json!({"commandSha256":new_policy.command_sha256,"origin":new_policy.origin.as_str()}),
        )
    };
    let mut requested_relation = json!({
        "protocol": 1,
        "target": if replacing_pre_return { "pre_return" } else { "check" },
        "classification": "defective_contract",
        "reason": options.reason,
        "source": source_binding,
        "leadOwner": {"agent": lead_agent, "nativeTaskName": lead_native},
        "predecessorHeadEventSha256": events.last().map(|event| event["eventSha256"].clone()).unwrap_or(Value::Null),
        "oldPolicy": old_replacement_policy,
        "newPolicy": new_replacement_policy,
    });
    if !replacing_pre_return {
        if let Some(relation) = requested_relation.as_object_mut() {
            relation.remove("target");
        }
    }
    let existing_claim = crate::run::ledger::load(loaded, &ledger).and_then(|(path, _, _)| {
        crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&path))
    })?;
    if options.recover_partial && existing_claim.is_none() {
        return Err(
            "saved successor claim disappeared; recovery stopped without a new claim".into(),
        );
    }
    let mut current_scope_observation = None;
    if let Some(observation) = &standalone_observation {
        requested_relation["protocol"] = json!(crate::run::check_replacement::STANDALONE_PROTOCOL);
        requested_relation["scope"] = json!({"kind":"standalone",
            "basis":"validated_current_caller_unbound", "caller":observation.caller.clone(),
            "currentBinding":binding});
        let current_decision = crate::session_goal::association::scope_decision_sha256(
            loaded,
            work,
            &requested_relation,
            observation,
        );
        let mut current_relation = requested_relation.clone();
        current_relation["scopeDecisionSha256"] = json!(current_decision.clone());
        current_scope_observation = Some(current_relation);
        let decision = if let Some(claim) = &existing_claim {
            let saved = &claim["checkReplacement"];
            if saved["protocol"].as_u64()
                != Some(crate::run::check_replacement::STANDALONE_PROTOCOL)
                || saved["scope"]["caller"] != observation.caller
            {
                return Err(
                    "a different successor is already claimed; no replacement was made".into(),
                );
            }
            requested_relation["scope"]["currentBinding"] =
                saved["scope"]["currentBinding"].clone();
            saved["scopeDecisionSha256"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        } else {
            current_decision
        };
        if options.apply && options.scope_decision_sha256 != Some(decision.as_str()) {
            return Err(
                "standalone scope decision changed after preview; no claim was created".into(),
            );
        }
        requested_relation["scopeDecisionSha256"] = json!(decision);
    }
    let new_path = crate::run::ledger::ledger_path(&loaded.state_root, new_ledger, true)?;
    let expected_new_relative = crate::config::rel(&loaded.state_root, &new_path.expected)?;
    let recovering_partial = existing_claim.is_some() && !new_path.path.exists();
    if let Some(claim) = &existing_claim {
        if claim.get("checkReplacement") != Some(&requested_relation)
            || claim["newLedgerPath"] != expected_new_relative
        {
            return Err("a different successor is already claimed; no replacement was made".into());
        }
        let successor_ledger = claim["newLedgerPath"].as_str().unwrap_or_default();
        let successor_work = crate::work::locator_for_ledger(successor_ledger)
            .ok_or("committed replacement claim has no Work identity")?;
        if new_path.path.exists() {
            let (_, successor_events, _) = crate::run::ledger::load(loaded, successor_ledger)?;
            let successor_start = successor_events
                .first()
                .ok_or("claimed successor ledger is empty")?;
            if successor_start["runId"] != claim["newRunId"]
                || successor_start["checkReplacement"] != requested_relation
            {
                return Err(
                    "claimed successor exists with mismatched provenance; no mutation was made"
                        .into(),
                );
            }
            if standalone {
                // A marker and run id alone are not proof that the saved
                // start belongs to this complete predecessor claim.
                run::RunSnapshot::capture(loaded, successor_ledger)?;
            }
            return Ok(
                json!({"effect":"no-change","state":"existing_successor_recovered","work":work,
                "predecessor":{"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"]},
                "successor":{"work":successor_work,"ledger":successor_ledger,"runId":claim["newRunId"],"checkReplacement":requested_relation},
                "recovery":detail_recovery(loaded, &successor_work, "Read the committed successor. Do not repeat replacement.")}),
            );
        }
        if !standalone || !options.recover_partial {
            let mut result = json!({"effect":"no-change","state":"partial_successor_claim","work":work,
            "predecessor":{"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"]},
            "successor":{"work":successor_work,"ledger":successor_ledger,"runId":claim["newRunId"],"checkReplacement":requested_relation,"ledgerPresent":false},
            "recovery":detail_recovery(loaded, work, if standalone { "Inspect the saved claim. Completing the exact saved standalone intent requires an explicit apply and a fresh current binding; it writes the reserved successor start." } else { "Inspect the predecessor's verified or unverified successor relation. Do not repeat replacement." })});
            if standalone {
                result["recoveryApplyCommand"] = replacement_apply_command(
                    loaded,
                    work,
                    binding,
                    &options,
                    new_pre_return_policy.as_ref(),
                    new_check_policy.as_ref(),
                    &requested_relation,
                    true,
                );
            }
            return Ok(result);
        }
    }
    if !options.apply {
        let successor_preview = if let Some(policy) = &new_pre_return_policy {
            json!({"ledger":new_ledger,"workflow":workflow,"goalSha256":crate::evidence::hash::text(goal),
                "preReturnPolicySha256":crate::evidence::hash::value(&policy.value()),
                "preReturnCommand":policy.command,"timeoutMs":policy.timeout_ms,"captureBytes":policy.capture_limit_bytes,
                "preservesFinalCheckPolicy":true,"diagnosticAttemptId":diagnostic_attempt_id})
        } else {
            let policy = new_check_policy
                .as_ref()
                .ok_or("successor check policy is required")?;
            json!({"ledger":new_ledger,"workflow":workflow,"goalSha256":crate::evidence::hash::text(goal),
                "checkCommandSha256":policy.command_sha256,"proofOrigin":policy.origin.as_str()})
        };
        return Ok(json!({
            "effect":"no-change", "state":"preview", "work":work,
            "predecessor":{"ledger":ledger,"runId":start["runId"],"headEventSha256":events.last().map(|event| &event["eventSha256"]),"checkPolicy":old_policy,"preReturnPolicy":start.get("preReturnPolicy")},
            "successor":successor_preview,
            "decision":requested_relation,
            "currentBinding":binding,
            "applyCommand": replacement_apply_command(loaded, work, binding, &options,
                new_pre_return_policy.as_ref(), new_check_policy.as_ref(), &requested_relation, false),
            "meaning": if replacing_pre_return { "Preview only. The predecessor remains immutable. Repeat with --apply and this current binding to create one claimed successor; the prior diagnostic is not a pass and the successor needs fresh evidence and review." } else { "Preview only. The predecessor remains immutable. Repeat with --apply and this current binding to create one claimed successor; the changed check earns no pass or review credit." }
        }));
    }
    let head = events
        .last()
        .and_then(|event| event["eventSha256"].as_str())
        .ok_or("predecessor head is missing")?;
    let value = if let Some(policy) = new_pre_return_policy {
        run::replace_pre_return_contract(
            loaded,
            &ledger,
            workflow,
            goal,
            new_ledger,
            policy,
            options.reason,
            source_event_sha256,
            diagnostic_attempt_id
                .as_deref()
                .ok_or("pre-return diagnostic attempt is missing")?,
            head,
            lead_agent,
            lead_native,
            standalone.then_some(&requested_relation),
            current_scope_observation.as_ref(),
            options.recover_partial,
        )?
    } else {
        run::replace_check_contract(
            loaded,
            &ledger,
            workflow,
            goal,
            new_ledger,
            options
                .check_command
                .ok_or("successor check policy is required")?,
            Some(proof_origin),
            options.reason,
            source_kind,
            source_event_sha256,
            head,
            lead_agent,
            lead_native,
            standalone.then_some(&requested_relation),
            current_scope_observation.as_ref(),
            options.recover_partial,
        )?
    };
    Ok(json!({
        "effect": if value["scopeStartEffect"] == "already_present" { "no-change" } else { "committed" },
        "state": if value["scopeStartEffect"] == "already_present" { "existing_successor_recovered" } else if recovering_partial { "partial_successor_recovered" } else { "successor_created" }, "work":work,
        "predecessor":{"ledger":ledger,"runId":start["runId"],"headEventSha256":events.last().map(|event| &event["eventSha256"])},
        "successor":value,
        "recovery":detail_recovery(loaded, value["work"].as_str().unwrap_or_default(), "The successor is committed. Inspect this current Work route; do not repeat replacement.")
    }))
}

fn replacement_apply_command(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    options: &Options<'_>,
    pre_return: Option<&crate::run_value::PreReturnPolicy>,
    check: Option<&crate::run_value::CheckPolicy>,
    relation: &Value,
    recover_partial: bool,
) -> Value {
    let mut route = crate::emitted::Route::new([
        "work",
        "replace-check",
        work,
        "--reason",
        options.reason,
        "--classification",
        "defective-contract",
        "--current-binding",
        binding,
        "--apply",
    ]);
    if let Some(policy) = pre_return {
        route = route.words(["--pre-return-command", policy.command.as_str()]);
        route = route.words([
            "--pre-return-timeout-ms".to_owned(),
            policy.timeout_ms.to_string(),
        ]);
        route = route.words([
            "--pre-return-capture-bytes".to_owned(),
            policy.capture_limit_bytes.to_string(),
        ]);
    } else if let Some(policy) = check {
        route = route.words(["--check-command", policy.command.as_str()]);
    }
    if options.successor_scope == Some("standalone") {
        route = route.words(["--successor-scope", "standalone"]);
        route = route.words([
            "--scope-decision-sha256",
            relation["scopeDecisionSha256"].as_str().unwrap_or_default(),
        ]);
    }
    if recover_partial {
        route = route.words(["--recover-partial"]);
    }
    crate::work::action_forms::command(loaded, route)
}

fn start_lead_owner(start: &Value) -> Result<(&str, &str), String> {
    let mut owners = start["plan"]["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        .filter(|agent| agent["role"] == "lead")
        .filter_map(|agent| Some((agent["name"].as_str()?, agent["nativeTaskName"].as_str()?)))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter();
    let (agent, native) = owners.next().ok_or("configured Lead owner is missing")?;
    if owners.next().is_some() {
        return Err("blocked replacement needs one unambiguous configured Lead owner".into());
    }
    if native.trim().is_empty() {
        return Err("configured Lead native task name is missing".into());
    }
    Ok((agent, native))
}

fn lead_native_for_agent<'a>(start: &'a Value, selected: &str) -> Result<&'a str, String> {
    let names = start["plan"]["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        .filter(|agent| agent["role"] == "lead" && agent["name"] == selected)
        .filter_map(|agent| agent["nativeTaskName"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if names.len() != 1 {
        return Err("current Lead has no unambiguous configured native identity".into());
    }
    names
        .into_iter()
        .next()
        .filter(|name| !name.trim().is_empty())
        .ok_or("configured Lead native identity is empty".into())
}

/// The read-only route to a Work's detail after a replacement step.
fn detail_recovery(loaded: &Loaded, work: &str, meaning: &str) -> Value {
    let mut route = crate::emitted::Route::new(["work", "detail", work, "--json"])
        .config(&loaded.path)
        .within(2048)
        .command_json();
    route["kind"] = json!("read_only");
    route["meaning"] = json!(meaning);
    route
}

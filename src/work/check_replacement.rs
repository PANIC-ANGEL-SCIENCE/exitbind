//! Lead-owned, previewable replacement of an explicitly defective frozen check.

use super::*;

pub(crate) struct Options<'a> {
    pub(crate) check_command: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) classification: &'a str,
    pub(crate) current_binding: Option<&'a str>,
    pub(crate) apply: bool,
}

pub(crate) fn replace(loaded: &Loaded, work: &str, options: Options<'_>) -> Result<Value, String> {
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
    let next = next_for(loaded, work, &ledger)?;
    if next["current"]["binding"] != binding {
        return Err(format!("current action form binding is stale; refresh with work next {work} --json and follow current.details.grouped"));
    }
    if state["status"] == "accepted" {
        return Err("accepted predecessor cannot be replaced".into());
    }
    if !matches!(state["status"].as_str(), Some("running" | "blocked")) {
        return Err("only a running or blocked predecessor can be continued".into());
    }
    let (lead_agent, lead_native) = if next["role"] == "lead" && next["assignment"].is_string() {
        (
            next["agent"]
                .as_str()
                .ok_or("current Lead identity is missing")?,
            next["nativeTaskName"]
                .as_str()
                .ok_or("current Lead native task identity is missing")?,
        )
    } else if next["action"] == "done" && state["status"] == "blocked" {
        // The route itself is an explicit sibling Lead decision bound to the
        // current terminal Work view; it does not create a submission.
        start_lead_owner(start)?
    } else {
        return Err("check replacement requires a current Lead decision or blocked Work with a configured Lead owner".into());
    };
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
    let continuation = crate::session_goal::continuation_view(loaded, work)?;
    if continuation["preparedChildren"]["error"].is_string() {
        return Err("prepared child state is unreadable; check replacement is blocked".into());
    }
    let children = continuation["preparedChildren"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if children.iter().any(|child| {
        !matches!(
            child["state"].as_str(),
            Some("recorded" | "failed" | "abandoned" | "superseded")
        ) || child["retainedResultBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
    }) {
        return Err(
            "check replacement is blocked by a live, prepared, or retained child result".into(),
        );
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
    if start["configSha256"] != crate::evidence::hash::text(&loaded.source) {
        return Err("predecessor configuration is stale; check replacement requires the current project configuration".into());
    }
    let old_policy = start
        .get("checkPolicy")
        .ok_or("predecessor has no frozen check policy")?;
    let proof_origin = old_policy["origin"].as_str().unwrap_or("local_report");
    let new_policy =
        crate::run_value::policy_from_cli(Some(options.check_command), Some(proof_origin))?
            .ok_or("successor check policy is required")?;
    if crate::run_value::policy_from_value(old_policy, 0)? == new_policy {
        return Err("replacement check policy must differ from the frozen predecessor".into());
    }
    let (source_kind, source_event_sha256) = events
        .iter()
        .rev()
        .find_map(|event| {
            let failed_check = event["action"] == "check"
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
    let workflow = start["workflow"]
        .as_str()
        .ok_or("predecessor workflow is missing")?;
    let goal = start["goal"]
        .as_str()
        .ok_or("predecessor Goal is missing")?;
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
            new_policy.command_sha256,
            options.reason
        ))
    );
    let new_ledger = generated_ledger.as_str();
    let requested_relation = json!({
        "protocol": 1,
        "classification": "defective_contract",
        "reason": options.reason,
        "source": {"kind": source_kind, "eventSha256": source_event_sha256},
        "leadOwner": {"agent": lead_agent, "nativeTaskName": lead_native},
        "predecessorHeadEventSha256": events.last().map(|event| event["eventSha256"].clone()).unwrap_or(Value::Null),
        "oldPolicy": {"commandSha256": old_policy["commandSha256"], "origin": old_policy["origin"]},
        "newPolicy": {"commandSha256": new_policy.command_sha256, "origin": new_policy.origin.as_str()},
    });
    let new_path = crate::run::ledger::ledger_path(&loaded.state_root, new_ledger, true)?;
    let expected_new_relative = crate::config::rel(&loaded.state_root, &new_path.expected)?;
    let existing_claim = crate::run::ledger::load(loaded, &ledger).and_then(|(path, _, _)| {
        crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&path))
    })?;
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
            return Ok(
                json!({"effect":"no-change","state":"existing_successor_recovered","work":work,
                "predecessor":{"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"]},
                "successor":{"work":successor_work,"ledger":successor_ledger,"runId":claim["newRunId"],"checkReplacement":requested_relation},
                "recovery":{"kind":"read_only","command":["work","detail",successor_work,"--json"],"meaning":"Read the committed successor. Do not repeat replacement."}}),
            );
        }
        return Ok(
            json!({"effect":"no-change","state":"partial_successor_claim","work":work,
            "predecessor":{"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"]},
            "successor":{"work":successor_work,"ledger":successor_ledger,"runId":claim["newRunId"],"checkReplacement":requested_relation,"ledgerPresent":false},
            "recovery":{"kind":"read_only","command":["work","detail",work,"--json"],"meaning":"Inspect the predecessor's verified or unverified successor relation. Do not repeat replacement."}}),
        );
    }
    if !options.apply {
        return Ok(json!({
            "effect":"no-change", "state":"preview", "work":work,
            "predecessor":{"ledger":ledger,"runId":start["runId"],"headEventSha256":events.last().map(|event| &event["eventSha256"]),"checkPolicy":old_policy},
            "successor":{"ledger":new_ledger,"workflow":workflow,"goalSha256":crate::evidence::hash::text(goal),"checkCommandSha256":new_policy.command_sha256,"proofOrigin":new_policy.origin.as_str()},
            "decision":requested_relation,
            "currentBinding":binding,
            "applyCommand": crate::work::action_forms::command(loaded, vec![
                "work".into(), "replace-check".into(), work.into(),
                "--check-command".into(), options.check_command.into(),
                "--reason".into(), options.reason.into(),
                "--classification".into(), "defective-contract".into(),
                "--current-binding".into(), binding.into(), "--apply".into(),
            ]),
            "meaning":"Preview only. The predecessor remains immutable. Repeat with --apply and this current binding to create one claimed successor; the changed check earns no pass or review credit."
        }));
    }
    let value = run::replace_check_contract(
        loaded,
        &ledger,
        workflow,
        goal,
        new_ledger,
        options.check_command,
        Some(proof_origin),
        options.reason,
        source_kind,
        source_event_sha256,
        events
            .last()
            .and_then(|event| event["eventSha256"].as_str())
            .ok_or("predecessor head is missing")?,
        lead_agent,
        lead_native,
    )?;
    Ok(json!({
        "effect":"committed", "state":"successor_created", "work":work,
        "predecessor":{"ledger":ledger,"runId":start["runId"],"headEventSha256":events.last().map(|event| &event["eventSha256"])},
        "successor":value,
        "recovery":{"kind":"read_only","command":["work","detail",value["work"],"--json"],"meaning":"The successor is committed. Inspect this current Work route; do not repeat replacement."}
    }))
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

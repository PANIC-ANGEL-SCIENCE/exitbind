//! Configured-Lead preview and exact decision for interrupted pre-return work.
use super::*;
use crate::run::{ledger, pre_return::recovery as protocol};
use protocol::evidence;

const MAX_DECISION_BYTES: u64 = 32 * 1024;

fn blocked_option(
    loaded: &Loaded,
    work: &str,
    ledger_name: &str,
    assignment: &str,
) -> Result<Value, String> {
    let next = super::next_for(loaded, work, ledger_name)?;
    let binding = next["current"]["binding"]
        .as_str()
        .ok_or("current Work binding is unavailable")?;
    Ok(blocked_option_for_binding(
        loaded, work, assignment, binding,
    ))
}

fn blocked_option_for_binding(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
) -> Value {
    let reason = crate::emitted::slot("REASON");
    let rendered_command = super::action_forms::command(
        loaded,
        crate::emitted::Route::new([
            "work",
            "return",
            work,
            assignment,
            "--outcome",
            "blocked",
            "--reason",
            &reason,
            "--current-binding",
            binding,
            "--json",
        ]),
    );
    json!({"command":rendered_command,"actor":"worker","placeholders":["REASON"],
        "stdin":{"required":true,"kind":"result_artifact",
            "description":"Supply the complete truthful blocked report on stdin."},
        "meaning":"Report a blocker without retrying the checker or granting check credit; retained completion evidence remains available."})
}

fn decision_refusal(loaded: &Loaded, work: &str, bound: &str, current: &str) -> String {
    crate::drift::refusal(
        crate::drift::Class::PreReturnRecoveryDecision,
        bound,
        Some(current),
        None,
        crate::drift::Route::WorkDetail {
            work,
            config: &loaded.path,
        },
    )
}

fn supplied_binding(value: Option<&str>) -> Result<&str, String> {
    value
        .filter(|hash| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .ok_or_else(|| "pre-return recovery requires a valid current binding".into())
}

fn decision_facts(decision: &Value) -> Result<Value, String> {
    let keys = [
        "version",
        "work",
        "agent",
        "authority",
        "approved",
        "reason",
        "executionDomain",
        "effects",
        "effectsReason",
        "currentBinding",
        "snapshot",
    ];
    if !decision.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    }) {
        return Err("pre-return decision must contain exactly the previewed fields".into());
    }
    let mut facts = decision.clone();
    facts.as_object_mut().unwrap().remove("snapshot");
    Ok(facts)
}

/// Surface the interruption before ordinary completion is offered. A second
/// interrupted generation is inspection-only: the one retry was consumed.
pub(super) fn action(
    loaded: &Loaded,
    work: &str,
    next: &Value,
    binding: &str,
) -> Result<Option<Value>, String> {
    if next["role"] != "worker" || next["action"] != "spawn" {
        return Ok(None);
    }
    let ledger_name = resolve(loaded, work)?;
    let (_, events, _) = ledger::load(loaded, &ledger_name)?;
    let state = run::state::reduce(&events)?;
    let assignments = run::assignment::pending(&state)
        .into_iter()
        .filter(|assignment| {
            run::assignment::handle(work, assignment).ok().as_deref() == next["assignment"].as_str()
        })
        .collect::<Vec<_>>();
    let [assignment] = assignments.as_slice() else {
        return Ok(None);
    };
    let pending = protocol::pending(loaded, &ledger_name, &state, assignment)?;
    if pending.is_empty() {
        return Ok(None);
    }
    if let Some(event) = protocol::for_assignment(&state, assignment)? {
        let projection = run::pre_return::projection(loaded, work)?;
        let latest = &projection["attempt"];
        if latest["attempt"]["binding"]["preReturnReconciliationSha256"] == event["eventSha256"]
            && latest["result"].is_object()
            && latest["result"]["state"] != "passed"
        {
            let id = latest["attempt"]["attemptId"]
                .as_str()
                .ok_or("retry admission identity is unavailable")?;
            let inspect = super::action_forms::command(
                loaded,
                crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
            );
            let handle = next["assignment"]
                .as_str()
                .ok_or("current worker assignment is unavailable")?;
            let mut blocked = blocked_option_for_binding(loaded, work, handle, binding);
            blocked["label"] = json!("blocked");
            return Ok(Some(json!({"version":1,"state":"pre_return_retry_failed",
                "binding":binding,"actor":"worker","assignment":handle,
                "leadChoiceRequired":false,"choices":[blocked],"mechanicalAction":Value::Null,
                "meaning":"The one linked retry did not pass. Inspect its saved result or report blocked; changing the result cannot inherit the exact-held reconciliation, and another retry is not admitted.",
                "diagnostic":{"command":inspect,"readOnly":true}})));
        }
        let predecessor = &event["recovery"]["snapshot"]["predecessor"]["attemptId"];
        if pending
            .iter()
            .all(|attempt| attempt["attemptId"] == *predecessor)
        {
            if let Err(error) =
                evidence::verify_saved(loaded, work, &event["recovery"]["snapshot"]["evidence"])
            {
                let id = predecessor
                    .as_str()
                    .ok_or("missing pre-return admission identity")?;
                let inspect = super::action_forms::command(
                    loaded,
                    crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
                );
                return Ok(Some(
                    json!({"version":1,"state":"pre_return_evidence_unavailable",
                    "binding":binding,"leadChoiceRequired":true,"choices":[],"mechanicalAction":Value::Null,
                    "meaning":"The bound observer evidence is unavailable or changed. Preserve the admission and inspect it before any retry.",
                    "reason":error,"diagnostic":{"command":inspect,"readOnly":true}}),
                ));
            }
            return Ok(None);
        }
        let retry = pending
            .iter()
            .find(|attempt| attempt["attemptId"] != *predecessor)
            .and_then(|attempt| attempt["attemptId"].as_str())
            .ok_or("retry admission identity is unavailable")?;
        let inspect = super::action_forms::command(
            loaded,
            crate::emitted::Route::new(["run", "pre-return", "read", work, retry, "--json"]),
        );
        return Ok(Some(
            json!({"version":1,"state":"pre_return_retry_interrupted",
            "binding":binding,"leadChoiceRequired":true,"choices":[],"mechanicalAction":Value::Null,
            "meaning":"The one reconciled pre-return retry is itself unfinished. Preserve it and inspect its evidence; another retry is not admitted.",
            "diagnostic":{"command":inspect,"readOnly":true}}),
        ));
    }
    if pending.len() != 1 {
        let id = pending[0]["attemptId"]
            .as_str()
            .ok_or("missing pre-return admission identity")?;
        let inspect = super::action_forms::command(
            loaded,
            crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
        );
        return Ok(Some(json!({"version":1,"state":"pre_return_ambiguous",
            "binding":binding,"leadChoiceRequired":true,"choices":[],"mechanicalAction":Value::Null,
            "meaning":"More than one unfinished pre-return admission exists; inspect the evidence before any completion.",
            "diagnostic":{"command":inspect,"readOnly":true}})));
    }
    let id = pending[0]["attemptId"]
        .as_str()
        .ok_or("missing pre-return admission identity")?;
    let retained = run::pre_return::read(loaded, work, id)?;
    if !retained["result"].is_null() {
        let rendered_command = super::action_forms::command(
            loaded,
            crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
        );
        let assignment = next["assignment"]
            .as_str()
            .ok_or("current worker assignment is unavailable")?;
        let mut blocked = blocked_option_for_binding(loaded, work, assignment, binding);
        blocked["label"] = json!("blocked");
        blocked["sideEffect"] = json!("records only the truthful blocked report after the worker supplies its reason and result");
        return Ok(Some(
            json!({"version":1,"state":"pre_return_uncertain_result",
            "binding":binding,"actor":"worker","assignment":assignment,"leadChoiceRequired":false,"choices":[blocked],"mechanicalAction":Value::Null,
            "meaning":"The retained pre-return result is uncertain. Inspect it or report blocked; missing-result reconciliation does not apply.",
            "diagnostic":{"command":rendered_command,"readOnly":true}}),
        ));
    }
    let rendered_command = super::action_forms::command(
        loaded,
        crate::emitted::Route::new(["work", "recover-pre-return", work, "--json"]),
    );
    let inspect = super::action_forms::command(
        loaded,
        crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
    );
    Ok(Some(
        json!({"version":1,"state":"interrupted_pre_return","binding":binding,
        "work":work,"assignment":next["assignment"],"actor":"lead","leadChoiceRequired":true,
        "meaning":"A pre-return execution was admitted but its result was not recorded. The configured Lead must review retained supervisor evidence and decide whether termination and effects are sufficiently established. Completion is unavailable until reconciliation and a fresh passing pre-return.",
        "diagnostic":{"command":inspect,"readOnly":true,"meaning":"Inspect the unfinished admission without executing it."},
        "choices":[{"label":"preview interrupted pre-return recovery","command":rendered_command,
            "placeholders":[],"stdin":{"required":false,"kind":"none"},
            "meaning":"Read-only preview of product-owned observer evidence; older admissions without that evidence remain unresolved.",
            "sideEffect":"none"}],"mechanicalAction":Value::Null}),
    ))
}

pub(crate) fn recover(
    loaded: &Loaded,
    work: &str,
    expected_binding: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    let bytes = if apply {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(MAX_DECISION_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_DECISION_BYTES {
            return Err("recovery decision exceeds 32768 bytes".into());
        }
        Some(
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|e| format!("invalid recovery decision: {e}"))?,
        )
    } else {
        None
    };
    details::ensure_current_config(loaded)?;
    let ledger_name = resolve(loaded, work)?;
    let (path, _, _) = ledger::load(loaded, &ledger_name)?;
    let (event, replay, preview) = ledger::with_lock(&path, || {
        if ledger::claim_path(&path).exists() {
            return Err("superseded Work cannot reconcile pre-return".into());
        }
        let (_, events, source) = ledger::load_at(loaded, &path)?;
        if let Some(decision) = bytes.as_ref() {
            if let Some(saved) = events.iter().find(|event| {
                event["action"] == protocol::ACTION
                    && event["recovery"]["currentBinding"].as_str() == expected_binding
            }) {
                let facts = decision_facts(decision)?;
                if saved["recovery"]["decision"] != facts
                    || decision["snapshot"] != saved["recovery"]["snapshot"]
                    || decision["work"] != work
                {
                    let bound = json!({"decision":saved["recovery"]["decision"],
                        "snapshot":saved["recovery"]["snapshot"],"work":work});
                    let current = json!({"decision":facts,"snapshot":decision["snapshot"],
                        "work":decision["work"]});
                    return Err(decision_refusal(
                        loaded,
                        work,
                        &hash::value(&bound),
                        &hash::value(&current),
                    ));
                }
                evidence::verify_saved(loaded, work, &saved["recovery"]["snapshot"]["evidence"])?;
                return Ok((Some(saved.clone()), true, Value::Null));
            }
        }
        let state = run::reduce_live(loaded, &events)?;
        if state["status"] != "running" {
            return Err("pre-return recovery requires a running Work".into());
        }
        if state.get("projectContinuation").is_some() {
            return Err("pre-return recovery is unavailable for continuation-managed Work".into());
        }
        let assignments = run::assignment::pending(&state)
            .into_iter()
            .filter(|assignment| assignment["role"] == "worker")
            .collect::<Vec<_>>();
        let [assignment] = assignments.as_slice() else {
            return Err("pre-return recovery requires one current worker assignment".into());
        };
        let assignment_handle = run::assignment::handle(work, assignment)?;
        let attempts = protocol::pending(loaded, &ledger_name, &state, assignment)?;
        let [predecessor] = attempts.as_slice() else {
            return Err("pre-return recovery requires one unique unfinished admission".into());
        };
        if state["governor"]["enabled"] == true && state["governor"]["state"] != "ready" {
            if apply {
                return Err(
                    "pre-return reconciliation requires the current governor to be ready".into(),
                );
            }
            return Ok((
                None,
                false,
                json!({"governorRequired":true,
                "reason":"The current governor is not ready. Review its existing action before reconciliation; changes may make the frozen admission ineligible.",
                "nextAction":{"command":super::action_forms::current_detail_command(loaded, work),
                    "readOnly":true,"meaning":"Inspect the current Work and its existing governor action."}}),
            ));
        }
        if state["verificationPreparationProtocol"] != 1 {
            if apply {
                return Err("pre-return recovery requires previously selected owned verification preparation; no fresh scratch can be promised without changing the frozen environment".into());
            }
            let id = predecessor["attemptId"]
                .as_str()
                .ok_or("missing pre-return admission identity")?;
            let inspect = super::action_forms::command(
                loaded,
                crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
            );
            return Ok((
                None,
                false,
                json!({"unsupported":true,
                "reason":"This Work did not select owned verification preparation. A fresh owned scratch cannot be promised without changing its frozen environment.",
                "nextAction":{"command":inspect,"readOnly":true,
                    "meaning":"Inspect the unfinished admission; this does not execute the checker."},
                "blockedOption":blocked_option(loaded, work, &ledger_name, &assignment_handle)?}),
            ));
        }
        let lead = protocol::configured_lead(&state)?;
        if loaded.lead() != lead["agent"].as_str()
            || loaded
                .agent(lead["agent"].as_str().unwrap_or_default())
                .is_none()
        {
            return Err("frozen and configured Lead do not agree".into());
        }
        let result_sha = predecessor["binding"]["resultSha256"]
            .as_str()
            .ok_or("pre-return result digest is missing")?;
        let held = held::discover(loaded, work, &assignment_handle)?
            .into_iter()
            .filter(|item| item["sha256"] == result_sha)
            .collect::<Vec<_>>();
        let [held] = held.as_slice() else {
            return Err("the exact retained worker result is missing or ambiguous".into());
        };
        let (_, current_predecessor) = protocol::candidate(
            loaded,
            &path,
            &events,
            &state,
            assignment,
            &assignment_handle,
            result_sha,
        )?;
        if current_predecessor != *predecessor {
            return Err(protocol::predecessor_refusal(
                loaded,
                work,
                predecessor,
                &current_predecessor,
            ));
        }
        let observed = match protocol::observer_evidence(loaded, predecessor) {
            Ok(evidence) => evidence,
            Err(error) if !apply => {
                let id = predecessor["attemptId"]
                    .as_str()
                    .ok_or("missing pre-return admission identity")?;
                let inspect = super::action_forms::command(
                    loaded,
                    crate::emitted::Route::new(["run", "pre-return", "read", work, id, "--json"]),
                );
                return Ok((
                    None,
                    false,
                    json!({"evidenceRequired":true,
                    "reason":error,"nextAction":{"command":inspect,"readOnly":true,
                        "meaning":"Inspect and preserve the unfinished admission. Recovery needs a complete product-owned observer proof; no check or return is launched."},
                    "blockedOption":blocked_option(loaded, work, &ledger_name, &assignment_handle)?}),
                ));
            }
            Err(error) => return Err(error),
        };
        let producer = crate::producer::build_identity();
        let snapshot = json!({"work":work,"assignment":assignment_handle,
            "assignmentSha256":hash::value(assignment),"predecessor":predecessor,
            "held":held,"ledgerSourceSha256":hash::text(&source),
            "governorSha256":hash::value(&state["governor"]),"lead":lead,
            "newProducer":{"producer":crate::producer::evidence_for_version(8),
                "executableSha256":producer["executableSha256"]},"evidence":observed,
            "nativeExit":Value::Null,"capture":"incomplete","checkCredit":false});
        let binding = protocol::request_binding(&snapshot);
        let template = json!({"version":1,"work":work,"agent":lead["agent"],
            "authority":"caller_reported","approved":false,"reason":crate::emitted::slot("REASON"),
            "executionDomain":"unknown","effects":"unknown","effectsReason":crate::emitted::slot("EFFECTS_REASON"),
            "currentBinding":binding,"snapshot":snapshot});
        if !apply {
            let route = crate::emitted::Route::new(["work", "recover-pre-return", work]);
            let rendered_command = super::action_forms::command(
                loaded,
                route.words(["--apply", "--current-binding", binding.as_str(), "--json"]),
            );
            return Ok((
                None,
                false,
                json!({"decision":template,"apply":{"command":rendered_command,
                "stdin":{"required":true,"kind":"configured_lead_decision_json"},
                "meaning":"The configured Lead reviews the supplied host observations and explicitly reports whether the execution domain ended and effects were reconciled. This records no check result and executes no checker."}}),
            ));
        }
        let decision = bytes.as_ref().ok_or("missing Lead decision")?;
        if expected_binding != Some(binding.as_str()) {
            return Err(decision_refusal(
                loaded,
                work,
                supplied_binding(expected_binding)?,
                &binding,
            ));
        }
        if decision["currentBinding"] != binding {
            return Err(decision_refusal(
                loaded,
                work,
                supplied_binding(decision["currentBinding"].as_str())?,
                &binding,
            ));
        }
        if decision["snapshot"] != snapshot {
            return Err(decision_refusal(
                loaded,
                work,
                &hash::value(&decision["snapshot"]),
                &hash::value(&snapshot),
            ));
        }
        if decision["work"] != work
            || decision["agent"] != lead["agent"]
            || decision["version"] != 1
        {
            return Err("pre-return decision has an invalid Work, Lead, or version".into());
        }
        evidence::verify_saved(loaded, work, &snapshot["evidence"])?;
        let last = events.last().ok_or("missing ledger head")?;
        let event = protocol::event(&state, last, snapshot, decision_facts(decision)?)?;
        protocol::validate(&event)?;
        protocol::validate_prefix(&event, source.trim_end_matches('\n'), &path)?;
        let mut all = events;
        all.push(event.clone());
        run::state::reduce(&all)?;
        ledger::append(&path, &event, false, &source)?;
        Ok((Some(event), false, Value::Null))
    })?;
    if let Some(event) = event {
        let route = super::action_forms::current_detail_command(loaded, work);
        Ok(
            json!({"work":work,"effect":if replay {"no-change"} else {"recorded"},
            "replayed":replay,"reconciliation":event,"checkRecorded":false,
            "nativeExit":Value::Null,"nextAction":{"command":route,"readOnly":true,
                "meaning":"Read the current action; only an actual fresh passing pre-return can complete the retained result."}}),
        )
    } else {
        if preview["governorRequired"] == true {
            return Ok(
                json!({"work":work,"effect":"no-change","state":"governor_required",
                "reason":preview["reason"],"nextAction":preview["nextAction"]}),
            );
        }
        if preview["unsupported"] == true {
            return Ok(
                json!({"work":work,"effect":"no-change","state":"unsupported",
                "reason":preview["reason"],"nextAction":preview["nextAction"],
                "blockedOption":preview["blockedOption"]}),
            );
        }
        if preview["evidenceRequired"] == true {
            return Ok(
                json!({"work":work,"effect":"no-change","state":"evidence_required",
                "reason":preview["reason"],"blockedOption":preview["blockedOption"],
                "nextAction":preview["nextAction"]}),
            );
        }
        Ok(
            json!({"work":work,"effect":"no-change","ownerDecision":preview["decision"],
            "apply":preview["apply"],"meaning":"Preview only. The configured Lead must judge the presented termination and effect evidence."}),
        )
    }
}

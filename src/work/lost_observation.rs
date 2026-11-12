//! Preview and explicitly apply a configured-Lead lost-caller report.
use super::*;
use crate::run::{ledger, lost_observation as protocol};

/// Only the facts that the preview fixes, excluding the Lead's decision and evidence.
fn decision_facts(decision: &Value, option_binding: Option<&str>) -> Value {
    json!({"work":decision["work"], "agent":decision["agent"],
        "assignment":decision["assignment"], "snapshot":decision["snapshot"],
        "hostContext":decision["hostContext"], "reporter":decision["reporter"],
        "currentBinding":decision["currentBinding"], "optionBinding":option_binding})
}

fn decision_refusal(loaded: &Loaded, work: &str, bound: &Value, current: &Value) -> String {
    crate::drift::refusal(
        crate::drift::Class::LostCallerDecision,
        &hash::value(bound),
        Some(&hash::value(current)),
        None,
        crate::drift::Route::WorkDetail {
            work,
            config: &loaded.path,
        },
    )
}

pub(crate) fn report(
    loaded: &Loaded,
    work: &str,
    binding: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    let ledger_name = resolve(loaded, work)?;
    let (path, _, _) = ledger::load(loaded, &ledger_name)?;
    let decision = if apply {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(protocol::MAX_DECISION_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > protocol::MAX_DECISION_BYTES {
            return Err("lost-caller decision exceeds 32768 bytes".into());
        }
        Some(
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|e| format!("invalid lost-caller decision: {e}"))?,
        )
    } else {
        None
    };
    let (event, replay, preview) = ledger::with_lock(&path, || {
        if ledger::valid_claim(&ledger::claim_path(&path))?.is_some() {
            return Err("superseded Work cannot report a lost caller".into());
        }
        let (_, events, source) = ledger::load_at(loaded, &path)?;
        let state = run::state::reduce(&events)?;
        details::ensure_current_config(loaded)?;
        run::check_observation::assert_current(loaded, &state)?;
        run::artifact::assert_current(loaded, &state)?;
        let bound_inputs = state["checkObservation"]["binding"]["inputsSha256"]
            .as_str()
            .ok_or("lost-caller admission input binding is missing")?;
        let current_inputs = run::inputs::fingerprint_for_state(loaded, &state)?;
        if bound_inputs != current_inputs {
            return Err(crate::drift::refusal(
                crate::drift::Class::LostCallerInputs,
                bound_inputs,
                Some(&current_inputs),
                None,
                crate::drift::Route::WorkDetail {
                    work,
                    config: &loaded.path,
                },
            ));
        }
        let head = events.last().ok_or("missing lost-caller head")?;
        let host = run::observer_context::current();
        if host["state"] != "available" {
            return Err("current host, boot or PID namespace context is unavailable".into());
        }
        let reporter = json!({"producer":crate::producer::evidence_for_version(8),
            "executableSha256":crate::producer::observation()["executableSha256"]});
        if reporter["executableSha256"].as_str().is_none() {
            return Err("current reporting executable digest is unavailable".into());
        }
        if apply && head["action"] == protocol::ACTION {
            let d = decision.as_ref().ok_or("missing lost-caller decision")?;
            protocol::decision(d)?;
            if head["observation"]["decision"] != *d
                || d["work"] != work
                || d["currentBinding"].as_str() != binding
                || !run::observer_context::matching(&d["hostContext"], &host)
                || d["reporter"] != reporter
            {
                let recorded = &head["observation"]["decision"];
                let bound = json!({"decision":recorded, "work":work,
                    "optionBinding":recorded["currentBinding"],
                    "hostContext":recorded["hostContext"], "reporter":recorded["reporter"]});
                let current = json!({"decision":d, "work":work,
                    "optionBinding":binding, "hostContext":host, "reporter":reporter});
                return Err(decision_refusal(loaded, work, &bound, &current));
            }
            return Ok((Some(head.clone()), true, Value::Null));
        }
        let next = next_for(loaded, work, &ledger_name)?;
        let (agent, assignment) = protocol::assignment(work, &state)?;
        if loaded.lead.as_deref() != Some(agent.as_str())
            || loaded.agent(&agent).is_none()
            || next["role"] != "lead"
            || next["agent"] != agent
            || next["action"] != "lead_decision"
            || head["action"] != "check_observation"
            || state["checkObservation"]["state"] != "running"
        {
            return Err(
                "no exact running admission awaits a configured-Lead lost-caller report".into(),
            );
        }
        let snapshot =
            protocol::snapshot(&state, &json!(hash::text(&source)), &head["eventSha256"]);
        let current_binding = protocol::request_binding(work, &state, &snapshot, &host, &reporter)?;
        let legacy = state["checkObservation"]["observer"]
            .get("context")
            .is_none();
        let template = json!({"version":1, "work":work, "agent":agent, "assignment":assignment,
            "approved":false, "reason":crate::emitted::slot("REASON"), "currentBinding":current_binding,
            "snapshot":snapshot, "hostContext":host, "reporter":reporter, "legacySameOriginAttested":false, "evidence":[]});
        if !apply {
            let command = crate::emitted::Route::new([
                "work",
                "report-lost-caller",
                work,
                "--apply",
                "--current-binding",
                current_binding.as_str(),
                "--json",
            ])
            .config(&loaded.path)
            .within(16 * 1024)
            .argv_json();
            return Ok((
                None,
                false,
                json!({"ownerDecision":template, "legacyHostAttestationRequired":legacy,
                "apply":{"command":command, "stdin":{"required":true, "kind":"ownerDecision_json",
                    "description":"Set approved and reason, supply one to four retained incident texts as {source, content, sha256}, and explicitly attest same-origin host only for a legacy caller lacking recorded context."}},
                "meaning":"Preview only. A report preserves unknown execution and provides no native observation, cleanup fact, check credit, retry or mutation authority. Legacy host provenance remains unverified."}),
            ));
        }
        let d = decision.as_ref().ok_or("missing lost-caller decision")?;
        protocol::decision(d)?;
        if d["snapshot"] != template["snapshot"]
            || d["hostContext"] != template["hostContext"]
            || d["reporter"] != template["reporter"]
            || d["work"] != work
            || d["agent"] != agent
            || d["assignment"] != assignment
            || d["currentBinding"] != current_binding
            || d["currentBinding"].as_str() != binding
        {
            return Err(decision_refusal(
                loaded,
                work,
                &decision_facts(d, binding),
                &decision_facts(&template, Some(&current_binding)),
            ));
        }
        let observer = &state["checkObservation"]["observer"];
        match observer.get("context") {
            Some(recorded)
                if run::observer_context::matching(recorded, &host)
                    && d["legacySameOriginAttested"] == false => {}
            None if d["legacySameOriginAttested"] == true => {}
            _ => {
                return Err(
                    "recorded observer context differs or legacy same-origin host was not attested"
                        .into(),
                )
            }
        }
        if !run::check_observation::observer_ended(&state["checkObservation"]) {
            return Err("observation caller is live or its local identity is uncertain".into());
        }
        let event = run::check_observation::event(
            &state,
            head,
            protocol::ACTION,
            json!({"admissionEventSha256":snapshot["admissionEventSha256"], "decision":d}),
        )?;
        let mut all = events;
        all.push(event.clone());
        run::state::reduce(&all)?;
        ledger::append(&path, &event, false, &source)?;
        Ok((Some(event), false, Value::Null))
    })?;
    let Some(event) = event else {
        return Ok(json!({"work":work, "effect":"no-change", "preview":preview}));
    };
    let mut response = json!({"work":work, "effect":if replay {"no-change"} else {"recorded"},
        "event":{"action":protocol::ACTION,"eventSha256":event["eventSha256"]},
        "reportEventSha256":event["eventSha256"], "provenance":"configured_lead_reported",
        "execution":"unknown", "checkRecorded":false, "oldCredited":false, "oldReset":false,
        "legacyHostProvenance":"unverified when attested", "evidence":"Lead-supplied retained incident text, not native observation proof"});
    match next_for(loaded, work, &ledger_name) {
        Ok(next) => response["next"] = next,
        Err(error) => response["projectionError"] = json!(error),
    }
    let config = loaded.path.to_str().ok_or("config is not UTF-8")?;
    let mut out = bounded_mutation(&response, work, None, &ledger_name, config);
    out["effect"] = response["effect"].clone();
    out["provenance"] = json!("configured_lead_reported");
    out["execution"] = json!("unknown");
    out["checkRecorded"] = json!(false);
    out["oldCredited"] = json!(false);
    out["oldReset"] = json!(false);
    out["legacyHostProvenance"] = json!("unverified when attested");
    Ok(out)
}

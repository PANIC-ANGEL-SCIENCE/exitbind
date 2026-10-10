//! Supported preview and exact replay for Lead-attested failed-check recovery.
use super::*;
use crate::run::{check_recovery as protocol, ledger};

pub(crate) fn recover(
    loaded: &Loaded,
    work: &str,
    binding: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    let ledger_name = resolve(loaded, work)?;
    let (path, _, _) = ledger::load(loaded, &ledger_name)?;
    let bytes = if apply {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(protocol::MAX_DECISION_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > protocol::MAX_DECISION_BYTES {
            return Err("recovery decision exceeds 32768 bytes".into());
        }
        Some(bytes)
    } else {
        None
    };
    let (event, replay, template) = ledger::with_lock(&path, || {
        if ledger::claim_path(&path).exists() {
            return Err("superseded Work cannot recover a check".into());
        }
        let (_, events, source) = ledger::load_at(loaded, &path)?;
        let state = run::state::reduce(&events)?;
        run::check_observation::assert_current(loaded, &state)?;
        run::artifact::assert_current(loaded, &state)?;
        if state["checkObservation"]["binding"]["inputsSha256"] != run::inputs::fingerprint(loaded)?
        {
            return Err(
                "recovery requires the original observed inputs; no execution was launched".into(),
            );
        }
        let head = events.last().ok_or("missing recovery head")?;
        let decision: Option<Value> = bytes
            .as_ref()
            .map(|b| {
                serde_json::from_slice(b).map_err(|e| format!("invalid recovery decision: {e}"))
            })
            .transpose()?;
        if apply && head["action"] == protocol::ACTION {
            let d = decision.as_ref().ok_or("missing decision")?;
            if state["status"] != "running"
                || head["observation"]["recovery"] != *d
                || d["work"] != work
                || d["currentBinding"].as_str() != binding
            {
                return Err("recovery replay differs from the current committed decision".into());
            }
            verify_partials(loaded, &protocol::decision(d)?["observation"])?;
            return Ok((Some(head.clone()), true, Value::Null));
        }
        let next = next_for(loaded, work, &ledger_name)?;
        let agent = loaded.lead.as_deref().ok_or("no configured Lead")?;
        loaded
            .agent(agent)
            .ok_or("configured Lead is unavailable")?;
        if next["role"] != "lead"
            || next["agent"] != agent
            || next["action"] != "lead_decision"
            || head["action"] != "check_observation"
            || state["checkObservation"]["state"] != "running"
        {
            return Err("no exact running admission awaits configured-Lead recovery".into());
        }
        let template = json!({"version": 1, "work": work, "agent": agent,
            "approved": false, "reason": "<REASON>", "currentBinding": next["current"]["binding"],
            "snapshot": protocol::snapshot(&state, &source),
            "response": "<COMPLETE_SAVED_STORAGE_FAILURE_RESPONSE>", "responseSha256": "<SHA256_OF_RESPONSE_BYTES>"});
        if !apply {
            return Ok((None, false, template));
        }
        details::ensure_action_binding(
            loaded,
            work,
            Some(binding.ok_or("--apply requires --current-binding")?),
        )?;
        let d = decision.as_ref().ok_or("missing decision")?;
        if d["snapshot"] != template["snapshot"]
            || d["work"] != work
            || d["agent"] != agent
            || d["currentBinding"].as_str() != binding
        {
            return Err("recovery decision or original observed inputs are stale".into());
        }
        let response = protocol::decision(d)?;
        verify_partials(loaded, &response["observation"])?;
        let event = run::check_observation::event(
            &state,
            head,
            protocol::ACTION,
            json!({"admissionEventSha256": d["snapshot"]["admissionEventSha256"],
                "facts": response["observation"], "recovery": d}),
        )?;
        let mut all = events;
        all.push(event.clone());
        run::state::reduce(&all)?;
        ledger::append(&path, &event, false, &source)?;
        Ok((Some(event), false, Value::Null))
    })?;
    let Some(event) = event else {
        return Ok(
            json!({"work": work, "effect": "no-change", "ownerDecision": template,
        "meaning": "Preview only. The configured Lead must approve and bind the complete retained known-ended response. This records reported failure, not a passing check or execution."}),
        );
    };
    let mut response = json!({"work": work, "observationFailure": event, "checkRecorded": false, "recovered": replay});
    match next_for(loaded, work, &ledger_name) {
        Ok(next) => response["next"] = next,
        Err(error) => response["projectionError"] = json!(error),
    }
    let config = loaded.path.to_str().ok_or("config is not UTF-8")?;
    let mut out = bounded_mutation(&response, work, None, &ledger_name, config);
    out["provenance"] = json!("configured_lead_reported");
    out["effect"] = json!(if replay { "no-change" } else { "recorded" });
    Ok(out)
}

fn verify_partials(loaded: &Loaded, facts: &Value) -> Result<(), String> {
    for partial in facts["partialCaptures"]
        .as_array()
        .ok_or("missing partial list")?
    {
        run::artifact::read(loaded, partial, "retained recovery partial")?;
    }
    Ok(())
}

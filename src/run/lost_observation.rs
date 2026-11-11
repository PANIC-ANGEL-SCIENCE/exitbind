//! Configured-Lead report of a lost observation caller. This is not a native
//! failure or a check result; the execution and its effects remain unknown.
use super::*;

pub(crate) const ACTION: &str = "check_observation_caller_lost";
pub(crate) const MAX_DECISION_BYTES: usize = 32 * 1024;
pub(crate) const MAX_EVIDENCE_BYTES: usize = 20 * 1024;

fn exact(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}
fn sha(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn text(value: &Value, maximum: usize) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.len() <= maximum && !s.contains('\0'))
}

pub(crate) fn snapshot(state: &Value, prefix: &Value, head: &Value) -> Value {
    let current = &state["checkObservation"];
    json!({"admissionEventSha256":current["eventSha256"], "headEventSha256":head,
        "ledgerSourceSha256":prefix, "binding":current["binding"],
        "timeoutMs":current["timeoutMs"], "caller":current["observer"],
        "originalProducer":current["producer"]})
}

pub(crate) fn assignment(work: &str, state: &Value) -> Result<(String, String), String> {
    let lead = crate::run::assignment::pending(state)
        .into_iter()
        .find(|a| a["role"] == "lead")
        .ok_or("lost-caller report requires the current Lead assignment")?;
    let agent = lead["agent"]
        .as_str()
        .ok_or("Lead agent is missing")?
        .to_owned();
    Ok((agent, crate::run::assignment::handle(work, &lead)?))
}

pub(crate) fn request_binding(
    work: &str,
    state: &Value,
    snapshot: &Value,
    host: &Value,
    reporter: &Value,
) -> Result<String, String> {
    let (agent, assignment) = assignment(work, state)?;
    Ok(hash::value(
        &json!({"domain":"check_observation_caller_lost_request_v1",
        "work":work, "lead":{"agent":agent,"assignment":assignment}, "snapshot":snapshot, "hostContext":host, "reporter":reporter}),
    ))
}

pub(crate) fn decision(value: &Value) -> Result<(), String> {
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > MAX_DECISION_BYTES
        || !exact(
            value,
            &[
                "version",
                "work",
                "agent",
                "assignment",
                "approved",
                "reason",
                "currentBinding",
                "snapshot",
                "hostContext",
                "reporter",
                "legacySameOriginAttested",
                "evidence",
            ],
        )
        || value["version"] != 1
        || value["approved"] != true
        || !crate::work::valid_work_handle(value["work"].as_str().unwrap_or(""))
        || !text(&value["agent"], 128)
        || !text(&value["assignment"], 128)
        || !text(&value["reason"], 1024)
        || !sha(&value["currentBinding"])
        || !crate::run::observer_context::valid(&value["hostContext"])
        || value["hostContext"]["state"] != "available"
        || !exact(&value["reporter"], &["producer", "executableSha256"])
        || !crate::producer::valid(&value["reporter"]["producer"])
        || !sha(&value["reporter"]["executableSha256"])
        || !value["legacySameOriginAttested"].is_boolean()
    {
        return Err("invalid bounded configured-Lead lost-caller decision".into());
    }
    let s = &value["snapshot"];
    if !exact(
        s,
        &[
            "admissionEventSha256",
            "headEventSha256",
            "ledgerSourceSha256",
            "binding",
            "timeoutMs",
            "caller",
            "originalProducer",
        ],
    ) || !sha(&s["admissionEventSha256"])
        || !sha(&s["headEventSha256"])
        || !sha(&s["ledgerSourceSha256"])
        || !crate::producer::valid(&s["originalProducer"])
    {
        return Err("invalid lost-caller admission snapshot".into());
    }
    let evidence = value["evidence"]
        .as_array()
        .ok_or("retained incident evidence is required")?;
    if evidence.is_empty()
        || evidence.len() > 4
        || evidence.iter().any(|item| {
            !exact(item, &["source", "content", "sha256"])
                || !text(&item["source"], 128)
                || !text(&item["content"], MAX_EVIDENCE_BYTES)
                || !sha(&item["sha256"])
                || item["sha256"] != hash::text(item["content"].as_str().unwrap_or_default())
        })
    {
        return Err("incident evidence must contain one to four bounded UTF-8 texts and their exact SHA-256 digests".into());
    }
    Ok(())
}

pub(crate) fn validate(event: &Value) -> Result<(), String> {
    let o = &event["observation"];
    if event["version"] != 8
        || event["action"] != ACTION
        || !exact(o, &["admissionEventSha256", "decision"])
    {
        return Err("invalid lost-caller event".into());
    }
    decision(&o["decision"])?;
    if o["admissionEventSha256"] != o["decision"]["snapshot"]["admissionEventSha256"]
        || o["decision"]["reporter"]["producer"] != event["producer"]
    {
        return Err("lost-caller decision names a different admission".into());
    }
    Ok(())
}

pub(crate) fn validate_prefix(
    event: &Value,
    prefix: &str,
    ledger: &run_ledger::LedgerPath,
) -> Result<(), String> {
    let d = &event["observation"]["decision"];
    if d["snapshot"]["ledgerSourceSha256"] != hash::text(&format!("{prefix}\n"))
        || d["work"] != canonical_work_for_ledger(ledger)?
    {
        return Err("lost-caller report does not bind the exact Work and ledger prefix".into());
    }
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    validate(event)?;
    let d = &event["observation"]["decision"];
    let current = &state["checkObservation"];
    let work = d["work"].as_str().ok_or("invalid lost-caller Work")?;
    let (agent, assignment) = assignment(work, state)?;
    let s = snapshot(
        state,
        &d["snapshot"]["ledgerSourceSha256"],
        &event["previousEventSha256"],
    );
    let caller = &current["observer"];
    let recorded_context = caller.get("context");
    let provenance_valid = match recorded_context {
        Some(context) => {
            d["legacySameOriginAttested"] == false
                && crate::run::observer_context::matching(context, &d["hostContext"])
        }
        None => d["legacySameOriginAttested"] == true,
    };
    if state["status"] != "running"
        || state["checkObservationProtocol"] != check_observation::PROTOCOL
        || current["state"] != "running"
        || state["pendingDisposition"].is_object()
        || !caller["pid"]
            .as_u64()
            .is_some_and(|pid| (1..=u32::MAX as u64).contains(&pid))
        || !caller["startTimeTicks"]
            .as_u64()
            .is_some_and(|ticks| ticks > 0)
        || event["previousEventSha256"] != current["eventSha256"]
        || d["snapshot"] != s
        || d["agent"] != agent
        || d["assignment"] != assignment
        || d["currentBinding"]
            != request_binding(work, state, &s, &d["hostContext"], &d["reporter"])?
        || !provenance_valid
    {
        return Err(
            "lost-caller decision is not bound to the current Lead, caller and admission".into(),
        );
    }
    state["checkObservation"]["state"] = json!("reported_lost");
    state["checkObservation"]["reportEventSha256"] = event["eventSha256"].clone();
    state["checkObservation"]["provenance"] = json!("configured_lead_reported");
    state["pendingDisposition"] = json!({"owner":"lead", "triggerEventSha256":event["eventSha256"],
        "basisSha256":state["basis"]["sha256"], "kind":"check_observation_caller_lost", "findingSha256s":[event["eventSha256"]]});
    Ok(())
}

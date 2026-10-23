//! Exact committed-owner readback repairs accounting metadata, never execution.
use super::*;

pub(crate) fn recover(
    loaded: &Loaded,
    mandate: &str,
    task: &str,
    expected: Option<&str>,
    apply: bool,
) -> Result<Value, String> {
    let relative = journal::relative(loaded, mandate)?;
    let (record, _) = journal::read(loaded, &relative)?;
    let record = record.ok_or("selected mandate is missing")?;
    let claim = record
        .claims
        .iter()
        .find(|claim| claim.task_id == task)
        .ok_or("task has no selected admission claim")?;
    let ledger_relative = crate::work::resolve(loaded, &claim.work)?;
    let (_, events, _) = ledger::load(loaded, &ledger_relative)?;
    let start = events
        .first()
        .ok_or("reserved Work start is missing; do not retry execution")?;
    if start["projectContinuation"]
        != serde_json::to_value(&claim.association).map_err(|e| e.to_string())?
        || crate::run::start_event_intent_sha256(start)? != claim.start_intent_sha256
    {
        return Err(
            "committed Work start differs from its exact selected claim; no recovery".into(),
        );
    }
    let state = crate::run::reduce_live(loaded, &events)?;
    let key = format!("admission-{}", hash::text(&claim.work));
    let input = json!({"plan":claim.plan_sha256,"startIntent":claim.start_intent_sha256});
    let old = record
        .account
        .operations
        .iter()
        .find(|old| old.key == key)
        .ok_or("committed Work has no admission debit")?;
    if old.intent.kind != control::Kind::Admission
        || old.intent.work != claim.work
        || old.intent.input_sha256 != hash::value(&input)
        || old.association != claim.association
    {
        return Err("committed start conflicts with its retained admission debit".into());
    }
    let mut works = vec![claim.work.clone()];
    let head = association::current_head(loaded, claim);
    if let Ok(head) = &head {
        if !works.contains(head) {
            works.push(head.clone());
        }
    }
    let mut decisions = Vec::new();
    for work in works {
        let ledger = crate::work::resolve(loaded, &work)?;
        for event in crate::run::saved_continuation_events(loaded, &ledger)? {
            if record.account.operations.iter().any(|old| {
                old.key == event["operation"]
                    && old.intent.work == work
                    && serde_json::to_value(old.intent.kind).ok().as_ref() == Some(&event["kind"])
                    && old.intent.input_sha256 == event["inputSha256"]
                    && old.association == claim.association
                    && matches!(
                        old.outcome,
                        control::Outcome::Reserved | control::Outcome::Unknown
                    )
            }) {
                decisions.push(json!({"work":work,"ledger":ledger,"event":event}));
            }
        }
    }
    let binding = hash::value(&json!({"accountHead":record.event_sha256,
        "work":claim.work,"start":start["eventSha256"],"operation":key,"decisions":decisions}));
    let mut response = json!({"version":1,"readOnly":true,"effect":"no-change",
        "state":"verified_saved_start","work":claim.work,"taskId":task,"currentBinding":binding,
        "outcome":old.outcome,"recovery":"Account observation only; no Work, child, check or effect is started.",
        "committedDecisionRecovery":decisions,"lineageHeld":head.err(),
        "apply":{"command":argv(loaded,&["recover",mandate,"--task",task,"--current-binding",&binding,"--apply"]).argv()?}});
    if !apply {
        return Ok(response);
    }
    if expected != Some(binding.as_str()) {
        return Err(
            "saved-start recovery preview is stale; inspect its exact current claim".into(),
        );
    }
    runtime::recover_observed(loaded, &state, &key, control::Kind::Admission, &input, true)?;
    for decision in &decisions {
        crate::run::recover_continuation_event(
            loaded,
            decision["ledger"]
                .as_str()
                .ok_or("recovery ledger identity missing")?,
            decision["event"]["eventSha256"]
                .as_str()
                .ok_or("recovery event identity missing")?,
        )?;
    }
    response["readOnly"] = json!(false);
    response["effect"] = json!("saved_start_observed");
    response["outcome"] = json!(control::Outcome::Succeeded);
    response["decisionsObserved"] = json!(decisions.len());
    Ok(response)
}

//! Current worker recovery forms over the existing governor and held owners.
use super::*;

pub(super) fn compact(result: &mut Value) {
    if let Some(recovery) = result.get_mut("recovery").and_then(Value::as_object_mut) {
        recovery.remove("command");
        recovery.remove("heldResults");
        recovery.insert("requiresDetail".into(), json!(true));
    }
}

pub(super) fn attach(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    binding: &str,
    next: &Value,
    result: &mut Value,
) {
    if next["packet"]["projectContinuation"].is_object() {
        match crate::project::continuation::time_amendment_hold(
            loaded,
            work,
            &next["packet"],
            binding,
        ) {
            Ok(Some(held)) => {
                *result = held;
                return;
            }
            Err(reason) => {
                *result = json!({
                    "version":1,"state":"held_time_amendment_unavailable","binding":binding,
                    "work":work,"leadChoiceRequired":true,"choices":[],"mechanicalAction":Value::Null,
                    "meaning":"The frozen continuation no longer matches its selected mandate, and its amendment state could not be verified. No Work action is offered.",
                    "diagnostic":{"readOnly":true,"reason":reason}
                });
                return;
            }
            Ok(None) => {}
        }
    }
    if next["action"] != "spawn" || next["role"] != "worker" {
        return;
    }
    let held: Vec<&Value> = next
        .get("held")
        .into_iter()
        .chain(next["heldResults"].as_array().into_iter().flatten())
        .collect();
    let state = next["packet"]["context"]["loop"]["state"].as_str();
    if state == Some("replan_required")
        && held.is_empty()
        && next["packet"]["context"]["loop"]["completionInformation"].is_object()
    {
        // New information is already recorded (the Lead's repair): the
        // worker's ordinary return choices stand and no re-plan is offered.
        return;
    }
    let form = match state {
        Some("replan_required") => Some(choice(
            loaded,
            crate::emitted::Route::new([
                "work",
                "replan",
                work,
                assignment,
                "--hypothesis",
                crate::emitted::slot("HYPOTHESIS").as_str(),
                "--scope-decision",
                crate::emitted::slot("SCOPE_DECISION").as_str(),
                "--evidence-request",
                crate::emitted::slot("EVIDENCE_REQUEST").as_str(),
            ]),
            "replan",
            &["HYPOTHESIS", "SCOPE_DECISION", "EVIDENCE_REQUEST"],
            no_input(),
        )),
        Some("evidence_required") => Some(choice(
            loaded,
            crate::emitted::Route::new([
                "work",
                "evidence",
                work,
                assignment,
                "--artifact-root",
                crate::emitted::slot("ARTIFACT_ROOT").as_str(),
                "--artifact",
                crate::emitted::slot("ARTIFACT_PATH").as_str(),
            ]),
            "evidence",
            &["ARTIFACT_ROOT", "ARTIFACT_PATH"],
            no_input(),
        )),
        _ => None,
    };
    if let Some(form) = form {
        result["recovery"] = json!({"state":state,"command":form["command"],
            "meaning":"supply meaningful current recovery evidence or a Lead re-plan, then refresh detail; spent authority and held bytes remain unchanged"});
        if !held.is_empty() {
            result["recovery"]["heldResults"] =
                json!(held.iter().map(|item| json!({
                "reference": item["reference"], "sha256": item["sha256"], "bytes": item["bytes"]
            })).collect::<Vec<_>>());
            result.as_object_mut().unwrap().remove("currentGrant");
        }
        result["choices"] = json!([form]);
        result["leadChoiceRequired"] = json!(true);
        result.as_object_mut().unwrap().remove("beforeEditing");
        return;
    }
    if state == Some("blocked") {
        result["recovery"] = json!({"state":"blocked","reason":"governor blocked; inspect the owner boundary; no retry or re-plan grants authority"});
        result["choices"] = json!([]);
        if held.is_empty() {
            if let Ok(ledger) = crate::work::resolve(loaded, work) {
                if let Ok(Some(candidate)) =
                    crate::run::governor_recovery::draft(loaded, work, &ledger)
                {
                    let predecessor = candidate["decision"]["predecessor"]["ledgerPath"]
                        .as_str()
                        .unwrap_or_default();
                    let workflow = candidate["workflow"].as_str().unwrap_or_default();
                    let goal = candidate["goal"].as_str().unwrap_or_default();
                    result["recovery"] = json!({
                        "state":"blocked",
                        "reason":"this owner recovery requires an explicit decision and verified no-effect evidence",
                        "ownerRecoveryDraft":candidate["decision"],
                        "effectsInventory":candidate["effectsInventory"],
                        "ownerDecisionRequired":true,
                        "command":command(loaded,crate::emitted::Route::new(["run", "supersede", predecessor, "--workflow", workflow, "--goal", goal, "--ledger", crate::emitted::slot("SUCCESSOR_LEDGER").as_str(), "--owner-recovery", crate::emitted::slot("OWNER_RECOVERY_STATE_ARTIFACT").as_str()])),
                        "placeholders":["SUCCESSOR_LEDGER","OWNER_RECOVERY_STATE_ARTIFACT"],
                        "meaning":"This read-only candidate records no approval or effect conclusion. The owner must verify exact invocation and complete effect evidence before executing the emitted successor command."
                    });
                }
            }
        }
        result.as_object_mut().unwrap().remove("beforeEditing");
        result.as_object_mut().unwrap().remove("currentGrant");
        return;
    }
    if held.is_empty() {
        return;
    }
    let forms: Vec<Value> = held.into_iter().filter_map(|held| {
        let reference = held["reference"].as_str()?;
        Some(json!({"reference":reference,"sha256":held["sha256"],"bytes":held["bytes"],
            "command":command(loaded,crate::emitted::Route::new(["work", "return", work, assignment, "--current-binding", binding, "--outcome", "completed", "--result-ref", reference, "--reason", crate::emitted::slot("REASON").as_str()])),
            "placeholders":["REASON"],"stdin":no_input()}))
    }).collect();
    result["recovery"] = json!({"state":"retained_result","heldResults":forms,
        "meaning":"return the selected exact retained bytes once, then perform the fresh applicable check and review"});
    result.as_object_mut().unwrap().remove("currentGrant");
    result.as_object_mut().unwrap().remove("beforeEditing");
}

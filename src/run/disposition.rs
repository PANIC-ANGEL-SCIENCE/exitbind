//! Lead disposition of a pending finding cycle.
//!
//! Reviewer and worker adverse evidence never starts work by itself: it
//! pends here until the Lead decides.  `repair` and `supersede` start one
//! bounded attempt carrying the Lead's repair terms; `defer` and `reject`
//! keep the finding as evidence, start nothing, and resolve the review cycle
//! without turning it into an approval.

use super::state::{lead_stage, subject_for_basis_revision, worker_stage};
use crate::kernel::basis::{parse_basis, validate_successor};
use crate::kernel::disposition::{parse_disposition, Disposition};
use serde_json::{json, Value};

/// Lead outcomes that remain available while a finding cycle is pending.
pub(crate) const PENDING_LEAD_OUTCOMES: &[&str] = &["disposition", "blocked", "rejected"];

pub(crate) fn validate(state: &Value, disposition: &Disposition) -> Result<(), String> {
    let pending = state
        .get("pendingDisposition")
        .filter(|value| value.is_object())
        .ok_or("Lead disposition is not currently pending")?;
    if disposition.basis_sha256.as_deref() != state["basis"]["sha256"].as_str()
        || pending["basisSha256"] != state["basis"]["sha256"]
        || pending["owner"] != "lead"
        || pending["findingSha256s"] != json!(disposition.finding_sha256s)
    {
        return Err("Lead disposition does not match the pending finding cycle".into());
    }
    if disposition.resolves_review() && pending["kind"] != "review_rework" {
        return Err("defer and reject resolve only a reviewer finding cycle".into());
    }
    if let Some(successor) = &disposition.successor_basis {
        let current = parse_basis(&state["basis"], "current basis")?;
        validate_successor(&current, successor)?;
    }
    Ok(())
}

/// Admit a submission's disposition at write time: the Lead may not skip a
/// pending disposition, and only a Lead disposition carries a record that
/// matches the pending cycle.  Historical ledgers replay without this check.
pub(crate) fn submission(
    state: &Value,
    role: &Value,
    outcome: &str,
    raw: Option<&str>,
    explicit: bool,
) -> Result<Option<Disposition>, String> {
    lead_outcome_allowed(state, role, outcome)?;
    if outcome == "disposition" && role != "lead" {
        return Err("disposition requires the pending Lead assignment".into());
    }
    if outcome != "disposition" {
        if explicit {
            return Err("--disposition requires --outcome disposition".into());
        }
        return Ok(None);
    }
    let raw = raw.ok_or("a disposition submission requires --disposition JSON")?;
    let parsed: Value = serde_json::from_str(raw)
        .map_err(|error| format!("disposition is not valid JSON: {error}"))?;
    let parsed = parse_disposition(&parsed, "disposition")?;
    // A reviewer finding is decided with a named decision, so a repair always
    // carries the Lead's boundary to the next attempt.
    if parsed.decision.is_none() && state["pendingDisposition"]["kind"] == "review_rework" {
        return Err("a reviewer finding needs a Lead decision: exitbind work disposition WORK ASSIGNMENT --decision repair|defer|reject|supersede --reason TEXT".into());
    }
    validate(state, &parsed)?;
    Ok(Some(parsed))
}

fn lead_outcome_allowed(state: &Value, role: &Value, outcome: &str) -> Result<(), String> {
    if role == "lead"
        && state["pendingDisposition"].is_object()
        && !PENDING_LEAD_OUTCOMES.contains(&outcome)
    {
        return Err(
            "a pending finding requires a Lead disposition first: exitbind work disposition WORK ASSIGNMENT --decision repair|defer|reject|supersede --reason TEXT"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    let disposition = event
        .get("disposition")
        .ok_or("Lead disposition is missing")?;
    let disposition =
        parse_disposition(disposition, "disposition").map_err(|error| error.to_string())?;
    validate(state, &disposition)?;
    let mut submission = json!({
        "stage": event["stage"],
        "attempt": event["attempt"],
        "agent": event["agent"],
        "role": event["role"],
        "outcome": "disposition",
        "artifact": event["artifact"],
        "eventSha256": event["eventSha256"],
        "disposition": disposition.value(),
    });
    if let Some(inputs) = event.get("inputsSha256") {
        submission["inputsSha256"] = inputs.clone();
        state["inputsSha256"] = inputs.clone();
    }
    for field in ["basisSha256", "reviewDecisionSha256"] {
        if let Some(value) = event.get(field) {
            submission[field] = value.clone();
        }
    }
    state["submissions"]
        .as_array_mut()
        .ok_or("run state submissions are invalid")?
        .push(submission);
    state["dispositions"]
        .as_array_mut()
        .ok_or("run disposition history is invalid")?
        .push(disposition.value());
    state["pendingDisposition"] = Value::Null;
    if disposition.resolves_review() {
        state["reviewResolution"] = json!({
            "state": "resolved_by_lead_disposition",
            "decision": disposition.decision.map(|decision| decision.as_str()),
            "dispositionSha256": disposition.sha256,
            "findingSha256s": disposition.finding_sha256s,
            "attempt": state["attempt"],
            "reviewDecisionSha256": state["reviewPolicy"]["sha256"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": state["inputsSha256"],
        });
        state["currentStage"] = json!(lead_stage(state)?);
        return Ok(());
    }
    if let Some(successor) = disposition.successor_basis.clone() {
        let previous_basis = state["basis"].clone();
        state["basisHistory"]
            .as_array_mut()
            .ok_or("run basis history is invalid")?
            .push(json!({
                "basis": previous_basis,
                "dispositionSha256": disposition.sha256,
                "triggeringFindingSha256s": disposition.finding_sha256s,
            }));
        state["basis"] = successor.value();
        state["subject"] =
            subject_for_basis_revision(state, &state["basis"]["sha256"], &disposition.sha256);
    }
    state["currentStage"] = json!(worker_stage(state)?);
    let attempt = state["attempt"].as_u64().ok_or("run attempt is invalid")?;
    state["attempt"] = json!(attempt + 1);
    if let (Some(decision), Some(repair)) = (disposition.decision, &disposition.repair) {
        state["authorizedRepair"] = json!({
            "authority": "lead",
            "decision": decision.as_str(),
            "reason": disposition.reason,
            "repairBoundary": repair.repair_boundary,
            "decisiveRegression": repair.decisive_regression,
            "findingSha256s": disposition.finding_sha256s,
            "dispositionSha256": disposition.sha256,
            "basisSha256": state["basis"]["sha256"],
            "attempt": attempt + 1,
        });
    }
    Ok(())
}

/// The review cycle the Lead resolved by `defer` or `reject`, while it still
/// belongs to the current attempt, review decision, and tested inputs.
pub(crate) fn current_resolution(state: &Value) -> Option<Value> {
    let resolution = state
        .get("reviewResolution")
        .filter(|value| value.is_object())?;
    if resolution["attempt"] != state["attempt"]
        || resolution["reviewDecisionSha256"] != state["reviewPolicy"]["sha256"]
        || state["pendingDisposition"].is_object()
    {
        return None;
    }
    // The resolution holds only for the result and tested inputs the Lead
    // decided on, like an approval.
    if resolution["subjectSha256"] != state["subject"]["sha256"]
        || (state["version"].as_u64() >= Some(6)
            && resolution["inputsSha256"] != state["inputsSha256"])
    {
        return None;
    }
    Some(resolution.clone())
}

/// What the Lead needs to decide a pending cycle, including prior cycles so a
/// repeated finding surfaces as a question about the basis, criteria, or
/// reviewer scope rather than as another automatic repair.
pub(crate) fn lead_view(state: &Value) -> Option<Value> {
    let pending = state
        .get("pendingDisposition")
        .filter(|value| value.is_object())?;
    let decisions = if pending["kind"] == "review_rework" {
        json!(["repair", "defer", "reject", "supersede"])
    } else {
        json!(["repair", "supersede"])
    };
    let prior = state["dispositions"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "decision": item.get("decision").cloned().unwrap_or(Value::Null),
                        "category": item.get("category").cloned().unwrap_or(Value::Null),
                        "findingSha256s": item["findingSha256s"],
                        "repairBoundary": item.get("repairBoundary").cloned().unwrap_or(Value::Null),
                        "sha256": item["sha256"],
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut view = json!({
        "kind": pending["kind"],
        "findingSha256s": pending["findingSha256s"],
        "decisions": decisions,
        "rule": "a finding is evidence, not a requirement; the Lead decides its scope and any repair boundary",
        "priorCycles": prior,
    });
    // Another finding after authorized repair work is the loop worth
    // questioning; a deferred or rejected finding started no work.
    let repaired = prior
        .iter()
        .any(|item| !matches!(item["decision"].as_str(), Some("defer" | "reject")));
    if repaired {
        view["advice"] = json!(
            "a finding followed Lead-authorized work in this run; before another repair, reconsider the basis, acceptance criteria, evidence, or reviewer scope"
        );
    }
    Some(view)
}

/// The Lead's repair terms for the current attempt, shown to its worker and
/// reviewer.  Only decision records produce these, so historical packets and
/// their hashes are unchanged.
pub(crate) fn repair_view(state: &Value) -> Option<Value> {
    state
        .get("authorizedRepair")
        .filter(|value| value.is_object() && value["attempt"] == state["attempt"])
        .map(|value| {
            let mut view = value.clone();
            if let Some(object) = view.as_object_mut() {
                object.remove("attempt");
            }
            view
        })
}

//! Apply validated run events to the canonical state.

use super::*;

pub(super) fn apply_event(state: &mut Value, event: &Value) -> Result<(), String> {
    match event["action"].as_str() {
        Some("submit") => apply_submission(state, event),
        Some("govern") => apply_govern(state, event),
        Some("review_policy") => apply_review_policy(state, event),
        Some("check") => check_stage::apply_check(state, event),
        Some("protect") => apply_protection(state, event),
        _ => Err("run event action is invalid".into()),
    }
}

pub(super) fn apply_govern(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" {
        return Err("governor action requires a running run".into());
    }
    if event["governorEvent"]["action"] == "evidence" {
        crate::run::validate_evidence_governor_identity(state, event)?;
    }
    if event["governorEvent"]["action"] == "replan" {
        // v0.22/v0.24-rc.2 ledgers did not persist the assignment packet
        // binding and may transition identity in their markerless re-plan.
        // Preserve those append-only histories; current run markers take the
        // strict path below.
        if state["governor"]["defaults"]["replanBinding"] != "assignment_packet_v1" {
            return Ok(());
        }
        if event["governorEvent"]["identityTransition"].is_null()
            && state["governor"]["defaults"]["replanBinding"] == "assignment_packet_v1"
            && (event["subjectSha256"] != state["subject"]["sha256"]
                || event["attempt"] != state["attempt"])
        {
            return Err("legacy re-plan cannot change identity under the current binding".into());
        }
        if state["governor"]["defaults"]["replanBinding"] == "assignment_packet_v1"
            && event["governorEvent"]["identityTransition"] != "carried_mutation_v1"
        {
            return Err("current re-plan is missing its identity transition binding".into());
        }
        let prior_input = state["events"].as_array().and_then(|events| {
            let current = events
                .iter()
                .position(|candidate| candidate["eventSha256"] == event["eventSha256"])?;
            events[..current]
                .iter()
                .rev()
                .find_map(|candidate| candidate["inputsSha256"].as_str())
        });
        let assignment = crate::run::assignment::pending(state)
            .into_iter()
            .find(|item| item["agent"] == event["agent"])
            .ok_or("re-plan actor is not currently assigned")?;
        if assignment["role"] != "worker"
            || event["role"] != "worker"
            || assignment["stage"] != event["stage"]
            || assignment["attempt"] != event["attempt"]
            || assignment["role"] != event["role"]
            || event["subjectSha256"] != state["subject"]["sha256"]
            || state["inputsSha256"]
                .as_str()
                .or(prior_input)
                .is_some_and(|expected| event["inputsSha256"] != expected)
        {
            return Err("re-plan is not bound to the current worker assignment".into());
        }
        if let Some(packet_sha256) = event["governorEvent"]["assignmentPacketSha256"].as_str() {
            if packet_sha256 != crate::evidence::hash::value(&assignment) {
                return Err("re-plan assignment packet is stale or mismatched".into());
            }
        } else if state["governor"]["defaults"]["replanBinding"] == "assignment_packet_v1" {
            return Err("carried re-plan is missing its assignment packet binding".into());
        }
    }
    Ok(())
}

fn apply_review_policy(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" || state.get("basisProtocol").is_none() {
        return Err("review policy requires a marked running run".into());
    }
    let current = state["reviewPolicy"]["sha256"]
        .as_str()
        .ok_or("review policy has no current decision identity")?;
    if event["previousDecisionSha256"].as_str() != Some(current)
        || event["basisSha256"] != state["basis"]["sha256"]
    {
        return Err("review policy transition is stale".into());
    }
    let policy = event
        .get("reviewPolicy")
        .ok_or("review policy transition is missing its decision")?;
    let parsed = crate::kernel::basis::parse_review(policy, "reviewPolicy")
        .map_err(|error| error.to_string())?;
    if parsed.previous_sha256.as_deref() != Some(current) {
        return Err("review policy transition does not chain its prior decision".into());
    }
    if policy["decision"] == state["reviewPolicy"]["decision"]
        && policy["reason"] == state["reviewPolicy"]["reason"]
    {
        return Err("duplicate review policy decision is refused".into());
    }
    state["reviewPolicy"] = policy.clone();
    state["reviewDecisions"]
        .as_array_mut()
        .ok_or("review decision history is invalid")?
        .push(policy.clone());
    if policy["decision"] == "omitted" && state["pendingDisposition"].is_null() {
        let current_stage = state["currentStage"].as_u64().unwrap_or_default();
        let reviewer_pending = state["plan"]["stages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|stage| {
                stage["stage"] == current_stage
                    && stage["agents"].as_array().is_some_and(|agents| {
                        agents.iter().any(|agent| agent["role"] == "reviewer")
                    })
            });
        if reviewer_pending {
            state["currentStage"] = json!(lead_stage(state)?);
        }
    } else if policy["decision"] == "required" {
        let current_stage = state["currentStage"].as_u64().unwrap_or_default();
        let lead_pending = state["plan"]["stages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|stage| {
                stage["stage"] == current_stage
                    && stage["agents"]
                        .as_array()
                        .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == "lead"))
            });
        if lead_pending
            && state["pendingDisposition"].is_null()
            && current_stage == lead_stage(state)?
        {
            state["currentStage"] = json!(reviewer_stage(state)?);
        }
    }
    Ok(())
}

fn apply_protection(state: &mut Value, event: &Value) -> Result<(), String> {
    // A v6 protection is judged on the tested inputs it declares; that judgment
    // does not leak into later state.
    let live = state.get("inputsSha256").cloned();
    if let Some(inputs) = event.get("inputsSha256") {
        state["inputsSha256"] = inputs.clone();
    }
    let validated = crate::run_value::validate_protection_against_state(state, event, 0);
    match live {
        Some(live) => state["inputsSha256"] = live,
        None => {
            if let Some(object) = state.as_object_mut() {
                object.remove("inputsSha256");
            }
        }
    }
    validated.map_err(|error| error.replacen("line 0", "state", 1))?;
    if !crate::run_exit::reduce(state)?.subject_is_current(&event["subjectSha256"]) {
        return Err("protection is bound to a stale subject".into());
    }
    state["protections"]
        .as_array_mut()
        .ok_or("run state protections are invalid")?
        .push(event.clone());
    Ok(())
}

pub(super) fn apply_submission(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" {
        return Err("run has already reached a terminal state".into());
    }
    let candidate = crate::run::assignment::pending(state)
        .into_iter()
        .find(|x| x["agent"] == event["agent"]);
    let historical = candidate
        .is_none()
        .then(|| historical_review::reviewer_assignment(state, event))
        .flatten();
    let assignment = candidate.or(historical.clone());
    let Some(assignment) = assignment else {
        return Err(format!(
            "agent '{}' is not currently pending",
            event["agent"]
        ));
    };
    if event["stage"] != assignment["stage"]
        || event["attempt"] != assignment["attempt"]
        || event["role"] != assignment["role"]
    {
        return Err("submission is out of order".into());
    }
    if state.get("basisProtocol").is_some() && event["basisSha256"] != state["basis"]["sha256"] {
        return Err("submission is bound to a stale basis".into());
    }
    if state["version"].as_u64() >= Some(5) {
        let expected = if event["role"] == "worker" && event["outcome"] == "completed" {
            subject_for_submission(state, &assignment, &event["artifact"])["sha256"].clone()
        } else {
            state["subject"]["sha256"].clone()
        };
        if event["subjectSha256"] != expected {
            return Err("submission is bound to a stale subject".into());
        }
        if event["role"] == "worker" && event["outcome"] == "completed" {
            state["subject"] = subject_for_submission(state, &assignment, &event["artifact"]);
        }
    }
    let role = event["role"].as_str().ok_or("submission role is invalid")?;
    let outcome = event["outcome"]
        .as_str()
        .ok_or("submission outcome is invalid")?;
    let all = match role {
        "lead" => &[
            "scoped",
            "blocked",
            "accepted",
            "rework",
            "rejected",
            "disposition",
        ][..],
        "adviser" => &["completed", "blocked"][..],
        "worker" => &["completed", "blocked", "contradiction"][..],
        "reviewer" => &["approved", "rework", "blocked", "unavailable"][..],
        _ => &[],
    };
    if !all.contains(&event["outcome"].as_str().unwrap_or("")) {
        return Err(format!(
            "outcome '{}' is not allowed for role '{role}'",
            event["outcome"]
        ));
    }
    if outcome == "unavailable" {
        return apply_unavailable(state, event, &assignment);
    }
    if outcome == "contradiction"
        && (state.get("basisProtocol").is_none() || state["basis"].get("sha256").is_none())
    {
        return Err("contradiction requires a marked basis record".into());
    }
    if outcome == "disposition" {
        if role != "lead" || state.get("basisProtocol").is_none() {
            return Err("disposition requires the marked Lead transition".into());
        }
        return crate::run::disposition::apply(state, event);
    }
    if role == "lead" && state["currentStage"] == 1 && !["scoped", "blocked"].contains(&outcome) {
        return Err(format!(
            "outcome '{}' is not allowed for this stage",
            event["outcome"]
        ));
    }
    if role == "lead" && outcome == "accepted" {
        if state["pendingDisposition"].is_object() {
            return Err("acceptance requires the pending Lead disposition".into());
        }
        if let Some(inputs) = event.get("inputsSha256") {
            // Acceptance is judged against the tested inputs it declares, so
            // replay and the live gate apply one rule.
            state["inputsSha256"] = inputs.clone();
        }
        crate::run_exit::reduce(state)?.acceptance_gate()?;
    }
    let mut submission = json!({"stage":event["stage"],"attempt":event["attempt"],"agent":event["agent"],"role":event["role"],"outcome":event["outcome"],"artifact":event["artifact"],"eventSha256":event["eventSha256"]});
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
    if historical.is_some() {
        if outcome == "rework" {
            if historical_review::old_rework_to_lead(state, event)
                && reviewer_transition::apply(state, event)?
            {
                return Ok(());
            }
            return rework_to_worker(state);
        }
        historical_review::complete(state, outcome);
        return Ok(());
    }
    if ["accepted", "rejected", "blocked"].contains(&outcome) {
        state["status"] = json!(outcome);
        return Ok(());
    }
    if outcome == "rework" {
        if reviewer_transition::apply(state, event)? {
            return Ok(());
        }
        return rework_to_worker(state);
    }
    if outcome == "contradiction" {
        state["pendingDisposition"] = json!({
            "owner": "lead",
            "triggerEventSha256": event["eventSha256"],
            "basisSha256": state["basis"]["sha256"],
            "kind": "contradiction",
            "findingSha256s": [event["eventSha256"]]
        });
        state["currentStage"] = json!(lead_stage(state)?);
        return Ok(());
    }
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("run state plan stages are invalid")?;
    let required = stages
        .iter()
        .find(|s| s["stage"] == state["currentStage"])
        .ok_or("run current stage is invalid")?["agents"]
        .as_array()
        .ok_or("run current stage agents are invalid")?
        .len();
    let completed = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?
        .iter()
        .filter(|x| x["stage"] == state["currentStage"] && x["attempt"] == state["attempt"])
        // An `unavailable` reviewer recorded a failure to execute, not a stage
        // completion; counting it would advance the stage on no verdict.
        .filter(|x| x["outcome"] != "unavailable")
        .count();
    if completed == required && state["currentStage"] != stages.len() {
        let current = state["currentStage"]
            .as_u64()
            .ok_or("run current stage is invalid")?;
        state["currentStage"] = json!(current + 1);
        if state["reviewPolicy"]["decision"] == "omitted"
            && state["plan"]["stages"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|stage| {
                    stage["stage"] == state["currentStage"]
                        && stage["agents"].as_array().is_some_and(|agents| {
                            agents.iter().any(|agent| agent["role"] == "reviewer")
                        })
                })
        {
            state["currentStage"] = json!(lead_stage(state)?);
        }
    }
    Ok(())
}

fn rework_to_worker(state: &mut Value) -> Result<(), String> {
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("run state plan stages are invalid")?;
    let worker = stages
        .iter()
        .find(|stage| {
            stage["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == "worker"))
        })
        .ok_or("rework requires a worker stage")?;
    state["currentStage"] = worker["stage"].clone();
    let attempt = state["attempt"].as_u64().ok_or("run attempt is invalid")?;
    state["attempt"] = json!(attempt + 1);
    Ok(())
}

/// Record that a reviewer target could not execute for a bounded operational
/// reason.  This is never a verdict: it neither completes the stage nor
/// advances it, and it is refused outright once any verdict exists for the same
/// stage and attempt, so a substitution can never escape an adverse review.
pub(super) fn apply_unavailable(
    state: &mut Value,
    event: &Value,
    assignment: &Value,
) -> Result<(), String> {
    let stage = event["stage"].clone();
    let attempt = event["attempt"].clone();
    let existing = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?;
    let shopped = existing.iter().any(|submission| {
        submission["stage"] == stage
            && submission["attempt"] == attempt
            && submission["role"] == "reviewer"
            && matches!(submission["outcome"].as_str(), Some("rework" | "blocked"))
    });
    if shopped {
        return Err(
            "unavailable is not admissible after a reviewer verdict; fallback cannot re-open a decided review"
                .into(),
        );
    }
    let prior_unavailable = existing
        .iter()
        .filter(|submission| {
            submission["stage"] == stage
                && submission["attempt"] == attempt
                && submission["outcome"] == "unavailable"
        })
        .count();
    // A substitution is a packet re-issued onto the alternate binding, not one
    // that merely carries an authorized alternative.
    let is_substitution = assignment["substitution"].is_object();
    if is_substitution && prior_unavailable == 0 {
        return Err("a fallback reviewer requires a recorded primary unavailability".into());
    }
    if !is_substitution && prior_unavailable > 0 {
        return Err("the primary reviewer already reported unavailable for this attempt".into());
    }
    state["submissions"]
        .as_array_mut()
        .ok_or("run state submissions are invalid")?
        .push(json!({
            "stage": stage, "attempt": attempt, "agent": event["agent"],
            "role": "reviewer", "outcome": "unavailable",
            "artifact": event["artifact"], "eventSha256": event["eventSha256"],
            "fallback": event["fallback"],
        }));
    // The substitution is bounded: it happens at most once per stage+attempt. A
    // second operational failure — the fallback's own — ends the attempt at
    // blocked rather than searching for a third target.
    if is_substitution {
        state["status"] = json!("blocked");
        return Ok(());
    }
    // With no authorized binding left to execute the same contract, the run has
    // no admissible reviewer and stays blocked.
    if authorized_fallback_runtime(state, event["agent"].as_str().unwrap_or("")).is_none() {
        state["status"] = json!("blocked");
    }
    Ok(())
}

/// The alternate execution binding the plan authorized for `agent`, if any.
fn authorized_fallback_runtime<'a>(state: &'a Value, agent: &str) -> Option<&'a Value> {
    let current = state["currentStage"].as_u64()?;
    state["plan"]["stages"]
        .as_array()?
        .iter()
        .find(|stage| stage["stage"].as_u64() == Some(current))?["agents"]
        .as_array()?
        .iter()
        .find(|selected| selected["name"].as_str() == Some(agent))?
        .get("fallbackRuntime")
        .filter(|binding| binding.is_object())
}

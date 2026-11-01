//! Admission and exact replay for a native Work successor.

use super::*;
use crate::{config::Loaded, evidence::hash, session_goal};
use serde_json::Value;

/// Called while the predecessor run owner holds its lock and before its claim.
pub(crate) fn admit_successor(
    loaded: &Loaded,
    predecessor: &str,
    work: &str,
    start: &Value,
    claimed: bool,
    started: bool,
) -> Result<(String, bool), String> {
    let scope = session_goal::native::scope(loaded, false)?;
    scope.require_owner()?;
    let session = scope
        .session
        .as_ref()
        .ok_or("native Work supersession requires its owner session")?;
    let owner = scope
        .owner
        .as_ref()
        .ok_or("native Work supersession requires its Goal owner")?;
    verify_owner_session(loaded, predecessor, session.clone())?;
    let current = session_goal::read(&scope.root)?.ok_or("native Goal is unavailable")?;
    if current["closure"]["closed"] != false {
        return Err("native Work supersession requires an open Goal".into());
    }
    if !matches!(context_for_work(loaded, predecessor), WorkContext::Owner(parent)
        if parent["goalId"] == current["goalId"])
    {
        return Err("predecessor is not bound to the current native Goal owner".into());
    }
    let intent = crate::run::native_successor_intent(work, start)?;
    let goal = start["goal"].as_str().ok_or("successor Goal is missing")?;
    let pending = session_goal::journey::find_admission(&current, work)?;
    let admissions = current["journeyAdmissions"]
        .as_array()
        .ok_or("native Goal admissions are unavailable")?;
    if let Some(admission) = pending {
        if admissions.len() != 1
            || admission.start_intent_sha256 != intent
            || admission.work_goal_sha256 != hash::text(goal)
            || current["goalId"] != admission.parent_goal_id
        {
            return Err(
                "pending native admission conflicts with the exact successor request".into(),
            );
        }
        admission::validate_admission_for_loaded(loaded, &admission, session, owner)?;
        validate_pending_goal_history(&scope.root, &current, &admission)?;
        return Ok((intent, false));
    }
    if !admissions.is_empty() {
        return Err("native Work supersession is blocked by unresolved Goal admissions".into());
    }
    if claimed || started {
        // A linked exact replay must have its original strong admission in the
        // immutable Goal history. Never attach a legacy started successor.
        if !(claimed && started) {
            return Err(
                "successor claim has no matching native admission; no link was created".into(),
            );
        }
        let mut found = false;
        for record in session_goal::records(&scope.root)? {
            if let Some(prior) = session_goal::journey::find_admission(&record, work)? {
                if prior.start_intent_sha256 == intent
                    && prior.work_goal_sha256 == hash::text(goal)
                    && admission::validate_admission_for_loaded(loaded, &prior, session, owner)
                        .is_ok()
                {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err("started successor has no matching prior strong native admission".into());
        }
        verify_owner_session(loaded, work, session.clone())?;
        return Ok((intent, false));
    }
    let candidate = prepare(&scope)?.ok_or("native Goal cannot admit a successor")?;
    if current["goalId"] != candidate.goal_id || current["eventSha256"] != candidate.event_sha256 {
        return Err("native Goal changed while preparing successor admission".into());
    }
    admit(&scope.root, loaded, &candidate, work, &intent, goal)?;
    Ok((intent, true))
}

/// A pending admission is valid only across its own append and same-owner
/// session adoptions. A changed-and-restored Goal is still an intervening
/// change, so inspect every validated historical revision.
pub(super) fn validate_pending_goal_history(
    root: &std::path::Path,
    current: &Value,
    admission: &session_goal::journey::Admission,
) -> Result<(), String> {
    let history = session_goal::records(root)?;
    let parent = history
        .iter()
        .position(|record| {
            record["goalId"] == admission.parent_goal_id
                && record["eventSha256"] == admission.parent_observed_goal_event_sha256
        })
        .ok_or("native successor admission's Goal observation is unavailable")?;
    let expected = goal_facts(&history[parent])?;
    let mut adoptions = history[parent]["sessionAdoptions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let wanted = current["journeyAdmissions"].clone();
    if history.get(parent + 1).is_none() || history.last() != Some(current) {
        return Err("native successor admission has no current Goal history".into());
    }
    for record in &history[parent + 1..] {
        let next = record["sessionAdoptions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if goal_facts(record)? != expected
            || record["journeyAdmissions"] != wanted
            || !next.starts_with(&adoptions)
        {
            return Err(
                "native Goal changed after successor admission; no new credit was granted".into(),
            );
        }
        adoptions = next;
    }
    Ok(())
}

fn goal_facts(record: &Value) -> Result<Value, String> {
    let mut facts = record.clone();
    let object = facts
        .as_object_mut()
        .ok_or("native Goal record is malformed")?;
    for key in [
        "revision",
        "eventSha256",
        "previousEventSha256",
        "predecessor",
        "successorOf",
        "journeyAdmissions",
        "sessionAdoptions",
    ] {
        object.remove(key);
    }
    Ok(facts)
}

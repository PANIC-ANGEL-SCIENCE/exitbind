//! Forward recovery of one current Lead repair when a later permit exhausted
//! the governor's no-information streak. Historical starts are left intact.

use super::*;

/// Returns the identity of the repair event this call appended, if any. The
/// caller's view (`binding`) is checked under the same lock, before the
/// append, so an event appended after that view is never mistaken for this
/// call's own repair.
pub(super) fn before_worker_completion(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    assignment_handle: &str,
    expected: &AssignmentIdentity,
    binding: &dyn Fn() -> Result<(), String>,
) -> Result<Option<String>, String> {
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, path)?;
        let state = reduce_live(loaded, &events)?;
        super::pre_return::guard_worker_before_effect(
            loaded,
            path,
            &events,
            &state,
            expected,
            assignment_handle,
        )?;
        if state["checkObservationProtocol"] == crate::run::check_observation::PROTOCOL {
            if let Err(error) =
                crate::run::guidance_continuity::assert_config_current(loaded, &state)
            {
                let (bound, current) = crate::drift::split(&error)
                    .map(|(_, value)| {
                        (value["boundSha256"].clone(), value["currentSha256"].clone())
                    })
                    .unwrap_or_default();
                let ledger = path.path.display().to_string();
                return Err(crate::drift::refusal(
                    crate::drift::Class::RepairConfiguration,
                    bound.as_str().unwrap_or_default(),
                    current.as_str(),
                    None,
                    crate::drift::Route::Supersede {
                        ledger: Some(&ledger),
                        config: Some(&loaded.path),
                    },
                ));
            }
        }
        if state["governor"]["state"] != "replan_required" {
            return Ok(None);
        }
        warn_drift(action_drift_warning(loaded, &state)?.as_ref());
        crate::run::artifact::assert_current(loaded, &state)?;
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .find(|item| item["agent"] == expected.agent)
            .ok_or("forward repair has no pending worker assignment")?;
        let work = canonical_work_for_ledger(path)?;
        if !expected.matches(&assignment)
            || crate::run::assignment::handle(&work, &assignment)? != assignment_handle
        {
            return Err("forward repair assignment changed; no mutation was made".into());
        }
        let Some(repair) = run_state::forward_repair_candidate(&state, &events, &assignment) else {
            // A hard stop or an already consumed decision is left for the
            // ordinary completion boundary to refuse and hold truthfully.
            return Ok(None);
        };
        binding()?;
        let inputs = live_inputs(&state)?;
        let last = events.last().ok_or("run ledger has no event head")?;
        let previous = json!({"eventSha256": state["governor"]["headSha256"]});
        let governor_event = crate::context::event(
            Some(&previous),
            json!({
                "action": "replan",
                "repairRecoveryProtocol": 1,
                "dispositionSha256": repair["dispositionSha256"],
                "repairDecision": repair["decision"],
                "identityTransition": "carried_mutation_v1",
                "assignmentPacketSha256": hash::value(&assignment),
                "runId": state["runId"],
                "subjectSha256": state["subject"]["sha256"],
                "attempt": state["attempt"],
                "checkpoint": state["governor"]["spent"],
                "inputSha256": inputs.clone(),
                "lineageSha256": state["governor"]["lineageSha256"],
                "hypothesis": repair["reason"],
                "scopeDecision": repair["repairBoundary"],
                "evidenceRequest": repair["decisiveRegression"],
            }),
        );
        let version = state["version"]
            .as_u64()
            .ok_or("run state is missing version")?;
        let event = run_state::make_event(json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "govern",
            "runId": state["runId"],
            "stage": assignment["stage"],
            "attempt": assignment["attempt"],
            "agent": assignment["agent"],
            "role": assignment["role"],
            "subjectSha256": state["subject"]["sha256"],
            "inputsSha256": inputs,
            "assignmentSha256": hash::text(assignment_handle),
            "operation": "authorized_repair_recovery_v1",
            "governorEvent": governor_event,
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
        }));
        let mut projected = events;
        projected.push(event.clone());
        run_state::reduce(&projected)?;
        let ticket = super::continuation_effect::begin_event(
            loaded,
            &state,
            crate::project::continuation::control::Kind::Effect,
            &event,
        )?;
        append(path, &event, false, &source)?;
        super::continuation_effect::complete(loaded, &ticket, true, true)?;
        Ok(event["eventSha256"].as_str().map(str::to_owned))
    })
}

/// Return the exact prefix before this call's contiguous, recorded effects.
pub(super) fn own_event_prefix(
    events: &[Value],
    own_repair: Option<&str>,
    own_admission: Option<&str>,
) -> Option<usize> {
    if own_repair.is_none() && own_admission.is_none() {
        return None;
    }
    let mut prefix = events.len();
    for expected in [own_admission, own_repair].into_iter().flatten() {
        prefix = prefix.checked_sub(1)?;
        if events[prefix]["eventSha256"].as_str() != Some(expected) {
            return None;
        }
    }
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    fn events(shas: &[&str]) -> Vec<Value> {
        shas.iter().map(|sha| json!({"eventSha256": sha})).collect()
    }

    fn own_event_prefix(
        events: &[Value],
        own_repair: Option<&str>,
        own_admission: Option<&str>,
    ) -> Option<usize> {
        super::own_event_prefix(
            events.get(..events.len().checked_sub(1)?)?,
            own_repair,
            own_admission,
        )
    }

    /// W3 D3: only the repair this call appended, directly before its own
    /// submission, skips the binding check; a repair another call appended
    /// (the pre-F7 count-based reading accepted it) or any later event does not.
    #[test]
    fn only_this_calls_repair_directly_before_the_submission_is_its_own() {
        assert_eq!(
            own_event_prefix(&events(&["view", "own", "submission"]), Some("own"), None),
            Some(1)
        );
        assert_eq!(
            own_event_prefix(&events(&["view", "other", "submission"]), None, None),
            None
        );
        assert_eq!(
            own_event_prefix(&events(&["view", "other", "submission"]), Some("own"), None),
            None
        );
        assert_eq!(
            own_event_prefix(
                &events(&["view", "own", "foreign", "submission"]),
                Some("own"),
                None
            ),
            None
        );
        assert_eq!(
            own_event_prefix(&events(&["submission"]), Some("own"), None),
            None
        );
        assert_eq!(
            own_event_prefix(
                &events(&["view", "repair", "admission", "submission"]),
                Some("repair"),
                Some("admission")
            ),
            Some(1)
        );
        assert_eq!(
            own_event_prefix(
                &events(&["view", "repair", "foreign", "admission", "submission"]),
                Some("repair"),
                Some("admission")
            ),
            None
        );
        assert_eq!(
            own_event_prefix(
                &events(&["view", "admission", "foreign", "submission"]),
                None,
                Some("admission")
            ),
            None
        );
    }
}

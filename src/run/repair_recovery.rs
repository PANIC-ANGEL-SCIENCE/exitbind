//! Forward recovery of one current Lead repair when a later permit exhausted
//! the governor's no-information streak. Historical starts are left intact.

use super::*;

pub(super) fn before_worker_completion(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    assignment_handle: &str,
    expected: &AssignmentIdentity,
) -> Result<(), String> {
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, path)?;
        let state = reduce_live(loaded, &events)?;
        if state["checkObservationProtocol"] == crate::run::check_observation::PROTOCOL
            && hash::text(&loaded.source) != state["configSha256"]
        {
            return Err(
                "forward repair refuses changed configuration; use authorized supersession".into(),
            );
        }
        if state["governor"]["state"] != "replan_required" {
            return Ok(());
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
            return Ok(());
        };
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
        append(path, &event, false, &source)?;
        Ok(())
    })
}

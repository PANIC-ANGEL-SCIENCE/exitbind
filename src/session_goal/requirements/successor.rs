//! Exact contributor transfer for an actual successor Work.
use super::*;
use std::path::Path;

struct VerifiedSuccessor {
    predecessor: String,
    successor: String,
    predecessor_start: String,
    successor_start: String,
    contract: Contract,
}

/// Retire one exact contributor and activate its verified successor in one
/// canonical Goal append. No check, review, acceptance, or grant is transferred.
pub(crate) fn assign_successor(
    root: &Path,
    loaded: &Loaded,
    goal_id: &str,
    ids: &str,
    predecessor: &str,
    successor: &str,
) -> Result<Value, String> {
    super::super::mutate(root, |previous| {
        let previous = previous.ok_or("no canonical named Goal is open")?;
        let proven = verify_actual(root, loaded, predecessor, successor)?;
        if previous["goalId"] != goal_id || proven.contract.goal_id != goal_id {
            return Err("successor belongs to a different named Goal".into());
        }
        let mut goal = Goal::read(previous)?;
        if goal.project_identity != identity(loaded)?
            || goal.project_identity != proven.contract.project_identity
        {
            return Err("successor belongs to a different project".into());
        }
        let references = selected(&goal, ids)?;
        if !transfer(&mut goal, &references, &proven)? {
            return Ok(previous.clone());
        }
        if previous["closure"]["closed"] != false {
            return Err("closed named Goal cannot change contributors".into());
        }
        let mut record = next_record(previous, goal_id)?;
        record["continuation"] = serde_json::to_value(goal).map_err(|e| e.to_string())?;
        Ok(super::super::sealed(record, Some(previous)))
    })
}

/// Verify the ordinary live project-Goal contract and every selected mapping
/// before a fresh retirement claim can make this Work a predecessor.
pub(crate) fn ensure_linked_for_retirement(
    request: PrepareRequest<'_>,
    work_id: &str,
    start_sha256: &str,
) -> Result<(), String> {
    let prepared = prepare(request)?;
    let record = super::super::read(request.root)?.ok_or("no canonical named Goal is open")?;
    let goal = Goal::read(&record)?;
    let refs = selected(&goal, request.ids)?;
    let work_contract = work::contract(request.loaded, work_id)?;
    let current_checker = work::checker_input(request.loaded, request.checker, request.command)?;
    if prepared.project_identity != work_contract.project_identity
        || work_contract.goal_id != request.goal_id
        || work_contract.integration != request.integration
        || work_contract.checker_input != current_checker
        || refs
            .iter()
            .any(|reference| !work_contract.covers(reference))
    {
        return Err(
            "predecessor's current named Goal contract differs from its frozen Work".into(),
        );
    }
    for reference in refs {
        let exact = goal
            .mappings
            .iter()
            .filter(|mapping| {
                mapping.work == work_id
                    && mapping.start_event_sha256 == start_sha256
                    && mapping.requirement == reference
            })
            .count();
        let active = goal
            .mappings
            .iter()
            .filter(|mapping| {
                mapping.work == work_id
                    && mapping.start_event_sha256 == start_sha256
                    && mapping.requirement == reference
                    && mapping.active
            })
            .count();
        if exact != 1 || active != 1 {
            return Err("predecessor lacks its exact active named Goal mapping; no retirement claim was created".into());
        }
    }
    Ok(())
}

fn verify_actual(
    root: &Path,
    loaded: &Loaded,
    predecessor: &str,
    successor: &str,
) -> Result<VerifiedSuccessor, String> {
    if predecessor == successor {
        return Err("successor requires a distinct canonical Work identity".into());
    }
    let old_contract = work::contract(loaded, predecessor)?;
    let contract = work::contract(loaded, successor)?;
    if !work::scope_matches_root(loaded, predecessor, root)?
        || !work::scope_matches_root(loaded, successor, root)?
        || serde_json::to_value(old_contract).map_err(|e| e.to_string())?
            != serde_json::to_value(&contract).map_err(|e| e.to_string())?
    {
        return Err("successor changed the exact frozen named Goal contract or journal".into());
    }
    let old_ledger = crate::work::resolve(loaded, predecessor)?;
    let new_ledger = crate::work::resolve(loaded, successor)?;
    let (old_path, old_events, old_source) = crate::run::ledger::load(loaded, &old_ledger)?;
    let (new_path, new_events, _) = crate::run::ledger::load(loaded, &new_ledger)?;
    if crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&new_path))?.is_some() {
        return Err(
            "successor is already retired; late recovery cannot reactivate an obsolete contributor"
                .into(),
        );
    }
    let old_start = old_events
        .first()
        .ok_or("predecessor start is unavailable")?;
    let new_start = new_events.first().ok_or("successor start is unavailable")?;
    let old_head = old_events.last().ok_or("predecessor head is unavailable")?;
    let old_state = crate::run::state::reduce(&old_events)?;
    let retirement = crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&old_path))?
        .ok_or("predecessor has no actual retirement claim")?;
    let old_hash = hash::text(&old_source);
    if retirement["oldLedgerPath"] != old_ledger
        || retirement["oldLedgerSha256"] != old_hash
        || retirement["oldRunId"] != old_start["runId"]
        || retirement["oldHeadEventSha256"] != old_head["eventSha256"]
        || retirement["oldConfigSha256"] != old_start["configSha256"]
        || retirement["newLedgerPath"] != new_ledger
        || retirement["newRunId"] != new_start["runId"]
        || retirement["workflow"] != new_start["workflow"]
        || retirement["goalSha256"]
            != hash::text(
                new_start["goal"]
                    .as_str()
                    .ok_or("successor Goal is unavailable")?,
            )
        || retirement["configSha256"] != new_start["configSha256"]
        || retirement["timestamp"] != new_start["timestamp"]
        || new_start["supersedes"]
            != json!({
                "ledgerPath":old_ledger,"ledgerSha256":old_hash,"runId":old_start["runId"],
                "headEventSha256":old_head["eventSha256"],"configSha256":old_start["configSha256"]
            })
    {
        return Err(
            "successor does not match the exact actual predecessor retirement and starts".into(),
        );
    }
    for field in [
        "workflow",
        "goal",
        "configSha256",
        "plan",
        "checkPolicy",
        "preservation",
        "preReturnPolicy",
        "basisProtocol",
        "basis",
        "reviewPolicy",
        "recoveryProtocol",
        "observationNoveltyProtocol",
        "checkObservationProtocol",
        "verificationPreparationProtocol",
        "verificationCheckerSources",
        "verificationInputContract",
        "projectContinuation",
    ] {
        if old_state.get(field) != new_start.get(field) {
            return Err(format!(
                "successor changed frozen {field}; contributor transfer refuses expansion"
            ));
        }
    }
    if !crate::work::action::continuation_native_facts(loaded, predecessor, &old_state)?.all_ended {
        return Err("predecessor owned execution is not known ended".into());
    }
    Ok(VerifiedSuccessor {
        predecessor: predecessor.into(),
        successor: successor.into(),
        predecessor_start: old_start["eventSha256"]
            .as_str()
            .ok_or("predecessor start hash is missing")?
            .into(),
        successor_start: new_start["eventSha256"]
            .as_str()
            .ok_or("successor start hash is missing")?
            .into(),
        contract,
    })
}

fn transfer(
    goal: &mut Goal,
    references: &[RequirementRef],
    proven: &VerifiedSuccessor,
) -> Result<bool, String> {
    let matches = |mapping: &Mapping, work: &str, start: &str, reference: &RequirementRef| {
        mapping.work == work
            && mapping.start_event_sha256 == start
            && mapping.requirement == *reference
    };
    let mut additions = 0usize;
    for reference in references {
        if goal.mappings.iter().any(|mapping| {
            mapping.requirement == *reference
                && ((mapping.work == proven.predecessor
                    && mapping.start_event_sha256 != proven.predecessor_start)
                    || (mapping.work == proven.successor
                        && mapping.start_event_sha256 != proven.successor_start))
        }) {
            return Err(
                "contributor mapping does not match its actual immutable Work start".into(),
            );
        }
        if !proven.contract.covers(reference) {
            return Err("successor did not freeze the exact current requirement revision".into());
        }
        let old = goal
            .mappings
            .iter()
            .filter(|m| matches(m, &proven.predecessor, &proven.predecessor_start, reference))
            .collect::<Vec<_>>();
        let new = goal
            .mappings
            .iter()
            .filter(|m| matches(m, &proven.successor, &proven.successor_start, reference))
            .collect::<Vec<_>>();
        if old.len() != 1 || new.len() > 1 {
            return Err("exact predecessor mapping is unavailable or ambiguous".into());
        }
        if !old[0].active && !new.first().is_some_and(|mapping| mapping.active) {
            return Err("predecessor was retired by another Goal decision; no obsolete contributor is revived".into());
        }
        additions += usize::from(new.is_empty());
    }
    if goal
        .mappings
        .len()
        .checked_add(additions)
        .map_or(true, |count| count > MAX_MAPPINGS)
    {
        return Err("declared Work mapping bound exceeded".into());
    }
    let mut changed = false;
    for reference in references {
        for mapping in &mut goal.mappings {
            if mapping.active
                && matches(
                    mapping,
                    &proven.predecessor,
                    &proven.predecessor_start,
                    reference,
                )
            {
                mapping.active = false;
                changed = true;
            }
        }
        if let Some(mapping) = goal
            .mappings
            .iter_mut()
            .find(|m| matches(m, &proven.successor, &proven.successor_start, reference))
        {
            changed |= !mapping.active;
            mapping.active = true;
        } else {
            goal.mappings.push(Mapping {
                work: proven.successor.clone(),
                requirement: reference.clone(),
                start_event_sha256: proven.successor_start.clone(),
                active: true,
            });
            changed = true;
        }
    }
    Ok(changed)
}

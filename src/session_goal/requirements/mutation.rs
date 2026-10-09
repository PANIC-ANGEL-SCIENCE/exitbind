//! Revisioned intake, explicit coverage and idempotent Work mapping.

use super::*;

pub(crate) fn require(
    loaded: &Loaded,
    goal_id: &str,
    id: &str,
    text: &str,
    artifact: &str,
) -> Result<Value, String> {
    model::validate_text(id, 64)?;
    if id.contains(',') {
        return Err("requirement ID cannot contain a comma".into());
    }
    model::validate_text(text, 1024)?;
    let approved = source(loaded, artifact)?;
    if !approved.text.contains(text) {
        return Err("requirement must be an exact excerpt of the approved source file".into());
    }
    let project = identity(loaded)?;
    super::super::mutate(&loaded.state_root, |previous| {
        let previous =
            previous.ok_or("incorporate the agreed goal before adding named requirements")?;
        if previous["goalId"] != goal_id {
            return Err("canonical goal identity does not match".into());
        }
        let mut goal = if named(previous) {
            Goal::read(previous)?
        } else {
            if !previous["continuation"].is_null() {
                return Err("existing single-Work continuation must be preserved; use a distinct canonical goal".into());
            }
            Goal {
                kind: "goal_requirements".into(),
                version: 2,
                project_identity: project.clone(),
                source: approved.clone(),
                requirements: vec![],
                mappings: vec![],
            }
        };
        if goal.project_identity != project {
            return Err("named goal belongs to another project".into());
        }
        if let Some(requirement) = goal.requirements.iter_mut().find(|item| item.id == id) {
            if requirement.text == text
                && requirement.source.sha256 == approved.sha256
                && requirement.source.reference == approved.reference
            {
                return Ok(previous.clone());
            }
            if requirement.history.len() >= MAX_REQUIREMENTS {
                return Err("requirement correction history bound exceeded".into());
            }
            requirement.history.push(requirement.reference());
            requirement.revision = requirement
                .revision
                .checked_add(1)
                .ok_or("requirement revision exhausted")?;
            requirement.text = text.into();
            requirement.source = approved.clone();
        } else {
            if goal.requirements.len() >= MAX_REQUIREMENTS {
                return Err("named requirement bound exceeded".into());
            }
            goal.requirements.push(Requirement {
                id: id.into(),
                revision: 1,
                text: text.into(),
                source: approved.clone(),
                history: vec![],
            });
        }
        goal.source.coverage_confirmed = false;
        let mut record = next_record(previous, goal_id)?;
        record["categories"]["obligations"] = json!("considered");
        record["continuation"] = serde_json::to_value(goal).map_err(|e| e.to_string())?;
        Ok(super::super::sealed(record, Some(previous)))
    })
}

pub(crate) fn cover(loaded: &Loaded, goal_id: &str) -> Result<Value, String> {
    super::super::mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical goal is open")?;
        if previous["goalId"] != goal_id {
            return Err("canonical goal identity does not match".into());
        }
        let mut goal = Goal::read(previous)?;
        if goal.project_identity != identity(loaded)? {
            return Err("named goal belongs to another project".into());
        }
        if goal.source.coverage_confirmed {
            return Ok(previous.clone());
        }
        goal.source.coverage_confirmed = true;
        let mut record = next_record(previous, goal_id)?;
        record["continuation"] = serde_json::to_value(goal).map_err(|e| e.to_string())?;
        Ok(super::super::sealed(record, Some(previous)))
    })
}

/// Add contributors, or explicitly replace the selected requirements' active
/// contributors. Retired mappings stay in history; requirements never shrink.
pub(crate) fn assign(
    loaded: &Loaded,
    goal_id: &str,
    ids: &str,
    work_id: &str,
    replace: bool,
) -> Result<Value, String> {
    let contract = work::contract(loaded, work_id)?;
    let ledger = crate::work::resolve(loaded, work_id)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work has no start event")?["eventSha256"]
        .as_str()
        .ok_or("Work start identity is missing")?
        .to_owned();
    super::super::mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical goal is open")?;
        if previous["goalId"] != goal_id || contract.goal_id != goal_id {
            return Err("Work belongs to a different goal".into());
        }
        let mut goal = Goal::read(previous)?;
        if goal.project_identity != identity(loaded)?
            || contract.project_identity != goal.project_identity
        {
            return Err("Work belongs to a different project".into());
        }
        let references = selected(&goal, ids)?;
        if references.iter().any(|item| !contract.covers(item)) {
            return Err(
                "Work did not freeze coverage of the exact requirement revision before execution"
                    .into(),
            );
        }
        let before = serde_json::to_value(&goal).map_err(|e| e.to_string())?;
        for reference in references {
            if replace {
                for mapping in &mut goal.mappings {
                    if mapping.requirement.id == reference.id
                        && (mapping.work != work_id || mapping.requirement != reference)
                    {
                        mapping.active = false;
                    }
                }
            }
            if let Some(mapping) = goal.mappings.iter_mut().find(|item| {
                item.work == work_id
                    && item.requirement == reference
                    && item.start_event_sha256 == start
            }) {
                mapping.active = true;
            } else {
                if goal.mappings.len() >= MAX_MAPPINGS {
                    return Err("declared Work mapping bound exceeded".into());
                }
                goal.mappings.push(Mapping {
                    work: work_id.into(),
                    requirement: reference,
                    start_event_sha256: start.clone(),
                    active: true,
                });
            }
        }
        let after = serde_json::to_value(goal).map_err(|e| e.to_string())?;
        if before == after {
            return Ok(previous.clone());
        }
        let mut record = next_record(previous, goal_id)?;
        record["continuation"] = after;
        Ok(super::super::sealed(record, Some(previous)))
    })
}

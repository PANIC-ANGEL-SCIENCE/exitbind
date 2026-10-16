//! Restore bookkeeping for an actual successor without transferring evidence.
//! No account lock is held while the existing Goal owner appends mappings.

use super::super::association;
use crate::{config::Loaded, run::ledger, session_goal::requirements};

pub(crate) fn link_successor_goal(loaded: &Loaded, new_ledger: &str) -> Result<(), String> {
    let (_, events, _) = ledger::load(loaded, new_ledger)?;
    let start = events.first().ok_or("successor start is missing")?;
    if start.get("projectContinuation").is_none() {
        return Ok(());
    }
    let bound = association::for_state(loaded, start)?
        .ok_or("successor has no verified continuation claim")?;
    let work = association::ledger_work(new_ledger)?;
    if bound.work != work || work == bound.claim.work || start.get("supersedes").is_none() {
        return Err("Goal linkage requires an actual claimed successor".into());
    }
    let root = crate::work::resolve(loaded, &bound.claim.work)?;
    let (_, roots, _) = ledger::load(loaded, &root)?;
    let root_start = roots.first().ok_or("claimed root start is missing")?;
    if start.get("basis") != root_start.get("basis") {
        return Err("successor changed its frozen named Goal contract".into());
    }
    let plan = &bound.claim.plan;
    let prepared = requirements::prepare(requirements::PrepareRequest {
        root: &loaded.state_root,
        loaded,
        goal_id: &plan.goal_id,
        ids: &plan.requirements.join(","),
        checker: &plan.checker,
        command: &plan.check_command,
        basis: None,
        integration: plan.integration,
        scope: requirements::ContractScope::Project,
    })?;
    let current_basis = crate::kernel::basis::parse_basis_text(&prepared.basis, "basis")?.value();
    if start.get("basis") != Some(&current_basis) {
        return Err("current named Goal/checker differs from the exact frozen successor contract; no mapping was changed".into());
    }
    let start_hash = start["eventSha256"]
        .as_str()
        .ok_or("successor start hash is missing")?;
    if !existing_scope(loaded, &work, &plan.goal_id, &prepared.project_identity)? {
        if let Err(error) =
            requirements::write_scope_companion(loaded, &work, start_hash, &prepared)
        {
            // Another owner may have installed the same immutable companion.
            // Verify its actual bytes; never overwrite or infer a scope.
            if !existing_scope(loaded, &work, &plan.goal_id, &prepared.project_identity)? {
                return Err(error);
            }
        }
    }
    requirements::assign(
        &loaded.state_root,
        loaded,
        &plan.goal_id,
        &plan.requirements.join(","),
        &work,
        false,
    )?;
    Ok(())
}

fn existing_scope(loaded: &Loaded, work: &str, goal: &str, project: &str) -> Result<bool, String> {
    let Some(value) = crate::session_goal::association::read_private_companion(
        loaded,
        work,
        ".named-goal-scope.json",
        "named Goal scope companion",
        "named_goal_scope",
        1,
        &["scope", "goalId", "projectIdentitySha256"],
    )?
    else {
        return Ok(false);
    };
    if value["scope"] != "project"
        || value["goalId"] != goal
        || value["projectIdentitySha256"] != project
    {
        return Err("successor's existing named Goal scope differs; no mapping was changed".into());
    }
    Ok(true)
}

//! The configuration of the project that owns the session's native Goal.
//!
//! A hook takes its project from the payload `cwd` only when no native Goal
//! owner is known. The locations tried here already exist in the session's
//! records — the saved recovery hint and the Goal's journey members — and a
//! location is accepted only when the configuration it loads has the owner
//! identity the Goal's fixed owner record names (identity hash and project
//! ID), never by its path alone. Nothing is written.

use super::hint::{canonical_path, read_hint};
use crate::{config, session_goal};
use serde_json::Value;
use std::time::{Duration, Instant};

/// Distinct locations tried; journey members usually share one.
const MAX_CANDIDATES: usize = 16;
/// The lookup leaves the rest of the host's hook timeout to the event itself.
const BUDGET: Duration = Duration::from_millis(1500);

pub(crate) struct GoalOwner {
    pub(crate) owner: session_goal::native::Owner,
    /// The verified owner configuration and its canonical path, when found.
    pub(crate) config: Option<(String, config::Loaded)>,
    /// The same bounded acquisition, available to the SessionStart display.
    pub(crate) recovery: Option<
        Box<(
            session_goal::native::Session,
            session_goal::native::RecoverySnapshot,
        )>,
    >,
}

/// `None` when the session has no native Goal owner (or its selectors are
/// unusable); the caller then uses the working-directory project.
pub(crate) fn goal_owner(
    payload: &serde_json::Map<String, Value>,
) -> Result<Option<GoalOwner>, String> {
    goal_owner_with_access(payload, session_goal::native::Access::RequireWritable)
}

fn goal_owner_with_access(
    payload: &serde_json::Map<String, Value>,
    access: session_goal::native::Access,
) -> Result<Option<GoalOwner>, String> {
    let Some(session) = session(payload, access) else {
        return Ok(None);
    };
    let Some(snapshot) = session.recovery_snapshot(access)? else {
        return Ok(None);
    };
    let mut result = goal_owner_for_snapshot(&session, &snapshot);
    result.recovery = Some(Box::new((session, snapshot)));
    Ok(Some(result))
}

/// Use a callback's explicit host and payload selector without inheriting an
/// unrelated parent process's native session environment.
pub(crate) fn goal_owner_for_snapshot(
    session: &session_goal::native::Session,
    snapshot: &session_goal::native::RecoverySnapshot,
) -> GoalOwner {
    let started = Instant::now();
    let owner = snapshot.owner.clone();
    let mut candidates = Vec::new();
    if let Ok(Some(hint)) = read_hint(&session.root) {
        if hint.host == session.host
            && hint.session_key == session.key
            && hint.owner_identity_sha256 == owner.identity_sha256
            && hint.owner_project_id == owner.project_id
        {
            candidates.push((hint.owner_config_path, None));
        }
    }
    let correction = &snapshot.owner_config;
    if let Some(binding) = correction {
        candidates.clear();
        if binding.local_identity_sha256 == owner.identity_sha256
            && binding.project_id == owner.project_id
        {
            candidates.push((binding.path.clone(), Some(binding.clone())));
        }
    }
    if let Some(record) = &snapshot.latest {
        let members = record["journeyMembers"].as_array().into_iter().flatten();
        for member in
            members.filter(|member| member["ownerIdentitySha256"] == owner.identity_sha256.as_str())
        {
            if let Some(path) = member["configPath"].as_str() {
                if correction.is_none() && !candidates.iter().any(|(known, _)| known == path) {
                    candidates.push((path.to_owned(), None));
                }
            }
        }
    }
    let config = candidates
        .into_iter()
        .take(MAX_CANDIDATES)
        .take_while(|_| started.elapsed() < BUDGET)
        .find_map(|(path, binding)| {
            let loaded = verified(&path, &owner)?;
            if let Some(expected) = binding.as_ref() {
                let current = session_goal::recovery::config_binding_for_current(&loaded).ok()?;
                if !session_goal::owner_correction::same_owner_binding(&current, expected) {
                    return None;
                }
            }
            Some((path, loaded))
        });
    GoalOwner {
        owner,
        config,
        recovery: None,
    }
}

/// Whether `loaded` is the project the owner record names.
pub(crate) fn is_owner(loaded: &config::Loaded, owner: &session_goal::native::Owner) -> bool {
    session_goal::native::owner_for(loaded).is_ok_and(|local| {
        local.identity_sha256 == owner.identity_sha256 && local.project_id == owner.project_id
    })
}

fn verified(path: &str, owner: &session_goal::native::Owner) -> Option<config::Loaded> {
    if !std::path::Path::new(path).is_absolute() {
        return None;
    }
    let loaded = config::load(Some(path)).ok()?;
    let canonical = canonical_path(&loaded.path, "native owner configuration").ok()?;
    (canonical == path && is_owner(&loaded, owner)).then_some(loaded)
}

/// The host session the event belongs to: the process selectors, which the
/// payload must not contradict; without selectors, the Claude Code payload
/// session ID (a Claude owner record exists only for a Claude session key).
fn session(
    payload: &serde_json::Map<String, Value>,
    access: session_goal::native::Access,
) -> Option<session_goal::native::Session> {
    let reported = payload.get("session_id").and_then(Value::as_str);
    match session_goal::native::current_with_access(access) {
        Ok(Some(session)) => reported
            .map_or(true, |id| session.reports(id))
            .then_some(session),
        Ok(None) => reported.and_then(|id| session_goal::native::claude_session(id).ok().flatten()),
        Err(_) => None,
    }
}

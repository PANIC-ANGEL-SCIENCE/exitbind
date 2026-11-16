// Validates the persisted native Goal recovery extension.
// Refuses invalid or unsupported extensions; see `src/session_goal/recovery.rs`.
//! The persisted `nativeGoalRecovery` extension of a canonical Goal record:
//! its field, request and fixture bounds, decision digest, and carrying the
//! extension forward across a same-Goal rewrite.
use super::{
    ActiveDecision, RecoveryState, FIELD, GROUP_VERSION, MAX_FIXTURES, MAX_REQUESTS, VERSION,
};
use crate::evidence::hash;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn valid_optional_text(value: &Option<String>, max: usize) -> bool {
    value.as_deref().map_or(true, |text| valid_text(text, max))
}

pub(super) fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn validated_state(record: &Value) -> Result<Option<RecoveryState>, String> {
    let Some(value) = record.get(FIELD) else {
        return Ok(None);
    };
    if value["version"] == VERSION
        && (value.get("groups").is_some()
            || value.get("ownerConfig").is_some()
            || value["requests"].as_array().is_some_and(|requests| {
                requests.iter().any(|request| request.get("mode").is_some())
            }))
    {
        return Err("native Goal recovery v1 has unsupported group fields".into());
    }
    let state: RecoveryState = serde_json::from_value(value.clone())
        .map_err(|_| "native Goal recovery extension is malformed or has unknown fields")?;
    validate_state(&state)?;
    Ok(Some(state))
}

pub(super) fn validate_state(state: &RecoveryState) -> Result<(), String> {
    if !matches!(state.version, VERSION | GROUP_VERSION)
        || (state.version == VERSION && !state.groups.is_empty())
        || (state.version == VERSION && state.owner_config.is_some())
        || (state.version == GROUP_VERSION && state.owner_config.is_none())
        || state.groups.len() >= 32
        || state.requests.is_empty()
        || state.requests.len() > MAX_REQUESTS
        || (state.version == GROUP_VERSION
            && !state
                .requests
                .iter()
                .any(|request| request.mode.as_deref() == Some("group-upsert")))
    {
        return Err("native Goal recovery extension is invalid or unsupported".into());
    }
    if let Some(owner) = &state.owner_config {
        if !valid_text(&owner.path, 4096)
            || !valid_text(&owner.product_root, 4096)
            || !valid_text(&owner.state_root, 4096)
            || !valid_optional_text(&owner.project_id, 128)
            || !valid_hash(&owner.config_sha256)
            || !valid_hash(&owner.project_identity_sha256)
            || !valid_hash(&owner.local_identity_sha256)
            || owner.local_identity_sha256 != state.active.owner_identity_sha256
            || owner.project_id != state.active.owner_project_id
        {
            return Err("native Goal recovery owner configuration is invalid".into());
        }
    }
    let mut all_members = BTreeSet::new();
    let mut decisions = Vec::new();
    for decision in state.decisions() {
        validate_decision(decision)?;
        if state.version == GROUP_VERSION
            && (state
                .owner_config
                .as_ref()
                .is_some_and(|owner| super::same_group(owner, &decision.source_config))
                || decisions.iter().any(|prior: &&ActiveDecision| {
                    super::same_group(&prior.source_config, &decision.source_config)
                })
                || decision.goal_id != state.active.goal_id
                || decision.host != state.active.host
                || decision.session_key != state.active.session_key
                || decision.owner_identity_sha256 != state.active.owner_identity_sha256
                || decision.owner_project_id != state.active.owner_project_id
                || decision.historical_fixtures != state.active.historical_fixtures
                || decision.source_members.iter().any(|item| {
                    item.member.local_identity_sha256
                        != decision.source_config.local_identity_sha256
                        || item.member.host != decision.host
                        || item.member.session_key != decision.session_key
                        || item.member.owner_identity_sha256 != decision.owner_identity_sha256
                        || item.member.owner_project_id != decision.owner_project_id
                        || !all_members.insert(item.member.work.clone())
                }))
        {
            return Err("native Goal recovery groups overlap or have different identities".into());
        }
        decisions.push(decision);
    }
    if state.version == GROUP_VERSION
        && (all_members.len() > 32
            || state
                .active
                .historical_fixtures
                .iter()
                .any(|item| all_members.contains(&item.work)))
    {
        return Err(
            "native Goal recovery membership bound or historical overlap is invalid".into(),
        );
    }
    let mut requests = BTreeSet::new();
    for request in &state.requests {
        if !valid_text(&request.request_id, 128)
            || !valid_hash(&request.request_sha256)
            || !valid_hash(&request.preview_sha256)
            || !matches!(request.mode.as_deref(), None | Some("group-upsert"))
            || (state.version == VERSION && request.mode.is_some())
            || !requests.insert(request.request_id.clone())
        {
            return Err("native Goal recovery request history is invalid".into());
        }
    }
    Ok(())
}

fn validate_decision(decision: &ActiveDecision) -> Result<(), String> {
    if decision.source_members.is_empty()
        || decision.source_members.len() > 32
        || decision.historical_fixtures.len() > MAX_FIXTURES
        || !valid_text(&decision.goal_id, 128)
        || decision.applied_from_revision == 0
        || !valid_hash(&decision.applied_from_event_sha256)
        || !matches!(decision.host.as_str(), "codex" | "claude")
        || !valid_hash(&decision.session_key)
        || !valid_hash(&decision.owner_identity_sha256)
        || !valid_text(&decision.reason, 1024)
        || !valid_hash(&decision.source_config.config_sha256)
        || !valid_hash(&decision.source_config.project_identity_sha256)
        || !valid_hash(&decision.source_config.local_identity_sha256)
        || !valid_hash(&decision.decision_sha256)
        || !valid_text(&decision.source_config.path, 4096)
        || !valid_text(&decision.source_config.product_root, 4096)
        || !valid_text(&decision.source_config.state_root, 4096)
        || !valid_optional_text(&decision.source_config.project_id, 128)
    {
        return Err("native Goal recovery extension is invalid or unsupported".into());
    }
    let mut work = BTreeSet::new();
    for member in &decision.source_members {
        if member.member_sha256 != super::super::journey::member_identity(&member.member)
            || !valid_hash(&member.member_sha256)
            || !work.insert(member.member.work.clone())
            || member.member.project_identity_sha256
                != decision.source_config.project_identity_sha256
            || member.member.product_root != decision.source_config.product_root
            || member.member.state_root != decision.source_config.state_root
            || member.endpoint_config.product_root != member.member.product_root
            || member.endpoint_config.state_root != member.member.state_root
            || member.endpoint_config.project_identity_sha256
                != decision.source_config.project_identity_sha256
            || member.endpoint_config.project_id != decision.source_config.project_id
            || member.endpoint_config.local_identity_sha256 != member.member.local_identity_sha256
            || !valid_text(&member.endpoint_config.path, 4096)
            || !valid_hash(&member.endpoint_config.config_sha256)
            || !valid_hash(&member.endpoint_config.local_identity_sha256)
        {
            return Err("native Goal recovery source-member binding is invalid".into());
        }
    }
    let mut fixtures = BTreeSet::new();
    for fixture in &decision.historical_fixtures {
        if !valid_text(&fixture.work, 256)
            || !valid_hash(&fixture.start_event_sha256)
            || !valid_hash(&fixture.member_sha256)
            || !valid_text(&fixture.reason, 1024)
            || !fixtures.insert(fixture.work.clone())
            || work.contains(&fixture.work)
        {
            return Err("native Goal recovery historical disposition is invalid".into());
        }
    }
    let actual = decision_digest(decision)?;
    if actual != decision.decision_sha256 {
        return Err("native Goal recovery decision digest mismatch".into());
    }
    Ok(())
}

pub(super) fn decision_digest(decision: &ActiveDecision) -> Result<String, String> {
    let mut value = serde_json::to_value(decision).map_err(|error| error.to_string())?;
    value
        .as_object_mut()
        .ok_or("native Goal recovery decision is malformed")?
        .remove("decisionSha256");
    Ok(hash::value(&value))
}

pub(crate) fn validate_record(
    record: &Value,
    previous: Option<&Value>,
    line: usize,
) -> Result<(), String> {
    let state = validated_state(record)
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))?;
    if let Some(previous) = previous.filter(|prior| prior["goalId"] == record["goalId"]) {
        let prior = validated_state(previous)?;
        validate_transition(prior.as_ref(), state.as_ref())
            .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))?;
    }
    match state {
        Some(state) => {
            let selection = super::retirement::selected(record, &state.active)
                .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))?;
            let previous_selection = previous.and_then(|prior| prior.get(super::retirement::FIELD));
            let current_selection = record.get(super::retirement::FIELD);
            if let Some(selection) = selection {
                if previous_selection != current_selection {
                    let prior = previous.map(validated_state).transpose()?.flatten()
                        .ok_or_else(|| format!("invalid canonical session goal line {line}: retirement has no prior recovery"))?;
                    if selection.prior_recovery_decision_sha256 != prior.active.decision_sha256 {
                        return Err(format!("invalid canonical session goal line {line}: retirement prior recovery binding changed"));
                    }
                } else {
                    // The journal reader already validated the preceding record.
                    let prior = previous
                        .and_then(|record| record.get(FIELD))
                        .and_then(|recovery| recovery.get("active"))
                        .and_then(|active| active.get("decisionSha256"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("invalid canonical session goal line {line}: retirement has no prior recovery"))?;
                    if prior != selection.recovery_decision_sha256.as_str() {
                        return Err(format!("invalid canonical session goal line {line}: stale retirement selection cannot regain authority"));
                    }
                }
            } else if current_selection.is_some() && current_selection != previous_selection {
                return Err(format!("invalid canonical session goal line {line}: stale retirement selection was not carried from the previous record"));
            }
            Ok(())
        }
        None if record.get(super::retirement::FIELD).is_some() => Err(format!(
            "invalid canonical session goal line {line}: retirement selection has no active recovery"
        )),
        None => Ok(()),
    }
}

fn validate_transition(
    prior: Option<&RecoveryState>,
    next: Option<&RecoveryState>,
) -> Result<(), String> {
    let Some(prior) = prior else {
        if next.is_some_and(|state| {
            state.version == GROUP_VERSION
                && state
                    .requests
                    .last()
                    .and_then(|request| request.mode.as_deref())
                    != Some("group-upsert")
        }) {
            return Err("native Goal recovery v2 requires an explicit group upsert".into());
        }
        return Ok(());
    };
    if prior.version != GROUP_VERSION && !next.is_some_and(|state| state.version == GROUP_VERSION) {
        return Ok(());
    }
    let next = next.ok_or("native Goal recovery groups cannot be dropped")?;
    if next.version != GROUP_VERSION {
        return Err("native Goal recovery v2 cannot be downgraded".into());
    }
    if prior == next {
        return Ok(());
    }
    if prior
        .owner_config
        .as_ref()
        .zip(next.owner_config.as_ref())
        .is_some_and(|(old, new)| old.path != new.path)
    {
        return Err("native Goal recovery owner configuration alias changed".into());
    }
    if next.requests.len() != prior.requests.len() + 1
        || !next.requests.starts_with(&prior.requests)
        || next
            .requests
            .last()
            .and_then(|request| request.mode.as_deref())
            != Some("group-upsert")
    {
        return Err("native Goal recovery group change requires one explicit upsert".into());
    }
    let mut changed = 0;
    for old in prior.decisions() {
        let new = next
            .decisions()
            .find(|item| super::same_group(&old.source_config, &item.source_config))
            .ok_or("native Goal recovery group cannot be removed")?;
        if old.source_config.path != new.source_config.path {
            return Err("native Goal recovery group configuration alias changed".into());
        }
        changed += usize::from(old != new);
    }
    changed += next
        .decisions()
        .filter(|item| {
            !prior
                .decisions()
                .any(|old| super::same_group(&old.source_config, &item.source_config))
        })
        .count();
    if changed > 1 {
        return Err("native Goal recovery upsert can change only one group".into());
    }
    Ok(())
}

pub(crate) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] == next["goalId"] {
        if previous.get(FIELD).is_some() && next.get(FIELD).is_none() {
            next[FIELD] = previous[FIELD].clone();
        }
        if previous["goal"] == next["goal"] {
            if previous.get(super::retirement::FIELD).is_some()
                && next.get(super::retirement::FIELD).is_none()
            {
                next[super::retirement::FIELD] = previous[super::retirement::FIELD].clone();
            }
        } else if let Some(next) = next.as_object_mut() {
            next.remove(super::retirement::FIELD);
        }
    }
}

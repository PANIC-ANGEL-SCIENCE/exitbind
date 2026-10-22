//! The persisted `nativeGoalRecovery` extension of a canonical Goal record:
//! its field, request and fixture bounds, decision digest, and carrying the
//! extension forward across a same-Goal rewrite.
use super::{ActiveDecision, RecoveryState, FIELD, MAX_FIXTURES, MAX_REQUESTS, VERSION};
use crate::evidence::hash;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn valid_optional_text(value: &Option<String>, max: usize) -> bool {
    value.as_deref().map_or(true, |text| valid_text(text, max))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn validated_state(record: &Value) -> Result<Option<RecoveryState>, String> {
    let Some(value) = record.get(FIELD) else {
        return Ok(None);
    };
    let state: RecoveryState = serde_json::from_value(value.clone())
        .map_err(|_| "native Goal recovery extension is malformed or has unknown fields")?;
    validate_state(&state)?;
    Ok(Some(state))
}

pub(super) fn validate_state(state: &RecoveryState) -> Result<(), String> {
    let decision = &state.active;
    if state.version != VERSION
        || state.requests.is_empty()
        || state.requests.len() > MAX_REQUESTS
        || decision.source_members.is_empty()
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
    let mut requests = BTreeSet::new();
    for request in &state.requests {
        if !valid_text(&request.request_id, 128)
            || !valid_hash(&request.request_sha256)
            || !valid_hash(&request.preview_sha256)
            || !requests.insert(request.request_id.clone())
        {
            return Err("native Goal recovery request history is invalid".into());
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

pub(crate) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    validated_state(record)
        .map(|_| ())
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))
}

pub(crate) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] == next["goalId"]
        && previous.get(FIELD).is_some()
        && next.get(FIELD).is_none()
    {
        next[FIELD] = previous[FIELD].clone();
    }
}

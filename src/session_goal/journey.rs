//! Bounded session-journey membership and explicit final-current integration.
//!
//! Context-only membership never proves task acceptance. A local integration
//! Work may refresh only the exact previously accepted members frozen into
//! its immutable Work basis.

use crate::{config::Loaded, evidence::hash};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

/// Frozen integration-marker protocol used inside Work start bases.
pub(crate) const PROTOCOL: u64 = 1;
/// Membership v2 adds pre-start admissions. Old readers that understand v1
/// therefore refuse an in-flight admission instead of treating it as ready.
const MEMBERSHIP_PROTOCOL: u64 = 2;
const MAX_MEMBERS: usize = 32;
const MAX_INTEGRATIONS: usize = 32;
const MAX_ADMISSIONS: usize = 32;
const MAX_PATH_BYTES: usize = 512;
const MAX_MARKER_BYTES: usize = 1024;
const HEADER_PREFIX: &str = "exitbind.session-journey-integration.v1.header:";
const MEMBER_PREFIX: &str = "exitbind.session-journey-integration.v1.member:";
const RESERVED_PREFIX: &str = "exitbind.session-journey-integration.";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Member {
    work: String,
    start_event_sha256: String,
    parent_goal_id: String,
    parent_observed_goal_event_sha256: String,
    work_goal_sha256: String,
    host: String,
    session_key: String,
    owner_identity_sha256: String,
    owner_project_id: Option<String>,
    project_identity_sha256: String,
    local_identity_sha256: String,
    config_path: String,
    config_sha256: String,
    product_root: String,
    state_root: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntegrationEntry {
    member: Member,
    contract_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Admission {
    pub(crate) work: String,
    pub(crate) start_intent_sha256: String,
    pub(crate) parent_goal_id: String,
    pub(crate) parent_observed_goal_event_sha256: String,
    pub(crate) work_goal_sha256: String,
    pub(crate) host: String,
    pub(crate) session_key: String,
    pub(crate) owner_identity_sha256: String,
    pub(crate) owner_project_id: Option<String>,
    pub(crate) project_identity_sha256: String,
    pub(crate) local_identity_sha256: String,
    pub(crate) config_path: String,
    pub(crate) config_sha256: String,
    pub(crate) product_root: String,
    pub(crate) state_root: String,
}

/// Inputs that bind a Work admission to the observed native Goal and owner.
pub(crate) struct AdmissionRequest<'a> {
    pub(crate) work: &'a str,
    pub(crate) start_intent_sha256: &'a str,
    pub(crate) work_goal: &'a str,
    pub(crate) parent_goal_id: &'a str,
    pub(crate) parent_event_sha256: &'a str,
    pub(crate) session: &'a super::native::Session,
    pub(crate) owner: &'a super::native::Owner,
}

/// Exact Work/start and parent identities used when linking a verified start.
#[derive(Clone, Copy)]
pub(crate) struct WorkLink<'a> {
    pub(crate) work: &'a str,
    pub(crate) start_event_sha256: &'a str,
    pub(crate) work_goal: &'a str,
    pub(crate) parent_goal_id: &'a str,
    pub(crate) parent_event_sha256: &'a str,
    pub(crate) session: &'a super::native::Session,
    pub(crate) owner: &'a super::native::Owner,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CoveredMember {
    #[serde(rename = "w")]
    work: String,
    #[serde(rename = "m")]
    member_sha256: String,
    #[serde(rename = "a")]
    accepted_endpoint_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    #[serde(rename = "v")]
    protocol: u64,
    #[serde(rename = "h")]
    host: String,
    #[serde(rename = "s")]
    session_key: String,
    #[serde(rename = "o")]
    owner_identity_sha256: String,
    #[serde(rename = "op")]
    owner_project_id: Option<String>,
    #[serde(rename = "p")]
    project_identity_sha256: String,
    #[serde(rename = "l")]
    local_identity_sha256: String,
    #[serde(rename = "c")]
    config_binding_sha256: String,
    #[serde(rename = "k")]
    checker: super::requirements::CheckerInput,
}

#[derive(Clone, Debug, Default)]
pub(super) struct JourneyState {
    protocol: u64,
    members: Vec<Member>,
    integrations: Vec<IntegrationEntry>,
    admissions: Vec<Admission>,
}

pub(crate) struct Prepared {
    basis: String,
}

impl Prepared {
    pub(crate) fn basis(&self) -> &str {
        &self.basis
    }
}

#[derive(Debug)]
pub(crate) struct Readiness {
    pub(crate) protocol: u64,
    pub(crate) state: &'static str,
    pub(crate) reason: Option<String>,
    pub(crate) total: usize,
    pub(crate) current: usize,
    pub(crate) integrated: usize,
    pub(crate) pending_admissions: usize,
    pub(crate) unresolved: Vec<Value>,
    pub(crate) integration_work: Option<String>,
}

impl Readiness {
    pub(crate) fn ready(&self) -> bool {
        self.state == "current"
    }

    pub(crate) fn value(&self) -> Value {
        json!({
            "protocol": self.protocol,
            "state": self.state,
            "reason": self.reason,
            "members": {"total":self.total,"current":self.current,"integrationSupported":self.integrated},
            "pendingAdmissions": self.pending_admissions,
            "unresolved": self.unresolved,
            "integrationWork": self.integration_work,
            "readOnly": true,
        })
    }
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_work(value: &str) -> bool {
    crate::work::valid_work_handle(value)
}

fn canonical_path(path: &Path, label: &str) -> Result<String, String> {
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("{label} unavailable: {error}"))?;
    let text = canonical
        .to_str()
        .ok_or_else(|| format!("{label} is not valid UTF-8"))?;
    if text.len() > MAX_PATH_BYTES || text.contains('\0') {
        return Err(format!("{label} exceeds its bounded journey identity"));
    }
    Ok(text.to_owned())
}

fn valid_absolute_path(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && value.len() <= MAX_PATH_BYTES
        && !value.contains('\0')
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

fn validate_member(value: &Member) -> Result<(), String> {
    if !valid_work(&value.work)
        || !valid_sha(&value.start_event_sha256)
        || !valid_sha(&value.parent_observed_goal_event_sha256)
        || !valid_sha(&value.work_goal_sha256)
        || value.parent_goal_id.trim().is_empty()
        || value.parent_goal_id.len() > 128
        || value.parent_goal_id.contains('\0')
        || !matches!(value.host.as_str(), "codex" | "claude")
        || !valid_sha(&value.session_key)
        || !valid_sha(&value.owner_identity_sha256)
        || !(value.owner_project_id.is_none()
            || value
                .owner_project_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128 && !id.contains('\0')))
        || !valid_sha(&value.project_identity_sha256)
        || !valid_sha(&value.local_identity_sha256)
        || !valid_sha(&value.config_sha256)
        || !valid_absolute_path(&value.config_path)
        || !valid_absolute_path(&value.product_root)
        || !valid_absolute_path(&value.state_root)
    {
        return Err("native journey member is invalid or unsupported".into());
    }
    Ok(())
}

pub(super) fn state(record: &Value) -> Result<Option<JourneyState>, String> {
    let has_protocol = record.get("journeyMembershipProtocol").is_some();
    let has_members = record.get("journeyMembers").is_some();
    let has_integrations = record.get("journeyIntegrations").is_some();
    let has_admissions = record.get("journeyAdmissions").is_some();
    if !has_protocol && !has_members && !has_integrations && !has_admissions {
        return Ok(None);
    }
    let membership_protocol = record["journeyMembershipProtocol"]
        .as_u64()
        .ok_or("unsupported native journey membership protocol")?;
    if !matches!(membership_protocol, 1 | MEMBERSHIP_PROTOCOL)
        || (has_admissions && membership_protocol != MEMBERSHIP_PROTOCOL)
        || !has_members
        || !has_integrations
        || (membership_protocol == MEMBERSHIP_PROTOCOL && !has_admissions)
    {
        return Err("unsupported native journey membership protocol".into());
    }
    let read_entries = |field: &str, max: usize| -> Result<Vec<Value>, String> {
        let Some(value) = record.get(field) else {
            if field == "journeyAdmissions" && membership_protocol == 1 {
                return Ok(Vec::new());
            }
            return Err(format!("native journey {field} list is malformed"));
        };
        let raw = value
            .as_array()
            .ok_or_else(|| format!("native journey {field} list is malformed"))?;
        if raw.len() > max {
            return Err(format!("native journey {field} bound is invalid"));
        }
        Ok(raw.clone())
    };
    let mut members = Vec::new();
    let mut integrations = Vec::new();
    let mut seen = BTreeSet::new();
    for item in read_entries("journeyMembers", MAX_MEMBERS)? {
        let member: Member = serde_json::from_value(item)
            .map_err(|_| "native journey member is malformed or has unknown fields")?;
        validate_member(&member)?;
        if !seen.insert(member.work.clone()) {
            return Err("native journey member is duplicated".into());
        }
        members.push(member);
    }
    for item in read_entries("journeyIntegrations", MAX_INTEGRATIONS)? {
        let integration: IntegrationEntry = serde_json::from_value(item)
            .map_err(|_| "native journey integration is malformed or has unknown fields")?;
        validate_member(&integration.member)?;
        if !valid_sha(&integration.contract_sha256) || !seen.insert(integration.member.work.clone())
        {
            return Err("native journey integration identity is invalid or duplicated".into());
        }
        integrations.push(integration);
    }
    let mut admissions = Vec::new();
    for item in read_entries("journeyAdmissions", MAX_ADMISSIONS)? {
        let admission: Admission = serde_json::from_value(item)
            .map_err(|_| "native journey admission is malformed or has unknown fields")?;
        validate_admission(&admission)?;
        if !seen.insert(admission.work.clone()) {
            return Err("native journey admission duplicates an existing Work identity".into());
        }
        admissions.push(admission);
    }
    Ok(Some(JourneyState {
        protocol: membership_protocol,
        members,
        integrations,
        admissions,
    }))
}

fn validate_admission(value: &Admission) -> Result<(), String> {
    if !valid_work(&value.work)
        || !valid_sha(&value.start_intent_sha256)
        || !valid_sha(&value.parent_observed_goal_event_sha256)
        || !valid_sha(&value.work_goal_sha256)
        || value.parent_goal_id.trim().is_empty()
        || value.parent_goal_id.len() > 128
        || value.parent_goal_id.contains('\0')
        || !matches!(value.host.as_str(), "codex" | "claude")
        || !valid_sha(&value.session_key)
        || !valid_sha(&value.owner_identity_sha256)
        || !(value.owner_project_id.is_none()
            || value
                .owner_project_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128 && !id.contains('\0')))
        || !valid_sha(&value.project_identity_sha256)
        || !valid_sha(&value.local_identity_sha256)
        || !valid_sha(&value.config_sha256)
        || !valid_absolute_path(&value.config_path)
        || !valid_absolute_path(&value.product_root)
        || !valid_absolute_path(&value.state_root)
    {
        return Err("native journey admission is invalid or unsupported".into());
    }
    Ok(())
}

pub(crate) fn ensure_member_capacity(record: &Value) -> Result<(), String> {
    if state(record)?.is_some_and(|journey| {
        journey
            .members
            .len()
            .saturating_add(journey.admissions.len())
            >= MAX_MEMBERS
    }) {
        return Err("native journey Work bound reached; no Work was started".into());
    }
    Ok(())
}

pub(crate) fn ensure_admission_capacity(record: &Value) -> Result<(), String> {
    if record["journeyMembershipProtocol"].as_u64() != Some(MEMBERSHIP_PROTOCOL) {
        return Err(
            "native session Goal needs explicit journey protocol v2 before a Work can be admitted"
                .into(),
        );
    }
    if state(record)?.is_some_and(|journey| journey.admissions.len() >= MAX_ADMISSIONS) {
        return Err("native journey pending-admission bound reached; no Work was started".into());
    }
    Ok(())
}

pub(crate) fn find_admission(record: &Value, work: &str) -> Result<Option<Admission>, String> {
    Ok(state(record)?.and_then(|journey| {
        journey
            .admissions
            .into_iter()
            .find(|item| item.work == work)
    }))
}

pub(crate) fn new_admission(
    loaded: &Loaded,
    request: AdmissionRequest<'_>,
) -> Result<Admission, String> {
    let AdmissionRequest {
        work,
        start_intent_sha256,
        work_goal,
        parent_goal_id,
        parent_event_sha256,
        session,
        owner,
    } = request;
    let admission = Admission {
        work: work.to_owned(),
        start_intent_sha256: start_intent_sha256.to_owned(),
        parent_goal_id: parent_goal_id.to_owned(),
        parent_observed_goal_event_sha256: parent_event_sha256.to_owned(),
        work_goal_sha256: hash::text(work_goal),
        host: session.host.to_owned(),
        session_key: session.key.clone(),
        owner_identity_sha256: owner.identity_sha256.clone(),
        owner_project_id: owner.project_id.clone(),
        project_identity_sha256: super::requirements::identity(loaded)?,
        local_identity_sha256: super::native::owner_for(loaded)?.identity_sha256,
        config_path: canonical_path(&loaded.path, "configured project path")?,
        config_sha256: hash::text(&loaded.source),
        product_root: canonical_path(&loaded.product_root, "configured product root")?,
        state_root: canonical_path(&loaded.state_root, "configured state root")?,
    };
    validate_admission(&admission)?;
    Ok(admission)
}

pub(crate) fn append_admission(record: &mut Value, admission: Admission) -> Result<(), String> {
    validate_admission(&admission)?;
    let mut journey = state(record)?.ok_or("native session Goal has no explicit journey protocol; explicitly incorporate it before starting a Work")?;
    if journey.admissions.len() >= MAX_ADMISSIONS {
        return Err("native journey pending-admission bound reached; no Work was started".into());
    }
    if journey
        .members
        .iter()
        .any(|item| item.work == admission.work)
        || journey
            .integrations
            .iter()
            .any(|item| item.member.work == admission.work)
        || journey
            .admissions
            .iter()
            .any(|item| item.work == admission.work)
    {
        return Err(
            "native journey Work identity already has a member or pending admission".into(),
        );
    }
    journey.admissions.push(admission);
    write_state(record, journey);
    Ok(())
}

pub(crate) fn remove_admission(record: &mut Value, work: &str) -> Result<bool, String> {
    let Some(mut journey) = state(record)? else {
        return Ok(false);
    };
    let before = journey.admissions.len();
    journey.admissions.retain(|item| item.work != work);
    if before != journey.admissions.len() {
        write_state(record, journey);
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(crate) fn member_for_work(record: &Value, work: &str) -> Result<Option<Value>, String> {
    let Some(journey) = state(record)? else {
        return Ok(None);
    };
    let member = journey
        .members
        .into_iter()
        .find(|item| item.work == work)
        .or_else(|| {
            journey
                .integrations
                .into_iter()
                .find(|item| item.member.work == work)
                .map(|item| item.member)
        });
    member
        .map(|item| serde_json::to_value(item).map_err(|error| error.to_string()))
        .transpose()
}

/// Resolve the original journey member for this Work, following only fully
/// validated same-goal successor claims already in the run ledger.
pub(crate) fn member_for_work_with_lineage(
    loaded: &Loaded,
    record: &Value,
    work: &str,
) -> Result<Option<Value>, String> {
    let Some(journey) = state(record)? else {
        return Ok(None);
    };
    let candidates = journey
        .members
        .into_iter()
        .chain(journey.integrations.into_iter().map(|entry| entry.member))
        .collect::<Vec<_>>();
    for member in candidates {
        if member.work == work || integration::descends_from_member(loaded, &member, work)? {
            return serde_json::to_value(member)
                .map(Some)
                .map_err(|error| error.to_string());
        }
    }
    Ok(None)
}

pub(crate) fn validate_member_for_session(
    loaded: &Loaded,
    value: &Value,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<(), String> {
    let member: Member = serde_json::from_value(value.clone())
        .map_err(|_| "native journey member is malformed or unsupported".to_owned())?;
    validate_member(&member)?;
    if member.host != session.host
        || member.session_key != session.key
        || member.owner_identity_sha256 != owner.identity_sha256
        || member.owner_project_id != owner.project_id
    {
        return Err("native journey member belongs to another session or owner".into());
    }
    let original = integration::project_loaded(loaded, &member)?;
    let header = Header {
        protocol: PROTOCOL,
        host: member.host.clone(),
        session_key: member.session_key.clone(),
        owner_identity_sha256: member.owner_identity_sha256.clone(),
        owner_project_id: member.owner_project_id.clone(),
        project_identity_sha256: member.project_identity_sha256.clone(),
        local_identity_sha256: member.local_identity_sha256.clone(),
        config_binding_sha256: config_binding(&original)?,
        checker: super::requirements::CheckerInput {
            path: String::new(),
            sha256: String::new(),
        },
    };
    validate_member_loaded(&original, &member, &header)
}

pub(super) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    state(record)
        .map(|_| ())
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))
}

pub(super) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] != next["goalId"]
        || previous.get("journeyMembershipProtocol").is_none()
        || next.get("journeyMembershipProtocol").is_some()
    {
        return;
    }
    for key in [
        "journeyMembershipProtocol",
        "journeyMembers",
        "journeyIntegrations",
    ] {
        next[key] = previous[key].clone();
    }
    if previous.get("journeyAdmissions").is_some() {
        next["journeyAdmissions"] = previous["journeyAdmissions"].clone();
    }
}

/// Explicit native-session Goal incorporation initializes this protocol and
/// carries the same conversation's memberships across an explicit Goal ID.
pub(crate) fn transition(
    previous: Option<&Value>,
    next: &mut Value,
    native_session: bool,
) -> Result<(), String> {
    if !native_session {
        return Ok(());
    }
    let carried = previous
        .map(state)
        .transpose()?
        .flatten()
        .unwrap_or_default();
    next["journeyMembershipProtocol"] = json!(MEMBERSHIP_PROTOCOL);
    next["journeyMembers"] = json!(carried.members);
    next["journeyIntegrations"] = json!(carried.integrations);
    next["journeyAdmissions"] = json!(carried.admissions);
    Ok(())
}

fn member_identity(member: &Member) -> String {
    hash::value(&serde_json::to_value(member).expect("journey member serializes"))
}

fn new_member(loaded: &Loaded, request: WorkLink<'_>) -> Result<Member, String> {
    let WorkLink {
        work,
        start_event_sha256,
        work_goal,
        parent_goal_id,
        parent_event_sha256,
        session,
        owner,
    } = request;
    let member = Member {
        work: work.to_owned(),
        start_event_sha256: start_event_sha256.to_owned(),
        parent_goal_id: parent_goal_id.to_owned(),
        parent_observed_goal_event_sha256: parent_event_sha256.to_owned(),
        work_goal_sha256: hash::text(work_goal),
        host: session.host.to_owned(),
        session_key: session.key.clone(),
        owner_identity_sha256: owner.identity_sha256.clone(),
        owner_project_id: owner.project_id.clone(),
        project_identity_sha256: super::requirements::identity(loaded)?,
        local_identity_sha256: super::native::owner_for(loaded)?.identity_sha256,
        config_path: canonical_path(&loaded.path, "configured project path")?,
        config_sha256: hash::text(&loaded.source),
        product_root: canonical_path(&loaded.product_root, "configured product root")?,
        state_root: canonical_path(&loaded.state_root, "configured state root")?,
    };
    validate_member(&member)?;
    Ok(member)
}

fn write_state(record: &mut Value, state: JourneyState) {
    record["journeyMembershipProtocol"] = json!(MEMBERSHIP_PROTOCOL);
    record["journeyMembers"] = json!(state.members);
    record["journeyIntegrations"] = json!(state.integrations);
    record["journeyAdmissions"] = json!(state.admissions);
}

fn config_binding(loaded: &Loaded) -> Result<String, String> {
    Ok(hash::value(&json!({
        "configPath": canonical_path(&loaded.path, "configured project path")?,
        "configSha256": hash::text(&loaded.source),
    })))
}
mod integration;
mod readiness;

use integration::{
    accepted_endpoint, current_integration_for_project, project_loaded, validate_member_loaded,
};

#[cfg(test)]
#[path = "journey/tests.rs"]
mod tests;

pub(crate) use integration::{append_work, prepare, reject_unprepared_markers};
#[cfg(test)]
pub(super) use readiness::readiness_for_state;
pub(crate) use readiness::{
    progress_for_record, readiness_for_record, require_close_ready,
    require_resolved_replacement_admissions, result_ref_current_or_supported,
};

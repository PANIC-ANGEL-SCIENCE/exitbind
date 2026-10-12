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

#[derive(Clone, Debug)]
struct Contract {
    header: Header,
    members: Vec<CoveredMember>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct JourneyState {
    protocol: u64,
    members: Vec<Member>,
    integrations: Vec<IntegrationEntry>,
    admissions: Vec<Admission>,
}

pub(crate) struct Prepared {
    pub(crate) basis: String,
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
    if state(record)?.is_some_and(|journey| journey.members.len() >= MAX_MEMBERS) {
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
    work: &str,
    start_intent_sha256: &str,
    work_goal: &str,
    parent_goal_id: &str,
    parent_event_sha256: &str,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<Admission, String> {
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
    let header = Header {
        protocol: PROTOCOL,
        host: member.host.clone(),
        session_key: member.session_key.clone(),
        owner_identity_sha256: member.owner_identity_sha256.clone(),
        owner_project_id: member.owner_project_id.clone(),
        project_identity_sha256: member.project_identity_sha256.clone(),
        local_identity_sha256: member.local_identity_sha256.clone(),
        config_binding_sha256: config_binding(loaded)?,
        checker: super::requirements::CheckerInput {
            path: String::new(),
            sha256: String::new(),
        },
    };
    validate_member_loaded(loaded, &member, &header)
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

fn new_member(
    loaded: &Loaded,
    work: &str,
    start_event_sha256: &str,
    work_goal: &str,
    parent_goal_id: &str,
    parent_event_sha256: &str,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<Member, String> {
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

/// A Work is either a context-only journey member or the explicitly selected
/// integration proof for the exact snapshot in its frozen start basis.
pub(crate) fn append_work(
    record: &mut Value,
    loaded: &Loaded,
    work: &str,
    start_event_sha256: &str,
    work_goal: &str,
    parent_goal_id: &str,
    parent_event_sha256: &str,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<(), String> {
    let Some(mut journey) = state(record)? else {
        return Ok(());
    };
    if journey.members.len() >= MAX_MEMBERS && journey.integrations.len() >= MAX_INTEGRATIONS {
        return Err("native journey Work bound reached; no new membership was recorded".into());
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work ledger has no start event")?;
    if start["action"] != "start" || start["eventSha256"] != start_event_sha256 {
        return Err("Work start identity changed before native journey linkage".into());
    }
    if let Some(contract) = parse_contract(start)? {
        validate_header_for_loaded(loaded, &contract.header)?;
        if contract.header.host != session.host
            || contract.header.session_key != session.key
            || contract.header.owner_identity_sha256 != owner.identity_sha256
            || contract.header.owner_project_id != owner.project_id
        {
            return Err("journey integration belongs to another native session or owner".into());
        }
        let expected = covered_snapshot(loaded, &journey.members, &contract.header)?;
        if expected != contract.members {
            return Err(
                "journey integration snapshot changed before its Work start was linked".into(),
            );
        }
        let member = new_member(
            loaded,
            work,
            start_event_sha256,
            work_goal,
            parent_goal_id,
            parent_event_sha256,
            session,
            owner,
        )?;
        if journey.integrations.len() >= MAX_INTEGRATIONS
            || journey
                .integrations
                .iter()
                .any(|entry| entry.member.work == work)
        {
            return Err("native journey integration bound or identity is invalid".into());
        }
        journey.integrations.push(IntegrationEntry {
            member,
            contract_sha256: contract_sha256(&contract),
        });
    } else {
        let member = new_member(
            loaded,
            work,
            start_event_sha256,
            work_goal,
            parent_goal_id,
            parent_event_sha256,
            session,
            owner,
        )?;
        if journey.members.len() >= MAX_MEMBERS
            || journey.members.iter().any(|item| item.work == work)
            || journey
                .integrations
                .iter()
                .any(|entry| entry.member.work == work)
        {
            return Err("native journey member bound or identity is invalid".into());
        }
        journey.members.push(member);
    }
    write_state(record, journey);
    Ok(())
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

fn validate_header_for_loaded(loaded: &Loaded, header: &Header) -> Result<(), String> {
    let scope = super::native::scope(loaded, false)?;
    let session = scope
        .session
        .ok_or("journey integration requires a current native session identity")?;
    let owner = scope
        .owner
        .ok_or("journey integration requires the fixed native session owner")?;
    let local = super::native::owner_for(loaded)?;
    if header.protocol != PROTOCOL
        || !matches!(header.host.as_str(), "codex" | "claude")
        || !valid_sha(&header.session_key)
        || !valid_sha(&header.owner_identity_sha256)
        || !(header.owner_project_id.is_none()
            || header
                .owner_project_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128))
        || !valid_sha(&header.project_identity_sha256)
        || !valid_sha(&header.local_identity_sha256)
        || !valid_sha(&header.config_binding_sha256)
        || !valid_sha(&header.checker.sha256)
        || header.checker.path.trim().is_empty()
        || header.checker.path.len() > 256
        || header.checker.path.contains('\0')
        || header.host != session.host
        || header.session_key != session.key
        || header.owner_identity_sha256 != owner.identity_sha256
        || header.owner_project_id != owner.project_id
        || header.project_identity_sha256 != super::requirements::identity(loaded)?
        || header.local_identity_sha256 != local.identity_sha256
        || header.config_binding_sha256 != config_binding(loaded)?
    {
        return Err("journey integration scope or configured checker binding changed".into());
    }
    Ok(())
}

fn marker(prefix: &str, value: &impl Serialize) -> Result<String, String> {
    let marker = format!(
        "{prefix}{}",
        serde_json::to_string(value).map_err(|error| error.to_string())?
    );
    if marker.len() > MAX_MARKER_BYTES {
        return Err("journey integration marker exceeds the frozen 1024-byte basis bound".into());
    }
    Ok(marker)
}

fn parse_contract(start: &Value) -> Result<Option<Contract>, String> {
    let Some(basis) = start.get("basis") else {
        return Ok(None);
    };
    let constraints = basis["constraints"]
        .as_array()
        .ok_or("Work basis constraints are malformed")?;
    let mut header_markers = Vec::new();
    let mut member_markers = Vec::new();
    for value in constraints.iter().filter_map(Value::as_str) {
        if value.starts_with(RESERVED_PREFIX) && value.len() > MAX_MARKER_BYTES {
            return Err("journey integration marker exceeds its bounded size".into());
        }
        if let Some(raw) = value.strip_prefix(HEADER_PREFIX) {
            header_markers.push(raw);
        } else if let Some(raw) = value.strip_prefix(MEMBER_PREFIX) {
            member_markers.push(raw);
        } else if value.starts_with(RESERVED_PREFIX) {
            return Err("unsupported journey integration marker".into());
        }
    }
    if header_markers.is_empty() && member_markers.is_empty() {
        return Ok(None);
    }
    if header_markers.len() != 1 || member_markers.is_empty() || member_markers.len() > MAX_MEMBERS
    {
        return Err(
            "journey integration header/member markers are missing, duplicated or oversized".into(),
        );
    }
    let header: Header = serde_json::from_str(header_markers[0])
        .map_err(|_| "journey integration header is malformed or has unknown fields")?;
    let mut members = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in member_markers {
        let member: CoveredMember = serde_json::from_str(raw)
            .map_err(|_| "journey integration member is malformed or has unknown fields")?;
        if !valid_work(&member.work)
            || !valid_sha(&member.member_sha256)
            || !valid_sha(&member.accepted_endpoint_sha256)
            || !seen.insert(member.work.clone())
        {
            return Err("journey integration member identity is invalid or duplicated".into());
        }
        members.push(member);
    }
    members.sort_by(|a, b| a.work.cmp(&b.work));
    Ok(Some(Contract { header, members }))
}

fn contract_sha256(contract: &Contract) -> String {
    hash::value(&json!({"header":&contract.header,"members":&contract.members}))
}

fn member_contract_marker(contract: &Contract) -> Result<Vec<String>, String> {
    let mut result = vec![marker(HEADER_PREFIX, &contract.header)?];
    result.extend(
        contract
            .members
            .iter()
            .map(|member| marker(MEMBER_PREFIX, member))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(result)
}

fn current_scope(
    loaded: &Loaded,
) -> Result<
    (
        super::native::Scope,
        super::native::Session,
        super::native::Owner,
    ),
    String,
> {
    let scope = super::native::scope(loaded, false)?;
    let session = scope
        .session
        .clone()
        .ok_or("journey integration requires a current native session identity")?;
    let owner = scope
        .owner
        .clone()
        .ok_or("journey integration requires an incorporated current session Goal")?;
    Ok((scope, session, owner))
}

/// Freeze the exact accepted local journey set before creating the integration
/// Work's immutable start event.
pub(crate) fn prepare(
    loaded: &Loaded,
    supplied_basis: Option<&str>,
    checker: &str,
    command: &str,
) -> Result<Prepared, String> {
    let (scope, session, owner) = current_scope(loaded)?;
    let record = super::read(&scope.root)?.ok_or("no native session Goal is incorporated")?;
    if record["closure"]["closed"] != false {
        return Err("native session Goal is closed; start a new side Work to reopen the journey before integration".into());
    }
    let journey = state(&record)?.ok_or("native session Goal has no explicit journey membership protocol; explicitly incorporate it before integration")?;
    if journey.integrations.len() >= MAX_INTEGRATIONS {
        return Err("native journey integration bound reached; no Work was started".into());
    }
    let project_identity_sha256 = super::requirements::identity(loaded)?;
    let local_identity_sha256 = super::native::owner_for(loaded)?.identity_sha256;
    let config_binding_sha256 = config_binding(loaded)?;
    let checker = super::requirements::checker_input(loaded, checker, command)?;
    let mut covered = covered_snapshot(
        loaded,
        &journey.members,
        &Header {
            protocol: PROTOCOL,
            host: session.host.to_owned(),
            session_key: session.key.clone(),
            owner_identity_sha256: owner.identity_sha256.clone(),
            owner_project_id: owner.project_id.clone(),
            project_identity_sha256: project_identity_sha256.clone(),
            local_identity_sha256: local_identity_sha256.clone(),
            config_binding_sha256: config_binding_sha256.clone(),
            checker: checker.clone(),
        },
    )?;
    covered.sort_by(|a, b| a.work.cmp(&b.work));
    if covered.is_empty() {
        return Err("no native journey members are recorded for this configured project".into());
    }
    let contract = Contract {
        header: Header {
            protocol: PROTOCOL,
            host: session.host.to_owned(),
            session_key: session.key,
            owner_identity_sha256: owner.identity_sha256,
            owner_project_id: owner.project_id,
            project_identity_sha256,
            local_identity_sha256,
            config_binding_sha256,
            checker,
        },
        members: covered,
    };
    validate_header_for_loaded(loaded, &contract.header)?;
    let mut basis = if let Some(basis) = supplied_basis {
        crate::kernel::basis::parse_basis_text(basis, "basis")?.value()
    } else {
        json!({"version":1,"constraints":["Preserve the selected task scope, existing authority and evidence."],
            "openZones":["Implementation within the approved project boundary."],
            "decisiveCases":["A current observed integration check covers the frozen accepted journey members."]})
    };
    basis
        .as_object_mut()
        .ok_or("basis is malformed")?
        .remove("sha256");
    let constraints = basis["constraints"]
        .as_array_mut()
        .ok_or("basis constraints are malformed")?;
    if constraints.iter().any(|item| {
        item.as_str()
            .is_some_and(|text| text.starts_with(RESERVED_PREFIX))
    }) {
        return Err("journey integration scope must be selected through --journey-integration, not an injected basis marker".into());
    }
    constraints.extend(
        member_contract_marker(&contract)?
            .into_iter()
            .map(Value::String),
    );
    let basis_text = serde_json::to_string(&basis).map_err(|error| error.to_string())?;
    crate::kernel::basis::parse_basis_text(&basis_text, "basis")?;
    Ok(Prepared { basis: basis_text })
}

fn validate_member_loaded(loaded: &Loaded, member: &Member, header: &Header) -> Result<(), String> {
    if member.host != header.host
        || member.session_key != header.session_key
        || member.owner_identity_sha256 != header.owner_identity_sha256
        || member.owner_project_id != header.owner_project_id
        || member.project_identity_sha256 != super::requirements::identity(loaded)?
        || member.local_identity_sha256 != super::native::owner_for(loaded)?.identity_sha256
        || member.config_path != canonical_path(&loaded.path, "configured project path")?
        || member.config_sha256 != hash::text(&loaded.source)
        || member.product_root != canonical_path(&loaded.product_root, "configured product root")?
        || member.state_root != canonical_path(&loaded.state_root, "configured state root")?
    {
        return Err(
            "journey member belongs to another or changed project/session configuration".into(),
        );
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Endpoint {
    work: String,
    identity_sha256: String,
}

fn accepted_endpoint(loaded: &Loaded, member: &Member) -> Result<Option<Endpoint>, String> {
    const MAX_SUCCESSORS: usize = 64;
    let mut current_work = member.work.clone();
    let mut expected_start = Some(member.start_event_sha256.clone());
    let mut expected_claim: Option<Value> = None;
    let mut seen = BTreeSet::new();
    for depth in 0..=MAX_SUCCESSORS {
        let relative = crate::work::resolve(loaded, &current_work)?;
        if !seen.insert(relative.clone()) {
            return Err("journey member successor lineage contains a cycle".into());
        }
        let (ledger, events, source) = crate::run::ledger::load(loaded, &relative)?;
        let start = events
            .first()
            .ok_or("journey Work ledger has no start event")?;
        let start_sha = start["eventSha256"]
            .as_str()
            .ok_or("journey Work start identity is missing")?;
        if start["action"] != "start"
            || expected_start
                .as_deref()
                .is_some_and(|expected| expected != start_sha)
            || start["configSha256"] != member.config_sha256
            || hash::text(start["goal"].as_str().unwrap_or_default()) != member.work_goal_sha256
        {
            return Err(
                "journey Work start, configuration or frozen task Goal identity changed".into(),
            );
        }
        if let Some(claim) = expected_claim.take() {
            validate_successor(&claim, &relative, &ledger, &events)?;
        }
        let state = crate::run::state::reduce(&events)?;
        let accepted = state["status"] == "accepted"
            && events.last().is_some_and(|event| {
                event["action"] == "submit"
                    && event["role"] == "lead"
                    && event["outcome"] == "accepted"
            });
        let claim_path = crate::run::ledger::claim_path(&ledger);
        let Some(claim) = crate::run::ledger::valid_claim(&claim_path)? else {
            if !accepted {
                return Ok(None);
            }
            return Ok(Some(Endpoint {
                work: current_work.clone(),
                identity_sha256: hash::value(&json!({
                    "work":current_work,
                    "startEventSha256":start_sha,
                    "acceptanceEventSha256":events.last().unwrap()["eventSha256"],
                    "goalSha256":member.work_goal_sha256,
                    "configSha256":start["configSha256"],
                    "rawLedgerSha256":hash::bytes(source.as_bytes()),
                })),
            }));
        };
        if depth == MAX_SUCCESSORS {
            return Err("journey member successor lineage exceeds the supported bound".into());
        }
        validate_predecessor_claim(&claim, &relative, &events, &source)?;
        let successor = claim["newLedgerPath"]
            .as_str()
            .ok_or("journey successor claim has no successor ledger")?;
        let work = crate::work::locator_for_ledger(successor)
            .ok_or("journey successor claim names a non-Work ledger")?;
        if crate::work::resolve(loaded, &work)? != successor {
            return Err("journey successor claim does not bind the exact Work ledger path".into());
        }
        current_work = work;
        expected_start = None;
        expected_claim = Some(claim);
    }
    Ok(None)
}

fn validate_predecessor_claim(
    claim: &Value,
    relative: &str,
    events: &[Value],
    source: &str,
) -> Result<(), String> {
    let start = events
        .first()
        .ok_or("journey predecessor ledger is empty")?;
    let head = events.last().ok_or("journey predecessor ledger is empty")?;
    if claim["oldLedgerPath"] != relative
        || claim["oldLedgerSha256"] != hash::bytes(source.as_bytes())
        || claim["oldRunId"] != start["runId"]
        || claim["oldHeadEventSha256"] != head["eventSha256"]
        || claim["oldConfigSha256"] != start["configSha256"]
        || claim["goalSha256"] != hash::text(start["goal"].as_str().unwrap_or_default())
    {
        return Err(
            "journey successor claim does not bind the exact predecessor or same task Goal".into(),
        );
    }
    Ok(())
}

fn validate_successor(
    claim: &Value,
    relative: &str,
    ledger: &crate::run::ledger::LedgerPath,
    events: &[Value],
) -> Result<(), String> {
    let start = events.first().ok_or("journey successor ledger is empty")?;
    let actual_relative = ledger
        .expected
        .strip_prefix(&ledger.root)
        .map_err(|_| "journey successor escapes its configured StateRoot")?
        .to_str()
        .ok_or("journey successor ledger path is not UTF-8")?;
    if actual_relative != relative
        || claim["newLedgerPath"] != relative
        || claim["newRunId"] != start["runId"]
        || claim["workflow"] != start["workflow"]
        || claim["configSha256"] != start["configSha256"]
        || claim["goalSha256"] != hash::text(start["goal"].as_str().unwrap_or_default())
        || start["supersedes"]["ledgerPath"] != claim["oldLedgerPath"]
        || start["supersedes"]["ledgerSha256"] != claim["oldLedgerSha256"]
        || start["supersedes"]["runId"] != claim["oldRunId"]
        || start["supersedes"]["headEventSha256"] != claim["oldHeadEventSha256"]
        || start["supersedes"]["configSha256"] != claim["oldConfigSha256"]
    {
        return Err("journey successor start does not match its validated same-goal claim".into());
    }
    Ok(())
}

fn covered_snapshot(
    loaded: &Loaded,
    members: &[Member],
    header: &Header,
) -> Result<Vec<CoveredMember>, String> {
    let mut covered = Vec::new();
    let project_identity = super::requirements::identity(loaded)?;
    for member in members
        .iter()
        .filter(|member| member.project_identity_sha256 == project_identity)
    {
        validate_member_loaded(loaded, member, header)?;
        let endpoint = accepted_endpoint(loaded, member)?
            .ok_or_else(|| format!("journey member {} has no terminal accepted result; integration cannot create acceptance", member.work))?;
        let evidence = crate::run::result_ref_evidence(loaded, &endpoint.work)?
            .ok_or_else(|| format!("journey member {} evidence is unavailable", member.work))?;
        if !evidence.accepted
            || !evidence.artifact_current
            || evidence.drift.is_some()
            || evidence.inputs_current.is_none()
        {
            return Err(format!(
                "journey member {} has unavailable acceptance/configuration evidence",
                member.work
            ));
        }
        covered.push(CoveredMember {
            work: member.work.clone(),
            member_sha256: member_identity(member),
            accepted_endpoint_sha256: endpoint.identity_sha256,
        });
    }
    covered.sort_by(|a, b| a.work.cmp(&b.work));
    Ok(covered)
}

fn contract_for_work(loaded: &Loaded, work: &str) -> Result<Option<(Contract, Value)>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events
        .first()
        .ok_or("journey integration Work has no start event")?;
    let Some(contract) = parse_contract(start)? else {
        return Ok(None);
    };
    validate_header_for_loaded(loaded, &contract.header)?;
    Ok(Some((contract, start.clone())))
}

fn current_integration_for_project(
    loaded: &Loaded,
    state: &JourneyState,
    members: &[Member],
    project_identity: &str,
) -> Result<Option<String>, String> {
    let project_members = members
        .iter()
        .filter(|member| member.project_identity_sha256 == project_identity)
        .collect::<Vec<_>>();
    if project_members.is_empty() {
        return Ok(None);
    }
    for entry in state
        .integrations
        .iter()
        .filter(|entry| entry.member.project_identity_sha256 == project_identity)
    {
        let member = &entry.member;
        let header_binding = Header {
            protocol: PROTOCOL,
            host: member.host.clone(),
            session_key: member.session_key.clone(),
            owner_identity_sha256: member.owner_identity_sha256.clone(),
            owner_project_id: member.owner_project_id.clone(),
            project_identity_sha256: member.project_identity_sha256.clone(),
            local_identity_sha256: member.local_identity_sha256.clone(),
            config_binding_sha256: config_binding(loaded)?,
            checker: super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        };
        validate_member_loaded(loaded, member, &header_binding)?;
        let Some((contract, start)) = contract_for_work(loaded, &member.work)? else {
            continue;
        };
        if start["eventSha256"] != member.start_event_sha256
            || hash::text(start["goal"].as_str().unwrap_or_default()) != member.work_goal_sha256
            || contract_sha256(&contract) != entry.contract_sha256
        {
            continue;
        }
        let expected = covered_snapshot(loaded, members, &contract.header)?;
        if expected != contract.members {
            continue;
        }
        let evidence = super::requirements::current_checker_evidence(
            loaded,
            &member.work,
            &contract.header.checker,
        )?;
        if evidence["state"] == "current" && evidence["accepted"] == true {
            return Ok(Some(member.work.clone()));
        }
    }
    Ok(None)
}

fn project_loaded(loaded: &Loaded, member: &Member) -> Result<Loaded, String> {
    if canonical_path(&loaded.path, "configured project path")? == member.config_path {
        return Ok(crate::config::load(Some(member.config_path.as_str()))?);
    }
    crate::config::load(Some(member.config_path.as_str()))
}

fn member_currentness(
    loaded: &Loaded,
    state: &JourneyState,
    member: &Member,
) -> Result<(&'static str, Option<String>), String> {
    let project = project_loaded(loaded, member)?;
    let session = super::native::current()?.ok_or("native session identity is unavailable")?;
    let scope = super::native::scope(&project, false)?;
    let owner = scope.owner.ok_or("native session owner is unavailable")?;
    if member.host != session.host
        || member.session_key != session.key
        || member.owner_identity_sha256 != owner.identity_sha256
        || member.owner_project_id != owner.project_id
    {
        return Err("journey member belongs to another native session or owner".into());
    }
    validate_member_loaded(
        &project,
        member,
        &Header {
            protocol: PROTOCOL,
            host: session.host.to_owned(),
            session_key: session.key,
            owner_identity_sha256: owner.identity_sha256,
            owner_project_id: owner.project_id,
            project_identity_sha256: member.project_identity_sha256.clone(),
            local_identity_sha256: member.local_identity_sha256.clone(),
            config_binding_sha256: config_binding(&project)?,
            checker: super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        },
    )?;
    let Some(endpoint) = accepted_endpoint(&project, member)? else {
        return Ok(("unresolved", None));
    };
    let evidence = crate::run::result_ref_evidence(&project, &endpoint.work)?
        .ok_or("journey member result evidence is unavailable")?;
    if !evidence.accepted || !evidence.artifact_current || evidence.drift.is_some() {
        return Ok(("unresolved", None));
    }
    match evidence.inputs_current {
        Some(true) => Ok(("current", None)),
        Some(false) => {
            let supported = current_integration_for_project(
                &project,
                state,
                &state.members,
                &member.project_identity_sha256,
            )?;
            if supported.is_some() {
                Ok(("integration_supported", supported))
            } else {
                Ok(("stale", None))
            }
        }
        None => Ok(("unresolved", None)),
    }
}

pub(super) fn readiness_for_state(loaded: &Loaded, state: Option<JourneyState>) -> Readiness {
    let Some(state) = state else {
        return Readiness {
            protocol: 0,
            state: "unavailable",
            reason: Some("journey_membership_protocol_missing".into()),
            total: 0,
            current: 0,
            integrated: 0,
            pending_admissions: 0,
            unresolved: vec![
                json!({"state":"unavailable","reason":"explicit_native_goal_incorporation_required"}),
            ],
            integration_work: None,
        };
    };
    let mut current = 0;
    let mut integrated = 0;
    let mut integration_work = None;
    let mut unresolved = Vec::new();
    if state.protocol != MEMBERSHIP_PROTOCOL {
        unresolved.push(json!({
            "state":"unavailable",
            "reason":"historical_journey_membership_has_no_admission_coverage",
        }));
    }
    for admission in &state.admissions {
        unresolved.push(json!({
            "work": admission.work,
            "state": "admission_pending_start_or_link",
            "reason": "exact_immutable_work_start_and_context_link_required",
        }));
    }
    for member in &state.members {
        match member_currentness(loaded, &state, member) {
            Ok(("current", _)) => current += 1,
            Ok(("integration_supported", Some(work))) => {
                integrated += 1;
                integration_work.get_or_insert(work);
            }
            Ok((status, _)) => unresolved.push(json!({"work":member.work,"state":status})),
            Err(error) => {
                unresolved.push(json!({"work":member.work,"state":"unavailable","reason":error}))
            }
        }
    }
    let state_name = if unresolved.is_empty() {
        "current"
    } else {
        "blocked"
    };
    Readiness {
        protocol: state.protocol,
        state: state_name,
        reason: (state_name != "current").then(|| {
            if state.protocol != MEMBERSHIP_PROTOCOL {
                "historical_journey_membership_has_no_admission_coverage".into()
            } else if !state.admissions.is_empty() {
                "native_journey_has_unresolved_work_admissions".into()
            } else {
                "journey_members_need_current_acceptance_or_checked_integration".into()
            }
        }),
        total: state.members.len(),
        current,
        integrated,
        pending_admissions: state.admissions.len(),
        unresolved: unresolved
            .into_iter()
            .take(MAX_MEMBERS + MAX_ADMISSIONS + 1)
            .collect(),
        integration_work,
    }
}

fn belongs_to_current_session(
    loaded: &Loaded,
    record: &Value,
) -> Result<Option<(super::native::Scope, Value)>, String> {
    let scope = super::native::scope(loaded, false)?;
    if !scope.is_session() || !scope.owner_matches {
        return Ok(None);
    }
    let history = super::records(&scope.root)?;
    if !history
        .iter()
        .any(|item| item["eventSha256"] == record["eventSha256"])
    {
        return Ok(None);
    }
    let current = history
        .last()
        .cloned()
        .ok_or("native session Goal history is empty")?;
    Ok(Some((scope, current)))
}

/// Applies the native-journey guard only to records in this host-reported
/// session journal. Explicit project Goal behavior remains legacy-compatible.
pub(crate) fn readiness_for_record(
    loaded: &Loaded,
    record: &Value,
) -> Result<Option<Readiness>, String> {
    let Some((_scope, current)) = belongs_to_current_session(loaded, record)? else {
        return Ok(None);
    };
    if current["goalId"] != record["goalId"] {
        return Ok(Some(Readiness {
            protocol: state(&current)?.map_or(0, |state| state.protocol),
            state: "blocked",
            reason: Some("a_newer_session_goal_is_current".into()),
            total: state(&current)?.map_or(0, |state| state.members.len()),
            current: 0,
            integrated: 0,
            pending_admissions: state(&current)?.map_or(0, |state| state.admissions.len()),
            unresolved: vec![
                json!({"state":"unresolved","reason":"newer_session_goal_is_current"}),
            ],
            integration_work: None,
        }));
    }
    Ok(Some(readiness_for_state(loaded, state(&current)?)))
}

pub(crate) fn require_close_ready(
    loaded: &Loaded,
    root: &Path,
    record: &Value,
) -> Result<(), String> {
    let scope = super::native::scope(loaded, false)?;
    if !scope.is_session() || !scope.owner_matches || scope.root != root {
        return Ok(());
    }
    let readiness = readiness_for_state(loaded, state(record)?);
    if readiness.ready() {
        Ok(())
    } else {
        Err(format!(
            "native session journey is not ready to close: {}",
            readiness.reason.unwrap_or_else(|| readiness.state.into())
        ))
    }
}

pub(crate) fn progress_for_record(loaded: Option<&Loaded>, record: &Value) -> Option<Value> {
    let loaded = loaded?;
    readiness_for_record(loaded, record)
        .ok()
        .flatten()
        .map(|readiness| readiness.value())
}

/// An exact journey member may retain its recorded result reference after its
/// tested inputs drift, but only a current accepted integration for the full
/// current local member set can refresh that currentness.
pub(crate) fn result_ref_current_or_supported(
    loaded: &Loaded,
    record: &Value,
    reference: &str,
    inputs_current: Option<bool>,
) -> Result<bool, String> {
    if inputs_current == Some(true) {
        return Ok(true);
    }
    if inputs_current != Some(false) {
        return Ok(false);
    }
    let Some(journey) = state(record)? else {
        return Ok(false);
    };
    let matched = journey
        .members
        .iter()
        .filter(|member| member.work == reference)
        .collect::<Vec<_>>();
    if matched.len() != 1 {
        return Ok(false);
    }
    let member = matched[0];
    let project = project_loaded(loaded, member)?;
    let local_endpoint = accepted_endpoint(&project, member)?;
    if local_endpoint.is_none() {
        return Ok(false);
    }
    let evidence = crate::run::result_ref_evidence(&project, reference)?;
    if !evidence
        .as_ref()
        .is_some_and(|e| e.accepted && e.artifact_current && e.drift.is_none())
    {
        return Ok(false);
    }
    Ok(current_integration_for_project(
        &project,
        &journey,
        &journey.members,
        &member.project_identity_sha256,
    )?
    .is_some())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn journey_protocol_refuses_unknown_fields_and_preserves_empty_native_extension() {
        let record =
            json!({"journeyMembershipProtocol":1,"journeyMembers":[],"journeyIntegrations":[]});
        assert_eq!(super::state(&record).unwrap().unwrap().protocol, 1);
        let admitted = json!({
            "journeyMembershipProtocol":2,
            "journeyMembers":[],
            "journeyIntegrations":[],
            "journeyAdmissions":[{
                "work":format!("smw_{}", "a".repeat(64)),
                "startIntentSha256":"b".repeat(64),
                "parentGoalId":"goal",
                "parentObservedGoalEventSha256":"c".repeat(64),
                "workGoalSha256":"d".repeat(64),
                "host":"codex",
                "sessionKey":"e".repeat(64),
                "ownerIdentitySha256":"f".repeat(64),
                "ownerProjectId":"project",
                "projectIdentitySha256":"1".repeat(64),
                "localIdentitySha256":"2".repeat(64),
                "configPath":"/tmp/project/exitbind.json",
                "configSha256":"3".repeat(64),
                "productRoot":"/tmp/project",
                "stateRoot":"/tmp/project"
            }]
        });
        assert_eq!(
            super::state(&admitted).unwrap().unwrap().admissions.len(),
            1
        );
        let unknown =
            json!({"journeyMembershipProtocol":9,"journeyMembers":[],"journeyIntegrations":[]});
        assert!(super::state(&unknown).is_err());
    }

    #[test]
    fn goal_successor_carries_pending_admissions_with_their_original_parent() {
        let previous = json!({
            "goalId":"original",
            "journeyMembershipProtocol":2,
            "journeyMembers":[],
            "journeyIntegrations":[],
            "journeyAdmissions":[{
                "work":format!("smw_{}", "a".repeat(64)),
                "startIntentSha256":"b".repeat(64),
                "parentGoalId":"original",
                "parentObservedGoalEventSha256":"c".repeat(64),
                "workGoalSha256":"d".repeat(64),
                "host":"codex",
                "sessionKey":"e".repeat(64),
                "ownerIdentitySha256":"f".repeat(64),
                "ownerProjectId":"project",
                "projectIdentitySha256":"1".repeat(64),
                "localIdentitySha256":"2".repeat(64),
                "configPath":"/tmp/project/exitbind.json",
                "configSha256":"3".repeat(64),
                "productRoot":"/tmp/project",
                "stateRoot":"/tmp/project"
            }]
        });
        assert!(super::state(&previous).unwrap().is_some());
        let expected_admissions = previous["journeyAdmissions"].clone();
        let mut successor = json!({"goalId":"successor"});
        super::transition(Some(&previous), &mut successor, true).unwrap();
        assert_eq!(successor["journeyMembershipProtocol"], 2);
        assert_eq!(successor["journeyAdmissions"], expected_admissions);
        assert_eq!(
            successor["journeyAdmissions"][0]["parentGoalId"],
            "original"
        );
    }

    #[test]
    fn integration_markers_are_separate_bounded_entries() {
        let covered = super::CoveredMember {
            work: format!("smw_{}", "a".repeat(64)),
            member_sha256: "a".repeat(64),
            accepted_endpoint_sha256: "b".repeat(64),
        };
        let marker = super::marker(super::MEMBER_PREFIX, &covered).unwrap();
        assert!(marker.len() <= super::MAX_MARKER_BYTES);
        let parsed: super::CoveredMember =
            serde_json::from_str(marker.strip_prefix(super::MEMBER_PREFIX).unwrap()).unwrap();
        assert_eq!(parsed, covered);
    }
}

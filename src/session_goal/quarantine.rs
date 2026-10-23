//! Owner-bound, uncredited Goal residuals for unresolved Works that receive a
//! separately authorized quarantine verification.

use crate::{config::Loaded, evidence::hash};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const FIELD: &str = "quarantineResiduals";
const FORMAT: u64 = 3;
const PROTOCOL: u64 = 1;
pub(super) const JOURNEY_MEMBERSHIP_PROTOCOL: u64 = 3;
const MAX_RESIDUALS: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Endpoint {
    work: String,
    member_sha256: String,
    start_event_sha256: String,
    head_event_sha256: String,
    ledger_sha256: String,
    config_sha256: String,
    run_id: String,
    subject_sha256: String,
    artifact_sha256: String,
    inputs_sha256: String,
    command_sha256: String,
    check_event_sha256: Option<String>,
    review_event_sha256: Option<String>,
    acceptance_event_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Residual {
    request_id: String,
    request_sha256: String,
    reason: String,
    source_config: super::recovery::ConfigBinding,
    old_member: super::journey::Member,
    old: Endpoint,
    verification: Endpoint,
    verified_artifact_sha256: String,
    verification_environment_sha256: String,
    verification_decision_sha256: String,
    old_accounting: Value,
    decision_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResidualState {
    protocol: u64,
    goal_id: String,
    host: String,
    session_key: String,
    owner_identity_sha256: String,
    owner_project_id: Option<String>,
    residuals: Vec<Residual>,
}

pub(crate) struct Request<'a> {
    pub(crate) source_config: &'a str,
    pub(crate) old_work: &'a str,
    pub(crate) verification_work: &'a str,
    pub(crate) request_id: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) apply: bool,
    pub(crate) current_binding: Option<&'a str>,
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn work(value: &str) -> bool {
    crate::work::valid_work_handle(value)
}

fn decision_digest(residual: &Residual) -> Result<String, String> {
    let mut value = serde_json::to_value(residual).map_err(|error| error.to_string())?;
    value
        .as_object_mut()
        .ok_or("quarantine Goal residual is malformed")?
        .remove("decisionSha256");
    Ok(hash::value(&value))
}

fn validate_endpoint(endpoint: &Endpoint) -> Result<(), String> {
    if !work(&endpoint.work)
        || !sha(&endpoint.member_sha256)
        || !sha(&endpoint.start_event_sha256)
        || !sha(&endpoint.head_event_sha256)
        || !sha(&endpoint.ledger_sha256)
        || !sha(&endpoint.config_sha256)
        || !valid_text(&endpoint.run_id, 256)
        || !sha(&endpoint.subject_sha256)
        || !sha(&endpoint.artifact_sha256)
        || !sha(&endpoint.inputs_sha256)
        || !sha(&endpoint.command_sha256)
        || endpoint
            .check_event_sha256
            .as_deref()
            .is_some_and(|v| !sha(v))
        || endpoint
            .review_event_sha256
            .as_deref()
            .is_some_and(|v| !sha(v))
        || endpoint
            .acceptance_event_sha256
            .as_deref()
            .is_some_and(|v| !sha(v))
    {
        return Err("native Goal quarantine endpoint identity is invalid".into());
    }
    Ok(())
}

fn state(record: &Value) -> Result<Option<ResidualState>, String> {
    let Some(value) = record.get(FIELD) else {
        if record.get("format") == Some(&json!(FORMAT)) {
            return Err("Goal format 3 requires its quarantine residual extension".into());
        }
        return Ok(None);
    };
    if record["format"].as_u64() != Some(FORMAT)
        || record["journeyMembershipProtocol"].as_u64() != Some(JOURNEY_MEMBERSHIP_PROTOCOL)
    {
        return Err("native Goal quarantine residual requires Goal format 3".into());
    }
    let selected: ResidualState = serde_json::from_value(value.clone())
        .map_err(|_| "native Goal quarantine residual is malformed or has unknown fields")?;
    validate_state(record, &selected)?;
    Ok(Some(selected))
}

pub(super) fn protocol3_marker_valid(record: &Value) -> bool {
    record["journeyMembershipProtocol"].as_u64() == Some(JOURNEY_MEMBERSHIP_PROTOCOL)
        && record["format"].as_u64() == Some(FORMAT)
        && record[FIELD]["protocol"].as_u64() == Some(PROTOCOL)
}

fn validate_state(record: &Value, selected: &ResidualState) -> Result<(), String> {
    if selected.protocol != PROTOCOL
        || selected.goal_id != record["goalId"].as_str().unwrap_or_default()
        || !matches!(selected.host.as_str(), "codex" | "claude")
        || !sha(&selected.session_key)
        || !sha(&selected.owner_identity_sha256)
        || selected
            .owner_project_id
            .as_deref()
            .is_some_and(|value| !valid_text(value, 128))
        || selected.residuals.is_empty()
        || selected.residuals.len() > MAX_RESIDUALS
    {
        return Err("native Goal quarantine residual scope is invalid".into());
    }
    let journey = super::journey::state(record)?
        .ok_or("native Goal quarantine residual requires explicit journey membership")?;
    let members = journey
        .members
        .iter()
        .map(|member| (member.work.as_str(), member))
        .collect::<std::collections::BTreeMap<_, _>>();
    let admissions = journey
        .admissions
        .iter()
        .map(|admission| admission.work.as_str())
        .collect::<BTreeSet<_>>();
    let mut old_works = BTreeSet::new();
    let mut old_members = BTreeSet::new();
    let mut request_ids = BTreeSet::new();
    for residual in &selected.residuals {
        validate_endpoint(&residual.old)?;
        validate_endpoint(&residual.verification)?;
        let old_member_identity = super::journey::member_identity(&residual.old_member);
        if residual.old.work == residual.verification.work
            || !members.contains_key(residual.old_member.work.as_str())
            || members
                .get(residual.old_member.work.as_str())
                .map(|member| super::journey::member_identity(member))
                != Some(old_member_identity.clone())
            || residual.old.member_sha256 != old_member_identity
            || !old_works.insert(residual.old.work.as_str())
            || !old_members.insert(residual.old_member.work.as_str())
            || !request_ids.insert(residual.request_id.as_str())
            || !valid_text(&residual.request_id, 128)
            || !valid_text(&residual.reason, 1024)
            || !sha(&residual.request_sha256)
            || !sha(&residual.verified_artifact_sha256)
            || !sha(&residual.verification_environment_sha256)
            || !sha(&residual.verification_decision_sha256)
            || !sha(&residual.decision_sha256)
            || residual.old.check_event_sha256.is_none()
            || residual.old.review_event_sha256.is_some()
            || residual.old.acceptance_event_sha256.is_some()
            || residual.verification.check_event_sha256.is_none()
            || residual.verification.review_event_sha256.is_none()
            || residual.verification.acceptance_event_sha256.is_none()
            || members
                .get(residual.verification.work.as_str())
                .map(|member| super::journey::member_identity(member))
                != Some(residual.verification.member_sha256.clone())
            || admissions.contains(residual.old_member.work.as_str())
            || admissions.contains(residual.verification.work.as_str())
            || residual.source_config.config_sha256 != residual.old.config_sha256
            || residual.source_config.config_sha256 != residual.verification.config_sha256
            || !valid_text(&residual.source_config.path, 4096)
            || !valid_text(&residual.source_config.product_root, 4096)
            || !valid_text(&residual.source_config.state_root, 4096)
            || !sha(&residual.source_config.project_identity_sha256)
            || !sha(&residual.source_config.local_identity_sha256)
            || residual
                .source_config
                .project_id
                .as_deref()
                .is_some_and(|value| !valid_text(value, 128))
            || [
                residual.old_member.work.as_str(),
                residual.verification.work.as_str(),
            ]
            .iter()
            .any(|work| {
                members.get(work).map_or(true, |member| {
                    if *work == residual.old_member.work.as_str() {
                        *member != &residual.old_member
                    } else {
                        let endpoint = &residual.verification;
                        member.start_event_sha256 != endpoint.start_event_sha256
                            || member.project_identity_sha256
                                != residual.source_config.project_identity_sha256
                            || member.local_identity_sha256
                                != residual.source_config.local_identity_sha256
                            || member.product_root != residual.source_config.product_root
                            || member.state_root != residual.source_config.state_root
                            || member.config_path != residual.source_config.path
                            || member.config_sha256 != residual.source_config.config_sha256
                    }
                })
            })
            || record["nativeGoalRecovery"]["active"]["historicalFixtures"]
                .as_array()
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item["work"] == residual.old.work
                            || item["work"] == residual.old_member.work
                    })
                })
            || residual.old_accounting.is_null()
            || decision_digest(residual)? != residual.decision_sha256
        {
            return Err("native Goal quarantine residual identity or accounting is invalid".into());
        }
    }
    Ok(())
}

pub(crate) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    state(record)
        .map(|_| ())
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))
}

pub(crate) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] != next["goalId"] || previous.get(FIELD).is_none() {
        return;
    }
    let Ok(mut preserved) = serde_json::from_value::<ResidualState>(previous[FIELD].clone()) else {
        return;
    };
    if let Ok(candidate) = serde_json::from_value::<ResidualState>(next[FIELD].clone()) {
        if candidate.goal_id == preserved.goal_id
            && candidate.host == preserved.host
            && candidate.session_key == preserved.session_key
            && candidate.owner_identity_sha256 == preserved.owner_identity_sha256
            && candidate.owner_project_id == preserved.owner_project_id
        {
            for residual in candidate.residuals {
                if !preserved
                    .residuals
                    .iter()
                    .any(|old| old.old.work == residual.old.work)
                {
                    preserved.residuals.push(residual);
                }
            }
            preserved
                .residuals
                .sort_by(|a, b| a.old.work.cmp(&b.old.work));
        }
    }
    if let Some(object) = next.as_object_mut() {
        object.insert("format".into(), json!(FORMAT));
        object.insert(
            "journeyMembershipProtocol".into(),
            json!(JOURNEY_MEMBERSHIP_PROTOCOL),
        );
        object.insert(
            FIELD.into(),
            serde_json::to_value(preserved).expect("validated residual state serializes"),
        );
    }
}

fn owner_scope(loaded: &Loaded, record: &Value) -> Result<(), String> {
    let scope = super::native::scope(loaded, false)?;
    scope.require_owner()?;
    let session = scope
        .session
        .as_ref()
        .ok_or("native Goal quarantine requires a current host session")?;
    let owner = scope
        .owner
        .as_ref()
        .ok_or("native Goal quarantine requires a fixed Goal owner")?;
    let Some(selected) = state(record)? else {
        return Ok(());
    };
    if !scope.is_session()
        || record["goalId"] != selected.goal_id
        || session.host != selected.host
        || session.key != selected.session_key
        || owner.identity_sha256 != selected.owner_identity_sha256
        || owner.project_id != selected.owner_project_id
    {
        return Err("native Goal quarantine belongs to another host session or fixed owner".into());
    }
    Ok(())
}

fn request_sha(request: &Request<'_>) -> String {
    hash::value(
        &json!({"sourceConfig":request.source_config,"oldWork":request.old_work,
        "verificationWork":request.verification_work,"requestId":request.request_id,
        "reason":request.reason}),
    )
}

fn member(record: &Value, work_id: &str) -> Result<super::journey::Member, String> {
    let journey = super::journey::state(record)?
        .ok_or("native session Goal has no explicit journey membership protocol")?;
    if journey
        .admissions
        .iter()
        .any(|admission| admission.work == work_id)
    {
        return Err("quarantine residual requires completed native Goal admissions".into());
    }
    journey
        .members
        .into_iter()
        .find(|value| value.work == work_id)
        .ok_or_else(|| format!("quarantine Work {work_id} is not a real member of this Goal"))
}

fn member_with_lineage(
    source: &Loaded,
    record: &Value,
    endpoint_work: &str,
) -> Result<super::journey::Member, String> {
    let member = super::journey::member_for_work_with_lineage(source, record, endpoint_work)?
        .ok_or_else(|| {
            format!("quarantine endpoint {endpoint_work} has no validated Goal member lineage")
        })?;
    let member: super::journey::Member = serde_json::from_value(member)
        .map_err(|_| "quarantine endpoint lineage returned a malformed Goal member")?;
    if !super::journey::state(record)?
        .is_some_and(|journey| journey.members.iter().any(|actual| actual == &member))
    {
        return Err("quarantine endpoint lineage does not terminate at a real Goal member".into());
    }
    Ok(member)
}

fn current_state(
    owner: &Loaded,
    source: &Loaded,
    record: &Value,
    request: &Request<'_>,
) -> Result<(ResidualState, String, Residual), String> {
    if record["closure"]["closed"] == true || super::requirements::named(record) {
        return Err("Goal quarantine requires an open native session Goal".into());
    }
    let old_member = member_with_lineage(source, record, request.old_work)?;
    let verification = member(record, request.verification_work)?;
    let (mut residual, marker) =
        evidence::exact_quarantine_evidence(source, &old_member, request.old_work, &verification)?;
    if marker["requestId"] != request.request_id || marker["reason"] != request.reason {
        return Err("request id or reason differs from the immutable quarantine marker".into());
    }
    residual.request_id = request.request_id.to_owned();
    residual.request_sha256 = request_sha(request);
    residual.decision_sha256 = decision_digest(&residual)?;
    let scope = super::native::scope(owner, false)?;
    scope.require_owner()?;
    if !scope.is_session() {
        return Err("goal quarantine requires the original native Goal owner context".into());
    }
    let session = scope
        .session
        .ok_or("native host session identity is unavailable")?;
    let owner_identity = scope
        .owner
        .ok_or("fixed native Goal owner is unavailable")?;
    let mut selected = state(record)?.unwrap_or(ResidualState {
        protocol: PROTOCOL,
        goal_id: record["goalId"]
            .as_str()
            .ok_or("native Goal identity is missing")?
            .into(),
        host: session.host.to_owned(),
        session_key: session.key.clone(),
        owner_identity_sha256: owner_identity.identity_sha256.clone(),
        owner_project_id: owner_identity.project_id.clone(),
        residuals: Vec::new(),
    });
    if selected.host != session.host
        || selected.session_key != session.key
        || selected.owner_identity_sha256 != owner_identity.identity_sha256
        || selected.owner_project_id != owner_identity.project_id
    {
        return Err("quarantine residual belongs to another native Goal owner".into());
    }
    if let Some(existing) = selected.residuals.iter().find(|item| {
        item.old.work == residual.old.work || item.old_member.work == residual.old_member.work
    }) {
        if existing.request_id == residual.request_id
            && existing.request_sha256 == residual.request_sha256
            && existing.decision_sha256 == residual.decision_sha256
        {
            residual = existing.clone();
        } else {
            return Err(
                "old unknown already has a quarantine residual; a second verification is refused"
                    .into(),
            );
        }
    } else {
        if selected.residuals.len() >= MAX_RESIDUALS {
            return Err("native Goal quarantine residual bound reached".into());
        }
        selected.residuals.push(residual.clone());
        selected
            .residuals
            .sort_by(|a, b| a.old.work.cmp(&b.old.work));
    }
    let preview = json!({"goalId":record["goalId"],"revision":record["revision"],
        "eventSha256":record["eventSha256"],"sourceConfig":residual.source_config,
        "residual":residual,"owner":{"host":selected.host,"sessionKey":selected.session_key,
            "identitySha256":selected.owner_identity_sha256,"projectId":selected.owner_project_id,
            "configPath":owner.path.to_str().ok_or("owner configuration path is not valid UTF-8")?,
            "configSha256":hash::text(&owner.source)}});
    Ok((selected, hash::value(&preview), residual))
}
pub(crate) fn record(owner: &Loaded, request: Request<'_>) -> Result<Value, String> {
    if !valid_text(request.request_id, 128)
        || !valid_text(request.reason, 1024)
        || !work(request.old_work)
        || !work(request.verification_work)
        || request.old_work == request.verification_work
    {
        return Err(
            "goal quarantine requires distinct valid Works, --request-id and --reason".into(),
        );
    }
    if !request.apply && request.current_binding.is_some() {
        return Err(
            "--current-binding is accepted only with --apply; request a fresh preview".into(),
        );
    }
    let source = crate::config::load(Some(request.source_config))
        .map_err(|error| format!("--source-config is unavailable: {error}"))?;
    let native_scope = super::native::scope(owner, false)?;
    native_scope.require_owner()?;
    if !native_scope.is_session() {
        return Err("goal quarantine requires the original native Goal owner context".into());
    }
    let root = native_scope.root.clone();
    if request.apply {
        let expected_request = request_sha(&request);
        if let Some(record) = super::with_history(&root, |history| {
            Ok(history
                .iter()
                .rev()
                .find(|saved| {
                    saved[FIELD]["residuals"].as_array().is_some_and(|items| {
                        items.iter().any(|item| {
                            item["requestId"] == request.request_id
                                && item["requestSha256"] == expected_request
                        })
                    })
                })
                .cloned())
        })? {
            owner_scope(owner, &record)?;
            let detail = current_for_member(owner, &record, request.old_work)?
                .ok_or("saved quarantine replay has no current residual")?;
            if detail["state"] != "quarantined_uncredited"
                || detail["verificationWork"] != request.verification_work
            {
                return Err(
                    "saved quarantine replay no longer matches its exact current evidence".into(),
                );
            }
            return Ok(
                json!({"kind":"native_goal_quarantine_replayed","readOnly":true,
                "event":record,"currentStatus":crate::emitted::goal_status(&owner.path)}),
            );
        }
    }
    let current = super::read(&root)?.ok_or("no native session Goal is incorporated")?;
    owner_scope(owner, &current)?;
    let (selected, binding, _residual) = current_state(owner, &source, &current, &request)?;
    let owner_config = owner
        .path
        .to_str()
        .ok_or("owner configuration path is not valid UTF-8")?;
    let apply_command = crate::emitted::Route::new([
        "goal",
        "quarantine",
        request.old_work,
        "--source-config",
        request.source_config,
        "--verification-work",
        request.verification_work,
        "--request-id",
        request.request_id,
        "--reason",
        request.reason,
        "--current-binding",
        binding.as_str(),
        "--apply",
        "--json",
    ])
    .config_text(owner_config)
    .within(16 * 1024);
    if !request.apply {
        return Ok(
            json!({"kind":"native_goal_quarantine_preview","readOnly":true,
            "currentBinding":binding,"residualState":selected,"applyArgv":apply_command.argv_value(),
            "sameConfigRequired":apply_command.needs_config(),"sameExecutableRequired":apply_command.needs_executable()}),
        );
    }
    if request.current_binding != Some(binding.as_str()) {
        return Err(
            "--current-binding does not match the exact current Goal quarantine preview".into(),
        );
    }
    let mut replayed = false;
    let saved = super::mutate_history(&root, |history| {
        if let Some(saved) = history.iter().rev().find(|saved| {
            saved[FIELD]["residuals"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["requestId"] == request.request_id
                        && item["requestSha256"] == request_sha(&request)
                })
            })
        }) {
            owner_scope(owner, saved)?;
            let detail = current_for_member(owner, saved, request.old_work)?
                .ok_or("saved quarantine replay has no current residual")?;
            if detail["state"] != "quarantined_uncredited"
                || detail["verificationWork"] != request.verification_work
            {
                return Err(
                    "saved quarantine replay no longer matches its exact current evidence".into(),
                );
            }
            replayed = true;
            return Ok(saved.clone());
        }
        let previous = history
            .last()
            .ok_or("no native session Goal is incorporated")?;
        let fresh_owner = crate::config::load(Some(owner.path.to_str().ok_or("owner configuration path is not valid UTF-8")?))
            .map_err(|error| format!("owner configuration changed or became unavailable; preview quarantine again: {error}"))?;
        let fresh_source = crate::config::load(Some(request.source_config))
            .map_err(|error| format!("Source configuration changed or became unavailable; preview quarantine again: {error}"))?;
        let (fresh, fresh_binding, _) =
            current_state(&fresh_owner, &fresh_source, previous, &request)?;
        if fresh_binding != binding || fresh != selected {
            return Err("native Goal quarantine preview is stale; request a fresh preview".into());
        }
        let revision = previous["revision"]
            .as_u64()
            .ok_or("native Goal revision is missing")?;
        let mut next = previous.clone();
        next.as_object_mut()
            .ok_or("native Goal record is malformed")?
            .remove("eventSha256");
        next["format"] = json!(FORMAT);
        next["journeyMembershipProtocol"] = json!(JOURNEY_MEMBERSHIP_PROTOCOL);
        next[FIELD] = serde_json::to_value(&fresh).map_err(|error| error.to_string())?;
        next["revision"] = json!(revision.saturating_add(1));
        next["predecessor"] = json!({"goalId":previous["goalId"],"revision":revision});
        next["successorOf"] = Value::Null;
        next["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
        Ok(super::sealed(next, Some(previous)))
    })?;
    Ok(
        json!({"kind":if replayed {"native_goal_quarantine_replayed"} else {"native_goal_quarantine_applied"},
        "readOnly":replayed,"goalEventAppended":!replayed,"workLedgerChanged":false,
        "event":saved,"residual":"quarantined_uncredited","oldCredited":false,"oldReset":false}),
    )
}

/// Revalidates the evidence link whenever journey readiness or closure reads
/// a Goal. A valid residual remains its own uncredited category.
pub(super) fn current_for_member(
    loaded: &Loaded,
    record: &Value,
    work_id: &str,
) -> Result<Option<Value>, String> {
    let Some(selected) = state(record)? else {
        return Ok(None);
    };
    let Some(residual) = selected.residuals.iter().find(|item| {
        item.old_member.work == work_id
            || item.old.work == work_id
            || item.verification.work == work_id
    }) else {
        return Ok(None);
    };
    owner_scope(loaded, record)?;
    let source = crate::config::load(Some(&residual.source_config.path))
        .map_err(|error| format!("quarantined Source configuration is unavailable: {error}"))?;
    if super::recovery::config_binding_for_current(&source)? != residual.source_config {
        return Err("quarantined Source configuration changed; residual is stale".into());
    }
    let old_member = member_with_lineage(&source, record, &residual.old.work)?;
    if old_member != residual.old_member {
        return Err("quarantined endpoint no longer descends from its original Goal member".into());
    }
    let verification = member(record, &residual.verification.work)?;
    let (fresh, marker) = evidence::exact_quarantine_evidence(
        &source,
        &old_member,
        &residual.old.work,
        &verification,
    )?;
    if fresh.old_member != residual.old_member {
        return Err("quarantined endpoint original Goal member changed".into());
    }
    if fresh.old != residual.old
        || fresh.verification != residual.verification
        || fresh.old_accounting != residual.old_accounting
        || fresh.verified_artifact_sha256 != residual.verified_artifact_sha256
        || fresh.verification_environment_sha256 != residual.verification_environment_sha256
        || fresh.verification_decision_sha256 != residual.verification_decision_sha256
        || marker["oldCredited"] != false
        || marker["oldReset"] != false
    {
        return Err("quarantined unknown or accepted verification evidence changed".into());
    }
    if work_id == residual.old_member.work || work_id == residual.old.work {
        Ok(Some(
            json!({"work":residual.old_member.work,"state":"quarantined_uncredited",
            "credited":false,"reset":false,"reason":residual.reason,
            "unknownEndpointWork":residual.old.work,
            "unknownEndpointStartEventSha256":residual.old.start_event_sha256,
            "verificationWork":residual.verification.work,
            "decisionSha256":residual.decision_sha256}),
        ))
    } else {
        Ok(Some(
            json!({"work":residual.verification.work,"state":"current",
            "quarantineVerificationDecisionSha256":residual.verification_decision_sha256}),
        ))
    }
}

mod evidence;
#[cfg(test)]
mod tests;

//! Owner reader barrier and effective-owner resolution from sealed Goal history.

use super::model::{decisions, OwnerCorrectionDecision, SavedRequest};
use crate::{
    evidence::hash,
    session_goal::native::{self, Access, Owner, Session},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Marker {
    kind: String,
    protocol: u64,
    host: String,
    session_key: String,
    original: Value,
    previous_owner_record_sha256: String,
    pub(super) base_decision_sha256: Option<String>,
    pub(super) pending: Option<OwnerCorrectionDecision>,
    pub(super) request: Option<SavedRequest>,
    sha256: String,
}

pub(in crate::session_goal) struct Resolved {
    pub(in crate::session_goal) owner: Owner,
    pub(in crate::session_goal) pending: bool,
    pub(in crate::session_goal) latest: Option<OwnerCorrectionDecision>,
}

fn digest(marker: &Marker) -> Result<String, String> {
    let mut value = serde_json::to_value(marker).map_err(|error| error.to_string())?;
    value
        .as_object_mut()
        .ok_or("owner marker malformed")?
        .remove("sha256");
    Ok(hash::value(&value))
}

impl Marker {
    pub(super) fn parse(session: &Session, value: &Value) -> Result<Self, String> {
        let marker: Self = serde_json::from_value(value.clone())
            .map_err(|_| "native Goal owner correction marker is malformed")?;
        native::validate_owner_v1(&marker.original, &session.key, session.host)?;
        if marker.original.get("journalSessionKey").is_some()
            || marker.kind != "native_session_goal_owner"
            || marker.protocol != 2
            || marker.host != session.host
            || marker.session_key != session.key
            || !native::valid_hash(&marker.previous_owner_record_sha256)
            || marker
                .base_decision_sha256
                .as_deref()
                .is_some_and(|sha| !native::valid_hash(sha))
            || digest(&marker)? != marker.sha256
        {
            return Err("native Goal owner correction marker is invalid".into());
        }
        if let Some(pending) = &marker.pending {
            pending.validate()?;
            let request = marker
                .request
                .as_ref()
                .ok_or("native Goal owner marker has no recovery request")?;
            if pending.host != session.host
                || pending.session_key != session.key
                || pending.old_owner_record_sha256 != marker.previous_owner_record_sha256
                || request.digest() != pending.request_sha256
                || request.owner_config != pending.old_config.path
                || request.request_id != pending.request_id
                || request.reason != pending.reason
                || !request.old_writers_quiesced
            {
                return Err("native Goal owner correction marker names another session".into());
            }
        } else if marker.request.is_some() {
            return Err("native Goal owner marker has an unexpected recovery request".into());
        }
        Ok(marker)
    }

    pub(super) fn new(
        session: &Session,
        value: &Value,
        base: Option<String>,
        pending: Option<OwnerCorrectionDecision>,
        request: Option<SavedRequest>,
    ) -> Result<Self, String> {
        let original = if value["protocol"] == 1 {
            native::validate_owner_v1(value, &session.key, session.host)?;
            if value.get("journalSessionKey").is_some() {
                return Err("an adopted session cannot correct Goal ownership".into());
            }
            value.clone()
        } else {
            Self::parse(session, value)?.original
        };
        let mut marker = Self {
            kind: "native_session_goal_owner".into(),
            protocol: 2,
            host: session.host.into(),
            session_key: session.key.clone(),
            original,
            previous_owner_record_sha256: legacy_digest(value)?,
            base_decision_sha256: base,
            pending,
            request,
            sha256: String::new(),
        };
        marker.sha256 = digest(&marker)?;
        Ok(marker)
    }

    pub(super) fn value(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|error| error.to_string())
    }
}

fn owner(value: &Value) -> Owner {
    Owner {
        project_id: value["ownerProjectId"].as_str().map(str::to_owned),
        identity_sha256: value["ownerIdentitySha256"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    }
}

fn scan(
    session: &Session,
    original: &Value,
    history: &[Value],
) -> Result<Vec<OwnerCorrectionDecision>, String> {
    let mut all: Vec<OwnerCorrectionDecision> = Vec::new();
    let mut preceding = Vec::new();
    let mut prior_goal = Value::Null;
    let mut effective = owner(original);
    for (index, record) in history.iter().enumerate() {
        let current = decisions(record)?;
        if record["goalId"] != prior_goal {
            preceding.clear();
        }
        if current.len() > preceding.len() {
            let decision = current.last().ok_or("owner correction is missing")?;
            let prior = index
                .checked_sub(1)
                .and_then(|position| history.get(position))
                .ok_or("native Goal owner correction has no predecessor")?;
            super::validate_recorded_selection(prior, decision)?;
            super::empty_goal(&history[..index], &decision.goal_id)?;
            if decision.host != session.host
                || decision.session_key != session.key
                || decision.old_config.local_identity_sha256 != effective.identity_sha256
                || decision.old_config.project_id != effective.project_id
                || all
                    .iter()
                    .any(|item| item.request_id == decision.request_id)
            {
                return Err("native Goal owner correction chain is inconsistent".into());
            }
            effective = Owner {
                project_id: decision.target_config.project_id.clone(),
                identity_sha256: decision.target_config.local_identity_sha256.clone(),
            };
            all.push(decision.clone());
        }
        preceding = current;
        prior_goal = record["goalId"].clone();
    }
    Ok(all)
}

pub(super) fn resolve_history(
    session: &Session,
    value: &Value,
    history: &[Value],
) -> Result<Resolved, String> {
    let marker = Marker::parse(session, value)?;
    let chain = scan(session, &marker.original, history)?;
    let latest = chain.last().cloned();
    let last_sha = latest
        .as_ref()
        .map(|decision| decision.decision_sha256.as_str());
    let base = marker.base_decision_sha256.as_deref();
    let pending = match &marker.pending {
        Some(decision) if last_sha == Some(decision.decision_sha256.as_str()) => {
            let preceding = chain
                .iter()
                .rev()
                .nth(1)
                .map(|item| item.decision_sha256.as_str());
            if preceding != base || latest.as_ref() != Some(decision) {
                return Err(
                    "native Goal owner marker does not match its committed correction".into(),
                );
            }
            false
        }
        Some(decision) if last_sha == base => {
            let head = history
                .last()
                .ok_or("native Goal correction predecessor is unavailable")?;
            if head["eventSha256"] != decision.previous_event_sha256
                || head["goalId"] != decision.goal_id
                || head["revision"] != decision.previous_revision
            {
                return Err("native Goal owner marker predecessor is inconsistent".into());
            }
            super::validate_recorded_selection(head, decision)?;
            super::empty_goal(history, &decision.goal_id)?;
            let effective = latest.as_ref().map_or_else(
                || owner(&marker.original),
                |prior| Owner {
                    project_id: prior.target_config.project_id.clone(),
                    identity_sha256: prior.target_config.local_identity_sha256.clone(),
                },
            );
            if decision.old_config.local_identity_sha256 != effective.identity_sha256
                || decision.old_config.project_id != effective.project_id
            {
                return Err("native Goal pending correction names another owner".into());
            }
            true
        }
        None if last_sha == base => false,
        _ => return Err("native Goal owner marker and Goal history disagree".into()),
    };
    let effective = latest.as_ref().map_or_else(
        || owner(&marker.original),
        |decision| Owner {
            project_id: decision.target_config.project_id.clone(),
            identity_sha256: decision.target_config.local_identity_sha256.clone(),
        },
    );
    Ok(Resolved {
        owner: effective,
        pending,
        latest,
    })
}

pub(in crate::session_goal) fn resolve_owner(
    session: &Session,
    value: &Value,
    access: Access,
) -> Result<Resolved, String> {
    let raw = super::super::raw_history(&session.root)?;
    let history = super::super::records_from_raw(raw.as_deref())?;
    let result = resolve_history(session, value, &history)?;
    if session.owner_value(access)?.as_ref() != Some(value)
        || super::super::raw_history(&session.root)? != raw
    {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    Ok(result)
}

pub(in crate::session_goal) fn validate_legacy_history(
    session: &Session,
    value: &Value,
    access: Access,
) -> Result<(), String> {
    let raw = super::super::raw_history(&session.root)?;
    if raw
        .as_deref()
        .map(legacy_contains_correction)
        .transpose()?
        .unwrap_or(false)
    {
        return Err("native Goal owner marker is missing for correction history".into());
    }
    if session.owner_value(access)?.as_ref() != Some(value) {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    Ok(())
}

/// Legacy owners need only detect an illicit marker downgrade here. Avoid
/// reducing and hashing the whole Goal history on every legacy owner lookup;
/// ordinary Goal reads still validate the complete chain independently.
fn legacy_contains_correction(raw: &str) -> Result<bool, String> {
    for line in raw.lines() {
        let physical: Value = serde_json::from_str(line)
            .map_err(|_| "native Goal journal contains malformed JSON")?;
        if physical.get(super::model::FIELD).is_some() {
            return Ok(true);
        }
        if physical["kind"] == "lead_session_goal_delta" {
            let changes = physical["changes"]
                .as_array()
                .ok_or("native Goal journal delta is malformed")?;
            if changes
                .iter()
                .any(|change| change[1].as_array().is_some_and(Vec::is_empty))
            {
                return Err("native Goal journal has an unsupported root replacement delta".into());
            }
            if changes.iter().any(|change| {
                change[1]
                    .as_array()
                    .and_then(|path| path.first())
                    .and_then(Value::as_str)
                    == Some(super::model::FIELD)
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn replace(session: &Session, current: &Value, next: &Marker) -> Result<(), String> {
    let ledger = crate::run::ledger::ledger_path(&session.root, native::OWNER_FILE, false)?;
    let serialized = next.value()?.to_string();
    if serialized.len() as u64 > native::OWNER_MAX_BYTES {
        return Err("native Goal owner marker exceeds its byte bound".into());
    }
    sync_directory(&session.root)?;
    crate::run::ledger::replace_exact(&ledger, &current.to_string(), &serialized)?;
    sync_directory(&session.root)
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &std::path::Path) -> Result<(), String> {
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| error.to_string())
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &std::path::Path) -> Result<(), String> {
    Err(
        "native Goal owner correction requires durable directory synchronization on this host"
            .into(),
    )
}

pub(super) fn legacy_digest(value: &Value) -> Result<String, String> {
    value["sha256"]
        .as_str()
        .filter(|sha| native::valid_hash(sha))
        .map(str::to_owned)
        .ok_or_else(|| "native Goal owner digest is unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_two_refuses_the_v1_reader_and_detects_marker_tampering() {
        let key = "a".repeat(64);
        let session = Session {
            host: "codex",
            key: key.clone(),
            selector_key: key.clone(),
            root: std::path::PathBuf::from("/fixture"),
        };
        let mut original = serde_json::json!({"kind":"native_session_goal_owner", "protocol":1,
            "host":"codex", "sessionKey":key, "ownerProjectId":"p", "ownerIdentitySha256":"b".repeat(64)});
        let digest = hash::value(&original);
        original["sha256"] = serde_json::json!(digest);
        assert!(native::validate_owner_v1(&original, &session.key, session.host).is_ok());
        let marker = Marker::new(&session, &original, None, None, None).unwrap();
        let value = marker.value().unwrap();
        assert!(native::validate_owner_v1(&value, &session.key, session.host).is_err());
        assert!(Marker::parse(&session, &value).is_ok());
        let mut tampered = value;
        tampered["original"]["ownerProjectId"] = serde_json::json!("other");
        assert!(Marker::parse(&session, &tampered).is_err());
    }

    #[test]
    fn legacy_marker_scan_sees_physical_deltas_and_json_escapes() {
        let direct = r#"{"nativeGoalOwner\u0043orrections":[]}"#;
        let delta = r#"{"kind":"lead_session_goal_delta","changes":[["set",["nativeGoalOwnerCorrections"],[]]]}"#;
        let ordinary = r#"{"source":"nativeGoalOwnerCorrections","kind":"lead_session_goal"}"#;
        assert!(legacy_contains_correction(direct).unwrap());
        assert!(legacy_contains_correction(delta).unwrap());
        assert!(!legacy_contains_correction(ordinary).unwrap());
    }
}

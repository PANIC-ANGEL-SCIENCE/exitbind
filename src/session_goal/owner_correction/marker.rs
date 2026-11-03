//! Owner reader barrier and effective-owner resolution from sealed Goal history.

use super::model::{decisions, OwnerCorrectionDecision, SavedRequest};
use crate::{
    evidence::hash,
    session_goal::native::{self, Access, Owner, Session},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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

/// Fold validated logical records without retaining the complete history. A
/// Goal ID may recur later, so eligibility is retained across Goal transitions.
struct OwnershipFold<'a> {
    session: &'a Session,
    effective: Owner,
    latest: Option<OwnerCorrectionDecision>,
    before_latest: Option<String>,
    request_ids: BTreeSet<String>,
    eligibility: BTreeMap<String, Option<String>>,
    previous_goal: Option<String>,
    preceding: usize,
}

impl<'a> OwnershipFold<'a> {
    fn new(session: &'a Session, original: &Value) -> Self {
        Self {
            session,
            effective: owner(original),
            latest: None,
            before_latest: None,
            request_ids: BTreeSet::new(),
            eligibility: BTreeMap::new(),
            previous_goal: None,
            preceding: 0,
        }
    }

    fn require_empty(&self, goal_id: &str) -> Result<(), String> {
        match self.eligibility.get(goal_id) {
            Some(None) => Ok(()),
            Some(Some(error)) => Err(error.clone()),
            None => Err("the named Goal history is unavailable".into()),
        }
    }

    fn observe(&mut self, record: &Value, previous: Option<&Value>) -> Result<(), String> {
        let goal_id = record["goalId"]
            .as_str()
            .ok_or("native Goal identity is unavailable")?;
        let current = decisions(record)?;
        if self.previous_goal.as_deref() != Some(goal_id) {
            self.preceding = 0;
        }
        if current.len() > self.preceding {
            if current.len() != self.preceding + 1 {
                return Err("native Goal owner correction chain is inconsistent".into());
            }
            let decision = current.last().ok_or("owner correction is missing")?;
            let prior = previous.ok_or("native Goal owner correction has no predecessor")?;
            super::validate_recorded_selection(prior, decision)?;
            self.require_empty(&decision.goal_id)?;
            if decision.host != self.session.host
                || decision.session_key != self.session.key
                || decision.old_config.local_identity_sha256 != self.effective.identity_sha256
                || decision.old_config.project_id != self.effective.project_id
                || !self.request_ids.insert(decision.request_id.clone())
            {
                return Err("native Goal owner correction chain is inconsistent".into());
            }
            self.effective = Owner {
                project_id: decision.target_config.project_id.clone(),
                identity_sha256: decision.target_config.local_identity_sha256.clone(),
            };
            self.before_latest = self
                .latest
                .as_ref()
                .map(|item| item.decision_sha256.clone());
            self.latest = Some(decision.clone());
        }
        let eligibility = self.eligibility.entry(goal_id.to_owned()).or_insert(None);
        if eligibility.is_none() {
            *eligibility = super::empty_record(record).err();
        }
        self.preceding = current.len();
        self.previous_goal = Some(goal_id.to_owned());
        Ok(())
    }
}

impl super::super::RecoveryObserver for OwnershipFold<'_> {
    fn logical(&mut self, record: &Value, previous: Option<&Value>) -> Result<(), String> {
        self.observe(record, previous)
    }
}

fn finish(
    marker: &Marker,
    fold: OwnershipFold<'_>,
    head: Option<&Value>,
) -> Result<Resolved, String> {
    let last_sha = fold
        .latest
        .as_ref()
        .map(|decision| decision.decision_sha256.as_str());
    let base = marker.base_decision_sha256.as_deref();
    let pending = match &marker.pending {
        Some(decision) if last_sha == Some(decision.decision_sha256.as_str()) => {
            if fold.before_latest.as_deref() != base || fold.latest.as_ref() != Some(decision) {
                return Err(
                    "native Goal owner marker does not match its committed correction".into(),
                );
            }
            false
        }
        Some(decision) if last_sha == base => {
            let head = head.ok_or("native Goal correction predecessor is unavailable")?;
            if head["eventSha256"] != decision.previous_event_sha256
                || head["goalId"] != decision.goal_id
                || head["revision"] != decision.previous_revision
            {
                return Err("native Goal owner marker predecessor is inconsistent".into());
            }
            super::validate_recorded_selection(head, decision)?;
            fold.require_empty(&decision.goal_id)?;
            if decision.old_config.local_identity_sha256 != fold.effective.identity_sha256
                || decision.old_config.project_id != fold.effective.project_id
            {
                return Err("native Goal pending correction names another owner".into());
            }
            true
        }
        None if last_sha == base => false,
        _ => return Err("native Goal owner marker and Goal history disagree".into()),
    };
    Ok(Resolved {
        owner: fold.effective,
        pending,
        latest: fold.latest,
    })
}

pub(super) fn resolve_history(
    session: &Session,
    value: &Value,
    history: &[Value],
) -> Result<Resolved, String> {
    let marker = Marker::parse(session, value)?;
    let mut fold = OwnershipFold::new(session, &marker.original);
    let mut previous = None;
    for record in history {
        fold.observe(record, previous)?;
        previous = Some(record);
    }
    finish(&marker, fold, previous)
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
        if legacy_physical_contains_correction(&physical)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn legacy_physical_contains_correction(physical: &Value) -> Result<bool, String> {
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
        return Ok(changes.iter().any(|change| {
            change[1]
                .as_array()
                .and_then(|path| path.first())
                .and_then(Value::as_str)
                == Some(super::model::FIELD)
        }));
    }
    Ok(false)
}

struct LegacyObserver;
impl super::super::RecoveryObserver for LegacyObserver {
    fn physical(&mut self, physical: &Value) -> Result<(), String> {
        if legacy_physical_contains_correction(physical)? {
            return Err("native Goal owner marker is missing for correction history".into());
        }
        Ok(())
    }
}

/// The version-2 envelope never represents an adoption. Its locator can be
/// resolved before reading a journal, but it grants no owner credit.
pub(in crate::session_goal) fn validate_marker_envelope(
    session: &Session,
    value: &Value,
) -> Result<(), String> {
    Marker::parse(session, value).map(|_| ())
}

/// Resolve effective ownership and the latest Goal from one bounded logical
/// fold, then compare a second bounded byte fingerprint and the owner record.
pub(in crate::session_goal) fn recovery_snapshot(
    session: &Session,
    access: Access,
) -> Result<Option<native::RecoverySnapshot>, String> {
    recovery_snapshot_inner(session, access, || {})
}

fn recovery_snapshot_inner(
    session: &Session,
    access: Access,
    after_fold: impl FnOnce(),
) -> Result<Option<native::RecoverySnapshot>, String> {
    let started = std::time::Instant::now();
    let Some(value) = session.owner_value(access)? else {
        return Ok(None);
    };
    super::super::recovery_time(started)?;
    let (owner, correction, goal) = if value["protocol"] == 2 {
        let marker = Marker::parse(session, &value)?;
        let mut observer = OwnershipFold::new(session, &marker.original);
        let goal = super::super::recovery_fold(&session.root, started, &mut observer, true)?;
        let resolved = finish(&marker, observer, goal.latest.as_ref())?;
        (
            resolved.owner,
            resolved.latest.map(|item| item.target_config),
            goal,
        )
    } else {
        native::validate_owner_v1(&value, &session.key, session.host)?;
        if value.get("journalSessionKey").is_some() {
            return Err("native session Goal owner record names another session's journal that cannot be read".into());
        }
        let goal = super::super::recovery_fold(&session.root, started, &mut LegacyObserver, true)?;
        (owner(&value), None, goal)
    };
    after_fold();
    super::super::recovery_time(started)?;
    if session.owner_value(access)?.as_ref() != Some(&value) {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    super::super::recovery_recheck(&session.root, started, &goal.fingerprint)?;
    if session.owner_value(access)?.as_ref() != Some(&value) {
        return Err("native Goal owner and journal snapshot is inconsistent; retry".into());
    }
    super::super::recovery_time(started)?;
    Ok(Some(native::RecoverySnapshot {
        owner,
        latest: goal.latest,
        owner_config: correction,
    }))
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
        let null = r#"{"nativeGoalOwnerCorrections":null}"#;
        let delta = r#"{"kind":"lead_session_goal_delta","changes":[["set",["nativeGoalOwnerCorrections"],[]]]}"#;
        let root = r#"{"kind":"lead_session_goal_delta","changes":[["set",[],{}]]}"#;
        let ordinary = r#"{"source":"nativeGoalOwnerCorrections","kind":"lead_session_goal"}"#;
        assert!(legacy_contains_correction(direct).unwrap());
        assert!(legacy_contains_correction(null).unwrap());
        assert!(legacy_contains_correction(delta).unwrap());
        assert!(legacy_contains_correction(root).is_err());
        assert!(!legacy_contains_correction(ordinary).unwrap());
        for value in [direct, null, delta, root] {
            let physical: Value = serde_json::from_str(value).unwrap();
            assert!(super::super::super::RecoveryObserver::physical(
                &mut LegacyObserver,
                &physical
            )
            .is_err());
        }
    }

    #[cfg(unix)]
    fn fixture(label: &str, protocol: u64) -> (crate::test_root::TempRoot, Session) {
        let root = crate::test_root::temp(label);
        let base = root.join("private");
        crate::project::portability::create_private_directory(&base).unwrap();
        let key = "a".repeat(64);
        let session = Session {
            host: "codex",
            key: key.clone(),
            selector_key: key.clone(),
            root: base.join(format!("session-{key}")),
        };
        crate::project::portability::create_private_directory(&session.root).unwrap();
        let state = session
            .root
            .join(crate::project::layout_types::state_namespace());
        crate::project::portability::create_private_directory(&state).unwrap();
        let mut original = serde_json::json!({"kind":"native_session_goal_owner", "protocol":1,
            "host":"codex", "sessionKey":key, "ownerProjectId":"fixture",
            "ownerIdentitySha256":"b".repeat(64)});
        original["sha256"] = serde_json::json!(hash::value(&original));
        let owner = if protocol == 1 {
            original
        } else {
            Marker::new(&session, &original, None, None, None)
                .unwrap()
                .value()
                .unwrap()
        };
        std::fs::write(session.root.join(native::OWNER_FILE), owner.to_string()).unwrap();
        std::fs::write(
            state.join("session-goal.jsonl"),
            include_str!("../../../tests/fixtures/session-goal-complete-records.jsonl"),
        )
        .unwrap();
        (root, session)
    }

    #[cfg(unix)]
    #[test]
    fn recovery_owner_refuses_over_budget_journals_before_credit_for_both_protocols() {
        for protocol in [1, 2] {
            let (_root, session) = fixture(&format!("owner-recovery-bound-{protocol}"), protocol);
            assert!(session
                .recovery_snapshot(Access::ReadExisting)
                .unwrap()
                .unwrap()
                .latest
                .is_some());
            let journal = session
                .root
                .join(crate::project::layout_types::state_namespace())
                .join("session-goal.jsonl");
            std::fs::write(&journal, "{}\n".repeat(4097)).unwrap();
            let error = match session.recovery_snapshot(Access::ReadExisting) {
                Err(error) => error,
                _ => panic!("protocol {protocol} credited a journal with too many records"),
            };
            assert!(error.contains("4096 physical records"), "{error}");
            std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&journal)
                .unwrap()
                .set_len(64 * 1024 * 1024 + 1)
                .unwrap();
            let error = match session.recovery_snapshot(Access::ReadExisting) {
                Err(error) => error,
                _ => panic!("protocol {protocol} credited an oversized journal"),
            };
            assert!(error.contains("byte bound"), "{error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn recovery_rechecks_owner_and_same_length_journal_drift() {
        for protocol in [1, 2] {
            let (_root, session) = fixture(&format!("owner-recovery-drift-{protocol}"), protocol);
            let journal = session
                .root
                .join(crate::project::layout_types::state_namespace())
                .join("session-goal.jsonl");
            let original = std::fs::read(&journal).unwrap();
            let mut changed = original.clone();
            changed[0] = b'[';
            let result = recovery_snapshot_inner(&session, Access::ReadExisting, || {
                std::fs::write(&journal, &changed).unwrap();
            });
            assert!(matches!(result, Err(ref error) if error.contains("snapshot is inconsistent")));
            std::fs::write(&journal, original).unwrap();
            let path = session.root.join(native::OWNER_FILE);
            let result = recovery_snapshot_inner(&session, Access::ReadExisting, || {
                let mut owner: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                owner["sha256"] = serde_json::json!("0".repeat(64));
                std::fs::write(&path, owner.to_string()).unwrap();
            });
            assert!(matches!(result, Err(ref error) if error.contains("snapshot is inconsistent")));
        }
    }

    #[cfg(unix)]
    #[test]
    fn fold_preserves_removed_work_and_reused_goal_id_ineligibility() {
        let (_root, session) = fixture("owner-recovery-history-eligibility", 1);
        let original: Value =
            serde_json::from_slice(&std::fs::read(session.root.join(native::OWNER_FILE)).unwrap())
                .unwrap();
        for field in ["journeyAdmissions", "mappings"] {
            let clean = super::super::tests::goal();
            let mut old = clean.clone();
            if field == "mappings" {
                let source = &clean["continuation"]["requirements"][0];
                old["continuation"]["mappings"] = serde_json::json!([{"work":format!("smw_{}", "a".repeat(64)),
                    "active":false,"startEventSha256":"b".repeat(64),
                    "requirement":{"id":"R1","revision":1,"text":source["text"],"sourceSha256":source["source"]["sha256"]}}]);
            } else {
                old[field] = serde_json::json!([{}]);
            }
            let mut other = clean.clone();
            other["goalId"] = serde_json::json!("other");
            let mut fold = OwnershipFold::new(&session, &original);
            fold.observe(&old, None).unwrap();
            fold.observe(&clean, Some(&old)).unwrap();
            fold.observe(&other, Some(&clean)).unwrap();
            fold.observe(&clean, Some(&other)).unwrap();
            assert!(fold.require_empty("g").is_err(), "{field}");
            assert!(fold.require_empty("other").is_ok());
        }
    }
}

// Validates explicit uncredited native-journey retirement selections.
// Never grants acceptance to a retired Work; see `src/session_goal/recovery.rs`.
//! A typed optional Goal-record extension for an owner's exact retirement
//! decision. The complete original journey membership remains authoritative.

use super::{valid_text, ActiveDecision, MAX_FIXTURES};
use crate::{
    config::Loaded,
    evidence::hash,
    run::ledger::{self, LedgerPath},
    session_goal::journey::{self, Member},
    work::supersede::UncreditedRetirementSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeSet, path::PathBuf};

pub(super) const FIELD: &str = "nativeGoalRetirementSelection";
const VERSION: u64 = 1;
const MAX_REQUEST_BYTES: usize = 16 * 1024;

thread_local! {
    static ACTIVE_LOCKS: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
}

struct ActiveLocks;

impl Drop for ActiveLocks {
    fn drop(&mut self) {
        ACTIVE_LOCKS.with(|active| *active.borrow_mut() = None);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RequestItem {
    pub(super) work: String,
    pub(super) reason: String,
    pub(super) replacement: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Replacement {
    pub(super) member: Member,
    pub(super) accepted_endpoint_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Item {
    pub(super) member: Member,
    pub(super) reason: String,
    pub(super) blocked: UncreditedRetirementSnapshot,
    pub(super) replacement: Replacement,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Selection {
    pub(super) version: u64,
    pub(super) goal_id: String,
    pub(super) goal_sha256: String,
    pub(super) prior_recovery_decision_sha256: String,
    pub(super) recovery_decision_sha256: String,
    pub(super) owner_identity_sha256: String,
    pub(super) session_key: String,
    pub(super) source_config_sha256: String,
    pub(super) source_writers_quiesced: bool,
    pub(super) items: Vec<Item>,
}

pub(super) fn request(
    raw: Option<&str>,
    quiesced: bool,
) -> Result<Option<Vec<RequestItem>>, String> {
    let Some(raw) = raw else {
        return if quiesced {
            Err("--source-writers-quiesced requires --retirements".into())
        } else {
            Ok(None)
        };
    };
    if !quiesced {
        return Err("explicit retirement requires --source-writers-quiesced".into());
    }
    if raw.len() > MAX_REQUEST_BYTES {
        return Err("--retirements exceeds its bounded size".into());
    }
    let mut items: Vec<RequestItem> = serde_json::from_str(raw).map_err(|_| {
        "--retirements must be an array of exact work, reason and replacement objects"
    })?;
    let mut seen = BTreeSet::new();
    if items.is_empty()
        || items.len() > MAX_FIXTURES
        || items.iter().any(|item| {
            !crate::work::valid_work_handle(&item.work)
                || !crate::work::valid_work_handle(&item.replacement)
                || item.work == item.replacement
                || !valid_text(&item.reason, 1024)
                || !seen.insert(item.work.clone())
        })
    {
        return Err("--retirements requires unique exact Work IDs, distinct replacements and bounded reasons".into());
    }
    items.sort_by(|left, right| left.work.cmp(&right.work));
    Ok(Some(items))
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_structure(selection: &Selection) -> Result<(), String> {
    if selection.version != VERSION
        || !valid_text(&selection.goal_id, 128)
        || !valid_sha(&selection.goal_sha256)
        || !valid_sha(&selection.recovery_decision_sha256)
        || !valid_sha(&selection.prior_recovery_decision_sha256)
        || selection.prior_recovery_decision_sha256 == selection.recovery_decision_sha256
        || !super::record::valid_hash(&selection.owner_identity_sha256)
        || !super::record::valid_hash(&selection.session_key)
        || !super::record::valid_hash(&selection.source_config_sha256)
        || !selection.source_writers_quiesced
        || selection.items.is_empty()
        || selection.items.len() > MAX_FIXTURES
    {
        return Err(
            "native Goal retirement selection is invalid or has a different owner or recovery"
                .into(),
        );
    }
    let mut previous: Option<&str> = None;
    for item in &selection.items {
        let work = item.member.work.as_str();
        if !crate::work::valid_work_handle(work)
            || !crate::work::valid_work_handle(&item.replacement.member.work)
            || previous.is_some_and(|prior| prior >= work)
            || !valid_text(&item.reason, 1024)
            || item.blocked.work != work
            || item.blocked.start_event_sha256 != item.member.start_event_sha256
            || item.blocked.config_sha256 != item.member.config_sha256
            || !valid_sha(&item.blocked.head_event_sha256)
            || !valid_sha(&item.blocked.raw_ledger_sha256)
            || !valid_sha(&item.blocked.governor_sha256)
            || !valid_sha(&item.replacement.accepted_endpoint_sha256)
            || item.replacement.member.work == work
        {
            return Err(
                "native Goal retirement member or accepted replacement binding is invalid".into(),
            );
        }
        previous = Some(work);
    }
    if selection.items.iter().any(|item| {
        selection
            .items
            .iter()
            .any(|retired| retired.member.work == item.replacement.member.work)
    }) {
        return Err("a retired Work cannot be its own or another retirement's replacement".into());
    }
    Ok(())
}

pub(super) fn validate(selection: &Selection, active: &ActiveDecision) -> Result<(), String> {
    validate_structure(selection)?;
    if selection.goal_id != active.goal_id
        || selection.recovery_decision_sha256 != active.decision_sha256
        || selection.owner_identity_sha256 != active.owner_identity_sha256
        || selection.session_key != active.session_key
        || selection.source_config_sha256 != active.source_config.config_sha256
    {
        return Err(
            "native Goal retirement selection is invalid or has a different owner or recovery"
                .into(),
        );
    }
    if selection.items.iter().any(|item| {
        !active
            .source_members
            .iter()
            .any(|source| source.member == item.member)
            || !active
                .source_members
                .iter()
                .any(|source| source.member == item.replacement.member)
    }) {
        return Err(
            "native Goal retirement member or accepted replacement binding is invalid".into(),
        );
    }
    Ok(())
}

pub(super) fn selected(
    record: &Value,
    active: &ActiveDecision,
) -> Result<Option<Selection>, String> {
    let Some(value) = record.get(FIELD) else {
        return Ok(None);
    };
    let selection: Selection = serde_json::from_value(value.clone())
        .map_err(|_| "native Goal retirement selection is malformed or has unknown fields")?;
    if selection.recovery_decision_sha256 != active.decision_sha256 {
        validate_structure(&selection)?;
        // A previous owner choice remains raw history, not authority for a
        // different recovery. Journal validation checks that it was carried.
        let journey = journey::state(record)?
            .ok_or("native Goal retirement selection requires the original journey")?;
        if selection.items.iter().any(|item| {
            !journey.members.contains(&item.member)
                || !journey.members.contains(&item.replacement.member)
        }) {
            return Err(
                "native Goal retirement member or accepted replacement binding is invalid".into(),
            );
        }
        return Ok(None);
    }
    validate(&selection, active)?;
    let goal = record["goal"]
        .as_str()
        .ok_or("native Goal retirement selection requires the original Goal text")?;
    if selection.goal_sha256 != hash::text(goal) {
        return Err("native Goal retirement selection belongs to different Goal text".into());
    }
    Ok(Some(selection))
}

pub(super) fn digest(selection: &Selection) -> String {
    hash::value(
        &json!({"domain":"exitbind.native-goal-uncredited-retirement.v1", "selection":selection}),
    )
}

pub(super) fn integration_digest(active: &ActiveDecision, selection: Option<&Selection>) -> String {
    selection.map_or_else(
        || active.decision_sha256.clone(),
        |selection| {
            hash::value(
                &json!({"domain":"exitbind.native-goal-retirement-integration.v1",
            "recoveryDecisionSha256":active.decision_sha256, "selectionSha256":digest(selection)}),
            )
        },
    )
}

/// Take the existing Run locks in deterministic order before a Goal lock.
/// These locks cannot fence an already-running old writer; the owner must keep
/// all source writers quiescent throughout selection and governed use.
pub(super) fn with_locks<T>(
    source: &Loaded,
    requests: &[RequestItem],
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    fn take<T>(
        locks: &[LedgerPath],
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        match locks.split_first() {
            Some((lock, rest)) => ledger::with_lock(lock, || take(rest, operation)),
            None => operation(),
        }
    }
    let works = requests
        .iter()
        .flat_map(|item| [&item.work, &item.replacement])
        .collect::<BTreeSet<_>>();
    let mut locks = Vec::with_capacity(works.len());
    for work in works {
        let relative = crate::work::resolve(source, work)?;
        locks.push(ledger::ledger_path(&source.state_root, &relative, false)?);
    }
    locks.sort_by(|left, right| left.lock.cmp(&right.lock));
    locks.dedup_by(|left, right| left.lock == right.lock);
    let paths = locks
        .iter()
        .map(|lock| lock.lock.clone())
        .collect::<Vec<_>>();
    if let Some(active) = ACTIVE_LOCKS.with(|current| current.borrow().clone()) {
        return if active == paths {
            operation()
        } else {
            Err("retirement selection changed while its Run locks were held".into())
        };
    }
    take(&locks, || {
        ACTIVE_LOCKS.with(|current| *current.borrow_mut() = Some(paths));
        let _guard = ActiveLocks;
        operation()
    })
}

pub(super) fn from_selection(selection: &Selection) -> Vec<RequestItem> {
    selection
        .items
        .iter()
        .map(|item| RequestItem {
            work: item.member.work.clone(),
            reason: item.reason.clone(),
            replacement: item.replacement.member.work.clone(),
        })
        .collect()
}

/// Collect current facts through their existing owners. The caller holds all
/// selected Run locks, and an owner has attested source-writer quiescence.
pub(super) fn observe(
    source: &Loaded,
    record: &Value,
    active: &ActiveDecision,
    requests: &[RequestItem],
    prior_recovery_decision_sha256: &str,
) -> Result<Selection, String> {
    let state = journey::state(record)?.ok_or("native Goal journey is unavailable")?;
    if !state.admissions.is_empty() {
        return Err("pending native Goal admission prevents retirement".into());
    }
    let mut items = Vec::with_capacity(requests.len());
    for request in requests {
        let selected = active
            .source_members
            .iter()
            .find(|source| source.member.work == request.work)
            .ok_or("retired Work is not an exact current Source member")?;
        let replacement = active
            .source_members
            .iter()
            .find(|source| source.member.work == request.replacement)
            .ok_or("retirement replacement is not an exact current Source member")?;
        let original = journey::original_for_retirement(source, &selected.member)?;
        let blocked = crate::work::supersede::uncredited_retirement_snapshot(
            &original,
            &selected.member.work,
        )?;
        let endpoint = crate::config::load(Some(&replacement.endpoint_config.path))?;
        if super::config_binding(&endpoint)? != replacement.endpoint_config {
            return Err("retirement replacement configuration changed".into());
        }
        journey::validate_recovery_endpoint(&endpoint, &replacement.member)?;
        let (endpoint_work, accepted_endpoint_sha256) =
            journey::accepted_identity_for_retirement(&endpoint, &replacement.member)?
                .ok_or("retirement replacement has no accepted endpoint")?;
        if endpoint_work != request.replacement {
            return Err("retirement replacement is not the exact accepted Work".into());
        }
        let evidence = crate::run::result_ref_evidence(&endpoint, &endpoint_work)?
            .ok_or("retirement replacement evidence is unavailable")?;
        if !evidence.accepted
            || !evidence.artifact_current
            || evidence.drift.is_some()
            || evidence.inputs_current.is_none()
        {
            return Err("retirement replacement acceptance evidence is unavailable".into());
        }
        items.push(Item {
            member: selected.member.clone(),
            reason: request.reason.clone(),
            blocked,
            replacement: Replacement {
                member: replacement.member.clone(),
                accepted_endpoint_sha256,
            },
        });
    }
    let selection = Selection {
        version: VERSION,
        goal_id: active.goal_id.clone(),
        goal_sha256: hash::text(
            record["goal"]
                .as_str()
                .ok_or("native Goal retirement selection requires the original Goal text")?,
        ),
        prior_recovery_decision_sha256: prior_recovery_decision_sha256.to_owned(),
        recovery_decision_sha256: active.decision_sha256.clone(),
        owner_identity_sha256: active.owner_identity_sha256.clone(),
        session_key: active.session_key.clone(),
        source_config_sha256: active.source_config.config_sha256.clone(),
        source_writers_quiesced: true,
        items,
    };
    validate(&selection, active)?;
    Ok(selection)
}

pub(super) fn revalidate(
    source: &Loaded,
    record: &Value,
    active: &ActiveDecision,
    selection: &Selection,
) -> Result<(), String> {
    validate(selection, active)?;
    let current = observe(
        source,
        record,
        active,
        &from_selection(selection),
        &selection.prior_recovery_decision_sha256,
    )?;
    if &current != selection {
        return Err(
            "retired Work, accounting or replacement changed; request a fresh owner selection"
                .into(),
        );
    }
    Ok(())
}

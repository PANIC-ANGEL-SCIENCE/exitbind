// Validates explicitly uncredited Work retained by an owner correction.
// Refuses unmatched admissions or changed identities; see `src/session_goal/owner_correction.rs`.
//! Exact historical association selection without transferring Work authority.

use super::model::{OwnerCorrectionDecision, UncreditedMember, MAX_UNCREDITED_MEMBERS};
use crate::{config::Loaded, evidence::hash};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct History {
    members: BTreeMap<String, Value>,
    pending: BTreeMap<String, Value>,
    linked_intents: BTreeMap<String, String>,
    error: Option<String>,
}

fn list(record: &Value, field: &str) -> Result<Vec<Value>, String> {
    match record.get(field) {
        None => Ok(Vec::new()),
        Some(value) => value
            .as_array()
            .cloned()
            .ok_or_else(|| format!("{field} is malformed")),
    }
}

fn keyed(values: Vec<Value>) -> Result<BTreeMap<String, Value>, String> {
    let mut result = BTreeMap::new();
    for value in values {
        let work = value["work"]
            .as_str()
            .ok_or("historical Work identity is unavailable")?
            .to_owned();
        if !crate::work::valid_work_handle(&work) || result.insert(work, value).is_some() {
            return Err("historical Work identity is invalid or duplicated".into());
        }
    }
    Ok(result)
}

fn eligible_record(record: &Value) -> Result<(), String> {
    for field in [
        "journeyIntegrations",
        "workAssociations",
        "sessionAdoptions",
    ] {
        if !list(record, field)?.is_empty() {
            return Err(
                "owner correction does not accept integration, association or adoption history"
                    .into(),
            );
        }
    }
    if record.get("nativeGoalRecovery").is_some() || record.get("quarantineResiduals").is_some() {
        return Err("owner correction does not accept recovery or quarantine state".into());
    }
    if !record["continuation"].is_null() {
        if !super::super::requirements::named(record) {
            return Err("owner correction does not accept an incompatible continuation".into());
        }
        super::super::requirements::recorded_sources(record)?;
        if !record["continuation"]["mappings"]
            .as_array()
            .is_some_and(Vec::is_empty)
        {
            return Err("owner correction requires no active or retired Work mappings".into());
        }
    }
    super::super::journey::state(record)?;
    Ok(())
}

fn same_admission_member(admission: &Value, member: &Value) -> bool {
    let mut a = admission.clone();
    let mut m = member.clone();
    if let (Some(a), Some(m)) = (a.as_object_mut(), m.as_object_mut()) {
        a.remove("startIntentSha256");
        m.remove("startEventSha256");
        a == m
    } else {
        false
    }
}

impl History {
    pub(super) fn observe(&mut self, record: &Value, previous: Option<&Value>) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.observe_inner(record, previous) {
            self.error = Some(error);
        }
    }

    fn observe_inner(&mut self, record: &Value, previous: Option<&Value>) -> Result<(), String> {
        eligible_record(record)?;
        let members = keyed(list(record, "journeyMembers")?)?;
        let pending = keyed(list(record, "journeyAdmissions")?)?;
        for (work, member) in &self.members {
            if members.get(work) != Some(member) {
                return Err("historical journey membership was removed or changed".into());
            }
        }
        for (work, admission) in &self.pending {
            match pending.get(work) {
                Some(current) if current == admission => {}
                Some(_) => return Err("historical Work admission changed".into()),
                None => {
                    let member = members
                        .get(work)
                        .ok_or("historical Work admission disappeared without linkage")?;
                    if !same_admission_member(admission, member) || self.members.contains_key(work)
                    {
                        return Err(
                            "historical Work admission does not match its linked member".into()
                        );
                    }
                    let intent = admission["startIntentSha256"]
                        .as_str()
                        .ok_or("historical Work admission intent is unavailable")?;
                    self.linked_intents.insert(work.clone(), intent.to_owned());
                }
            }
        }
        for (work, admission) in &pending {
            if !self.pending.contains_key(work) {
                let previous = previous.ok_or("historical admission has no Goal predecessor")?;
                if previous["goalId"] != record["goalId"]
                    || previous["eventSha256"] != admission["parentObservedGoalEventSha256"]
                    || record["goalId"] != admission["parentGoalId"]
                    || self.members.contains_key(work)
                {
                    return Err("historical Work admission names another Goal observation".into());
                }
            }
        }
        for (work, member) in &members {
            if member["parentGoalId"] != record["goalId"] {
                return Err("historical member belongs to another Goal".into());
            }
            if !self.members.contains_key(work) && !self.linked_intents.contains_key(work) {
                return Err("historical member has no exact consumed admission".into());
            }
        }
        self.members = members;
        self.pending = pending;
        Ok(())
    }

    fn complete(&self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if !self.pending.is_empty()
            || self.members.is_empty()
            || self.members.len() != self.linked_intents.len()
        {
            return Err("owner correction requires a complete nonempty linked Work set without pending admissions".into());
        }
        Ok(())
    }

    pub(super) fn validate_decision(
        &self,
        decision: &OwnerCorrectionDecision,
    ) -> Result<(), String> {
        self.complete()?;
        let selected = decision
            .uncredited_members
            .as_ref()
            .ok_or("uncredited Work selection is unavailable")?;
        if selected.len() != self.members.len() {
            return Err(
                "uncredited Work selection does not cover the exact historical member set".into(),
            );
        }
        for item in selected {
            let member = self
                .members
                .get(&item.work)
                .ok_or("uncredited Work is not a historical member")?;
            if hash::value(member) != item.member_sha256
                || member["startEventSha256"] != item.start_event_sha256
                || self.linked_intents.get(&item.work) != Some(&item.start_intent_sha256)
            {
                return Err(
                    "uncredited Work selection no longer matches its linked identity".into(),
                );
            }
        }
        Ok(())
    }
}

pub(super) fn parse_selection(raw: &str) -> Result<Vec<String>, String> {
    if raw.len() > 16 * 1024 {
        return Err("uncredited Work selection exceeds its bound".into());
    }
    let mut works: Vec<String> = serde_json::from_str(raw)
        .map_err(|_| "--uncredited-members must be a JSON list of Work identities")?;
    let original = works.len();
    works.sort();
    works.dedup();
    if works.len() > MAX_UNCREDITED_MEMBERS {
        return Err(
            "owner correction needs two free journey slots for fresh validation and integration"
                .into(),
        );
    }
    if works.is_empty()
        || works.len() != original
        || works
            .iter()
            .any(|work| !crate::work::valid_work_handle(work))
    {
        return Err("uncredited Work selection is empty, duplicated or invalid".into());
    }
    Ok(works)
}

#[derive(Clone)]
pub(super) struct SelectedMember {
    work: String,
    member: Value,
    intent: String,
}

pub(super) struct MemberLock {
    work: String,
    pub(super) ledger: crate::run::ledger::LedgerPath,
}

/// Establish the complete historical selection and original owner before
/// reading any configuration or Work path named by a member.
pub(super) fn select_history(
    history: &[Value],
    goal_id: &str,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    works: &[String],
) -> Result<Vec<SelectedMember>, String> {
    let mut observed = History::default();
    let mut previous = None;
    for record in history {
        if record["goalId"] == goal_id {
            observed.observe(record, previous);
        }
        previous = Some(record);
    }
    observed.complete()?;
    let actual: BTreeSet<_> = observed.members.keys().cloned().collect();
    if actual != works.iter().cloned().collect() {
        return Err(
            "uncredited Work selection must name every historical member exactly once".into(),
        );
    }
    works
        .iter()
        .map(|work| {
            let member = &observed.members[work];
            if member["host"] != session.host
                || member["sessionKey"] != session.key
                || member["parentGoalId"] != goal_id
                || member["ownerIdentitySha256"] != owner.identity_sha256
                || member["ownerProjectId"] != serde_json::json!(owner.project_id)
            {
                return Err("historical Work belongs to another session or owner".into());
            }
            Ok(SelectedMember {
                work: work.clone(),
                member: member.clone(),
                intent: observed.linked_intents[work].clone(),
            })
        })
        .collect()
}

pub(super) fn matches_binding(
    member: &Value,
    binding: &super::super::recovery::ConfigBinding,
) -> bool {
    member["projectIdentitySha256"] == binding.project_identity_sha256
        && member["localIdentitySha256"] == binding.local_identity_sha256
        && member["configPath"] == binding.path
        && member["configSha256"] == binding.config_sha256
        && member["productRoot"] == binding.product_root
        && member["stateRoot"] == binding.state_root
}

fn source(member: &Value) -> Result<Loaded, String> {
    let path = member["configPath"]
        .as_str()
        .ok_or("historical Work configuration path is unavailable")?;
    let source = super::target_config(path)?;
    let binding = super::super::recovery::config_binding_for_current(&source)?;
    if !matches_binding(member, &binding) {
        return Err("historical Work configuration or project identity changed".into());
    }
    Ok(source)
}

fn member_ledger(source: &Loaded, work: &str) -> Result<crate::run::ledger::LedgerPath, String> {
    let relative = crate::work::resolve(source, work)?;
    crate::run::ledger::ledger_path(&source.state_root, &relative, false)
}

pub(super) fn member_locks(selected: &[SelectedMember]) -> Result<Vec<MemberLock>, String> {
    selected
        .iter()
        .map(|item| {
            let source = source(&item.member)?;
            Ok(MemberLock {
                work: item.work.clone(),
                ledger: member_ledger(&source, &item.work)?,
            })
        })
        .collect()
}

fn require_locked(
    work: &str,
    current: &crate::run::ledger::LedgerPath,
    locks: &[MemberLock],
) -> Result<(), String> {
    let lock = locks
        .iter()
        .find(|lock| lock.work == work)
        .ok_or("historical Work was not locked")?;
    if lock.ledger.path != current.path
        || lock.ledger.expected != current.expected
        || lock.ledger.root != current.root
        || lock.ledger.lock != current.lock
    {
        return Err("historical Work ledger path changed after locking".into());
    }
    Ok(())
}

pub(super) fn select(
    selected: &[SelectedMember],
    target: &Loaded,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    locks: &[MemberLock],
) -> Result<(Vec<UncreditedMember>, bool), String> {
    const FIELDS: &[&str] = &[
        "host",
        "sessionKey",
        "parentGoalId",
        "parentObservedGoalEventSha256",
        "ownerProjectId",
        "ownerIdentitySha256",
    ];
    let target_binding = super::super::recovery::config_binding_for_current(target)?;
    let same_target = selected
        .iter()
        .all(|item| matches_binding(&item.member, &target_binding));
    let mut sealed = Vec::new();
    for item in selected {
        let work = &item.work;
        let member = &item.member;
        let source = source(member)?;
        super::super::journey::validate_member_for_session(&source, member, session, owner)?;
        let ledger = member_ledger(&source, work)?;
        require_locked(work, &ledger, locks)?;
        let (_, events, _) = crate::run::ledger::load_at(&source, &ledger)?;
        let start = events
            .first()
            .ok_or("historical Work start is unavailable")?;
        let intent = &item.intent;
        if start["action"] != "start"
            || start["eventSha256"] != member["startEventSha256"]
            || !start["supersedes"].is_null()
            || start["configSha256"] != member["configSha256"]
            || hash::text(start["goal"].as_str().unwrap_or_default()) != member["workGoalSha256"]
            || crate::run::start_event_intent_sha256(start)? != *intent
        {
            return Err("historical Work start does not match its consumed admission".into());
        }
        let companion = super::super::association::read_private_companion(
            &source,
            work,
            ".session-goal.json",
            "Work Goal context companion",
            "native_session_goal_context",
            1,
            FIELDS,
        )?
        .ok_or("historical Work context companion is unavailable")?;
        if companion["startEventSha256"] != member["startEventSha256"]
            || FIELDS
                .iter()
                .any(|field| companion[*field] != member[*field])
        {
            return Err("historical Work companion does not match its linked member".into());
        }
        // Re-read the configuration after the ledger and companion so a
        // changed source cannot be sealed through an earlier in-memory load.
        super::fresh(&source)?;
        sealed.push(UncreditedMember {
            work: work.clone(),
            member_sha256: hash::value(member),
            start_event_sha256: member["startEventSha256"]
                .as_str()
                .ok_or("Work start identity unavailable")?
                .to_owned(),
            start_intent_sha256: intent.clone(),
            companion_sha256: companion["sha256"]
                .as_str()
                .ok_or("Work companion identity unavailable")?
                .to_owned(),
        });
    }
    // Check every locked path again after the complete set was inspected.
    for item in selected {
        let source = source(&item.member)?;
        let ledger = member_ledger(&source, &item.work)?;
        require_locked(&item.work, &ledger, locks)?;
    }
    Ok((sealed, same_target))
}

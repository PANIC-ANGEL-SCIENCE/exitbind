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

pub(super) fn select(
    history: &[Value],
    goal_id: &str,
    target: &Loaded,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    works: &[String],
) -> Result<Vec<UncreditedMember>, String> {
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
    const FIELDS: &[&str] = &[
        "host",
        "sessionKey",
        "parentGoalId",
        "parentObservedGoalEventSha256",
        "ownerProjectId",
        "ownerIdentitySha256",
    ];
    let mut selected = Vec::new();
    for work in works {
        let member = &observed.members[work];
        super::super::journey::validate_member_for_session(target, member, session, owner)?;
        let relative = crate::work::resolve(target, work)?;
        let (_, events, _) = crate::run::ledger::load(target, &relative)?;
        let start = events
            .first()
            .ok_or("historical Work start is unavailable")?;
        let intent = &observed.linked_intents[work];
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
            target,
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
        selected.push(UncreditedMember {
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
    Ok(selected)
}

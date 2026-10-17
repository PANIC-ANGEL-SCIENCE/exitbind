//! Admission metadata only. Work ledgers remain the execution/evidence owner.
use super::model::{Plan, Spec};
use crate::{config::Loaded, evidence::hash, project::path, run::ledger};
use serde::{Deserialize, Serialize};

const LIMIT: u64 = 64 * 1024 * 1024;

fn exceeds_limit(existing: usize, addition: usize) -> bool {
    existing
        .checked_add(addition)
        .map_or(true, |total| total > LIMIT as usize)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Claim {
    pub task_id: String,
    pub plan_sha256: String,
    pub plan: Plan,
    pub work: String,
    pub start_intent_sha256: String,
    pub checker_sha256: String,
    pub admitted_at: String,
    pub association: super::control::Association,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    pub version: u64,
    pub revision: u64,
    pub project_identity: String,
    pub configuration_sha256: String,
    pub specification_path: String,
    pub specification_sha256: String,
    pub specification: Spec,
    pub selected_at: String,
    pub reason: String,
    pub stopped: bool,
    pub claims: Vec<Claim>,
    pub account: super::control::Account,
    pub previous_event_sha256: Option<String>,
    pub event_sha256: String,
}

pub(super) fn relative(loaded: &Loaded, mandate: &str) -> Result<String, String> {
    let identity = crate::session_goal::requirements::identity(loaded)?;
    Ok(format!(
        "{}/artifacts/project-continuation/{}.jsonl",
        crate::project::layout_types::state_namespace(),
        hash::value(&serde_json::json!({"project":identity,"mandate":mandate}))
    ))
}

pub(super) fn prepare(loaded: &Loaded, relative: &str) -> Result<ledger::LedgerPath, String> {
    let full = loaded.state_root.join(relative);
    crate::project::managed_files::ensure_state_directory(
        &loaded.state_root,
        full.parent().ok_or("continuation journal has no parent")?,
    )?;
    let selected = ledger::ledger_path(&loaded.state_root, relative, true)?;
    crate::project::git_preflight::refuse_tracked_targets(
        &loaded.state_root,
        &[selected.path.as_path(), selected.lock.as_path()],
    )?;
    Ok(selected)
}

pub(super) fn read(loaded: &Loaded, relative: &str) -> Result<(Option<Record>, String), String> {
    let bytes = match path::secure_bytes_observation_bounded(
        &loaded.state_root,
        relative,
        "project continuation journal",
        LIMIT,
    ) {
        path::SecureBytesResult::Bytes(bytes) => bytes,
        path::SecureBytesResult::Absent(_) => return Ok((None, String::new())),
        _ => return Err("continuation journal is unavailable or unsafe".into()),
    };
    let source = String::from_utf8(bytes).map_err(|_| "continuation journal is not UTF-8")?;
    if !source.ends_with('\n') {
        return Err(
            "continuation journal has an incomplete last record; inspect, never retry admission"
                .into(),
        );
    }
    let mut previous: Option<Record> = None;
    for line in source.lines() {
        let record: Record =
            serde_json::from_str(line).map_err(|_| "continuation journal record is malformed")?;
        record.specification.validate()?;
        let mut value = serde_json::to_value(&record).map_err(|e| e.to_string())?;
        value
            .as_object_mut()
            .ok_or("invalid record")?
            .remove("eventSha256");
        super::control::validate_account(&record.account)
            .map_err(|reason| format!("invalid continuation account: {reason:?}"))?;
        if record.version != 2
            || record.revision != previous.as_ref().map_or(1, |p| p.revision + 1)
            || record.event_sha256 != hash::value(&value)
            || record.previous_event_sha256 != previous.as_ref().map(|p| p.event_sha256.clone())
            || record.project_identity != crate::session_goal::requirements::identity(loaded)?
            || record.claims.len() > 64
            || record.specification.mandate.id.is_empty()
            || record.account.project != record.project_identity
            || record.account.mandate_id != record.specification.mandate.id
            || chrono::DateTime::parse_from_rfc3339(&record.selected_at)
                .map_err(|_| "invalid continuation origin")?
                .timestamp()
                != record.account.started_at
        {
            return Err("continuation journal identity, bounds or chain is invalid".into());
        }
        let mut tasks = std::collections::BTreeSet::new();
        for claim in &record.claims {
            if !tasks.insert(&claim.task_id)
                || claim.task_id != claim.plan.task_id
                || !crate::work::valid_work_handle(&claim.work)
                || super::admission::plan_digest(&claim.plan)? != claim.plan_sha256
                || claim.association.project != record.project_identity
                || claim.association.mandate_id != record.specification.mandate.id
                || claim.association.task_id != claim.task_id
                || claim.association.claim_root != claim.work
            {
                return Err("continuation journal claim is invalid".into());
            }
        }
        if let Some(prior) = &previous {
            if record.specification.mandate.id != prior.specification.mandate.id
                || record.selected_at != prior.selected_at
                || !record.claims.starts_with(&prior.claims)
                || record.account.started_at != prior.account.started_at
                || record.account.revision < prior.account.revision
                || record.account.operations.len() < prior.account.operations.len()
            {
                return Err("continuation mandate lineage or spent claims changed".into());
            }
            for (old, new) in prior
                .account
                .operations
                .iter()
                .zip(&record.account.operations)
            {
                if old.key != new.key
                    || old.intent != new.intent
                    || old.association != new.association
                    || (old.outcome == super::control::Outcome::Unknown
                        && new.outcome == super::control::Outcome::Reserved)
                    || (matches!(
                        old.outcome,
                        super::control::Outcome::Succeeded | super::control::Outcome::Failed
                    ) && old.outcome != new.outcome)
                {
                    return Err("continuation spent operation changed".into());
                }
            }
        }
        previous = Some(record);
    }
    Ok((previous, source))
}

pub(super) fn append(
    selected: &ledger::LedgerPath,
    mut record: Record,
    previous: Option<&Record>,
    raw: &str,
) -> Result<Record, String> {
    record.revision = previous.map_or(1, |p| p.revision + 1);
    record.previous_event_sha256 = previous.map(|p| p.event_sha256.clone());
    let mut value = serde_json::to_value(&record).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("invalid record")?
        .remove("eventSha256");
    record.event_sha256 = hash::value(&value);
    let serialized = serde_json::to_value(&record).map_err(|e| e.to_string())?;
    let addition = serde_json::to_vec(&serialized)
        .map_err(|e| e.to_string())?
        .len()
        .checked_add(1)
        .ok_or("continuation metadata bound reached; no admission was made")?;
    if exceeds_limit(raw.len(), addition) {
        return Err("continuation metadata bound reached; no admission was made".into());
    }
    ledger::append(selected, &serialized, raw.is_empty(), raw)?;
    Ok(record)
}

#[cfg(test)]
#[path = "journal/tests.rs"]
mod tests;

impl PartialEq for Claim {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(self).ok() == serde_json::to_value(other).ok()
    }
}

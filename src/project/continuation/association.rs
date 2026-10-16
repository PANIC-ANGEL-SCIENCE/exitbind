//! Validate a frozen marker against its actual claim and canonical Work lineage.
use super::{control, journal};
use crate::{config::Loaded, evidence::hash, run::ledger};
use serde_json::Value;

pub(super) struct Bound {
    pub relative: String,
    pub record: journal::Record,
    pub claim: journal::Claim,
    pub verified: control::VerifiedAssociation,
    pub work: String,
    pub retired: bool,
}

pub(super) fn marker(value: &Value) -> Result<control::Association, String> {
    let marker: control::Association = serde_json::from_value(value.clone())
        .map_err(|_| "invalid frozen project continuation association")?;
    marker
        .validate()
        .map_err(|reason| format!("invalid association: {reason:?}"))?;
    if !crate::work::valid_work_handle(&marker.claim_root) {
        return Err("continuation claim root is not a canonical Work".into());
    }
    Ok(marker)
}

pub(super) fn claim(
    loaded: &Loaded,
    marker: &control::Association,
) -> Result<(String, journal::Record, journal::Claim), String> {
    let relative = journal::relative(loaded, &marker.mandate_id)?;
    let (record, _) = journal::read(loaded, &relative)?;
    let record = record.ok_or("frozen mandate journal is missing; inspect without adoption")?;
    let claim = record
        .claims
        .iter()
        .find(|claim| {
            claim.work == marker.claim_root
                && claim.task_id == marker.task_id
                && claim.association == *marker
        })
        .cloned()
        .ok_or("frozen association has no exact selected claim")?;
    Ok((relative, record, claim))
}

pub(super) fn for_state(loaded: &Loaded, state: &Value) -> Result<Option<Bound>, String> {
    let Some(value) = state.get("projectContinuation") else {
        return Ok(None);
    };
    let marker = marker(value)?;
    let (relative, record, claim) = claim(loaded, &marker)?;
    let mut work = marker.claim_root.clone();
    let mut seen = std::collections::BTreeSet::new();
    for index in 0..64 {
        if !seen.insert(work.clone()) {
            return Err("continuation Work lineage cycle".into());
        }
        let path = crate::work::resolve(loaded, &work)?;
        let (selected, events, raw) = ledger::load(loaded, &path)?;
        let start = events.first().ok_or("associated Work start is missing")?;
        if start.get("projectContinuation") != Some(value) {
            return Err("successor changed its frozen continuation association".into());
        }
        if index == 0 && crate::run::start_event_intent_sha256(start)? != claim.start_intent_sha256
        {
            return Err("claimed Work start intent differs from its reservation".into());
        }
        let successor = ledger::valid_claim(&ledger::claim_path(&selected))?;
        if let Some(link) = &successor {
            if link["oldLedgerPath"] != path
                || link["oldLedgerSha256"] != hash::text(&raw)
                || link["oldRunId"] != start["runId"]
                || link["oldHeadEventSha256"] != events.last().ok_or("empty Work")?["eventSha256"]
            {
                return Err("continuation predecessor retirement provenance differs".into());
            }
        }
        if start["runId"] == state["runId"] {
            let verified = control::verify_association(&marker, &claim.association, &work)
                .map_err(|reason| format!("invalid selected association: {reason:?}"))?;
            return Ok(Some(Bound {
                relative,
                record,
                claim,
                verified,
                work,
                retired: successor.is_some(),
            }));
        }
        let successor = successor.ok_or("Work is not in its claimed continuation lineage")?;
        let next = successor["newLedgerPath"]
            .as_str()
            .ok_or("successor path is missing")?;
        work = crate::work::locator_for_ledger(next).ok_or("successor is not canonical Work")?;
    }
    Err("continuation Work lineage exceeds its finite bound".into())
}

pub(super) fn ledger_work(ledger: &str) -> Result<String, String> {
    crate::work::locator_for_ledger(ledger)
        .ok_or("associated operation needs canonical Work".into())
}

pub(super) fn current_head(loaded: &Loaded, claim: &journal::Claim) -> Result<String, String> {
    let mut work = claim.work.clone();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..64 {
        if !seen.insert(work.clone()) {
            return Err("continuation successor cycle".into());
        }
        let relative = crate::work::resolve(loaded, &work)?;
        let (selected, events, _) = ledger::load(loaded, &relative)?;
        let state = crate::run::reduce_live(loaded, &events)?;
        let bound = for_state(loaded, &state)?.ok_or("claimed successor lost its association")?;
        if !bound.retired {
            return Ok(work);
        }
        let link = ledger::valid_claim(&ledger::claim_path(&selected))?
            .ok_or("successor retirement disappeared")?;
        work = crate::work::locator_for_ledger(
            link["newLedgerPath"].as_str().ok_or("missing successor")?,
        )
        .ok_or("invalid successor Work")?;
    }
    Err("continuation successor bound reached".into())
}

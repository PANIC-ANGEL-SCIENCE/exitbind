//! Explicit imported facts and separate repair requests; never execute providers.
use super::{consume, store};
use crate::{config::Loaded, evidence::hash};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observation {
    id: String,
    classification: String,
    source_sha256: String,
    provider: String,
    run_id: String,
    status: String,
    conclusion: Option<String>,
    evidence: String,
    reason: String,
}
pub(super) fn record(loaded: &Loaded, id: &str, bytes: &[u8]) -> Result<serde_json::Value, String> {
    if bytes.len() > 8192 {
        return Err("shipping observation exceeds 8 KiB".into());
    }
    let observation: Observation = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    store::name(&observation.id)?;
    if !matches!(
        observation.classification.as_str(),
        "provider" | "infrastructure" | "product-defect"
    ) || !matches!(
        observation.status.as_str(),
        "queued" | "in_progress" | "completed" | "unknown"
    ) || (observation.status != "completed" && observation.conclusion.is_some())
        || !matches!(
            observation.conclusion.as_deref(),
            None | Some(
                "success"
                    | "failure"
                    | "cancelled"
                    | "timed_out"
                    | "skipped"
                    | "neutral"
                    | "action_required"
                    | "stale"
            )
        )
        || [
            &observation.provider,
            &observation.run_id,
            &observation.evidence,
            &observation.reason,
        ]
        .iter()
        .any(|s| s.is_empty() || s.len() > 1024)
    {
        return Err("invalid provider identity/status or causal classification".into());
    }
    let packet = store::packet(loaded, id)?;
    if observation.source_sha256 != hash::value(&packet["source"]) {
        return Err("provider observation belongs to another accepted source".into());
    }
    let work = packet["work"].as_str().ok_or("packet Work missing")?;
    let ledger = crate::work::resolve(loaded, work)?;
    let lock = crate::run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    crate::run::ledger::with_lock(&lock, || {
        let fact = json!({"acquisition":"imported_caller_reported","packet":id,"observation":observation,
            "authority":"none","acceptanceCredit":false});
        let mut selected = None;
        for n in 1..=16 {
            let leaf = format!("observation-{n}");
            match store::get(loaded, id, &leaf)? {
                Some(prior) if prior["observation"]["id"] == observation.id => {
                    if prior != fact {
                        return Err("observation identity collision".into());
                    }
                    selected = Some(leaf);
                    break;
                }
                None => {
                    selected = Some(leaf);
                    break;
                }
                _ => {}
            }
        }
        if observation.classification == "product-defect" {
            let source_id = hash::value(&packet["source"]);
            let request = json!({"id":format!("repair-source-{source_id}"),"packet":id,"source":packet["source"],
                "firstObservation":observation.id,"evidence":observation.evidence,"reason":observation.reason,
                "state":"requested_not_authorized","sourceWorkUnchanged":true});
            // Commit the source-wide hold first. Losing any later packet record
            // or response cannot authorize another destination or artifact.
            if store::get(loaded, &source_id, "source-defect")?.is_none() {
                store::put(loaded, &source_id, "source-defect", &request)?;
            }
        }
        store::put(
            loaded,
            id,
            &selected.ok_or("shipping observation limit reached")?,
            &fact,
        )?;
        if observation.classification == "product-defect" {
            let request = json!({"id":format!("repair-{id}"),"packet":id,"source":packet["source"],
                "firstObservation":observation.id,"evidence":observation.evidence,"reason":observation.reason,
                "state":"requested_not_authorized","sourceWorkUnchanged":true});
            // Preserve the first hold; subsequent evidence cannot clear it.
            if store::get(loaded, id, "repair")?.is_none() {
                store::put(loaded, id, "repair", &request)?;
            }
        }
        consume::status(loaded, id)
    })
}

pub(super) fn source_hold(
    loaded: &Loaded,
    packet: &serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    if let Some(hold) = store::get(loaded, &hash::value(&packet["source"]), "source-defect")? {
        return Ok(Some(hold));
    }
    // Initial development packets have only packet-local defect facts. Read
    // those too, including when the source-wide record was lost. Never infer
    // clearance from choosing a new packet identity.
    for id in store::identities(loaded)? {
        if store::read(loaded, &id, "packet")?.is_none() {
            continue;
        }
        let prior = store::packet(loaded, &id)?;
        if prior["source"] != packet["source"] {
            continue;
        }
        if let Some(repair) = store::get(loaded, &id, "repair")? {
            return Ok(Some(repair));
        }
        if facts(loaded, &id)?.1 {
            return Ok(Some(
                json!({"id":format!("repair-{id}"),"recordPending":true}),
            ));
        }
    }
    Ok(None)
}

/// A committed defect fact is itself a hold, even if the repair reply/storage was lost.
pub(super) fn facts(loaded: &Loaded, id: &str) -> Result<(Vec<serde_json::Value>, bool), String> {
    let mut facts = Vec::new();
    let mut defect = false;
    for n in 1..=16 {
        if let Some(fact) = store::get(loaded, id, &format!("observation-{n}"))? {
            defect |= fact["observation"]["classification"] == "product-defect";
            facts.push(json!({"reference":format!("observation-{n}"),"sha256":hash::value(&fact),
                "status":fact["observation"]["status"],"conclusion":fact["observation"]["conclusion"],
                "acquisition":"imported_caller_reported","acceptanceCredit":false}));
        }
    }
    Ok((facts, defect))
}

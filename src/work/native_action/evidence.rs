//! Resolve complete packet-bound worker evidence for a reviewer.

use super::*;

pub(super) struct ReviewEvidence {
    pub(super) reference: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
    pub(super) content: String,
}

pub(super) fn verified_worker_results(
    loaded: &Loaded,
    work: &str,
    role: &str,
    assignment: &Value,
) -> Result<Option<Vec<ReviewEvidence>>, String> {
    if role != "reviewer" {
        return Ok(None);
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_path, events, _source) = crate::run::ledger::load(loaded, &ledger)?;
    let packet_evidence = assignment["context"]["evidence"]
        .as_array()
        .ok_or("reviewer packet has no evidence list")?;
    let worker_events = current_worker_events(&events, assignment);
    if worker_events.is_empty() || worker_events.len() > 8 {
        return Err("reviewer needs one to eight current worker results".into());
    }
    let mut verified = Vec::with_capacity(worker_events.len());
    for event in worker_events {
        let submission = packet_evidence
            .iter()
            .find(|item| {
                item["kind"] == "submission"
                    && item["rawEventRef"]["selector"] == event["eventSha256"]
                    && item["artifact"]["sha256"] == event["artifact"]["sha256"]
            })
            .ok_or("current worker result has no packet-bound submission reference")?;
        let artifact = crate::run::artifact::read(loaded, &event["artifact"], "worker result")?;
        if artifact.bytes > MAX_RESULT_BYTES as u64
            || artifact.preview.len() != artifact.bytes as usize
        {
            return Err("current worker result exceeds the bounded reviewer evidence route".into());
        }
        verified.push(ReviewEvidence {
            reference: submission["rawEventRef"]["id"]
                .as_str()
                .ok_or("current worker result has no packet-bound evidence reference")?
                .to_owned(),
            sha256: artifact.sha256,
            bytes: artifact.bytes,
            content: String::from_utf8(artifact.preview)
                .map_err(|_| "current worker result is not UTF-8 JSON".to_owned())?,
        });
    }
    Ok(Some(verified))
}

pub(super) fn current_worker_events<'a>(events: &'a [Value], assignment: &Value) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
                && event["attempt"] == assignment["attempt"]
                && event["stage"]
                    .as_u64()
                    .is_some_and(|stage| stage < assignment["stage"].as_u64().unwrap_or(0))
        })
        .collect()
}

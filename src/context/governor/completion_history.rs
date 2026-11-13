// Orders retained completion history in a validated governor snapshot.
// Requires carried lineage for the relation; see `src/context/governor.rs`.
//! Conservative ordering of completion history in an already validated
//! canonical governor snapshot. This relation supplies no lineage or grant proof.
use serde_json::Value;

struct CompletionPosition<'a> {
    run: &'a str,
    attempt: u64,
    checkpoint: u64,
}

impl<'a> CompletionPosition<'a> {
    fn read(identity: &'a Value) -> Option<Self> {
        if identity["unit"] != "worker" || identity["operation"] != "completed_submission" {
            return None;
        }
        Some(Self {
            run: identity["runId"].as_str()?,
            attempt: identity["attempt"].as_u64().filter(|value| *value > 0)?,
            checkpoint: identity["checkpoint"].as_u64().filter(|value| *value > 0)?,
        })
    }
}

/// A carried completion may retain older accounting digests after its former
/// artifact leaves the current reviewer evidence. Both orders are required:
/// grant acknowledgements can share a checkpoint, so neither order alone is
/// chronology. The caller still validates full identities and current evidence.
pub(crate) fn retained_completion_precedes(run: &str, current: &Value, previous: &Value) -> bool {
    if current["carryLineage"] != true {
        return false;
    }
    let (Some(current), Some(previous)) = (
        CompletionPosition::read(current),
        CompletionPosition::read(previous),
    ) else {
        return false;
    };
    current.run == run
        && previous.run == run
        && previous.attempt < current.attempt
        && previous.checkpoint < current.checkpoint
}

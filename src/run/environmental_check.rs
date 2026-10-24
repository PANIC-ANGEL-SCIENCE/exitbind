//! The Lead's environmental attestation of a failed observed check.
//!
//! A frozen check can fail for a reason outside the result: a lock held by
//! another process, a removed build artifact. The Lead names such a failure
//! with the `environmental` disposition and a reason. The failed check stays
//! in the ledger and in the governor's observations, so the repeated-failure
//! limit counts it exactly as before; only the check-target view stops
//! treating it as the target's latest result, which makes the same target's
//! check pending again. A second attestation needs a new failed observation:
//! the attested check is no longer the target's latest result, so it can never
//! be named twice.

use crate::kernel::disposition::Disposition;
use serde_json::Value;
use std::collections::BTreeSet;

/// The disposition decision this module owns.
pub(crate) const DECISION: &str = "environmental";

/// Check events the Lead attested environmental in this run's state.
pub(crate) fn attested(state: &Value) -> BTreeSet<String> {
    named(state["dispositions"].as_array().into_iter().flatten())
}

/// The same set read from raw ledger events, for views built from events.
pub(crate) fn attested_in_events(events: &[Value]) -> BTreeSet<String> {
    named(
        events
            .iter()
            .filter(|event| {
                event["action"] == "submit"
                    && event["role"] == "lead"
                    && event["outcome"] == "disposition"
            })
            .map(|event| &event["disposition"]),
    )
}

/// Whether `events` hold a check of `target` (its target event and
/// requirement) that the Lead attested environmental (`attested`), or one
/// that is still a plain recorded result (`!attested`).
pub(crate) fn target_has_check(events: &[Value], target: &Value, attested: bool) -> bool {
    let named = attested_in_events(events);
    events.iter().any(|event| {
        event["action"] == "check"
            && event["targetEventSha256"] == target["targetEventSha256"]
            && event["requirementId"] == target["requirementId"]
            && event["eventSha256"]
                .as_str()
                .is_some_and(|sha| named.contains(sha))
                == attested
    })
}

fn named<'a>(records: impl Iterator<Item = &'a Value>) -> BTreeSet<String> {
    records
        .filter(|record| record["decision"] == DECISION)
        .flat_map(|record| record["findingSha256s"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// A failed observation whose checker process never started. The failure is
/// still a pending Lead finding; it is the same class as a failed observed
/// check for the purpose of this attestation, because the check's own result
/// exists neither way and the target stays pending.
pub(crate) fn never_ran(state: &Value) -> bool {
    state["checkObservation"]["state"] == "failed"
        && state["checkObservation"]["facts"]["processStarted"] == false
        && state["checkObservation"]["facts"]["termination"] == "ended"
        && state["pendingDisposition"]["kind"] == "check_observation_failed"
}

/// The failed observed checks that are the latest result of a current check
/// target, in target order: exactly what one attestation names. For a
/// never-ran observation the named finding is its own failure event.
pub(crate) fn current_findings(state: &Value) -> Result<Vec<String>, String> {
    if never_ran(state) {
        let finding = state["pendingDisposition"]["findingSha256s"]
            .as_array()
            .filter(|items| items.len() == 1)
            .and_then(|items| items[0].as_str())
            .ok_or("never-ran check observation has no single failure finding")?;
        return Ok(vec![finding.to_owned()]);
    }
    Ok(crate::run::gate::exit::assess(state)?
        .targets
        .iter()
        .filter(|target| target.is_failed() && target.acquisition.as_deref() == Some("observed"))
        .filter_map(|target| target.check_event_sha256.clone())
        .collect())
}

/// Admit an environmental attestation only for the exact current failed
/// observed checks, with no other Lead decision or observation pending.
pub(crate) fn validate(state: &Value, disposition: &Disposition) -> Result<(), String> {
    // The one pending finding this decision owns is a never-ran observation;
    // anything else pending, and any observation not known to have ended
    // before the checker started, still needs its own disposition first.
    let never_ran = never_ran(state);
    if state["pendingDisposition"].is_object() && !never_ran {
        return Err("a pending finding needs its own Lead disposition first".into());
    }
    if state["checkObservation"].is_object() && !never_ran {
        return Err("a check observation is still pending; inspect it first".into());
    }
    if disposition.basis_sha256.as_deref() != state["basis"]["sha256"].as_str() {
        return Err("environmental attestation does not match the current basis".into());
    }
    let findings = current_findings(state)?;
    if findings.is_empty() {
        return Err("no failed observed check is current; an environmental attestation needs a new failed observation".into());
    }
    if disposition.finding_sha256s != findings {
        return Err(
            "environmental attestation must name exactly the current failed observed checks".into(),
        );
    }
    Ok(())
}

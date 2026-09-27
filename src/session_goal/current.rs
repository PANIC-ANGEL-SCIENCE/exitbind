//! Read-only applicability of a recorded whole-goal closure to current inputs.
//! Historical closure remains in the ledger even when this projection is stale.

use super::{continuity, current_inputs, direct_completion_inputs, presentation, CATEGORIES};
use crate::config::Loaded;
use serde_json::{json, Value};

pub(super) enum Readiness {
    Current,
    Stale(&'static str),
    Unknown(&'static str),
}

impl Readiness {
    pub(super) fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }

    pub(super) fn value(&self) -> Value {
        match self {
            Self::Current => json!({"state":"current"}),
            Self::Stale(reason) => json!({"state":"stale","reason":reason}),
            Self::Unknown(reason) => json!({"state":"unknown","reason":reason}),
        }
    }
}

/// A failure to inspect optional display applicability suppresses the room;
/// it never edits the recorded closure or authorizes another effect.
pub(super) fn evaluate(loaded: &Loaded, record: Option<&Value>) -> Readiness {
    let Some(record) = record else {
        return Readiness::Stale("no_named_goal");
    };
    if presentation(Some(record))["explicitLeadClosure"] != true {
        return Readiness::Stale("whole_goal_not_closed");
    }
    let Some(closure_refs) = record["closure"]["resultRefs"].as_array() else {
        return Readiness::Unknown("closure_references_unavailable");
    };
    if record["closure"]["kind"] == "direct" {
        let Some(recorded) = record["closure"]["inputsSha256"].as_str() else {
            return Readiness::Unknown("direct_closure_inputs_unavailable");
        };
        match current_inputs(loaded) {
            Ok(current) if current == recorded => {}
            Ok(_) => return Readiness::Stale("direct_closure_inputs_changed"),
            Err(_) => return Readiness::Unknown("direct_closure_inputs_unavailable"),
        }
    } else if closure_refs.is_empty() {
        return Readiness::Stale("closure_result_missing");
    }
    for reference in closure_refs {
        let Some(reference) = reference.as_str() else {
            return Readiness::Unknown("closure_reference_invalid");
        };
        if let Some(result) = reference_current(loaded, reference) {
            return result;
        }
    }
    for category in CATEGORIES {
        let Some(items) = record[category].as_array() else {
            return Readiness::Unknown("goal_category_unavailable");
        };
        for item in items {
            if item["disposition"] == "direct" {
                match direct_completion_inputs(loaded, record, item) {
                    Ok(Some((recorded, current))) if recorded == current => {}
                    Ok(Some(_)) => return Readiness::Stale("direct_item_inputs_changed"),
                    Ok(None) => return Readiness::Unknown("direct_item_invalid"),
                    Err(_) => return Readiness::Unknown("direct_item_inputs_unavailable"),
                }
                continue;
            }
            let Some(refs) = item["resultRefs"].as_array() else {
                return Readiness::Unknown("goal_item_references_unavailable");
            };
            if refs.is_empty() {
                return Readiness::Stale("goal_item_result_missing");
            }
            for reference in refs {
                let Some(reference) = reference.as_str() else {
                    return Readiness::Unknown("goal_item_reference_invalid");
                };
                if let Some(result) = reference_current(loaded, reference) {
                    return result;
                }
            }
        }
    }
    match continuity::support_current(loaded, record) {
        Ok(true) => Readiness::Current,
        Ok(false) => Readiness::Stale("continuation_support_changed"),
        Err(_) => Readiness::Unknown("continuation_support_unavailable"),
    }
}

/// Returns a non-current finding, or None for a currently valid result.
fn reference_current(loaded: &Loaded, reference: &str) -> Option<Readiness> {
    let evidence = match crate::run::result_ref_evidence(loaded, reference) {
        Ok(Some(evidence)) => evidence,
        Ok(None) => return Some(Readiness::Stale("governed_result_missing")),
        Err(_) => return Some(Readiness::Unknown("governed_result_unavailable")),
    };
    if !evidence.accepted || !evidence.artifact_current || evidence.drift.is_some() {
        return Some(Readiness::Stale("governed_result_changed"));
    }
    match evidence.inputs_current {
        Some(true) => None,
        Some(false) => Some(Readiness::Stale("governed_inputs_changed")),
        None => Some(Readiness::Unknown("governed_inputs_unverifiable")),
    }
}

//! Conversational presentation derived from canonical run state.
//!
//! The lead speaks from Exitbind's state model instead of inventing narration:
//! this module classifies the state Exitbind already knows, names the
//! transition between the previously presented classification and the current
//! one, and hands back a short language-independent key plus a terse phrase.
//!
//! Only a transition speaks. Reading the same state again says nothing new, so
//! the line stays sparse. Nothing here calls a model, and no phrase claims a
//! fact the kernel did not establish.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Derived, replaceable memory of what was last presented for one work item.
/// This is a cache beside the run state, never a second ledger.
fn cache_path(state_root: &Path, work: &str) -> PathBuf {
    state_root
        .join(crate::project_layout::state_namespace())
        .join("presentation")
        .join(format!("{work}.json"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Classification {
    exit_state: String,
    next: String,
    check: &'static str,
    review: &'static str,
    preservation: &'static str,
    owner_decision: String,
    head: String,
    inputs: String,
}

impl Classification {
    /// The signature decides whether anything worth speaking about changed.
    fn signature(&self) -> String {
        crate::hash::value(&json!([
            self.exit_state,
            self.next,
            self.check,
            self.review,
            self.preservation,
            self.owner_decision,
            self.head,
            self.inputs,
        ]))
    }

    fn value(&self) -> Value {
        json!({
            "exitState": self.exit_state,
            "next": self.next,
            "check": self.check,
            "review": self.review,
            "preservation": self.preservation,
            "ownerDecision": self.owner_decision,
        })
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            exit_state: value["exitState"].as_str()?.to_owned(),
            next: value["next"].as_str()?.to_owned(),
            check: known(value["check"].as_str()?)?,
            review: known(value["review"].as_str()?)?,
            preservation: known(value["preservation"].as_str()?)?,
            owner_decision: value["ownerDecision"].as_str()?.to_owned(),
            head: String::new(),
            inputs: String::new(),
        })
    }
}

fn known(value: &str) -> Option<&'static str> {
    [
        "none",
        "missing",
        "stale",
        "current",
        "failed",
        "approved",
        "active",
        "satisfied",
    ]
    .into_iter()
    .find(|candidate| *candidate == value)
}

fn classify(packet: &Value, progress: &Value) -> Classification {
    let targets = packet["stillValid"].as_array().cloned().unwrap_or_default();
    let remaining = packet["remaining"].as_array().cloned().unwrap_or_default();
    let obligation = |name: &str| {
        remaining
            .iter()
            .any(|item| item["obligation"].as_str() == Some(name))
    };
    let valid = |evidence: &str| {
        targets
            .iter()
            .any(|item| item["evidence"].as_str() == Some(evidence))
    };
    let check = if obligation("rework_after_failed_check") {
        "failed"
    } else if valid("current_check") {
        "current"
    } else if obligation("check") {
        // An earlier check that no longer applies reads as missing work with a
        // recorded past: the packet's help distinguishes the two.
        if packet["humanHelp"]["whatHappened"]
            .as_str()
            .is_some_and(|text| text.contains("no longer applies"))
        {
            "stale"
        } else {
            "missing"
        }
    } else {
        "none"
    };
    let review = if obligation("review") {
        "missing"
    } else if valid("current_review") {
        "approved"
    } else if packet["next"].as_str() == Some("lead_decision")
        && packet["humanHelp"]["whatHappened"]
            .as_str()
            .is_some_and(|text| text.contains("given on different tested files"))
    {
        "stale"
    } else {
        "none"
    };
    let preservation = if obligation("rework_after_failed_preservation") {
        "failed"
    } else if valid("preservation") {
        "satisfied"
    } else if remaining
        .iter()
        .any(|item| item["obligation"].as_str() == Some("preservation"))
    {
        "active"
    } else {
        "none"
    };
    Classification {
        exit_state: progress["state"].as_str().unwrap_or("UNKNOWN").to_owned(),
        next: packet["next"].as_str().unwrap_or("none").to_owned(),
        check,
        review,
        preservation,
        owner_decision: packet["humanHelp"]["ownerDecision"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
        head: packet["snapshot"]["headEventSha256"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        inputs: packet["snapshot"]["inputsSha256"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    }
}

/// The transition worth speaking about, if any. Terminal readiness is excluded:
/// `EXIT READY` speaks for itself and takes no flavor line.
fn transition(previous: Option<&Classification>, current: &Classification) -> Option<&'static str> {
    let previous = previous?;
    if current.exit_state == "READY" {
        return None;
    }
    // Entering a state speaks once; staying in it stays quiet.
    if previous.review != "stale" && current.review == "stale" {
        return Some("review_became_stale");
    }
    if previous.check != "stale" && current.check == "stale" {
        return Some("check_became_stale");
    }
    if previous.preservation != "active" && current.preservation == "active" {
        return Some("preservation_became_active");
    }
    if previous.check != "failed" && current.check == "failed" {
        return Some("check_failed");
    }
    if (previous.check != "current" && current.check == "current")
        || (previous.review != "approved" && current.review == "approved")
    {
        return Some("evidence_became_current");
    }
    if previous.owner_decision != "required" && current.owner_decision == "required" {
        return Some("owner_boundary_reached");
    }
    None
}

/// Product-owned wording: terse, dry, and derived from the transition above.
fn phrase(key: &str) -> &'static str {
    match key {
        "check_became_stale" => "That key fit the earlier result.",
        "review_became_stale" => "The review is one step behind.",
        "preservation_became_active" => "This part has to stay as it is.",
        "check_failed" => "The check came back against it.",
        "evidence_became_current" => "It fits now.",
        "owner_boundary_reached" => "This one is not mine to decide.",
        "resumed_with_valid_evidence" => "That door is already open.",
        _ => "",
    }
}

/// The Holytail line appears only while a preservation obligation is genuinely
/// in play. Exitbind carries no quality or route axis, so those stay unbound
/// rather than invented; the evidence axis is the acquisition Exitbind recorded.
fn holytail_line(packet: &Value, classification: &Classification) -> Option<String> {
    if !matches!(
        classification.preservation,
        "active" | "failed" | "satisfied"
    ) {
        return None;
    }
    let evidence = packet["humanHelp"]["preservationEvidence"]
        .as_str()
        .unwrap_or("agent_declared");
    Some(format!("Holytail :MODE-UNBOUND · evidence={evidence}"))
}

/// Build the presentation block for one projected packet, remembering what was
/// presented so an unchanged state does not speak twice.
pub(crate) fn project(
    state_root: &Path,
    work: &str,
    packet: &Value,
    progress: &Value,
    resumed: bool,
) -> Value {
    let current = classify(packet, progress);
    let path = cache_path(state_root, work);
    let stored = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let unchanged = stored
        .as_ref()
        .and_then(|value| value["signature"].as_str())
        .is_some_and(|signature| signature == current.signature());
    let previous = stored
        .as_ref()
        .map(|value| &value["classification"])
        .and_then(Classification::from_value);

    let key = if unchanged {
        None
    } else if resumed && current.check == "current" && previous.is_none() {
        Some("resumed_with_valid_evidence")
    } else {
        transition(previous.as_ref(), &current)
    };

    remember(&path, &current, state_root);

    let applicable = progress["applicable"] == Value::Bool(true);
    let neuro = progress["percent"]
        .as_u64()
        .filter(|_| applicable)
        .map(|percent| format!("[Neuro] Exitbind progress: {percent}%."));
    json!({
        "progress": if applicable { progress["percent"].clone() } else { Value::Null },
        "exitState": current.exit_state,
        "next": current.next,
        "state": current.value(),
        "transition": key,
        "phraseKey": key,
        "phrase": key.map(phrase),
        "neuro": neuro,
        "holytail": holytail_line(packet, &current),
    })
}

fn remember(path: &Path, current: &Classification, state_root: &Path) {
    let Some(directory) = path.parent() else {
        return;
    };
    if crate::managed_files::ensure_managed_directory(state_root, directory).is_err() {
        return;
    }
    let document = json!({
        "signature": current.signature(),
        "classification": current.value(),
    });
    let _ = std::fs::write(path, format!("{document}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classification(check: &'static str, review: &'static str) -> Classification {
        Classification {
            exit_state: "BLOCKED".to_owned(),
            next: "check".to_owned(),
            check,
            review,
            preservation: "none",
            owner_decision: "not_required".to_owned(),
            head: "head".to_owned(),
            inputs: "inputs".to_owned(),
        }
    }

    #[test]
    fn only_meaningful_transitions_speak() {
        let current = classification("current", "approved");
        assert_eq!(transition(None, &current), None);
        assert_eq!(transition(Some(&current), &current), None);
        assert_eq!(
            transition(Some(&current), &classification("stale", "approved")),
            Some("check_became_stale")
        );
        assert_eq!(
            transition(Some(&current), &classification("current", "stale")),
            Some("review_became_stale")
        );
        assert_eq!(
            transition(
                Some(&classification("stale", "approved")),
                &classification("current", "approved")
            ),
            Some("evidence_became_current")
        );
    }

    #[test]
    fn terminal_readiness_takes_no_flavor_line() {
        let mut ready = classification("current", "approved");
        ready.exit_state = "READY".to_owned();
        assert_eq!(
            transition(Some(&classification("stale", "approved")), &ready),
            None
        );
        assert!(phrase("exit_ready").is_empty());
    }
}

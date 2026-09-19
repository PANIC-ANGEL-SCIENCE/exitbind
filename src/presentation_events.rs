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

/// A cache holds at most one small document; anything larger is not ours.
const CACHE_LIMIT: u64 = 64 * 1024;

/// Derived, replaceable memory of what was last presented for one work item.
/// This is a cache beside the run state, never a second ledger: losing it can
/// only repeat an optional line, and nothing read from it becomes evidence or
/// instruction.
fn cache_relative(work: &str) -> String {
    format!(
        "{}/presentation/{work}.json",
        crate::project_layout::state_namespace()
    )
}

fn cache_path(state_root: &Path, work: &str) -> PathBuf {
    state_root.join(cache_relative(work))
}

/// Read the remembered presentation as raw text plus its parsed document.
///
/// The read follows no symlink at any component, refuses anything that is not
/// a regular file, and rejects a file that changes underneath it. An oversized,
/// unsafe, corrupt, or absent cache simply reads as nothing.
fn remembered(state_root: &Path, work: &str) -> Option<(String, Value)> {
    let path = cache_path(state_root, work);
    if std::fs::symlink_metadata(path.as_path()).ok()?.len() > CACHE_LIMIT {
        return None;
    }
    let bytes = match crate::project_path::secure_bytes_observation(
        state_root,
        &cache_relative(work),
        "presentation cache",
    ) {
        crate::project_path::SecureBytesResult::Bytes(bytes) => bytes,
        _ => return None,
    };
    let text = String::from_utf8(bytes).ok()?;
    let document = serde_json::from_str::<Value>(&text).ok()?;
    if document["kind"] != "derived_display_memo" {
        // Written by something else, or by a version that meant something
        // else by this file. Read nothing from it and replace nothing in it.
        return None;
    }
    Some((text, document))
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

/// Classify from the canonical facts the work layer already derived. The
/// packet supplies only structured display metadata - the owner decision and
/// the preservation provenance - never prose.
fn classify(packet: &Value, progress: &Value, facts: &Value) -> Classification {
    let state = |name: &str| known(facts[name].as_str().unwrap_or("none")).unwrap_or("none");
    Classification {
        exit_state: progress["state"].as_str().unwrap_or("UNKNOWN").to_owned(),
        next: packet["next"].as_str().unwrap_or("none").to_owned(),
        check: state("check"),
        review: state("review"),
        preservation: state("preservation"),
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
    if crate::run_exit::is_ready(&current.exit_state) {
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
/// in play.
///
/// Route and quality are two axes and neither is inferred from the other, from
/// a model name, or from a host effort label: the run's accepted preservation
/// requirements resolve them through the recorded assignment, and an assignment
/// Exitbind cannot resolve stays `MODE-UNBOUND`. The evidence axis reports how
/// the preservation evidence was acquired, never that it was independently
/// verified.
fn holytail_line(packet: &Value, classification: &Classification) -> Option<String> {
    if !matches!(
        classification.preservation,
        "active" | "failed" | "satisfied"
    ) {
        return None;
    }
    let assignment = &packet["humanHelp"]["preservationAssignment"];
    let quality = assignment["quality"].as_str().unwrap_or("MODE-UNBOUND");
    let route = assignment["route"].as_str().unwrap_or("FORMAL");
    // Before any requirement check runs there is nothing to label: saying so
    // beats borrowing a stronger word.
    let evidence = packet["humanHelp"]["preservationEvidence"]
        .as_str()
        .unwrap_or("none");
    Some(format!(
        "Holytail :{quality} · {route} · evidence={evidence}"
    ))
}

/// Build the presentation block for one projected packet, remembering what was
/// presented so an unchanged state does not speak twice.
pub(crate) fn project(
    state_root: &Path,
    work: &str,
    packet: &Value,
    progress: &Value,
    facts: &Value,
    resumed: bool,
) -> Value {
    let current = classify(packet, progress, facts);
    let remembered = remembered(state_root, work);
    let stored = remembered.as_ref().map(|(_, document)| document);
    let unchanged = stored
        .and_then(|value| value["signature"].as_str())
        .is_some_and(|signature| signature == current.signature());
    let previous = stored
        .map(|value| &value["classification"])
        .and_then(Classification::from_value);

    let key = if unchanged {
        None
    } else if resumed && current.check == "current" && previous.is_none() {
        Some("resumed_with_valid_evidence")
    } else {
        transition(previous.as_ref(), &current)
    };

    remember(
        state_root,
        work,
        &current,
        remembered.as_ref().map(|(text, _)| text.as_str()),
    );

    let applicable = progress["applicable"] == Value::Bool(true);
    let neuro = progress["percent"]
        .as_u64()
        .filter(|_| applicable)
        .map(|percent| format!("[Neuro] Exitbind progress: {percent}%."));
    // The host copies the product-owned terminal block verbatim. Keeping it
    // separate from machine state prevents host-generated suffixes, and it is
    // offered only while the acceptance still describes the files present now:
    // history must not speak for a tree that changed under it.
    let terminal = (crate::run_exit::is_ready(&current.exit_state)
        && facts["acceptance"] == "current")
        .then_some("EXIT READY");
    json!({
        "progress": if applicable { progress["percent"].clone() } else { Value::Null },
        "exitState": current.exit_state,
        "terminal": terminal,
        "next": current.next,
        "state": current.value(),
        "transition": key,
        "phraseKey": key,
        "phrase": key.map(phrase),
        "neuro": neuro,
        "holytail": holytail_line(packet, &current),
    })
}

/// Replace the cache through the hardened writer, and only when it still holds
/// exactly what this read started from. A refusal - an unsafe path, a file
/// something else replaced, an unwritable state root - is dropped: the run
/// state is authoritative and unaffected either way.
fn remember(state_root: &Path, work: &str, current: &Classification, previous: Option<&str>) {
    let path = cache_path(state_root, work);
    let Some(directory) = path.parent() else {
        return;
    };
    if crate::managed_files::ensure_managed_directory(state_root, directory).is_err() {
        return;
    }
    // The file says what it is. A reader that finds it while browsing state
    // must not mistake a replaceable display memo for the run record, and a
    // live host observed doing exactly that is why this field exists.
    let document = json!({
        "kind": "derived_display_memo",
        "authority": "none",
        "describes": "what was last shown for this work, to avoid repeating it",
        "signature": current.signature(),
        "classification": current.value(),
    });
    let _ = crate::hook_settings::atomic_write(
        &path,
        &format!("{document}\n"),
        Some(0o600),
        previous,
        state_root,
    );
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
    fn wording_changes_cannot_move_a_classification() {
        let facts = json!({"check": "stale", "review": "approved", "preservation": "active"});
        let packet = |what: &str| {
            json!({
                "next": "check",
                "humanHelp": {"whatHappened": what, "ownerDecision": "not_required"},
                "snapshot": {"headEventSha256": "head", "inputsSha256": "inputs"},
            })
        };
        let progress = json!({"state": "BLOCKED", "percent": 40, "applicable": true});
        let english = classify(
            &packet("Earlier evidence no longer applies."),
            &progress,
            &facts,
        );
        let reworded = classify(
            &packet("이전 증거는 더 이상 적용되지 않습니다."),
            &progress,
            &facts,
        );
        let silent = classify(&packet(""), &progress, &facts);
        assert_eq!(english, reworded);
        assert_eq!(english, silent);
        assert_eq!(english.check, "stale");
        assert_eq!(english.preservation, "active");
        assert_eq!(english.signature(), reworded.signature());
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
        ready.exit_state = crate::run_exit::ExitDecision::Ready.wire().0.to_owned();
        assert_eq!(
            transition(Some(&classification("stale", "approved")), &ready),
            None
        );
        assert!(phrase("exit_ready").is_empty());
    }
}

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

#[allow(dead_code)]
pub(crate) const SESSION_GOAL_CARD: &str =
    "+------------------------------+\n| Nothing remains here.        |\n+------------------------------+\n\nEXIT READY";

/// Render the explicit whole-session closure card from lead-owned facts. A
/// ready run alone is insufficient: every requested category must be empty and
/// closure must be explicitly recorded by the lead. This function is pure so
/// interactive renderers can keep it out of structured protocol output.
#[allow(dead_code)]
pub(crate) fn session_goal_card(
    packet: &Value,
    progress: &Value,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let goal = &packet["humanHelp"]["sessionGoal"];
    let empty = |key: &str| goal[key].as_array().is_some_and(|items| items.is_empty());
    if !interactive
        || closed_stdin
        || !crate::run_exit::is_ready(progress["state"].as_str().unwrap_or(""))
        || goal["explicitLeadClosure"] != true
        || ![
            "subgoals",
            "findings",
            "blockers",
            "decisions",
            "externalActions",
        ]
        .iter()
        .all(|key| empty(key))
    {
        return None;
    }
    Some(SESSION_GOAL_CARD)
}

/// Once-only transition helper for a renderer's replaceable memo. A new
/// request identity deliberately prevents reuse of an earlier closure.
#[allow(dead_code)]
pub(crate) fn session_goal_card_transition(
    previous: Option<&Value>,
    packet: &Value,
    progress: &Value,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let card = session_goal_card(packet, progress, interactive, closed_stdin)?;
    let request = packet["humanHelp"]["sessionGoal"]["requestId"].as_str()?;
    if previous.is_some_and(|value| {
        value["requestId"].as_str() == Some(request) && value["cardEmitted"] == true
    }) {
        None
    } else {
        Some(card)
    }
}

/// Render a direct-only closure card from the validated session-goal
/// projection. Direct completion has no run progress, so this path never
/// fabricates a READY state or a governed result.
pub(crate) fn session_goal_direct_card(
    packet: &Value,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let goal = &packet["humanHelp"]["sessionGoal"];
    let empty = |key: &str| goal[key].as_array().is_some_and(|items| items.is_empty());
    if !interactive
        || closed_stdin
        || goal["completionMode"] != "direct"
        || goal["explicitLeadClosure"] != true
        || ![
            "subgoals",
            "findings",
            "blockers",
            "decisions",
            "externalActions",
        ]
        .iter()
        .all(|key| empty(key))
    {
        return None;
    }
    Some(SESSION_GOAL_CARD)
}

/// Once-only transition helper for a direct closure card. The same display
/// memo as governed cards prevents repeat output without becoming authority.
pub(crate) fn session_goal_direct_card_transition(
    previous: Option<&Value>,
    packet: &Value,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let card = session_goal_direct_card(packet, interactive, closed_stdin)?;
    let request = packet["humanHelp"]["sessionGoal"]["requestId"].as_str()?;
    if previous.is_some_and(|value| {
        value["requestId"].as_str() == Some(request) && value["cardEmitted"] == true
    }) {
        None
    } else {
        Some(card)
    }
}

/// Human-only bridge for the lead-owned session closure card. The memo is a
/// replaceable display cache: loss or corruption can repeat an optional card,
/// but never changes canonical state, acceptance, or evidence.
pub(crate) fn session_goal_card_for_human(
    state_root: &Path,
    session_goal: &Value,
    progress: &Value,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let packet = json!({"humanHelp": {"sessionGoal": session_goal}});
    let previous = remembered_session_card(state_root);
    let card = session_goal_card_transition(
        previous.as_ref(),
        &packet,
        progress,
        interactive,
        closed_stdin,
    )?;
    remember_session_card(state_root, session_goal["requestId"].as_str()?);
    Some(card)
}

/// Human-only bridge for a validated direct session-goal closure. It shares
/// the governed card memo and remains silent for headless or machine output.
pub(crate) fn session_goal_direct_card_for_human(
    state_root: &Path,
    session_goal: &Value,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let packet = json!({"humanHelp": {"sessionGoal": session_goal}});
    let previous = remembered_session_card(state_root);
    let card =
        session_goal_direct_card_transition(previous.as_ref(), &packet, interactive, closed_stdin)?;
    remember_session_card(state_root, session_goal["requestId"].as_str()?);
    Some(card)
}

/// Record one optional human-only failure joke for a meaningful transition.
/// The memo is display state only and is safe to lose or corrupt.
pub(crate) fn failure_joke_once(
    state_root: &Path,
    transition_key: &str,
    interactive: bool,
    closed_stdin: bool,
) -> bool {
    if !interactive || closed_stdin {
        return false;
    }
    let relative = format!(
        "{}/presentation/failure-joke.json",
        crate::project::layout_types::state_namespace()
    );
    let (previous, previous_raw) = match crate::project::path::secure_bytes_observation(
        state_root,
        &relative,
        "failure joke cache",
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) => {
            let raw = String::from_utf8(bytes).ok();
            let value = raw
                .as_deref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok());
            (value, raw)
        }
        _ => (None, None),
    };
    if previous.as_ref().is_some_and(|value| {
        value["kind"] == "derived_failure_joke_memo" && value["transition"] == transition_key
    }) {
        return false;
    }
    let path = state_root.join(&relative);
    let Some(directory) = path.parent() else {
        return false;
    };
    if crate::project::managed_files::ensure_managed_directory(state_root, directory).is_err() {
        return false;
    }
    let document = json!({
        "kind": "derived_failure_joke_memo",
        "authority": "none",
        "transition": transition_key,
    });
    crate::host::settings::atomic_write(
        &path,
        &format!("{document}\n"),
        Some(0o600),
        previous_raw.as_deref(),
        state_root,
    )
    .is_ok()
}

fn session_card_relative() -> String {
    format!(
        "{}/presentation/session-goal.json",
        crate::project::layout_types::state_namespace()
    )
}

fn remembered_session_card(state_root: &Path) -> Option<Value> {
    let relative = session_card_relative();
    let bytes = match crate::project::path::secure_bytes_observation(
        state_root,
        &relative,
        "session card cache",
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
        _ => return None,
    };
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    (value["kind"] == "derived_session_card_memo").then_some(value)
}

fn remember_session_card(state_root: &Path, request_id: &str) {
    let relative = session_card_relative();
    let path = state_root.join(relative);
    let Some(directory) = path.parent() else {
        return;
    };
    if crate::project::managed_files::ensure_managed_directory(state_root, directory).is_err() {
        return;
    }
    let document = json!({
        "kind": "derived_session_card_memo",
        "authority": "none",
        "requestId": request_id,
        "cardEmitted": true,
    });
    let previous = std::fs::read_to_string(&path).ok();
    let _ = crate::host::settings::atomic_write(
        &path,
        &format!("{document}\n"),
        Some(0o600),
        previous.as_deref(),
        state_root,
    );
}

/// Derived, replaceable memory of what was last presented for one work item.
/// This is a cache beside the run state, never a second ledger: losing it can
/// only repeat an optional line, and nothing read from it becomes evidence or
/// instruction.
fn cache_relative(work: &str) -> String {
    format!(
        "{}/presentation/{work}.json",
        crate::project::layout_types::state_namespace()
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
    let bytes = match crate::project::path::secure_bytes_observation(
        state_root,
        &cache_relative(work),
        "presentation cache",
    ) {
        crate::project::path::SecureBytesResult::Bytes(bytes) => bytes,
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
        crate::evidence::hash::value(&json!([
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
        // Keep the optional field readable for compatible consumers while the
        // retired presentation line stays absent from new product output.
        "holytail": Value::Null,
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
    if crate::project::managed_files::ensure_managed_directory(state_root, directory).is_err() {
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
    let _ = crate::host::settings::atomic_write(
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

    #[test]
    fn session_card_requires_explicit_lead_closure_and_all_categories_resolved() {
        let progress = json!({"state": "READY"});
        let packet = json!({"humanHelp": {"sessionGoal": {
            "explicitLeadClosure": true,
            "subgoals": [], "findings": [], "blockers": [], "decisions": [], "externalActions": []
        }}});
        assert_eq!(
            session_goal_card(&packet, &progress, true, false),
            Some(SESSION_GOAL_CARD)
        );
        assert!(session_goal_card(&packet, &progress, false, false).is_none());
        assert!(session_goal_card(&packet, &progress, true, true).is_none());

        let mut unresolved = packet.clone();
        unresolved["humanHelp"]["sessionGoal"]["findings"] = json!(["open"]);
        assert!(session_goal_card(&unresolved, &progress, true, false).is_none());
        unresolved["humanHelp"]["sessionGoal"]["explicitLeadClosure"] = json!(false);
        unresolved["humanHelp"]["sessionGoal"]["findings"] = json!([]);
        assert!(session_goal_card(&unresolved, &progress, true, false).is_none());
    }

    #[test]
    fn direct_session_card_requires_validated_direct_closure_without_progress() {
        let packet = json!({"humanHelp": {"sessionGoal": {
            "requestId": "direct:r2",
            "completionMode": "direct",
            "explicitLeadClosure": true,
            "subgoals": [], "findings": [], "blockers": [], "decisions": [], "externalActions": []
        }}});
        assert_eq!(
            session_goal_direct_card(&packet, true, false),
            Some(SESSION_GOAL_CARD)
        );
        assert!(session_goal_direct_card(&packet, false, false).is_none());
        assert!(session_goal_direct_card(&packet, true, true).is_none());
        let mut governed = packet.clone();
        governed["humanHelp"]["sessionGoal"]["completionMode"] = json!("governed");
        assert!(session_goal_direct_card(&governed, true, false).is_none());
    }

    #[test]
    fn session_card_is_silent_after_emission_and_for_a_new_unclosed_request() {
        let progress = json!({"state": "READY"});
        let packet = json!({"humanHelp": {"sessionGoal": {
            "requestId": "one", "explicitLeadClosure": true,
            "subgoals": [], "findings": [], "blockers": [], "decisions": [], "externalActions": []
        }}});
        let previous = json!({"requestId": "one", "cardEmitted": true});
        assert!(
            session_goal_card_transition(Some(&previous), &packet, &progress, true, false)
                .is_none()
        );

        let mut new_request = packet.clone();
        new_request["humanHelp"]["sessionGoal"]["requestId"] = json!("two");
        new_request["humanHelp"]["sessionGoal"]["explicitLeadClosure"] = json!(false);
        assert!(session_goal_card_transition(
            Some(&previous),
            &new_request,
            &progress,
            true,
            false
        )
        .is_none());
    }

    #[test]
    fn interactive_session_card_bridge_is_once_only_and_cache_loss_is_safe() {
        let root = std::env::temp_dir().join(format!(
            "exitbind-session-card-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let goal = json!({
            "requestId": "one", "explicitLeadClosure": true,
            "subgoals": [], "findings": [], "blockers": [], "decisions": [], "externalActions": []
        });
        let progress = json!({"state": "READY"});
        assert_eq!(
            session_goal_card_for_human(&root, &goal, &progress, true, false),
            Some(SESSION_GOAL_CARD)
        );
        assert!(session_goal_card_for_human(&root, &goal, &progress, true, false).is_none());
        let cache = root.join(session_card_relative());
        std::fs::write(cache, b"corrupt").unwrap();
        assert_eq!(
            session_goal_card_for_human(&root, &goal, &progress, true, false),
            Some(SESSION_GOAL_CARD)
        );
        let mut new_goal = goal.clone();
        new_goal["requestId"] = json!("two");
        new_goal["explicitLeadClosure"] = json!(false);
        assert!(session_goal_card_for_human(&root, &new_goal, &progress, true, false).is_none());
        assert!(session_goal_card_for_human(&root, &goal, &progress, true, true).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failure_joke_bridge_is_interactive_transition_only() {
        let root = std::env::temp_dir().join(format!(
            "exitbind-failure-joke-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        assert!(failure_joke_once(
            &root,
            "run:blocked:check_missing",
            true,
            false
        ));
        assert!(!failure_joke_once(
            &root,
            "run:blocked:check_missing",
            true,
            false
        ));
        assert!(!failure_joke_once(
            &root,
            "run:blocked:check_missing",
            false,
            false
        ));
        assert!(!failure_joke_once(
            &root,
            "run:blocked:check_missing",
            true,
            true
        ));
        assert!(failure_joke_once(
            &root,
            "run:refused:check_failed",
            true,
            false
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}

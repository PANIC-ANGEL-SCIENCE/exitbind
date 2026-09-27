//! Deterministic, optional display of a currently ready named whole goal.

use serde_json::{json, Value};
use std::path::Path;

#[allow(dead_code)]
pub(crate) const SESSION_GOAL_CARD: &str =
    "+----------------------------+\n| This room remains nothing. |\n+----------------------------+";

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
    themed: bool,
) -> Option<&'static str> {
    let goal = &packet["humanHelp"]["sessionGoal"];
    let empty = |key: &str| goal[key].as_array().is_some_and(|items| items.is_empty());
    if !interactive
        || closed_stdin
        || !themed
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
    themed: bool,
) -> Option<&'static str> {
    let card = session_goal_card(packet, progress, interactive, closed_stdin, themed)?;
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
    themed: bool,
) -> Option<&'static str> {
    let goal = &packet["humanHelp"]["sessionGoal"];
    let empty = |key: &str| goal[key].as_array().is_some_and(|items| items.is_empty());
    if !interactive
        || closed_stdin
        || !themed
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
    themed: bool,
) -> Option<&'static str> {
    let card = session_goal_direct_card(packet, interactive, closed_stdin, themed)?;
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
    themed: bool,
) -> Option<&'static str> {
    let packet = json!({"humanHelp": {"sessionGoal": session_goal}});
    let previous = super::remembered_session_card(state_root);
    let card = session_goal_card_transition(
        previous.as_ref(),
        &packet,
        progress,
        interactive,
        closed_stdin,
        themed,
    )?;
    super::remember_session_card(state_root, session_goal["requestId"].as_str()?);
    Some(card)
}

/// Human-only bridge for a validated direct session-goal closure. It shares
/// the governed card memo and remains silent for headless or machine output.
pub(crate) fn session_goal_direct_card_for_human(
    state_root: &Path,
    session_goal: &Value,
    interactive: bool,
    closed_stdin: bool,
    themed: bool,
) -> Option<&'static str> {
    let packet = json!({"humanHelp": {"sessionGoal": session_goal}});
    let previous = super::remembered_session_card(state_root);
    let card = session_goal_direct_card_transition(
        previous.as_ref(),
        &packet,
        interactive,
        closed_stdin,
        themed,
    )?;
    super::remember_session_card(state_root, session_goal["requestId"].as_str()?);
    Some(card)
}

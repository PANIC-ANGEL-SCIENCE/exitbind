//! Host-owned capture for a prepared native child.
//!
//! `work child prepare` binds an intent to the parent session; SubagentStart
//! claims it and SubagentStop records the host's final message. Unprepared
//! children are not recorded. Host IDs are correlation locators, not
//! authentication or authority. Failed captures remain private and
//! inspectable; `work child recover` attaches retained results without rerun.

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;

use super::perspective::{self, Frozen};

#[path = "capture/actor_checks.rs"]
mod actor_checks;
#[path = "capture/adviser.rs"]
mod adviser;
#[path = "capture/context.rs"]
mod context;
#[path = "capture/delivery.rs"]
mod delivery;
#[path = "capture/raw.rs"]
mod raw;
#[path = "capture/recovery.rs"]
mod recovery;
#[path = "capture/return_result.rs"]
mod return_result;
#[path = "capture/same_host.rs"]
mod same_host;
#[path = "capture/storage.rs"]
mod storage;
#[path = "capture/summary.rs"]
mod summary;
pub(crate) use context::context;
pub(crate) use delivery::{
    advance as advance_child_delivery, has_presented, note_delivery, PermitTransition,
};
pub(crate) use raw::{latest as latest_raw_result, list as raw_results, read as raw_result};
pub(crate) use recovery::recover;
pub(crate) use same_host::{callback_scope, route as same_host_route};
use storage::{load, with_intents};
pub(crate) use summary::{blocking, summary};

const FILE: &str = "child-intents.json";
const MAX_ASSIGNMENT: usize = 128;
const MAX_AGENT_TYPE: usize = 64;
const MAX_RESULT: usize = 8 * 1024;
const MAX_KEPT: usize = 32;
const TERMINAL: [&str; 4] = ["recorded", "failed", "abandoned", "superseded"];
fn bounded<'a>(value: &'a str, field: &str, max: usize) -> Result<&'a str, String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must be non-empty text of at most {max} bytes"
        ));
    }
    Ok(value)
}

fn current_binding(loaded: &Loaded, work: &str) -> Result<(Value, Value), String> {
    let view = super::continuation_view_for_work(loaded, work)?;
    Ok((view["goalRevision"].clone(), view["binding"].clone()))
}

/// The receiving binding `intent` was prepared under, read now: the
/// continuation binding, or the native session that holds the Work.
fn binding_of(loaded: &Loaded, work: &str, intent: &Value) -> Result<(Value, Value), String> {
    if intent["receiving"] == same_host::RECEIVING {
        return Ok((Value::Null, same_host::of_intent(loaded, work, intent)?));
    }
    current_binding(loaded, work)
}

/// `binding_of` for a hook claim or stop: a same-host binding that no longer
/// verifies is a changed binding (null session, with why), so it takes the
/// same abandoned/failed branch as a changed cross-host binding.
fn observed_binding(loaded: &Loaded, work: &str, intent: &Value) -> Result<(Value, Value), String> {
    match binding_of(loaded, work, intent) {
        Err(error) if intent["receiving"] == same_host::RECEIVING => {
            Ok((Value::Null, json!({"session": null, "unverified": error})))
        }
        other => other,
    }
}

/// The failure reason for a changed binding, with why a same-host one failed.
fn changed(reason: &str, binding: &Value) -> String {
    match binding["unverified"].as_str() {
        Some(why) => format!("{reason}: {why}"),
        None => reason.to_owned(),
    }
}

fn selected_is_current(loaded: &Loaded, work: &str, selected: &Value) -> bool {
    if selected.is_null() {
        return true;
    }
    if !matches!(crate::work::focus::read(loaded), Ok(crate::work::focus::Focus::Work(current)) if current == work)
    {
        return false;
    }
    let Ok(view) = crate::work::next(loaded, work) else {
        return false;
    };
    selected_matches(selected, &view["next"])
}

/// Check the same selected identity against an already acquired Work view.
fn selected_matches_current(loaded: &Loaded, work: &str, selected: &Value, next: &Value) -> bool {
    selected.is_null()
        || (matches!(crate::work::focus::read(loaded), Ok(crate::work::focus::Focus::Work(current)) if current == work)
            && selected_matches(selected, next))
}

fn selected_matches(selected: &Value, next: &Value) -> bool {
    next["action"] == "spawn"
        && next["assignment"] == selected["assignment"]
        && next["packet"]["agent"] == selected["agent"]
        && next["packet"]["nativeTaskName"] == selected["nativeTaskName"]
        && next["packet"]["profileSha256"] == selected["profileSha256"]
}

fn direct_started(intents: &[Value], work: &str, assignment: &str) -> bool {
    intents.iter().any(|intent| {
        intent["work"] == work
            && intent["assignment"] == assignment
            && intent["directAssignment"] == true
            && intent["nativeChild"].is_string()
            && intent["state"] != "prepared"
    })
}

pub(crate) fn can_prepare(loaded: &Loaded, work: &str, assignment: &str) -> bool {
    load(loaded).is_ok_and(|intents| !direct_started(&intents, work, assignment))
}

/// The read route that returns a fresh context token for `work`.
fn continuation(loaded: &Loaded, work: &str) -> String {
    crate::emitted::command_line(&["work", "continuation", work], Some(&loaded.path))
}

/// Record one expected native child for the current receiving binding: the
/// `--context` fence `(goal revision, binding revision)` of a continuation
/// binding, or `None` for the native session that holds the Work.
// This is the CLI boundary: keep the work fence, native identity, and overlay explicit.
pub(crate) struct PrepareFence<'a> {
    pub(crate) continuation: Option<(u64, u64)>,
    pub(crate) action: Option<&'a str>,
}

pub(crate) fn prepare(
    loaded: &Loaded,
    work: &str,
    fence: PrepareFence<'_>,
    assignment: &str,
    agent_type: Option<&str>,
    perspectives: Option<&str>,
    replace: bool,
) -> Result<Value, String> {
    let assignment = bounded(assignment, "assignment", MAX_ASSIGNMENT)?;
    let agent_type = agent_type
        .map(|value| bounded(value, "agent type", MAX_AGENT_TYPE))
        .transpose()?;
    let perspectives = perspective::freeze(loaded, perspectives)?;
    crate::work::launch_fence::with_live_work(loaded, work, || {
        with_intents(loaded, |intents| {
            let (binding, binding_revision) = match fence.continuation {
                Some((expected_revision, binding_revision)) => {
                    let (goal, binding) = current_binding(loaded, work)?;
                    if goal.as_u64() != Some(expected_revision)
                        || binding["revision"].as_u64() != Some(binding_revision)
                    {
                        let route = continuation(loaded, work);
                        return Err(format!(
                            "work context is stale; read {route} for a fresh token"
                        ));
                    }
                    (binding, binding_revision)
                }
                None => (same_host::current(loaded, work)?, same_host::REVISION),
            };
            let Some(session) = binding["session"].as_str().map(str::to_owned) else {
                return Err("bind the receiving session before preparing a child".into());
            };
            let view = crate::work::next(loaded, work)?;
            if view["next"]["action"] == "spawn" {
                adviser::validate_requested_binding(
                    loaded,
                    &view["next"]["packet"],
                    binding["host"].as_str().unwrap_or(""),
                )?;
            }
            if crate::memory::contextual::enabled(loaded) {
                if view["next"]["action"] != "spawn" {
                    return Err("contextual memory assignment is not current; prepare no child until its recorded Work is resolved".into());
                }
                crate::memory::contextual::assert_assignment(loaded, &view["next"]["packet"])?;
            }
            let specialist_enabled = view["next"]["agent"]
                .as_str()
                .is_some_and(|name| crate::project::specialists::enabled(loaded, name));
            let direct_assignment = view["next"]["action"] == "spawn"
                && view["next"]["assignment"].as_str() == Some(assignment)
                && agent_type.is_some()
                && view["next"]["packet"]["nativeTaskName"].as_str() == agent_type;
            if direct_assignment && direct_started(intents, work, assignment) {
                return Err("this named assignment already started a native child; inspect its captured result or follow the current Work instead of launching it again".into());
            }
            if let Some(expected) = fence.action {
                crate::work::details::ensure_action_binding(loaded, work, Some(expected))?;
                if !direct_assignment || view["next"]["current"]["binding"] != expected {
                    return Err(
                        "current child assignment changed; refresh the prepared route".into(),
                    );
                }
            }
            let selected = if perspectives.is_empty()
                && loaded.capabilities.is_empty()
                && !specialist_enabled
                && !direct_assignment
            {
                Value::Null
            } else {
                let view = crate::work::next(loaded, work)?;
                let next = &view["next"];
                if next["action"] != "spawn"
                    || agent_type != next["packet"]["nativeTaskName"].as_str()
                    || !next["assignment"].is_string()
                    || !next["packet"]["profileSha256"].is_string()
                {
                    return Err(
                        "selected perspectives require the current named native assignment".into(),
                    );
                }
                json!({"assignment":next["assignment"],
                "agent":next["packet"]["agent"],
                "nativeTaskName":next["packet"]["nativeTaskName"],
                "profileSha256":next["packet"]["profileSha256"]})
            };
            if !selected_is_current(loaded, work, &selected) {
                let detail = crate::emitted::command_line(
                    &["work", "detail", work, "--json"],
                    Some(&loaded.path),
                );
                let focus =
                    crate::emitted::command_line(&["work", "focus", work], Some(&loaded.path));
                return Err(format!("the current named child assignment is unavailable; read `{detail}` and, if intended, select its Work with `{focus}` before preparing"));
            }
            let capability_binding = if selected.is_null() {
                Value::Null
            } else {
                let view = crate::work::next(loaded, work)?;
                let delivery = crate::project::capabilities::delivery(
                    loaded,
                    &view["next"]["packet"],
                    binding["host"].as_str().unwrap_or(""),
                )?;
                json!(hash::value(&json!(delivery)))
            };
            let specialist_binding = if selected.is_null() || !specialist_enabled {
                Value::Null
            } else {
                let next = &view["next"];
                let delivery = crate::project::specialists::delivery(
                    loaded,
                    next["agent"].as_str().unwrap_or(""),
                    next["role"].as_str().unwrap_or(""),
                    next["packet"]["goal"].as_str().unwrap_or(""),
                    binding["host"].as_str().unwrap_or(""),
                )?;
                json!(hash::value(&json!(delivery)))
            };
            let mut intent = json!({
                "work": work,
                "session": session,
                "host": binding["host"],
                "bindingRevision": binding_revision,
                "assignment": assignment,
                "agentType": agent_type,
                "selected": selected,
                "perspectives": perspectives,
                "capabilityBinding": capability_binding,
            });
            if direct_assignment {
                intent["directAssignment"] = json!(true);
                intent["directResultIdentity"] = json!({"role":view["next"]["role"],
                "agent":view["next"]["agent"],"stage":view["next"]["packet"]["stage"],
                "attempt":view["next"]["packet"]["attempt"]});
            }
            if !specialist_binding.is_null() {
                intent["specialistBinding"] = specialist_binding;
            }
            if fence.continuation.is_none() {
                intent["receiving"] = json!(same_host::RECEIVING);
            }
            let id = hash::value(&intent);
            let mut effect = "prepared";
            let mut superseded = Vec::new();
            // A prepared intent of this work under an older binding can never be
            // claimed, whichever session prepared it; retire it here.
            for existing in intents.iter_mut().filter(|existing| {
                existing["state"] == "prepared"
                    && (existing["session"] == session.as_str() || existing["work"] == work)
            }) {
                if existing["id"] == id.as_str() {
                    effect = "unchanged";
                } else if existing["bindingRevision"] != json!(binding_revision) {
                    existing["state"] = json!("abandoned");
                    existing["failure"] =
                        json!("the receiving binding changed before a child started");
                } else if replace {
                    existing["state"] = json!("superseded");
                    superseded.push(existing["assignment"].clone());
                } else {
                    return Err("another prepared child is still waiting for this session; launch it, or repeat this command with --replace to supersede it".into());
                }
            }
            if effect == "prepared" {
                let mut record = intent.clone();
                record["id"] = json!(id);
                record["state"] = json!("prepared");
                intents.push(record);
            }
            let next = if fence.continuation.is_none() {
                crate::emitted::command_line(
                    &["work", "detail", work, "--json"],
                    Some(&loaded.path),
                )
            } else {
                continuation(loaded, work)
            };
            Ok(json!({
                "work": work,
                "intent": id,
                "assignment": assignment,
                "effect": effect,
                "superseded": superseded,
                "selected": selected,
                "perspectives": perspectives,
                "next": format!("Launch the native child normally; the next compatible subagent this session starts is the one captured. Then read `{next}`."),
            }))
        })
    })
}

fn text<'a>(payload: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
}

fn claimable(intent: &Value, session: &str, agent_type: Option<&str>) -> bool {
    intent["state"] == "prepared"
        && matches!(intent["host"].as_str(), Some("claude" | "codex"))
        && intent["session"] == session
        && (intent["agentType"].is_null()
            || agent_type.is_some_and(|kind| intent["agentType"] == kind))
}

fn same_observed_agent_type(intent: &Value, agent_type: Option<&str>) -> bool {
    intent["observedAgentType"].as_str() == agent_type
}

fn rejected<'a>(
    intents: &'a [Value],
    session: &str,
    child: &str,
    agent_type: Option<&str>,
) -> Option<&'a Value> {
    intents.iter().rev().find(|intent| {
        intent["state"] == "abandoned"
            && intent["session"] == session
            && intent["nativeChild"] == child
            && (intent["agentType"].is_null()
                || agent_type.is_some_and(|kind| intent["agentType"] == kind))
    })
}

fn rejection_reason(intent: &Value) -> String {
    intent["failure"]
        .as_str()
        .unwrap_or("prepared child context was rejected")
        .to_owned()
}

/// SubagentStart: claim the single compatible prepared intent for the bound
/// parent session, or record nothing.
pub(crate) fn claim(
    loaded: &Loaded,
    payload: &serde_json::Map<String, Value>,
) -> Result<Option<String>, String> {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return Ok(None);
    };
    let agent_type = text(payload, "agent_type");
    // Ordinary subagents without a prepared intent cost one read, no write.
    let intents = load(loaded)?;
    // A rejected child identity stays rejected even if another intent was
    // prepared for the same receiving session before a duplicate hook event.
    if let Some(intent) = rejected(&intents, session, child, agent_type) {
        return Ok(Some(rejection_reason(intent)));
    }
    let candidates = intents
        .iter()
        .filter(|intent| claimable(intent, session, agent_type))
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return Ok(None);
    };
    let selected_work = candidate["work"]
        .as_str()
        .ok_or("prepared child Work identity is unavailable")?
        .to_owned();
    crate::work::launch_fence::with_live_work(loaded, &selected_work, || {
        with_intents(loaded, |intents| {
            // Re-read under the mutation lock. A concurrent callback may have
            // abandoned this child after the preliminary load above.
            if let Some(intent) = rejected(intents, session, child, agent_type) {
                return Ok(Some(rejection_reason(intent)));
            }
            let matches = intents
                .iter()
                .enumerate()
                .filter(|(_, intent)| claimable(intent, session, agent_type))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [index] = matches[..] else {
                return Ok(None);
            };
            let work = intents[index]["work"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if work != selected_work {
                return Err("prepared child Work changed before its fenced claim".into());
            }
            intents[index]["nativeChild"] = json!(child);
            intents[index]["observedAgentType"] = json!(agent_type);
            let (_, binding) = observed_binding(loaded, &work, &intents[index])?;
            if binding["session"] != session
                || binding["revision"] != intents[index]["bindingRevision"]
            {
                intents[index]["state"] = json!("abandoned");
                let reason = changed(
                    "the receiving binding changed before a child started",
                    &binding,
                );
                intents[index]["failure"] = json!(&reason);
                return Ok(Some(reason));
            }
            if !selected_is_current(loaded, &work, &intents[index]["selected"]) {
                intents[index]["state"] = json!("abandoned");
                let reason = "the selected assignment changed before a child started";
                intents[index]["failure"] = json!(reason);
                return Ok(Some(reason.to_owned()));
            }
            if intents[index]["selected"].is_null() {
                let current = crate::work::next(loaded, &work)?;
                if let Err(error) = adviser::validate_requested_binding(
                    loaded,
                    &current["next"]["packet"],
                    binding["host"].as_str().unwrap_or(""),
                ) {
                    intents[index]["state"] = json!("abandoned");
                    let reason = format!("native adviser context unavailable: {error}");
                    intents[index]["failure"] = json!(&reason);
                    return Ok(Some(reason));
                }
            }
            if !intents[index]["selected"].is_null() {
                let intent = intents[index]["id"].as_str().unwrap_or_default();
                // The core checks associate this child for observation only.
                // Selected-delivery proof is checked after the claim, at
                // presentation, before any automatic return can be credited.
                if let Err(error) = context::core_host_context(loaded, &work, intent) {
                    intents[index]["state"] = json!("abandoned");
                    let reason = format!("child context unavailable: {error}");
                    intents[index]["failure"] = json!(&reason);
                    return Ok(Some(reason));
                }
            }
            intents[index]["state"] = json!("claimed");
            Ok(None)
        })
    })
}

pub(crate) fn claimed_context(
    loaded: &Loaded,
    payload: &serde_json::Map<String, Value>,
) -> Result<Option<Value>, String> {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return Ok(None);
    };
    let Some(intent) = load(loaded)?.into_iter().find(|intent| {
        intent["state"] == "claimed"
            && intent["session"] == session
            && intent["nativeChild"] == child
            && !intent["selected"].is_null()
    }) else {
        return Ok(None);
    };
    let (Some(work), Some(id)) = (intent["work"].as_str(), intent["id"].as_str()) else {
        return Err("claimed child identity is malformed".into());
    };
    if intent["assignmentDelivery"]["state"] == "unavailable" {
        return Err("the original child presentation is unavailable".into());
    }
    context::host_context(loaded, work, id).map(Some)
}

fn is_busy(error: &str) -> bool {
    error.contains("busy")
}

/// SubagentStop: finalize a claimed intent from the host's final message.
pub(crate) fn finalize(
    loaded: &Loaded,
    payload: &serde_json::Map<String, Value>,
    unavailable: Option<&str>,
) -> Result<(), String> {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return Ok(());
    };
    let agent_type = text(payload, "agent_type");
    let owned = |intent: &Value| {
        intent["session"] == session
            && intent["nativeChild"] == child
            && same_observed_agent_type(intent, agent_type)
            && matches!(intent["state"].as_str(), Some("claimed" | "recorded"))
    };
    if !load(loaded)?.iter().any(owned) {
        return Ok(());
    }
    let message = text(payload, "last_assistant_message");
    let mut direct_return = None;
    with_intents(loaded, |intents| {
        let Some(intent) = intents.iter_mut().find(|intent| owned(intent)) else {
            return Ok(());
        };
        let digest = message.map(hash::text);
        if intent["state"] == "recorded" {
            // A repeated stop never changes a recorded child; a different
            // message is flagged for inspection.
            if let (Some(message), Some(digest)) = (message, &digest) {
                if intent["resultSha256"] != digest.as_str() {
                    intent["conflict"] = json!({"resultSha256": digest, "bytes": message.len()});
                } else if unavailable.is_none()
                    && intent["pendingReturn"] == true
                    && !intent["held"].is_object()
                {
                    direct_return = intent["id"].as_str().map(str::to_owned);
                }
            }
            return Ok(());
        }
        let (Some(message), Some(digest)) = (message, digest) else {
            intent["state"] = json!("failed");
            intent["failure"] = json!("the host reported no final message");
            return Ok(());
        };
        intent["resultSha256"] = json!(digest);
        if message.len() > MAX_RESULT {
            intent["state"] = json!("failed");
            intent["failure"] = json!(format!(
                "final message is {} bytes, above the {MAX_RESULT}-byte child result limit; it was not truncated",
                message.len()
            ));
            return Ok(());
        }
        let work = intent["work"].as_str().unwrap_or_default().to_owned();
        if let Some(reason) = unavailable {
            if intent["directAssignment"] == true {
                return_result::retain_raw(loaded, &work, intent, message, reason);
            } else {
                intent["state"] = json!("failed");
                intent["failure"] = json!(reason);
                intent["pendingResult"] = json!(message);
            }
            return Ok(());
        }
        let (goal, binding) = match observed_binding(loaded, &work, intent) {
            Ok(observed) => observed,
            Err(error) if intent["directAssignment"] == true => {
                return_result::retain_raw(loaded, &work, intent, message, &error);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if binding["session"] != session || binding["revision"] != intent["bindingRevision"] {
            let reason = changed(
                "the receiving binding changed before the child finished",
                &binding,
            );
            if intent["directAssignment"] == true {
                return_result::retain_raw(loaded, &work, intent, message, &reason);
                return Ok(());
            }
            intent["state"] = json!("failed");
            intent["failure"] = json!(reason);
            intent["pendingResult"] = json!(message);
            return Ok(());
        }
        if intent["directAssignment"] == true {
            if let Err(reason) = return_result::validate_capture(loaded, &work, intent, message) {
                return_result::retain_raw(loaded, &work, intent, message, &reason);
                return Ok(());
            }
            // A named assignment's existing Work return owns its artifact.
            // Recording a separate free-text continuation child here would
            // advance the Goal revision and stale the presented binding.
            intent["state"] = json!("recorded");
            intent["pendingResult"] = json!(message);
            intent["pendingReturn"] = json!(true);
            direct_return = intent["id"].as_str().map(str::to_owned);
            return Ok(());
        }
        if intent["receiving"] == same_host::RECEIVING {
            // No continuation record exists: the hash identifies the final
            // message, and the Work's own return carries the result.
            intent["state"] = json!("recorded");
            return Ok(());
        }
        match record(loaded, &work, &goal, intent, message) {
            Err(error) if is_busy(&error) => Err(error),
            Err(error) => {
                intent["state"] = json!("failed");
                intent["failure"] = json!(error.chars().take(512).collect::<String>());
                intent["pendingResult"] = json!(message);
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    })?;
    if let Some(id) = direct_return {
        return_result::process(loaded, &id)?;
    }
    Ok(())
}

fn record(
    loaded: &Loaded,
    work: &str,
    goal: &Value,
    intent: &mut Value,
    message: &str,
) -> Result<(), String> {
    if crate::memory::contextual::enabled(loaded) {
        let current = crate::work::next(loaded, work)?;
        if current["next"]["action"] != "spawn" {
            return Err("contextual child assignment is no longer current; retain its result without acceptance".into());
        }
        crate::memory::contextual::assert_assignment(loaded, &current["next"]["packet"])?;
    }
    super::continuation_child(
        loaded,
        work,
        goal.as_u64().unwrap_or_default(),
        intent["bindingRevision"].as_u64().unwrap_or_default(),
        intent["assignment"].as_str().unwrap_or_default(),
        intent["nativeChild"].as_str().unwrap_or_default(),
        message,
    )?;
    intent["state"] = json!("recorded");
    if let Some(object) = intent.as_object_mut() {
        object.remove("pendingResult");
        object.remove("failure");
    }
    Ok(())
}

#[cfg(test)]
#[path = "capture/tests.rs"]
mod tests;

//! Host-owned native child capture for a prepared delegation.
//!
//! A receiving parent prepares one child intent with `work child prepare`.
//! Claude Code's SubagentStart hook then claims it for the next compatible
//! subagent the same bound parent session starts, and SubagentStop finalizes
//! it from the host's final assistant message through the existing child
//! record. The parent model never relays the child ID or result. A subagent
//! started while no intent is prepared is never recorded. Host fields stay
//! host-reported; nothing here authenticates them or grants acceptance,
//! review, or permission. A capture that cannot attach stays inspectable here,
//! and `work child recover` records a retained result without re-running it.

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;

use super::perspective::{self, Frozen};

const FILE: &str = "child-intents.json";
const MAX_ASSIGNMENT: usize = 128;
const MAX_AGENT_TYPE: usize = 64;
const MAX_RESULT: usize = 8 * 1024;
const MAX_KEPT: usize = 32;
const TERMINAL: [&str; 4] = ["recorded", "failed", "abandoned", "superseded"];
/// The initial supported capture path is Claude Code's subagent lifecycle.
const CAPTURE_HOST: &str = "claude";

fn relative() -> String {
    format!("{}/{FILE}", crate::project::layout_types::state_namespace())
}

fn load(loaded: &Loaded) -> Result<Vec<Value>, String> {
    let path = loaded.state_root.join(relative());
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "child intent state is not JSON")?;
    match (value["version"].as_u64(), value["intents"].as_array()) {
        (Some(1), Some(intents)) => Ok(intents.clone()),
        _ => Err("child intent state is malformed".into()),
    }
}

fn save(loaded: &Loaded, intents: &[Value]) -> Result<(), String> {
    let path = loaded.state_root.join(relative());
    let staged = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let body = json!({"version": 1, "intents": intents}).to_string();
    let result = (|| -> Result<(), String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&staged).map_err(|error| error.to_string())?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        fs::rename(&staged, &path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

/// Mutate the intents under their own lock. An `Err` from `action` leaves the
/// file untouched, so a busy error can be retried by the caller.
fn with_intents<T>(
    loaded: &Loaded,
    action: impl FnOnce(&mut Vec<Value>) -> Result<T, String>,
) -> Result<T, String> {
    let ledger = crate::run::ledger::ledger_path(&loaded.state_root, &relative(), true)?;
    crate::run::ledger::with_lock(&ledger, || {
        let mut intents = load(loaded)?;
        let result = action(&mut intents)?;
        while intents.len() > MAX_KEPT {
            let Some(index) = intents
                .iter()
                .position(|intent| TERMINAL.iter().any(|state| intent["state"] == *state))
            else {
                break;
            };
            intents.remove(index);
        }
        save(loaded, &intents)?;
        Ok(result)
    })
}

fn bounded<'a>(value: &'a str, field: &str, max: usize) -> Result<&'a str, String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must be non-empty text of at most {max} bytes"
        ));
    }
    Ok(value)
}

fn current_binding(loaded: &Loaded, work: &str) -> Result<(Value, Value), String> {
    let view = super::continuation_view(loaded, work)?;
    Ok((view["goalRevision"].clone(), view["binding"].clone()))
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
    let next = &view["next"];
    next["action"] == "spawn"
        && next["assignment"] == selected["assignment"]
        && next["packet"]["agent"] == selected["agent"]
        && next["packet"]["nativeTaskName"] == selected["nativeTaskName"]
        && next["packet"]["profileSha256"] == selected["profileSha256"]
}

/// Record one expected native child for the current receiving binding.
// This is the CLI boundary: keep the work fence, native identity, and overlay explicit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    loaded: &Loaded,
    work: &str,
    expected_revision: u64,
    binding_revision: u64,
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
    with_intents(loaded, |intents| {
        let (goal, binding) = current_binding(loaded, work)?;
        if goal.as_u64() != Some(expected_revision)
            || binding["revision"].as_u64() != Some(binding_revision)
        {
            return Err("work context is stale; read work continuation for a fresh token".into());
        }
        let Some(session) = binding["session"].as_str().map(str::to_owned) else {
            return Err("bind the receiving session before preparing a child".into());
        };
        let selected = if perspectives.is_empty() {
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
        let intent = json!({
            "work": work,
            "session": session,
            "host": binding["host"],
            "bindingRevision": binding_revision,
            "assignment": assignment,
            "agentType": agent_type,
            "selected": selected,
            "perspectives": perspectives,
        });
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
                existing["failure"] = json!("the receiving binding changed before a child started");
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
        Ok(json!({
            "work": work,
            "intent": id,
            "assignment": assignment,
            "effect": effect,
            "superseded": superseded,
            "selected": selected,
            "perspectives": perspectives,
            "next": "Launch the native child normally; the next compatible subagent this session starts is the one captured. Then read `work continuation WORK`.",
        }))
    })
}

/// Current, complete, bounded bytes for one prepared named child. A saved
/// selector is never enough: binding, assignment and source hashes are rechecked.
pub(crate) fn context(loaded: &Loaded, work: &str, intent_id: &str) -> Result<Value, String> {
    let intent = load(loaded)?
        .into_iter()
        .find(|item| item["id"] == intent_id && item["work"] == work)
        .ok_or("prepared child intent is unknown")?;
    if !matches!(intent["state"].as_str(), Some("prepared" | "claimed")) {
        return Err("child context expired with its prepared assignment".into());
    }
    let (_, binding) = current_binding(loaded, work)?;
    if binding["session"] != intent["session"] || binding["revision"] != intent["bindingRevision"] {
        return Err("child context expired after binding change".into());
    }
    let selected = &intent["selected"];
    if selected.is_null() {
        return Err("no named assignment was frozen for this child".into());
    }
    if !selected_is_current(loaded, work, selected) {
        return Err("child context expired after assignment or profile change".into());
    }
    let agent_id = selected["agent"].as_str().ok_or("invalid selected agent")?;
    let agent = loaded
        .agent(agent_id)
        .ok_or("selected agent is unavailable")?;
    let bytes =
        crate::project::path::secure_bytes(&loaded.control_root, &agent.profile, "agent profile")?;
    if hash::bytes(&bytes) != selected["profileSha256"] {
        return Err("selected profile source changed".into());
    }
    let source = String::from_utf8(bytes).map_err(|_| "agent profile is not UTF-8")?;
    let presented = crate::host::runtime::redact(&source);
    let sources: Vec<Frozen> = serde_json::from_value(intent["perspectives"].clone())
        .map_err(|_| "prepared perspectives are malformed")?;
    let perspectives = perspective::current(loaded, &sources)?;
    let result = json!({
        "work":work,"intent":intent_id,"assignment":selected["assignment"],
        "configuredAgent":agent_id,"nativeTaskName":selected["nativeTaskName"],
        "bindingRevision":intent["bindingRevision"],
        "profile":{"path":agent.profile,"sourceSha256":selected["profileSha256"],
            "presentedSha256":hash::text(&presented),"content":presented,
            "transformation":"path-redaction","coverage":"complete"},
        "perspectives":perspectives,
        "evidence":"product-presented-for-current-assignment; native launch and behavior separate",
    });
    if serde_json::to_vec(&result)
        .map_err(|_| "child context serialization failed")?
        .len()
        > 16 * 1024
    {
        return Err("child context exceeds the complete 16 KiB envelope".into());
    }
    Ok(result)
}

fn text<'a>(payload: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
}

fn claimable(intent: &Value, session: &str, agent_type: Option<&str>) -> bool {
    intent["state"] == "prepared"
        && intent["host"] == CAPTURE_HOST
        && intent["session"] == session
        && (intent["agentType"].is_null()
            || agent_type.is_some_and(|kind| intent["agentType"] == kind))
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
    if !intents
        .iter()
        .any(|intent| claimable(intent, session, agent_type))
    {
        return Ok(None);
    }
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
        intents[index]["nativeChild"] = json!(child);
        intents[index]["observedAgentType"] = json!(agent_type);
        let (_, binding) = current_binding(loaded, &work)?;
        if binding["session"] != session || binding["revision"] != intents[index]["bindingRevision"]
        {
            intents[index]["state"] = json!("abandoned");
            let reason = "the receiving binding changed before a child started";
            intents[index]["failure"] = json!(reason);
            return Ok(Some(reason.to_owned()));
        }
        if !selected_is_current(loaded, &work, &intents[index]["selected"]) {
            intents[index]["state"] = json!("abandoned");
            let reason = "the selected assignment changed before a child started";
            intents[index]["failure"] = json!(reason);
            return Ok(Some(reason.to_owned()));
        }
        if !intents[index]["selected"].is_null() {
            let intent = intents[index]["id"].as_str().unwrap_or_default();
            if let Err(error) = context(loaded, &work, intent) {
                intents[index]["state"] = json!("abandoned");
                let reason = format!("child context unavailable: {error}");
                intents[index]["failure"] = json!(&reason);
                return Ok(Some(reason));
            }
        }
        intents[index]["state"] = json!("claimed");
        Ok(None)
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
    context(loaded, work, id).map(Some)
}

fn is_busy(error: &str) -> bool {
    error.contains("busy")
}

/// SubagentStop: finalize a claimed intent from the host's final message.
pub(crate) fn finalize(
    loaded: &Loaded,
    payload: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return Ok(());
    };
    let owned = |intent: &Value| {
        intent["session"] == session
            && intent["nativeChild"] == child
            && matches!(intent["state"].as_str(), Some("claimed" | "recorded"))
    };
    if !load(loaded)?.iter().any(owned) {
        return Ok(());
    }
    let message = text(payload, "last_assistant_message");
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
        let (goal, binding) = current_binding(loaded, &work)?;
        if binding["session"] != session || binding["revision"] != intent["bindingRevision"] {
            intent["state"] = json!("failed");
            intent["failure"] = json!("the receiving binding changed before the child finished");
            intent["pendingResult"] = json!(message);
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
    })
}

fn record(
    loaded: &Loaded,
    work: &str,
    goal: &Value,
    intent: &mut Value,
    message: &str,
) -> Result<(), String> {
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

/// Record a retained child result that failed to attach, under the same
/// receiving session and a current context, without re-running the child.
pub(crate) fn recover(
    loaded: &Loaded,
    work: &str,
    intent_id: &str,
    expected_revision: u64,
    binding_revision: u64,
) -> Result<Value, String> {
    with_intents(loaded, |intents| {
        let Some(intent) = intents.iter_mut().find(|intent| {
            intent["work"] == work
                && intent["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(intent_id))
        }) else {
            return Err("no prepared child with that intent for this work".into());
        };
        let Some(message) = intent["pendingResult"].as_str().map(str::to_owned) else {
            return Err("this child has no retained result to recover".into());
        };
        let (goal, binding) = current_binding(loaded, work)?;
        if goal.as_u64() != Some(expected_revision)
            || binding["revision"].as_u64() != Some(binding_revision)
        {
            return Err("work context is stale; read work continuation for a fresh token".into());
        }
        if binding["session"] != intent["session"] {
            return Err("the child ran under another receiving session; its result cannot be attributed to the current binding".into());
        }
        intent["bindingRevision"] = json!(binding_revision);
        record(loaded, work, &goal, intent, &message)?;
        Ok(json!({"work": work, "intent": intent["id"], "effect": "recorded"}))
    })
}

/// Read-only summary of this work's prepared children, without result text.
pub(crate) fn summary(loaded: &Loaded, work: &str) -> Value {
    let Ok(intents) = load(loaded) else {
        return json!({"error": "child intent state is unreadable"});
    };
    Value::Array(
        intents
            .iter()
            .filter(|intent| intent["work"] == work)
            .map(|intent| {
                let pending = intent["pendingResult"].as_str().map(str::len);
                json!({
                    "intent": intent["id"],
                    "assignment": intent["assignment"],
                    "state": intent["state"],
                    "nativeChild": intent["nativeChild"],
                    "agentType": intent["agentType"],
                    "failure": intent["failure"],
                    "conflict": intent["conflict"],
                    "resultSha256": intent["resultSha256"],
                    "retainedResultBytes": pending,
                    "recover": pending.map(|_| "work child recover WORK INTENT --context TOKEN"),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_identity_wins_over_a_new_prepared_intent() {
        let intents = vec![
            json!({
                "state": "abandoned",
                "session": "parent",
                "nativeChild": "child-a",
                "agentType": "worker",
                "failure": "the selected assignment changed before a child started"
            }),
            json!({
                "state": "prepared",
                "session": "parent",
                "nativeChild": null,
                "agentType": "worker"
            }),
        ];
        let intent = rejected(&intents, "parent", "child-a", Some("worker"))
            .expect("the prior rejected child identity must remain decisive");
        assert_eq!(
            rejection_reason(intent),
            "the selected assignment changed before a child started"
        );
    }
}

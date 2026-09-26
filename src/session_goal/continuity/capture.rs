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

/// Record one expected native child for the current receiving binding.
pub(crate) fn prepare(
    loaded: &Loaded,
    work: &str,
    expected_revision: u64,
    binding_revision: u64,
    assignment: &str,
    agent_type: Option<&str>,
    replace: bool,
) -> Result<Value, String> {
    let assignment = bounded(assignment, "assignment", MAX_ASSIGNMENT)?;
    let agent_type = agent_type
        .map(|value| bounded(value, "agent type", MAX_AGENT_TYPE))
        .transpose()?;
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
        let intent = json!({
            "work": work,
            "session": session,
            "host": binding["host"],
            "bindingRevision": binding_revision,
            "assignment": assignment,
            "agentType": agent_type,
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
            "next": "Launch the native child normally; the next compatible subagent this session starts is the one captured. Then read `work continuation WORK`.",
        }))
    })
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

/// SubagentStart: claim the single compatible prepared intent for the bound
/// parent session, or record nothing.
pub(crate) fn claim(
    loaded: &Loaded,
    payload: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let (Some(session), Some(child)) = (text(payload, "session_id"), text(payload, "agent_id"))
    else {
        return Ok(());
    };
    let agent_type = text(payload, "agent_type");
    // Ordinary subagents without a prepared intent cost one read, no write.
    if !load(loaded)?
        .iter()
        .any(|intent| claimable(intent, session, agent_type))
    {
        return Ok(());
    }
    with_intents(loaded, |intents| {
        let matches = intents
            .iter()
            .enumerate()
            .filter(|(_, intent)| claimable(intent, session, agent_type))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [index] = matches[..] else {
            return Ok(());
        };
        let work = intents[index]["work"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let (_, binding) = current_binding(loaded, &work)?;
        if binding["session"] != session || binding["revision"] != intents[index]["bindingRevision"]
        {
            intents[index]["state"] = json!("abandoned");
            intents[index]["failure"] =
                json!("the receiving binding changed before a child started");
            return Ok(());
        }
        intents[index]["state"] = json!("claimed");
        intents[index]["nativeChild"] = json!(child);
        intents[index]["observedAgentType"] = json!(agent_type);
        Ok(())
    })
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

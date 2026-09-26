//! Host-owned native child capture for a prepared delegation.
//!
//! A receiving parent prepares one child intent with `work child prepare`.
//! Claude Code's SubagentStart hook then claims it with the host-reported
//! parent session and child ID, and SubagentStop finalizes it from the host's
//! final assistant message through the existing child record. The parent
//! model never relays the child ID or result. Unprepared subagents are never
//! recorded. Host fields stay host-reported; nothing here authenticates them
//! or grants acceptance, review, or permission. A capture that cannot attach
//! stays inspectable in this private state instead of disappearing.

use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;

const FILE: &str = "child-intents.json";
const MAX_ASSIGNMENT: usize = 128;
const MAX_AGENT_TYPE: usize = 64;
const MAX_RESULT: usize = 8 * 1024;
const MAX_KEPT: usize = 32;

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

fn with_intents<T>(
    loaded: &Loaded,
    action: impl FnOnce(&mut Vec<Value>) -> Result<T, String>,
) -> Result<T, String> {
    let ledger = crate::run::ledger::ledger_path(&loaded.state_root, &relative(), true)?;
    crate::run::ledger::with_lock(&ledger, || {
        let mut intents = load(loaded)?;
        let result = action(&mut intents)?;
        let excess = intents.len().saturating_sub(MAX_KEPT);
        let removable = intents
            .iter()
            .take(excess)
            .all(|intent| intent["state"] == "recorded");
        if excess > 0 && removable {
            intents.drain(..excess);
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

/// Record one expected native child for the current receiving binding.
pub(crate) fn prepare(
    loaded: &Loaded,
    work: &str,
    expected_revision: u64,
    binding_revision: u64,
    assignment: &str,
    agent_type: Option<&str>,
) -> Result<Value, String> {
    let assignment = bounded(assignment, "assignment", MAX_ASSIGNMENT)?;
    let agent_type = agent_type
        .map(|value| bounded(value, "agent type", MAX_AGENT_TYPE))
        .transpose()?;
    let view = super::continuation_view(loaded, work)?;
    let current_goal = view["goalRevision"].as_u64();
    let current_binding = view["binding"]["revision"].as_u64();
    if current_goal != Some(expected_revision) || current_binding != Some(binding_revision) {
        return Err("work context is stale; read work continuation for a fresh token".into());
    }
    let Some(session) = view["binding"]["session"].as_str().map(str::to_owned) else {
        return Err("bind the receiving session before preparing a child".into());
    };
    let intent = json!({
        "work": work,
        "session": session,
        "host": view["binding"]["host"],
        "bindingRevision": binding_revision,
        "assignment": assignment,
        "agentType": agent_type,
    });
    let id = hash::value(&intent);
    with_intents(loaded, |intents| {
        let open = intents.iter().find(|existing| {
            existing["session"] == session.as_str() && existing["state"] == "prepared"
        });
        let effect = match open {
            Some(existing) if existing["id"] == id.as_str() => "unchanged",
            Some(_) => {
                return Err("another prepared child is still waiting for this session; launch it or let it finish first".into())
            }
            None => {
                let mut record = intent.clone();
                record["id"] = json!(id);
                record["state"] = json!("prepared");
                intents.push(record);
                "prepared"
            }
        };
        Ok(json!({
            "work": work,
            "intent": id,
            "assignment": assignment,
            "effect": effect,
            "next": "Launch the native child normally. Exitbind captures its host-reported ID and final message; then read `work continuation WORK`.",
        }))
    })
}

fn text<'a>(payload: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
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
    with_intents(loaded, |intents| {
        let matches = intents
            .iter()
            .enumerate()
            .filter(|(_, intent)| {
                intent["state"] == "prepared"
                    && intent["session"] == session
                    && (intent["agentType"].is_null()
                        || agent_type.is_some_and(|kind| intent["agentType"] == kind))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [index] = matches[..] else {
            return Ok(());
        };
        let work = intents[index]["work"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let view = super::continuation_view(loaded, &work)?;
        if view["binding"]["session"] != session
            || view["binding"]["revision"] != intents[index]["bindingRevision"]
        {
            return Ok(());
        }
        intents[index]["state"] = json!("claimed");
        intents[index]["nativeChild"] = json!(child);
        intents[index]["observedAgentType"] = json!(agent_type);
        Ok(())
    })
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
    let message = text(payload, "last_assistant_message");
    with_intents(loaded, |intents| {
        let Some(intent) = intents.iter_mut().find(|intent| {
            intent["session"] == session
                && intent["nativeChild"] == child
                && matches!(intent["state"].as_str(), Some("claimed" | "recorded"))
        }) else {
            return Ok(());
        };
        let Some(message) = message else {
            intent["state"] = json!("failed");
            intent["failure"] = json!("the host reported no final message");
            return Ok(());
        };
        let digest = hash::text(message);
        if intent["state"] == "recorded" {
            if intent["resultSha256"] != digest.as_str() {
                intent["conflict"] = json!({"resultSha256": digest, "bytes": message.len()});
            }
            return Ok(());
        }
        if message.len() > MAX_RESULT {
            intent["state"] = json!("failed");
            intent["failure"] = json!(format!(
                "final message is {} bytes, above the {MAX_RESULT}-byte child result limit; it was not truncated",
                message.len()
            ));
            intent["resultSha256"] = json!(digest);
            return Ok(());
        }
        let work = intent["work"].as_str().unwrap_or_default().to_owned();
        let view = super::continuation_view(loaded, &work)?;
        if view["binding"]["session"] != session
            || view["binding"]["revision"] != intent["bindingRevision"]
        {
            intent["state"] = json!("failed");
            intent["failure"] = json!("the receiving binding changed before the child finished");
            intent["pendingResult"] = json!(message);
            intent["resultSha256"] = json!(digest);
            return Ok(());
        }
        let recorded = super::continuation_child(
            loaded,
            &work,
            view["goalRevision"].as_u64().unwrap_or_default(),
            intent["bindingRevision"].as_u64().unwrap_or_default(),
            intent["assignment"].as_str().unwrap_or_default(),
            child,
            message,
        );
        match recorded {
            Ok(_) => {
                intent["state"] = json!("recorded");
                intent["resultSha256"] = json!(digest);
            }
            Err(error) => {
                intent["state"] = json!("failed");
                intent["failure"] = json!(error.chars().take(512).collect::<String>());
                intent["pendingResult"] = json!(message);
                intent["resultSha256"] = json!(digest);
            }
        }
        Ok(())
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
                json!({
                    "assignment": intent["assignment"],
                    "state": intent["state"],
                    "nativeChild": intent["nativeChild"],
                    "agentType": intent["agentType"],
                    "failure": intent["failure"],
                    "conflict": intent["conflict"],
                    "resultSha256": intent["resultSha256"],
                })
            })
            .collect(),
    )
}

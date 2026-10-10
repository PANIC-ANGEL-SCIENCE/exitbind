//! Current recipient instructions for the existing readable detail owner.
use crate::{
    config::Loaded,
    evidence::hash,
    project::{agent_context, agent_rules, path},
};
use serde_json::{json, Value};

const MAX_PROFILE_BYTES: usize = 16 * 1024;

/// The current public section pages the complete instructions independently
/// of the grouped response and its inline profile limit.
pub(crate) fn section(loaded: &Loaded, next: &Value) -> Result<Value, String> {
    let mut value = read(loaded, next)?;
    if value["available"] != true || value["profile"]["complete"] == true {
        return Ok(value);
    }
    let name = next["agent"].as_str().ok_or("current recipient missing")?;
    let agent = loaded
        .agent(name)
        .ok_or("current recipient not configured")?;
    let bytes = path::secure_bytes(&loaded.control_root, &agent.profile, "recipient profile")?;
    if bytes.len() > 64 * 1024 || hash::bytes(&bytes) != value["profile"]["sha256"] {
        return Err("recipient profile changed or exceeds the complete section bound".into());
    }
    let content = String::from_utf8(bytes).map_err(|_| "recipient profile is not UTF-8")?;
    value["profile"]["content"] = json!(content);
    value["profile"]["complete"] = json!(true);
    value["profile"]["access"] = Value::Null;
    value["profile"]["reason"] = Value::Null;
    value["complete"] = json!(true);
    Ok(value)
}

pub(super) fn read(loaded: &Loaded, next: &Value) -> Result<Value, String> {
    let Some(name) = next["agent"].as_str() else {
        return Ok(json!({"complete": true, "available": false}));
    };
    let agent = loaded
        .agent(name)
        .ok_or("current recipient is not configured")?;
    let bytes = path::secure_bytes(&loaded.control_root, &agent.profile, "recipient profile")?;
    let digest = hash::bytes(&bytes);
    if next["packet"]["profile"]["sha256"].is_string()
        && next["packet"]["profile"]["sha256"] != digest
    {
        return Err(
            "recipient profile changed during work detail; refresh the current assignment".into(),
        );
    }
    let content = if bytes.len() <= MAX_PROFILE_BYTES {
        std::str::from_utf8(&bytes).ok()
    } else {
        None
    };
    let profile = json!({"path": agent.profile, "sha256": digest, "bytes": bytes.len(),
        "complete": content.is_some(), "content": content,
        "reason": if content.is_some() { Value::Null } else { json!("oversized_or_non_utf8") },
        "access": if content.is_some() { Value::Null } else { json!({
            "command": super::super::response_recovery::bounded_argv(
                vec!["profile".into(), name.into(), "--config".into()], loaded.path.to_str(), 1024).argv,
            "readOnly": true, "sameConfigRequired": true, "sameExecutableRequired": true}) }});
    let rules = agent_rules::detail_projection(&agent_context::current_rules(loaded)?)?;
    let mut result = json!({"complete": content.is_some(), "available": true,
        "agent": name, "role": next["role"], "nativeName": agent.native_name(name),
        "displayName": agent.display_name.as_deref().unwrap_or(name), "purpose":agent.purpose,
        "configurationSha256": hash::text(&loaded.source), "profile": profile,
        "rules": rules,
        "declaredBoundary": next["packet"]["declaredBoundary"],
        "authority": "read-only delivery; declarations do not grant host permissions"});
    if let Some(architecture) = crate::project::architecture::delivery(loaded, &next["packet"])? {
        result["architectureContract"] = architecture;
    }
    if let Some(lessons) = crate::memory::lessons::delivery(
        loaded,
        name,
        next["packet"]["goal"].as_str().unwrap_or(""),
    )? {
        result["projectLessons"] = lessons;
    }
    Ok(result)
}

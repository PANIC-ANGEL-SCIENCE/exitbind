//! Current recipient instructions for the existing readable detail owner.
use crate::{
    config::Loaded,
    evidence::hash,
    project::{agent_context, path},
};
use serde_json::{json, Value};

const MAX_PROFILE_BYTES: usize = 16 * 1024;

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
    let rules = agent_context::current_rules(loaded)?
        .into_iter()
        .map(|(path, sha256, content)| {
            json!({"path": path, "sha256": sha256, "bytes": content.len(), "content": content,
            "complete": true})
        })
        .collect::<Vec<_>>();
    Ok(json!({"complete": content.is_some(), "available": true,
        "agent": name, "role": next["role"], "nativeName": agent.native_name(name),
        "configurationSha256": hash::text(&loaded.source), "profile": profile,
        "rules": rules,
        "declaredBoundary": next["packet"]["declaredBoundary"],
        "authority": "read-only delivery; declarations do not grant host permissions"}))
}

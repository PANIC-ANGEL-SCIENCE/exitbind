//! Current, role-scoped context delivered to a native assignment.
//!
//! This is a launch-time projection.  It carries no authority and never
//! substitutes for the assignment packet or the configured profile.  Every
//! instruction and memory source is delivered as complete UTF-8 bytes, with a
//! bounded refusal when the complete projection cannot fit.

use crate::{config::Loaded, evidence::hash, memory, project::path};
use serde_json::{json, Value};

const RULES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
const MAX_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_RULE_BYTES: usize = 16 * 1024;
const MAX_MEMORY_ITEM_BYTES: usize = 16 * 1024;
const MAX_MEMORY_ITEMS: usize = 8;
const MAX_REFERENCE_BYTES: usize = 8 * 1024;

/// The provider binding requested for this particular native launch.  It is
/// deliberately separate from the configured role and the Work identity.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestedBinding<'a> {
    pub(crate) host: &'a str,
    pub(crate) model: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) sandbox: Option<&'a str>,
}

/// Build the complete current context for one native assignment and verify the
/// assignment has not advanced while the source bytes were being read.
pub(crate) fn project(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    binding: RequestedBinding<'_>,
) -> Result<String, String> {
    let role = current["role"]
        .as_str()
        .ok_or("native assignment has no role")?;
    let agent_name = current["agent"]
        .as_str()
        .ok_or("native assignment has no configured agent")?;
    let agent = loaded
        .agent(agent_name)
        .ok_or_else(|| format!("configured agent '{agent_name}' is unavailable"))?;
    if current["action"] != "spawn"
        || current["packet"]["role"] != role
        || current["packet"]["agent"] != agent_name
    {
        return Err(
            "native assignment role or agent no longer matches the configured project".into(),
        );
    }

    let assignment = current["assignment"]
        .as_str()
        .ok_or("native assignment has no assignment identity")?;
    let packet = &current["packet"];
    let packet_sha256 = hash::value(packet);
    verify_current_work(loaded, work, current, assignment, &packet_sha256)?;

    let rules = current_rules(loaded)?;
    let references = memory::selection::resolve(loaded, agent_name)?;
    if references.len() > MAX_MEMORY_ITEMS {
        return Err(format!(
            "native current context has {} eligible memory items; the limit is {MAX_MEMORY_ITEMS}; use a smaller role-scoped selection",
            references.len()
        ));
    }
    let memory = current_memory(loaded, &references)?;
    let project = project_value(loaded)?;
    let role_value = json!({
        "agentId": agent_name,
        "nativeName": agent.native_name(agent_name),
        "role": role,
        "purpose": agent.purpose,
        "profilePath": agent.profile,
        "profileSha256": packet["profile"]["sha256"],
        "memoryRead": agent.memory_read,
        "crossContext": agent.cross_context,
    });
    let work_value = json!({
        "handle": work,
        "assignment": assignment,
        "stage": current["packet"]["stage"],
        "attempt": current["packet"]["attempt"],
        "role": role,
        "agent": agent_name,
        "packetSha256": packet_sha256,
    });
    let binding_value = json!({
        "host": binding.host,
        "model": binding.model,
        "reasoningEffort": binding.reasoning_effort,
        "sandbox": binding.sandbox,
    });

    let mut text = String::new();
    push_section(
        &mut text,
        "NATIVE CURRENT CONTEXT",
        &json!({
            "meaning": "launch-time read-only projection; it carries no authority and does not replace the profile or assignment packet",
            "configuredRole": role_value,
            "project": project,
            "currentWork": work_value,
            "requestedNativeBinding": binding_value,
        }),
    )?;
    text.push_str("\nCURRENT PROJECT RULES (complete current UTF-8 bytes):\n");
    if rules.is_empty() {
        text.push_str("(none)\n");
    } else {
        for (path_name, digest, content) in &rules {
            append_block(&mut text, "RULE", path_name, digest, content)?;
        }
    }
    text.push_str("\nELIGIBLE ROLE-SCOPED OPTED-IN MEMORY (complete current UTF-8 bytes):\n");
    if memory.is_empty() {
        text.push_str("(none)\n");
    } else {
        for (reference, content) in memory {
            let scope = reference["scope"].as_str().ok_or("invalid memory scope")?;
            let source = reference["sourcePath"]
                .as_str()
                .ok_or("invalid memory source path")?;
            let digest = reference["sourceSha256"]
                .as_str()
                .ok_or("invalid memory source hash")?;
            append_block(
                &mut text,
                &format!("MEMORY [{scope}]"),
                source,
                digest,
                &content,
            )?;
            let reference_line = format!(
                "  reference: {}\n",
                serde_json::to_string(&reference).map_err(|error| error.to_string())?
            );
            push_complete(&mut text, &reference_line)?;
        }
    }

    // Re-check both the packet and the role-scoped memory after all reads.  A
    // revoked item, changed source, or advanced Work therefore fails closed
    // before a provider process can start.
    let fresh = crate::work::next(loaded, work)?;
    let fresh_next = &fresh["next"];
    if fresh_next["action"] != "spawn"
        || fresh_next["assignment"] != assignment
        || hash::value(&fresh_next["packet"]) != packet_sha256
    {
        return Err(
            "native current context changed before launch; refresh the current Work assignment"
                .into(),
        );
    }
    if current_rules(loaded)? != rules {
        return Err(
            "project rules changed before native launch; refresh the assignment instead of using stale instructions".into(),
        );
    }
    let fresh_references = memory::selection::resolve(loaded, agent_name)?;
    if fresh_references != references {
        return Err(
            "role-scoped project memory changed before native launch; refresh the assignment"
                .into(),
        );
    }
    Ok(text)
}

fn verify_current_work(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    assignment: &str,
    packet_sha256: &str,
) -> Result<(), String> {
    let fresh = crate::work::next(loaded, work)?;
    let fresh_next = &fresh["next"];
    if fresh_next["action"] != "spawn"
        || fresh_next["assignment"] != assignment
        || hash::value(&fresh_next["packet"]) != packet_sha256
        || fresh_next["agent"] != current["agent"]
        || fresh_next["role"] != current["role"]
    {
        return Err("native assignment is no longer the current Work action".into());
    }
    Ok(())
}

fn project_value(loaded: &Loaded) -> Result<Value, String> {
    let product_root = loaded
        .product_root
        .to_str()
        .ok_or("project product root is not valid UTF-8")?;
    let control_root = loaded
        .control_root
        .to_str()
        .ok_or("project control root is not valid UTF-8")?;
    let state_root = loaded
        .state_root
        .to_str()
        .ok_or("project state root is not valid UTF-8")?;
    Ok(json!({
        "identity": {"configuredId": loaded.project_id, "location": product_root},
        "configuredId": loaded.project_id,
        "productRoot": product_root,
        "controlRoot": control_root,
        "stateRoot": state_root,
    }))
}

fn current_rules(loaded: &Loaded) -> Result<Vec<(String, String, String)>, String> {
    let mut rules = Vec::new();
    for name in RULES {
        let bytes = match path::secure_bytes_observation(&loaded.product_root, name, "project rule")
        {
            path::SecureBytesResult::Bytes(bytes) => bytes,
            path::SecureBytesResult::Absent(_) => continue,
            path::SecureBytesResult::Unsafe(reason)
            | path::SecureBytesResult::Unreadable(reason) => return Err(reason),
            #[cfg(not(unix))]
            path::SecureBytesResult::Unsupported(reason) => return Err(reason),
        };
        if bytes.len() > MAX_RULE_BYTES {
            return Err(format!(
                "project rule {name} exceeds the {MAX_RULE_BYTES}-byte native delivery bound; launch refused so instructions are not truncated"
            ));
        }
        let content = String::from_utf8(bytes.clone())
            .map_err(|_| format!("project rule {name} is not UTF-8; launch refused"))?;
        rules.push(((*name).to_owned(), hash::bytes(&bytes), content));
    }
    Ok(rules)
}

fn current_memory(loaded: &Loaded, references: &[Value]) -> Result<Vec<(Value, String)>, String> {
    let mut result = Vec::with_capacity(references.len());
    for reference in references {
        let source = reference["sourcePath"]
            .as_str()
            .ok_or("invalid memory source path")?;
        let expected = reference["sourceSha256"]
            .as_str()
            .ok_or("invalid memory source hash")?;
        let bytes = path::secure_bytes(&loaded.product_root, source, "memory source")?;
        if bytes.len() > MAX_MEMORY_ITEM_BYTES {
            return Err(format!(
                "memory source {source} exceeds the {MAX_MEMORY_ITEM_BYTES}-byte native delivery bound; launch refused so rules are not truncated"
            ));
        }
        if hash::bytes(&bytes) != expected {
            return Err(format!(
                "memory source changed before native launch: {source}"
            ));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| format!("memory source {source} is not UTF-8; launch refused"))?;
        let reference_bytes = serde_json::to_vec(reference).map_err(|error| error.to_string())?;
        if reference_bytes.len() > MAX_REFERENCE_BYTES {
            return Err(format!(
                "memory reference {source} exceeds the {MAX_REFERENCE_BYTES}-byte native delivery bound"
            ));
        }
        result.push((reference.clone(), content));
    }
    Ok(result)
}

fn push_section(text: &mut String, title: &str, value: &Value) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    push_complete(text, &format!("{title}:\n{serialized}\n"))
}

fn append_block(
    text: &mut String,
    kind: &str,
    path_name: &str,
    digest: &str,
    content: &str,
) -> Result<(), String> {
    push_complete(
        text,
        &format!("\n{kind} {path_name} (sha256 {digest}, complete):\n{content}\n"),
    )
}

fn push_complete(text: &mut String, addition: &str) -> Result<(), String> {
    if text.len().saturating_add(addition.len()) > MAX_CONTEXT_BYTES {
        return Err(format!(
            "native current context exceeds the {MAX_CONTEXT_BYTES}-byte bound; launch refused so project rules and memory are not silently truncated"
        ));
    }
    text.push_str(addition);
    Ok(())
}

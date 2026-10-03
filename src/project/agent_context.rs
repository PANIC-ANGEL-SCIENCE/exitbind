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

/// The context is split before serialization so callers cannot accidentally
/// place fixed guidance after volatile assignment bindings by searching for a
/// marker inside user supplied rule or profile bytes.
pub(crate) struct ContextParts {
    pub(crate) stable: String,
    pub(crate) volatile: String,
}

pub(crate) fn project_parts(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    binding: RequestedBinding<'_>,
) -> Result<ContextParts, String> {
    project_parts_after_reads(loaded, work, current, binding, || Ok(()))
}

fn project_parts_after_reads(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    binding: RequestedBinding<'_>,
    after_reads: impl FnOnce() -> Result<(), String>,
) -> Result<ContextParts, String> {
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

    let mut stable = String::new();
    stable.push_str(
        "CURRENT PROJECT RULES (complete current UTF-8 bytes; stable scoped guidance):\n",
    );
    if rules.is_empty() {
        stable.push_str("(none)\n");
    } else {
        for (path_name, digest, content) in &rules {
            append_block(&mut stable, "RULE", path_name, digest, content)?;
        }
    }
    let mut volatile = String::new();
    push_section(
        &mut volatile,
        "NATIVE CURRENT CONTEXT",
        &json!({
            "meaning": "launch-time read-only projection; it carries no authority and does not replace the profile or assignment packet",
            "configuredRole": role_value,
            "project": project,
            "currentWork": work_value,
            "requestedNativeBinding": binding_value,
        }),
    )?;
    volatile.push_str("\nELIGIBLE ROLE-SCOPED OPTED-IN MEMORY (complete current UTF-8 bytes):\n");
    if memory.is_empty() {
        volatile.push_str("(none)\n");
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
                &mut volatile,
                &format!("MEMORY [{scope}]"),
                source,
                digest,
                &content,
            )?;
            let reference_line = format!(
                "  reference: {}\n",
                serde_json::to_string(&reference).map_err(|error| error.to_string())?
            );
            push_complete(&mut volatile, &reference_line)?;
        }
    }

    // Re-check both the packet and the role-scoped memory after all reads.  A
    // revoked item, changed source, or advanced Work therefore fails closed
    // before a provider process can start.
    after_reads()?;
    let fresh = crate::work::next(loaded, work)?;
    let fresh_next = &fresh["next"];
    let fresh_rules = current_rules(loaded)?;
    let fresh_references = memory::selection::resolve(loaded, agent_name)?;
    validate_fresh_context(
        assignment,
        &packet_sha256,
        fresh_next,
        &rules,
        &fresh_rules,
        &references,
        &fresh_references,
    )?;
    if stable.len().saturating_add(volatile.len()) > MAX_CONTEXT_BYTES {
        return Err(format!(
            "native current context exceeds the {MAX_CONTEXT_BYTES}-byte bound; launch refused so project rules and memory are not silently truncated"
        ));
    }
    Ok(ContextParts { stable, volatile })
}

fn validate_fresh_context(
    assignment: &str,
    packet_sha256: &str,
    fresh_next: &Value,
    rules: &[(String, String, String)],
    fresh_rules: &[(String, String, String)],
    references: &[Value],
    fresh_references: &[Value],
) -> Result<(), String> {
    if fresh_next["action"] != "spawn"
        || fresh_next["assignment"] != assignment
        || hash::value(&fresh_next["packet"]) != packet_sha256
    {
        return Err(
            "native current context changed before launch; refresh the current Work assignment"
                .into(),
        );
    }
    if fresh_rules != rules {
        return Err(
            "project rules changed before native launch; refresh the assignment instead of using stale instructions".into(),
        );
    }
    if fresh_references != references {
        return Err(
            "role-scoped project memory changed before native launch; refresh the assignment"
                .into(),
        );
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::validate_fresh_context;
    use crate::evidence::hash;
    use serde_json::json;

    fn baseline() -> (
        String,
        String,
        serde_json::Value,
        Vec<(String, String, String)>,
        Vec<serde_json::Value>,
    ) {
        let packet = json!({"role":"worker", "agent":"worker", "stage":"implementation"});
        let packet_sha = hash::value(&packet);
        let next = json!({"action":"spawn", "assignment":"assignment-1", "packet":packet});
        let rules = vec![(
            "AGENTS.md".to_owned(),
            "rule-sha".to_owned(),
            "stable rule bytes".to_owned(),
        )];
        let references = vec![
            json!({"sourcePath":"memory/current.md", "sourceSha256":"memory-sha", "scope":"invariants"}),
        ];
        (
            "assignment-1".to_owned(),
            packet_sha,
            next,
            rules,
            references,
        )
    }

    #[test]
    fn synchronized_work_rule_and_memory_changes_fail_at_the_changed_boundary() {
        let (assignment, packet_sha, next, rules, references) = baseline();
        assert!(validate_fresh_context(
            &assignment,
            &packet_sha,
            &next,
            &rules,
            &rules,
            &references,
            &references,
        )
        .is_ok());

        let mut changed_work = next.clone();
        changed_work["assignment"] = json!("assignment-2");
        let error = validate_fresh_context(
            &assignment,
            &packet_sha,
            &changed_work,
            &rules,
            &rules,
            &references,
            &references,
        )
        .unwrap_err();
        assert!(error.contains("current Work assignment"));

        let mut changed_rules = rules.clone();
        changed_rules[0].2 = "changed rule bytes".to_owned();
        let error = validate_fresh_context(
            &assignment,
            &packet_sha,
            &next,
            &rules,
            &changed_rules,
            &references,
            &references,
        )
        .unwrap_err();
        assert!(error.contains("project rules changed"));

        let mut changed_memory = references.clone();
        changed_memory[0]["sourceSha256"] = json!("changed-memory-sha");
        let error = validate_fresh_context(
            &assignment,
            &packet_sha,
            &next,
            &rules,
            &rules,
            &references,
            &changed_memory,
        )
        .unwrap_err();
        assert!(error.contains("project memory changed"));
    }
    #[test]
    fn actual_context_reads_keep_binding_changes_volatile_and_refuse_mid_use_rule_change() {
        use super::{project_parts, project_parts_after_reads, RequestedBinding};
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "exitbind-context-mid-use-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let rule = root.join("AGENTS.md");
        std::fs::write(&rule, "stable rule with NATIVE CURRENT CONTEXT text\n").unwrap();
        let configuration = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: root.to_str().unwrap(),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .unwrap();
        let loaded = crate::config::load(configuration.to_str()).unwrap();
        let started = crate::work::begin(
            &loaded,
            crate::work::BeginOptions {
                workflow: "change",
                goal: "exercise source currentness",
                check_command: "true",
                boundary: None,
                harness_receipt: None,
                proof_origin: None,
                preserve_requirement: None,
                preservation_check_command: None,
                preservation_proof_origin: None,
                basis: None,
                review_policy: Some("omitted"),
            },
        )
        .unwrap();
        let work = started["work"].as_str().unwrap();
        let scope = crate::work::next(&loaded, work).unwrap();
        let scoped = crate::work::return_result_with(
            &loaded,
            work,
            scope["next"]["assignment"].as_str().unwrap(),
            "scoped",
            None,
            None,
            None,
            Some(b"Exercise current project rule reads in a worker assignment.\n".to_vec()),
            scope["current"]["binding"].as_str(),
        )
        .unwrap();
        assert_eq!(scoped["effect"], "recorded");
        let view = crate::work::next(&loaded, work).unwrap();
        let current = &view["next"];
        let binding = RequestedBinding {
            host: "codex",
            model: Some("fixture-one"),
            reasoning_effort: None,
            sandbox: None,
        };
        let first = project_parts(&loaded, work, current, binding).unwrap();
        let second = project_parts(
            &loaded,
            work,
            current,
            RequestedBinding {
                model: Some("fixture-two"),
                ..binding
            },
        )
        .unwrap();
        assert_eq!(first.stable, second.stable);
        assert_ne!(first.volatile, second.volatile);
        assert!(first
            .stable
            .contains("stable rule with NATIVE CURRENT CONTEXT text"));
        // Runs after authoritative initial reads and immediately before the
        // normal final recheck; production supplies only a no-op callback.
        let result = project_parts_after_reads(&loaded, work, current, binding, || {
            std::fs::write(&rule, "changed policy bytes\n").map_err(|error| error.to_string())
        });
        assert!(
            result.is_err(),
            "mid-use source drift must refuse before provider launch"
        );
        let fresh_view = crate::work::next(&loaded, work).unwrap();
        let changed = project_parts(&loaded, work, &fresh_view["next"], binding).unwrap();
        assert_ne!(first.stable, changed.stable);
        assert!(changed.stable.contains("changed policy bytes"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

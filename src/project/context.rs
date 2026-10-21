//! Read-only project facts shared by the native session hook and JIT CLI.
//! This projection never promotes a file, focus pointer, or observation into authority.

use crate::{config::Loaded, evidence::hash, memory, project::path};
use serde_json::{json, Value};

const RULES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

pub(crate) fn snapshot(loaded: &Loaded) -> Value {
    snapshot_for_task(loaded, None)
}

pub(crate) fn snapshot_for_task(loaded: &Loaded, task: Option<&str>) -> Value {
    let identity = match loaded.project_id.as_deref() {
        Some(id) => json!({"kind":"configured_id", "id":id}),
        None => json!({"kind":"location", "id":null}),
    };
    let focus = match crate::work::focus::read(loaded) {
        Ok(crate::work::focus::Focus::Absent) => json!({"state":"none"}),
        Ok(crate::work::focus::Focus::Work(work))
            if crate::work::resolve(loaded, &work).is_ok() =>
        {
            json!({"state":"work", "work":work})
        }
        Ok(crate::work::focus::Focus::Work(_)) => {
            json!({"state":"unavailable", "reason":"focused work is unavailable"})
        }
        Ok(crate::work::focus::Focus::Unusable(reason)) => {
            json!({"state":"unavailable", "reason":reason})
        }
        Err(reason) => json!({"state":"unavailable", "reason":reason}),
    };
    let lead = loaded.lead().and_then(|id| {
        loaded.agent(id).map(|agent| {
            let profile = path::secure_bytes_observation(
                &loaded.control_root,
                &agent.profile,
                "lead profile",
            );
            let profile_hash = match profile {
                path::SecureBytesResult::Bytes(ref bytes) => Some(hash::bytes(bytes)),
                _ => None,
            };
            json!({"agentId":id, "nativeName":agent.native_name(id),
            "displayName":agent.display_name.as_deref().unwrap_or(id), "purpose":agent.purpose,
            "profilePath":agent.profile, "profileSha256":profile_hash})
        })
    });
    let rules = RULES.iter().map(|name| {
        match path::secure_bytes_observation(&loaded.product_root, name, "project rule") {
            path::SecureBytesResult::Bytes(bytes) => if std::str::from_utf8(&bytes).is_ok() {
                json!({"path":name, "state":"current", "sha256":hash::bytes(&bytes), "bytes":bytes.len()})
            } else {
                json!({"path":name,"state":"unavailable","reason":"rule is not UTF-8"})
            },
            path::SecureBytesResult::Absent(_) => json!({"path":name,"state":"absent"}),
            path::SecureBytesResult::Unsafe(reason)
            | path::SecureBytesResult::Unreadable(reason) =>
                json!({"path":name,"state":"unavailable","reason":reason}),
            #[cfg(not(unix))]
            path::SecureBytesResult::Unsupported(reason) =>
                json!({"path":name,"state":"unavailable","reason":reason}),
        }
    }).collect::<Vec<_>>();
    let memory = match loaded.lead() {
        Some(id) => match memory::selection::resolve_for_task(loaded, id, task) {
            Ok(references) => json!({"state":"current", "references":references,
                "selection":memory::selection::diagnostics(loaded,id).ok()}),
            Err(reason) => json!({"state":"unavailable", "reason":reason, "references":[]}),
        },
        None => json!({"state":"unavailable", "reason":"no configured lead", "references":[]}),
    };
    let resources = crate::host::project_resources::observe(&loaded.product_root);
    let mut snapshot = json!({
        "version":1,
        "project":{"identity":identity, "productRoot":loaded.product_root,
            "controlRoot":loaded.control_root,
            "memoryIdentity":crate::host::assignment_context::project_identity(loaded).ok()},
        "placement":crate::project::portability::diagnostic(&loaded.product_root, &loaded.state_root, &loaded.control_root),
        "focus":focus, "lead":lead, "rules":rules, "memory":memory,
        "resources":resources,
        "integration":{"version":env!("CARGO_PKG_VERSION"),
            "build":crate::producer::build_identity()},
    });
    if !loaded.capabilities.is_empty() {
        snapshot["capabilities"] = json!({"state":"registered", "count":loaded.capabilities.len(),
            "route":crate::emitted::command_line(&["project", "capabilities", "--task", "TASK", "--host", "HOST", "--json"], Some(&loaded.path)),
            "meaning":"read current applicability; no launch, acknowledgement or live refresh inferred"});
    }
    match super::architecture::reference(loaded) {
        Ok(Some(reference)) => {
            snapshot["architectureContract"] = json!({"state": "current", "provenance": reference})
        }
        Ok(None) => {}
        Err(reason) => {
            snapshot["architectureContract"] = json!({"state": "unavailable", "reason": reason})
        }
    }
    snapshot
}

/// Full current content is read only for an eligible reference and checked again.
pub(crate) fn memory_content(
    loaded: &Loaded,
    item_id: &str,
    task: Option<&str>,
) -> Result<Value, String> {
    let lead = loaded.lead().ok_or("no configured lead")?;
    let reference = memory::selection::resolve_for_task(loaded, lead, task)?
        .into_iter()
        .find(|entry| entry["itemId"] == item_id)
        .ok_or("memory item is not currently eligible")?;
    let source = reference["sourcePath"]
        .as_str()
        .ok_or("invalid memory reference")?;
    let expected = reference["sourceSha256"]
        .as_str()
        .ok_or("invalid memory reference")?;
    let bytes = path::secure_bytes(&loaded.product_root, source, "memory source")?;
    if hash::bytes(&bytes) != expected {
        return Err("memory source changed; refresh project context".into());
    }
    if bytes.len() > 64 * 1024 {
        return Err(
            "memory item exceeds the bounded detail route; read its named source file".into(),
        );
    }
    let still_current = memory::selection::resolve_for_task(loaded, lead, task)?
        .into_iter()
        .any(|entry| entry == reference);
    if !still_current {
        return Err("memory item changed during retrieval".into());
    }
    let content = String::from_utf8(bytes).map_err(|_| "memory source is not UTF-8")?;
    Ok(json!({"reference":reference, "content":content}))
}

/// Read one current item for the named configured recipient. The selector's
/// role rights and current task/host choice remain the only eligibility proof.
pub(crate) fn memory_content_for_recipient(
    loaded: &Loaded,
    item_id: &str,
    agent_id: &str,
    task: &str,
    host: Option<&str>,
    source_sha256: &str,
) -> Result<Value, String> {
    if let Some(host) = host {
        if !matches!(host, "codex" | "claude") {
            return Err("recipient memory host must be codex or claude".into());
        }
    }
    loaded
        .agent(agent_id)
        .ok_or_else(|| format!("unknown configured recipient '{agent_id}'"))?;
    let reference = memory::selection::resolve_for_task(loaded, agent_id, Some(task))?
        .into_iter()
        .find(|entry| entry["itemId"] == item_id)
        .ok_or("memory item is not currently eligible for this recipient and task")?;
    if reference.get("contextSelection").is_some() {
        let host = host.ok_or("contextual recipient memory requires an exact --host binding")?;
        memory::choices::assert_host(loaded, agent_id, task, host)?;
        if !crate::project::agent_context::contextual_host_is_pinned(loaded, agent_id, task, host) {
            return Err("contextual recipient host is not the exact configured host".into());
        }
    }
    if reference["sourceSha256"].as_str() != Some(source_sha256) {
        return Err("memory source binding does not match the current eligible item".into());
    }
    let source = reference["sourcePath"]
        .as_str()
        .ok_or("invalid memory reference")?;
    let bytes = path::secure_bytes(&loaded.product_root, source, "memory source")?;
    if hash::bytes(&bytes) != source_sha256 {
        return Err("memory source changed; refresh recipient context".into());
    }
    if bytes.len() > 64 * 1024 {
        return Err("memory item exceeds the bounded detail route".into());
    }
    if reference.get("contextSelection").is_some() {
        let host = host.ok_or("contextual recipient memory requires an exact --host binding")?;
        memory::choices::assert_host(loaded, agent_id, task, host)?;
        if !crate::project::agent_context::contextual_host_is_pinned(loaded, agent_id, task, host) {
            return Err("contextual recipient host changed during retrieval".into());
        }
    }
    let still_current = memory::selection::resolve_for_task(loaded, agent_id, Some(task))?
        .into_iter()
        .any(|current| current == reference);
    if !still_current {
        return Err("memory item changed during recipient retrieval".into());
    }
    let content = crate::project::agent_context::selected_content(&reference, bytes)?;
    Ok(json!({"reference":reference, "content":content}))
}

/// Fit complete references only; the host hook never cuts an instruction mid-byte.
pub(crate) fn session_addendum(loaded: &Loaded, budget: usize) -> String {
    let facts = snapshot(loaded);
    let mut text = String::new();
    let add = |text: &mut String, line: &str| {
        if text.len() + line.len() <= budget {
            text.push_str(line);
            true
        } else {
            false
        }
    };
    let context_route =
        crate::emitted::command_line(&["project", "context", "--json"], Some(&loaded.path));
    if !add(
        &mut text,
        &format!("\nCurrent project facts: `{context_route}`."),
    ) {
        return text;
    }
    let focus = &facts["focus"];
    let focus_line = match focus["state"].as_str() {
        Some("work") => format!(
            "\nCurrent work: {} (navigation only).",
            focus["work"].as_str().unwrap_or("?")
        ),
        Some("none") => "\nCurrent work: none.".to_owned(),
        _ => format!(
            "\nCurrent work: unavailable; use `{}` to diagnose.",
            crate::emitted::command_line(&["work", "resume"], Some(&loaded.path))
        ),
    };
    add(&mut text, &focus_line);
    let resources = &facts["resources"];
    let observed = |name: &str| {
        let field = &resources[name];
        if field["state"] != "known" {
            return "unknown".to_owned();
        }
        field["value"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| field["value"].as_u64().map(|value| value.to_string()))
            .unwrap_or_else(|| "unknown".to_owned())
    };
    let resource_line = format!(
        "\nHost resources: OS {}, arch {}, online logical CPUs {}, project available bytes {}.",
        observed("os"),
        observed("architecture"),
        observed("logicalCpuCount"),
        observed("filesystemAvailableBytes")
    );
    add(&mut text, &resource_line);
    if let Some(lead) = facts["lead"].as_object() {
        let line = format!(
            "\nLead profile: {} -> {} (sha256 {}).",
            lead["agentId"].as_str().unwrap_or("?"),
            lead["nativeName"].as_str().unwrap_or("?"),
            lead["profileSha256"].as_str().unwrap_or("unavailable")
        );
        add(&mut text, &line);
    }
    let mut omitted = 0usize;
    for rule in facts["rules"].as_array().into_iter().flatten() {
        if rule["state"] != "current" {
            continue;
        }
        let line = format!(
            "\nProject rule: {} (sha256 {}; read file for current content).",
            rule["path"].as_str().unwrap_or("?"),
            rule["sha256"].as_str().unwrap_or("?")
        );
        if !add(&mut text, &line) {
            omitted += 1;
        }
    }
    let mut read_route_added = false;
    for reference in facts["memory"]["references"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !read_route_added {
            // One read route for every item line, printed once.
            let route = crate::emitted::command_line(
                &[
                    "project",
                    "context",
                    "memory",
                    &crate::emitted::slot("ITEM_ID"),
                ],
                Some(&loaded.path),
            );
            read_route_added = add(
                &mut text,
                &format!("\nRead a project memory item: `{route}`."),
            );
        }
        let line = format!(
            "\nProject memory: {} [{}] {} (sha256 {}).",
            reference["itemId"].as_str().unwrap_or("?"),
            reference["scope"].as_str().unwrap_or("?"),
            reference["sourcePath"].as_str().unwrap_or("?"),
            reference["sourceSha256"].as_str().unwrap_or("?"),
        );
        if !add(&mut text, &line) {
            omitted += 1;
        }
    }
    if facts["memory"]["state"] == "unavailable" {
        add(
            &mut text,
            "\nProject memory unavailable; do not assume earlier rules.",
        );
    }
    if omitted > 0 {
        let line = format!("\n{omitted} more references; run `{context_route}`.");
        if !add(&mut text, &line) {
            // Drop the final optional line to preserve a complete detail route.
            if let Some(last) = text.rfind('\n') {
                text.truncate(last);
            }
            let _ = add(&mut text, &line);
        }
    }
    text
}

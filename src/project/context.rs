// Projects read-only project facts for native hooks and the CLI.
// Refuses memory source drift; see `src/project/mod.rs`.
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
            let profile_hash = path::secure_digest_observation(
                &loaded.control_root,
                &agent.profile,
                "lead profile",
            )
            .ok()
            .flatten();
            let access = profile_hash.as_deref().map(|digest| {
                let binding = lead_profile_binding(loaded, id, &agent.profile, digest);
                match lead_profile_access(loaded, &binding) {
                    Ok(access) => access,
                    Err(reason) => json!({"available":false, "reason":reason}),
                }
            });
            json!({"agentId":id, "nativeName":agent.native_name(id),
            "displayName":agent.display_name.as_deref().unwrap_or(id), "purpose":agent.purpose,
            "profilePath":agent.profile, "profileSha256":profile_hash, "profileAccess":access})
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
            "route":crate::emitted::command_line(&["project", "capabilities", "--task", &crate::emitted::slot("TASK"), "--host", &crate::emitted::slot("HOST"), "--json"], Some(&loaded.path)),
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

const LEAD_PROFILE_PAGE_BYTES: usize = 4 * 1024;

fn lead_profile_access(loaded: &Loaded, binding: &str) -> Result<Value, String> {
    Ok(json!({"binding":binding,
        "command":lead_profile_route(loaded, binding, 0)?, "readOnly":true}))
}

fn lead_profile_binding(loaded: &Loaded, agent: &str, profile: &str, digest: &str) -> String {
    hash::value(&json!({"version":1, "configPath":loaded.path,
        "configSha256":hash::text(&loaded.source), "controlRoot":loaded.control_root,
        "productRoot":loaded.product_root, "stateRoot":loaded.state_root,
        "projectId":loaded.project_id, "agent":agent, "profilePath":profile,
        "profileSha256":digest}))
}

fn lead_profile_route(loaded: &Loaded, binding: &str, offset: u64) -> Result<Vec<String>, String> {
    crate::emitted::Route::new([
        "project",
        "context",
        "lead-profile",
        binding,
        &offset.to_string(),
        "--json",
    ])
    .config(&loaded.path)
    .full_argv()
}

/// Deliver the selected Lead's current profile as complete, bound UTF-8 pages.
pub(crate) fn lead_profile_page(
    loaded: &Loaded,
    binding: &str,
    offset: u64,
) -> Result<Value, String> {
    let refresh =
        crate::emitted::command_line(&["project", "context", "--json"], Some(&loaded.path));
    let stale = |current: Option<&str>| {
        crate::drift::refusal(
            crate::drift::Class::LeadProfileBinding,
            binding,
            current,
            None,
            crate::drift::Route::ProjectContext {
                config: &loaded.path,
            },
        )
    };
    let name = loaded.lead().ok_or_else(|| stale(None))?;
    let agent = loaded.agent(name).ok_or_else(|| stale(None))?;
    let page = path::secure_page_observation(
        &loaded.control_root,
        &agent.profile,
        "lead profile",
        offset,
        LEAD_PROFILE_PAGE_BYTES,
    )
    .map_err(|reason| format!("{reason}; {}", stale(None)))?
    .ok_or_else(|| stale(None))?;
    let observed_binding = lead_profile_binding(loaded, name, &agent.profile, &page.sha256);
    if observed_binding != binding {
        return Err(stale(Some(&observed_binding)));
    }
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let current = crate::config::load(Some(config_path)).map_err(|_| stale(None))?;
    if current.source != loaded.source
        || current.control_root != loaded.control_root
        || current.product_root != loaded.product_root
        || current.state_root != loaded.state_root
        || current.project_id != loaded.project_id
    {
        let current_binding = current.lead().and_then(|id| {
            current.agent(id).and_then(|current_agent| {
                path::secure_digest_observation(
                    &current.control_root,
                    &current_agent.profile,
                    "lead profile",
                )
                .ok()
                .flatten()
                .map(|digest| lead_profile_binding(&current, id, &current_agent.profile, &digest))
            })
        });
        return Err(stale(current_binding.as_deref()));
    }
    let latest_digest =
        path::secure_digest_observation(&loaded.control_root, &agent.profile, "lead profile")
            .map_err(|reason| format!("{reason}; {}", stale(None)))?;
    if latest_digest.as_deref() != Some(page.sha256.as_str()) {
        let current_binding = latest_digest
            .as_deref()
            .map(|digest| lead_profile_binding(loaded, name, &agent.profile, digest));
        return Err(stale(current_binding.as_deref()));
    }
    if offset > page.total_bytes || (offset == page.total_bytes && offset != 0) {
        return Err(format!(
            "Lead profile page offset is outside the current profile; refresh with {refresh}"
        ));
    }
    let content = match std::str::from_utf8(&page.bytes) {
        Ok(text) => text,
        Err(error)
            if error.error_len().is_none()
                && offset + (page.bytes.len() as u64) < page.total_bytes =>
        {
            std::str::from_utf8(&page.bytes[..error.valid_up_to()]).expect("valid UTF-8 prefix")
        }
        Err(_) => {
            return Err(format!(
                "Lead profile is not complete UTF-8 at this offset; refresh with {refresh}"
            ))
        }
    };
    if content.is_empty() && offset < page.total_bytes {
        return Err(format!(
            "Lead profile page offset is not a UTF-8 boundary; refresh with {refresh}"
        ));
    }
    let end = offset + content.len() as u64;
    let next = if end < page.total_bytes {
        json!({"command":lead_profile_route(loaded, binding, end)?, "readOnly":true})
    } else {
        Value::Null
    };
    let response = json!({"kind":"lead_profile_page", "binding":binding, "agent":name,
        "profileSha256":page.sha256, "totalBytes":page.total_bytes, "offset":offset,
        "bytes":content.len(), "content":content, "pageComplete":true,
        "complete":offset == 0 && next.is_null(), "next":next,
        "authority":"read-only content; no permission, review or acceptance"});
    if serde_json::to_vec(&response)
        .map_err(|error| error.to_string())?
        .len()
        + 1
        > 64 * 1024
    {
        return Err(format!(
            "Lead profile page metadata exceeds the readable bound; refresh with {refresh}"
        ));
    }
    Ok(response)
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

/// Facts whose absence would conceal role provenance or unavailable memory.
pub(crate) fn session_required(loaded: &Loaded, facts: &Value) -> String {
    session_required_parts(loaded, facts).0
}

// Keep the insertion point from the constructed route itself: quoted paths
// may contain newlines, so a rendered command cannot be split by lines.
fn session_required_parts(loaded: &Loaded, facts: &Value) -> (String, usize) {
    let context_route =
        crate::emitted::command_line(&["project", "context", "--json"], Some(&loaded.path));
    let mut text = format!("\nCurrent project facts: `{context_route}`.");
    let facts_end = text.len();
    if let Some(lead) = facts["lead"].as_object() {
        text.push_str(&format!(
            "\nLead profile: {} -> {} (sha256 {}). Read fully before material choices.",
            lead["agentId"].as_str().unwrap_or("?"),
            lead["nativeName"].as_str().unwrap_or("?"),
            lead["profileSha256"].as_str().unwrap_or("unavailable")
        ));
    }
    if facts["memory"]["state"] == "unavailable" {
        text.push_str("\nProject memory unavailable; do not assume earlier rules.");
    }
    (text, facts_end)
}

fn omitted_line(count: usize) -> String {
    format!(" +{count} references via this.")
}

/// Reserve the complete pointer before adding optional reference details.
pub(crate) fn session_reference_reserve(facts: &Value) -> usize {
    let rules = facts["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|rule| rule["state"] == "current")
        .count();
    let memory = facts["memory"]["references"].as_array().map_or(0, Vec::len);
    if rules + memory == 0 {
        0
    } else {
        omitted_line(rules + memory).len()
    }
}

/// The first-page route is usable only after the same current binding check
/// that a page read performs.
pub(crate) fn session_profile_line(loaded: &Loaded, facts: &Value) -> Option<String> {
    let binding = facts["lead"]["profileAccess"]["binding"].as_str()?;
    lead_profile_page(loaded, binding, 0).ok()?;
    let route = crate::emitted::command_line(
        &["project", "context", "lead-profile", binding, "0", "--json"],
        Some(&loaded.path),
    );
    Some(format!(
        "\nRead Lead profile: `{route}`. Reuse complete bytes acquired for this verified binding."
    ))
}

/// Fit complete references only; the host hook never cuts an instruction mid-byte.
pub(crate) fn session_addendum(
    loaded: &Loaded,
    facts: &Value,
    budget: usize,
    goal_line: Option<&str>,
    profile_line: Option<&str>,
) -> String {
    let (mut text, facts_end) = session_required_parts(loaded, facts);
    let reserve = session_reference_reserve(facts);
    let add = |text: &mut String, line: &str| {
        if text.len() + line.len() + reserve <= budget {
            text.push_str(line);
            true
        } else {
            false
        }
    };
    if let Some(line) = goal_line {
        add(&mut text, line);
    }
    if let Some(line) = profile_line {
        add(&mut text, line);
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
    let references = facts["memory"]["references"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // The pointer to omitted references is reserved before optional lines
    // spend the remaining budget and is attached to the complete facts route.
    let mut read_route_added = false;
    for reference in &references {
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
        if !read_route_added || !add(&mut text, &line) {
            omitted += 1;
        }
    }
    if omitted > 0 {
        let line = omitted_line(omitted);
        if text.len() + line.len() <= budget {
            text.insert_str(facts_end, &line);
        }
    }
    text
}

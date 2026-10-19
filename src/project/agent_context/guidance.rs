//! One validated, ordered projection of eligible memory and project lessons.
use crate::{config::Loaded, evidence::hash, memory, project::path};
use serde_json::{json, Value};

const MAX_ITEM_BYTES: usize = 16 * 1024;
type GuidanceItems = (Vec<Value>, Vec<Value>, Vec<Value>);

/// Read complete current source bytes. The selector keeps rights, task
/// association, revocation and ledger validation strict; unavailable optional
/// source bytes become a content-free notice.
fn guidance_items(
    loaded: &Loaded,
    agent: &str,
    task: Option<&str>,
    host: &str,
    scope_ceiling: Option<&[String]>,
    cross_context_ceiling: Option<&str>,
) -> Result<GuidanceItems, String> {
    let selection = match scope_ceiling {
        Some(ceiling) => memory::selection::resolve_for_task_optional_with_ceiling(
            loaded,
            agent,
            task,
            ceiling,
            cross_context_ceiling.unwrap_or("none"),
        )?,
        None => memory::selection::resolve_for_task_optional(loaded, agent, task)?,
    };
    if let Some(task) = task.filter(|_| {
        selection
            .references
            .iter()
            .any(|reference| reference.get("contextSelection").is_some())
            || selection
                .unavailable
                .iter()
                .any(|entry| entry["identity"].get("contextSelection").is_some())
    }) {
        memory::choices::assert_host(loaded, agent, task, host)?;
    }
    let references = selection.references;
    let mut items = Vec::with_capacity(references.len());
    let mut overflow = Vec::new();
    let mut unavailable = selection
        .unavailable
        .iter()
        .map(|item| json!({"itemId":item["identity"]["itemId"],"reason":item["reason"]}))
        .collect::<Vec<_>>();
    for reference in &references {
        if reference.get("contextSelection").is_some()
            && task.is_some()
            && !contextual_host_is_pinned(loaded, agent, task.unwrap_or_default(), host)
        {
            unavailable.push(unavailable_notice(
                reference,
                "contextual_host_unavailable_or_changed",
            ));
            continue;
        }
        let source = reference["sourcePath"]
            .as_str()
            .ok_or("eligible guidance has no source path")?;
        let expected = reference["sourceSha256"]
            .as_str()
            .ok_or("eligible guidance has no source hash")?;
        let byte_length = usize::try_from(
            reference["byteLength"]
                .as_u64()
                .ok_or("eligible guidance has no source byte length")?,
        )
        .map_err(|_| "eligible guidance byte length overflows this platform")?;
        if byte_length > MAX_ITEM_BYTES {
            overflow.push(overflow_item(
                loaded,
                agent,
                reference.clone(),
                task,
                host,
                byte_length,
                "item_size_limit",
            )?);
            continue;
        }
        match path::secure_bytes_observation(&loaded.product_root, source, "memory source") {
            path::SecureBytesResult::Bytes(bytes)
                if hash::bytes(&bytes) == expected && std::str::from_utf8(&bytes).is_ok() =>
            {
                let content = selected_content(reference, bytes)?;
                items.push(json!({"reference":reference,"content":content}));
            }
            path::SecureBytesResult::Absent(_) | path::SecureBytesResult::Unreadable(_) => {
                unavailable.push(unavailable_notice(
                    reference,
                    "source_unavailable_or_changed",
                ));
            }
            path::SecureBytesResult::Bytes(_) => {
                unavailable.push(unavailable_notice(
                    reference,
                    "source_unavailable_or_changed",
                ));
            }
            path::SecureBytesResult::Unsafe(error) => return Err(error),
            #[cfg(not(unix))]
            path::SecureBytesResult::Unsupported(error) => return Err(error),
        }
    }
    Ok((items, overflow, unavailable))
}

/// Version 1 choices do not record a host. A task-scoped expansion route can
/// therefore be emitted only when the configured recipient pins that host.
/// Version 2 retains its recorded choice-host check above.
pub(crate) fn contextual_host_is_pinned(
    loaded: &Loaded,
    agent: &str,
    task: &str,
    host: &str,
) -> bool {
    if memory::contextual::task_local(loaded) {
        return memory::choices::assert_host(loaded, agent, task, host).is_ok();
    }
    loaded
        .agent(agent)
        .and_then(|configured| configured.runtime.host.as_deref())
        == Some(host)
        && matches!(host, "codex" | "claude")
}

fn unavailable_notice(reference: &Value, reason: &str) -> Value {
    json!({"itemId":reference["itemId"],"reason":reason})
}

fn add_notice(notices: &mut Vec<Value>, notice: Value) {
    if !notices.contains(&notice) {
        notices.push(notice);
    }
}

/// Revalidate guidance after the rest of a launch context has been read. Any
/// source that disappeared, changed or lost eligibility is removed before it
/// reaches the host. This does not retry selection or widen a recipient's
/// rights; newly eligible items are reported for the next safe boundary.
pub(crate) fn retain_current(
    loaded: &Loaded,
    agent: &str,
    task: Option<&str>,
    host: &str,
    scope_ceiling: Option<&[String]>,
    cross_context_ceiling: Option<&str>,
    guidance: &mut Value,
) -> Result<(), String> {
    let current = match scope_ceiling {
        Some(ceiling) => memory::selection::resolve_for_task_optional_with_ceiling(
            loaded,
            agent,
            task,
            ceiling,
            cross_context_ceiling.unwrap_or("none"),
        )?,
        None => memory::selection::resolve_for_task_optional(loaded, agent, task)?,
    };
    if let Some(task) = task.filter(|_| {
        current
            .references
            .iter()
            .any(|reference| reference.get("contextSelection").is_some())
            || current
                .unavailable
                .iter()
                .any(|entry| entry["identity"].get("contextSelection").is_some())
    }) {
        memory::choices::assert_host(loaded, agent, task, host)?;
    }
    let mut notices = guidance["unavailable"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for field in ["items", "overflow"] {
        let entries = guidance[field]
            .as_array()
            .ok_or("guidance projection is malformed")?;
        let mut retained = Vec::with_capacity(entries.len());
        for entry in entries {
            let reference = &entry["reference"];
            if current
                .references
                .iter()
                .any(|candidate| candidate == reference)
            {
                retained.push(entry.clone());
            } else {
                add_notice(
                    &mut notices,
                    unavailable_notice(reference, "eligibility_or_source_changed_before_delivery"),
                );
            }
        }
        guidance[field] = Value::Array(retained);
    }
    for entry in current.unavailable {
        add_notice(
            &mut notices,
            json!({"itemId":entry["identity"]["itemId"],"reason":entry["reason"]}),
        );
    }
    let included: Vec<_> = guidance["items"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(guidance["overflow"].as_array().into_iter().flatten())
        .map(|entry| entry["reference"].clone())
        .collect();
    for reference in current.references {
        if !included.iter().any(|entry| entry == &reference)
            && !notices
                .iter()
                .any(|notice| notice["itemId"] == reference["itemId"])
        {
            add_notice(
                &mut notices,
                unavailable_notice(&reference, "selection_changed_before_delivery"),
            );
        }
    }
    let selected_count = guidance["items"].as_array().map_or(0, Vec::len);
    let overflow_count = guidance["overflow"].as_array().map_or(0, Vec::len);
    let used_bytes = guidance["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| serde_json::to_vec(item).map_or(0, |bytes| bytes.len()))
        .sum::<usize>();
    guidance["selectedCount"] = json!(selected_count);
    guidance["overflowCount"] = json!(overflow_count);
    guidance["unavailableCount"] = json!(notices.len());
    guidance["usedBytes"] = json!(used_bytes);
    guidance["unavailable"] = Value::Array(notices);
    Ok(())
}

pub(crate) fn selected_content(reference: &Value, bytes: Vec<u8>) -> Result<Value, String> {
    if reference["scope"] == memory::lessons::SCOPE {
        let lesson: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "selected project lesson is invalid JSON")?;
        Ok(json!({"id":lesson["id"],"fact":lesson["fact"],"owner":lesson["owner"]}))
    } else {
        let source = reference["sourcePath"]
            .as_str()
            .unwrap_or("selected guidance");
        String::from_utf8(bytes)
            .map(Value::String)
            .map_err(|_| format!("eligible guidance source {source} is not UTF-8"))
    }
}

fn overflow_item(
    loaded: &Loaded,
    agent: &str,
    reference: Value,
    task: Option<&str>,
    host: &str,
    bytes: usize,
    reason: &str,
) -> Result<Value, String> {
    let item_id = reference["itemId"]
        .as_str()
        .ok_or("eligible guidance has no item id")?;
    let Some(task) = task else {
        // Without a recipient task the only memory route would resolve as the
        // Lead, so no expansion is offered rather than another role's view.
        return Ok(json!({"reference":reference,"bytes":bytes,"reason":reason,
            "readRoute":null,"readRouteUnavailable":"recipient_task_required"}));
    };
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = vec![
        executable
            .to_str()
            .ok_or("executable path is not UTF-8")?
            .to_owned(),
        "project".into(),
        "context".into(),
        "memory".into(),
        item_id.into(),
    ];
    command.extend(["--agent".into(), agent.into(), "--task".into(), task.into()]);
    if !host.is_empty() {
        command.extend(["--host".into(), host.into()]);
    }
    command.extend([
        "--expected-sha256".into(),
        reference["sourceSha256"]
            .as_str()
            .ok_or("eligible guidance has no source hash")?
            .into(),
    ]);
    command.extend([
        "--json".into(),
        "--config".into(),
        loaded
            .path
            .to_str()
            .ok_or("configuration path is not UTF-8")?
            .into(),
    ]);
    Ok(json!({"reference":reference,"bytes":bytes,"reason":reason,
        "readRoute":{"command":command,"readOnly":true,"sameConfigRequired":true,
            "sameExecutableRequired":true}}))
}

/// Apply a recipient's byte budget without clipping items. Omitted entries
/// retain only their current selector reference and exact task-scoped route.
pub(crate) fn guidance(
    loaded: &Loaded,
    agent: &str,
    task: Option<&str>,
    host: &str,
    budget: usize,
) -> Result<Value, String> {
    guidance_with_ceiling(loaded, agent, task, host, budget, None, None)
}

pub(crate) fn guidance_for_assignment(
    loaded: &Loaded,
    agent: &str,
    task: Option<&str>,
    host: &str,
    budget: usize,
    packet: &Value,
) -> Result<Value, String> {
    let ceiling = if crate::run::guidance_continuity::enabled(packet) {
        Some(
            packet["frozenMemoryRead"]
                .as_array()
                .ok_or("marked assignment has no frozen memory-read ceiling")?
                .iter()
                .map(|scope| {
                    scope
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("marked assignment memory-read ceiling is malformed")
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    let cross_context = packet["frozenCrossContext"].as_str();
    guidance_with_ceiling(
        loaded,
        agent,
        task,
        host,
        budget,
        ceiling.as_deref(),
        cross_context,
    )
}

fn guidance_with_ceiling(
    loaded: &Loaded,
    agent: &str,
    task: Option<&str>,
    host: &str,
    budget: usize,
    scope_ceiling: Option<&[String]>,
    cross_context_ceiling: Option<&str>,
) -> Result<Value, String> {
    let (items, mut overflow, unavailable) = guidance_items(
        loaded,
        agent,
        task,
        host,
        scope_ceiling,
        cross_context_ceiling,
    )?;
    let mut selected = Vec::new();
    let mut used = 0usize;
    for item in items {
        let bytes = serde_json::to_vec(&item)
            .map_err(|error| error.to_string())?
            .len();
        if used.saturating_add(bytes) <= budget {
            used += bytes;
            selected.push(item);
            continue;
        }
        overflow.push(overflow_item(
            loaded,
            agent,
            item["reference"].clone(),
            task,
            host,
            bytes,
            "recipient_budget",
        )?);
    }
    let selected_count = selected.len();
    let overflow_count = overflow.len();
    let unavailable_count = unavailable.len();
    let mut result = json!({"items":selected,"overflow":overflow,"selectedCount":selected_count,
        "overflowCount":overflow_count,"unavailable":unavailable,
        "unavailableCount":unavailable_count,"budgetBytes":budget,"usedBytes":used,
        "meaning":"current eligible complete items; overflow is available through its role/task-scoped read route"});
    retain_current(
        loaded,
        agent,
        task,
        host,
        scope_ceiling,
        cross_context_ceiling,
        &mut result,
    )?;
    Ok(result)
}

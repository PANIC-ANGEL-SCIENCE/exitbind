//! Search current, recipient-authorized memory sources without an index.
//!
//! A generic accepted memory item is a source, not an assertion that it is
//! the provenance of any lesson. A lesson exposes only its recipient view.

use crate::{config::Loaded, evidence::hash, project::path, work};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// An actor can query all of its currently eligible sources without knowing a
/// source reference; the query text is the only task-specific input.
pub(crate) fn route(loaded: &Loaded, work: &str) -> Value {
    let query = format!("--query={}", crate::emitted::slot("LITERAL_QUERY"));
    let command = crate::emitted::Route::new(["memory", "query", work, query.as_str(), "--json"]);
    let mut route = crate::work::compact::continuation_route(&loaded.path, command);
    route["readOnly"] = json!(true);
    route["placeholder"] = json!("LITERAL_QUERY");
    route["meaning"] = json!("replace the LITERAL_QUERY placeholder inside --query= with the literal text; search current recipient-authorized originals and derived lessons without granting new rights");
    route
}

fn selection(
    loaded: &Loaded,
    agent: &str,
    task: &str,
    packet: &Value,
) -> Result<super::selection::OptionalResolution, String> {
    if !crate::run::guidance_continuity::enabled(packet) {
        return super::selection::resolve_for_task_optional(loaded, agent, Some(task));
    }
    let scopes = packet["frozenMemoryRead"]
        .as_array()
        .ok_or("marked assignment has no frozen memory-read ceiling")?
        .iter()
        .map(|scope| {
            scope
                .as_str()
                .map(str::to_owned)
                .ok_or("marked assignment memory-read ceiling is malformed")
        })
        .collect::<Result<Vec<_>, _>>()?;
    super::selection::resolve_for_task_optional_with_ceiling(
        loaded,
        agent,
        Some(task),
        &scopes,
        packet["frozenCrossContext"].as_str().unwrap_or("none"),
    )
}

fn assert_context_host(
    loaded: &Loaded,
    agent: &str,
    task: &str,
    host: &str,
    resolution: &super::selection::OptionalResolution,
) -> Result<(), String> {
    let contextual = resolution
        .references
        .iter()
        .any(|reference| reference.get("contextSelection").is_some())
        || resolution
            .unavailable
            .iter()
            .any(|item| item["identity"].get("contextSelection").is_some());
    if contextual {
        super::choices::assert_host(loaded, agent, task, host)?;
        if !crate::project::agent_context::contextual_host_is_pinned(loaded, agent, task, host) {
            return Err("contextual recipient host is not the exact configured host".into());
        }
    }
    Ok(())
}

fn source_route(
    loaded: &Loaded,
    agent: &str,
    task: &str,
    host: &str,
    reference: &Value,
) -> Result<Value, String> {
    let id = reference["itemId"]
        .as_str()
        .ok_or("memory item identity unavailable")?;
    let sha = reference["sourceSha256"]
        .as_str()
        .ok_or("memory source hash unavailable")?;
    let mut command = crate::emitted::Route::new([
        "project",
        "context",
        "memory",
        id,
        "--agent",
        agent,
        "--task",
        task,
        "--expected-sha256",
        sha,
        "--json",
    ]);
    if matches!(host, "codex" | "claude") {
        command = command.words(["--host", host]);
    }
    let mut route = crate::work::compact::continuation_route(&loaded.path, command);
    route["readOnly"] = json!(true);
    Ok(route)
}

/// Describe only the recipient's own policy and observations, never hidden items.
fn diagnostics(
    observation: &super::selection::QueryObservation,
    scope: &str,
    has_matches: bool,
    omitted: usize,
) -> Value {
    let source_requested = scope != "derived";
    let derived_requested = scope != "source";
    let source_allowed = source_requested && observation.source_allowed;
    let derived_allowed = derived_requested && observation.derived_allowed;
    let denied = (source_requested && !observation.source_allowed)
        || (derived_requested && !observation.derived_allowed);
    let stale = (source_requested && observation.source.stale)
        || (derived_requested && observation.derived.stale);
    let unavailable = (source_requested && observation.source.unavailable)
        || (derived_requested && observation.derived.unavailable);
    let ineligible = (source_requested && observation.source.ineligible)
        || (derived_requested && observation.derived.ineligible);
    let status = if !observation.enabled {
        "unavailable"
    } else if !source_allowed && !derived_allowed {
        "access_denied"
    } else if denied || stale || unavailable || ineligible || omitted != 0 {
        "incomplete"
    } else if has_matches {
        "matches"
    } else {
        "no_match"
    };
    let access = |requested: bool, allowed: bool| {
        if !requested {
            "not_requested"
        } else if !observation.enabled {
            "unavailable"
        } else if allowed {
            "allowed"
        } else {
            "denied"
        }
    };
    json!({"status":status,
        "scopeAccess":{"source":access(source_requested, observation.source_allowed),
            "derived":access(derived_requested, observation.derived_allowed)},
        "sourceStates":{"stale":stale,"unavailable":unavailable,"noLongerEligible":ineligible},
        "meaning":"no_match covers only fully observed authorized sources; denied scopes do not reveal whether hidden items exist"})
}

/// Search exactly the current pending recipient's eligible sources. The
/// existing memory policy bounds inputs; no new index or rights are created.
pub(crate) fn search(
    loaded: &Loaded,
    work_id: &str,
    query: &str,
    scope: &str,
) -> Result<Value, String> {
    if query.is_empty() || query.contains('\n') || query.contains('\r') {
        return Err("memory query requires non-empty single-line literal text".into());
    }
    if !matches!(scope, "source" | "derived" | "combined") {
        return Err("memory query --scope accepts source, derived, or combined".into());
    }
    let next = work::next(loaded, work_id)?;
    let action = &next["next"];
    if action["action"] != "spawn" {
        return Err("memory query requires a current pending recipient assignment".into());
    }
    let packet = &action["packet"];
    let agent = packet["agent"]
        .as_str()
        .ok_or("assignment agent unavailable")?;
    let task = packet["goal"]
        .as_str()
        .ok_or("assignment task unavailable")?;
    let host = packet["runtime"]["host"]
        .as_str()
        .or_else(|| {
            loaded
                .agent(agent)
                .and_then(|item| item.runtime.host.as_deref())
        })
        .unwrap_or("");
    let binding = action["current"]["binding"]
        .as_str()
        .ok_or("assignment binding unavailable")?;
    let selected = selection(loaded, agent, task, packet)?;
    assert_context_host(loaded, agent, task, host, &selected)?;
    // Use the existing configured memory budget for both input selection and
    // match bodies. Keep complete rows only and report omissions explicitly.
    let budget = super::policy::get(&loaded.config).map_or(0, |policy| policy.max_bytes);
    let mut used = 0usize;
    let mut omitted = 0usize;
    let mut matches: Vec<Value> = Vec::new();
    let mut by_text = BTreeMap::<String, usize>::new();
    let mut omitted_ids = BTreeSet::new();
    let mut omitted_sources = Vec::new();
    for reference in &selected.references {
        let kind = if reference["scope"] == super::lessons::SCOPE {
            "derived"
        } else {
            "source"
        };
        if scope != "combined" && scope != kind {
            continue;
        }
        let read_route = source_route(loaded, agent, task, host, reference)?;
        let path_name = reference["sourcePath"]
            .as_str()
            .ok_or("memory source path unavailable")?;
        let bytes = path::secure_bytes(&loaded.product_root, path_name, "memory query source")?;
        let expected = reference["sourceSha256"]
            .as_str()
            .ok_or("memory source hash unavailable")?;
        let current = hash::bytes(&bytes);
        if expected != current.as_str() {
            return Err(crate::drift::refusal(
                crate::drift::Class::MemorySource,
                expected,
                Some(&current),
                None,
                crate::drift::Route::Resolve {
                    agent,
                    config: &loaded.path,
                },
            ));
        }
        let content = crate::project::agent_context::selected_content(reference, bytes)?;
        let content = if kind == "derived" {
            content["fact"]
                .as_str()
                .ok_or("selected lesson fact unavailable")?
        } else {
            content.as_str().ok_or("selected source is not text")?
        };
        for (index, line) in content.lines().enumerate() {
            if line.contains(query) {
                let before = omitted;
                let provenance = json!({"kind":kind,"reference":reference,"line":index + 1,
                    "readRoute":read_route});
                if let Some(index) = by_text.get(line).copied() {
                    let size = serde_json::to_vec(&provenance)
                        .map_err(|e| e.to_string())?
                        .len()
                        + 1;
                    if used.saturating_add(size) <= budget {
                        matches[index]["provenance"]
                            .as_array_mut()
                            .expect("provenance array")
                            .push(provenance);
                        used += size;
                    } else {
                        omitted += 1;
                    }
                } else {
                    let result = json!({"text":line,"provenance":[provenance]});
                    let size = serde_json::to_vec(&result)
                        .map_err(|e| e.to_string())?
                        .len()
                        + 1;
                    if used.saturating_add(size) <= budget {
                        by_text.insert(line.to_owned(), matches.len());
                        matches.push(result);
                        used += size;
                    } else {
                        omitted += 1;
                    }
                }
                if omitted != before {
                    let id = reference["itemId"]
                        .as_str()
                        .ok_or("memory item identity unavailable")?;
                    if omitted_ids.insert(id.to_owned()) {
                        omitted_sources.push(
                            json!({"kind":kind,"reference":reference,"readRoute":read_route}),
                        );
                    }
                }
            }
        }
    }
    work::details::ensure_action_binding(loaded, work_id, Some(binding))?;
    let fresh = selection(loaded, agent, task, packet)?;
    assert_context_host(loaded, agent, task, host, &fresh)?;
    if fresh != selected {
        return Err("recipient memory eligibility changed during query".into());
    }
    let diagnostics = diagnostics(&selected.query, scope, !matches.is_empty(), omitted);
    Ok(
        json!({"kind":"recipient_memory_query","work":work_id,"assignment":action["assignment"],
        "agent":agent,"task":task,"host":host,"scope":scope,"query":query,
        "observedAt":chrono::Utc::now().to_rfc3339(),"matches":matches,
        "resultBytes":used,"resultByteBudget":budget,"omittedMatches":omitted,"omittedSources":omitted_sources,
        "unavailable":selected.unavailable,"diagnostics":diagnostics,
        "meaning":"literal case-sensitive line matches among currently eligible memory items; source items are independently authorized and are not inferred from lesson provenance"}),
    )
}

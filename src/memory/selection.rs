//! Eligibility filtering and deterministic, content-free memory references.

use crate::{config, evidence::hash, project::path as project_path};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

use super::{confined_target, discovery, policy, relative_project_path};

const REFERENCE_FIELDS: &[&str] = &[
    "itemId",
    "scope",
    "sourcePath",
    "sourceSha256",
    "ledgerPath",
    "ledgerHeadSha256",
    "state",
    "byteLength",
];
const MAX_REFERENCE_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn resolve(loaded: &config::Loaded, agent_name: &str) -> Result<Vec<Value>, String> {
    resolve_for_task(loaded, agent_name, None)
}

pub(crate) fn resolve_for_task(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
) -> Result<Vec<Value>, String> {
    references_for_task(loaded, agent_name, task, false)
}

/// Launch-time guidance can be absent or stale without making unrelated work
/// fail. Keep the selector's rights, ledger and contextual checks strict; only
/// source observations are converted into content-free omission notices.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OptionalResolution {
    pub(crate) references: Vec<Value>,
    pub(crate) unavailable: Vec<Value>,
}

pub(crate) fn resolve_for_task_optional(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
) -> Result<OptionalResolution, String> {
    optional_references_for_task(loaded, agent_name, task, None, None)
}

pub(crate) fn resolve_for_task_optional_with_ceiling(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
    scope_ceiling: &[String],
    cross_context_ceiling: &str,
) -> Result<OptionalResolution, String> {
    optional_references_for_task(
        loaded,
        agent_name,
        task,
        Some(scope_ceiling),
        Some(cross_context_ceiling),
    )
}

pub(crate) fn candidate_references(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
) -> Result<Vec<Value>, String> {
    references_for_task(loaded, agent_name, task, true)
}

fn references_for_task(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
    candidates: bool,
) -> Result<Vec<Value>, String> {
    Ok(references_for_task_with_optional_sources(
        loaded, agent_name, task, candidates, false, None, None,
    )?
    .references)
}

fn optional_references_for_task(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
    scope_ceiling: Option<&[String]>,
    cross_context_ceiling: Option<&str>,
) -> Result<OptionalResolution, String> {
    references_for_task_with_optional_sources(
        loaded,
        agent_name,
        task,
        false,
        true,
        scope_ceiling,
        cross_context_ceiling,
    )
}

fn references_for_task_with_optional_sources(
    loaded: &config::Loaded,
    agent_name: &str,
    task: Option<&str>,
    candidates: bool,
    tolerate_source_unavailable: bool,
    scope_ceiling: Option<&[String]>,
    cross_context_ceiling: Option<&str>,
) -> Result<OptionalResolution, String> {
    let agent = loaded
        .agent(agent_name)
        .ok_or_else(|| format!("unknown agent '{agent_name}'"))?;
    let Some(policy) = policy::get(&loaded.config) else {
        return Ok(OptionalResolution {
            references: Vec::new(),
            unavailable: Vec::new(),
        });
    };
    let Some(ledgers) = discovery::discover(loaded)? else {
        return Ok(OptionalResolution {
            references: Vec::new(),
            unavailable: Vec::new(),
        });
    };
    let mut scopes = eligible_scopes(agent, &policy);
    if let Some(ceiling) = scope_ceiling {
        scopes.retain(|scope| {
            ceiling.iter().any(|allowed| allowed == scope)
                && cross_context_ceiling
                    .map(|cross| scope_allowed_by_cross_context(scope, cross, &policy))
                    .unwrap_or(true)
        });
    }
    let mut references = Vec::new();
    let mut unavailable = Vec::new();
    let mut bytes = 0usize;
    let mut item_ids = BTreeSet::new();
    // Reserved experience cannot escape its sole applicability consumer through
    // a second generic scope, path, copied digest or retired ledger.
    let reserved: Vec<_> = ledgers
        .iter()
        .flat_map(|ledger| ledger.snapshot.items.values())
        .filter(|item| crate::project::specialists::is_scope(item["scope"].as_str().unwrap_or("")))
        .map(|item| {
            (
                item["source"]["path"].clone(),
                item["source"]["sha256"].clone(),
            )
        })
        .collect();
    for ledger in ledgers {
        for item in ledger.snapshot.items.values() {
            let item_id = item["itemId"].as_str().ok_or("invalid memory item id")?;
            if !item_ids.insert(item_id.to_owned()) {
                return Err(format!("duplicate memory item id: {item_id}"));
            }
            let scope = item["scope"].as_str().ok_or("invalid memory item scope")?;
            if crate::project::specialists::is_scope(scope) {
                continue;
            }
            if !scopes.iter().any(|candidate| candidate == scope) {
                continue;
            }
            if item["state"] != "accepted" {
                if tolerate_source_unavailable && item["state"] == "revoked" {
                    let source_path = item["source"]["path"]
                        .as_str()
                        .ok_or("invalid memory source path")?;
                    let source_sha = item["source"]["sha256"]
                        .as_str()
                        .ok_or("invalid memory source hash")?;
                    if !reserved
                        .iter()
                        .any(|(path, digest)| path == source_path || digest == source_sha)
                    {
                        let mut identity = json!({
                            "itemId": item["itemId"], "scope": scope,
                            "sourcePath": source_path, "sourceSha256": source_sha,
                            "ledgerPath": ledger.path,
                            "ledgerHeadSha256": ledger.snapshot.last_event_sha256,
                            "state": "revoked"
                        });
                        if !candidates && super::contextual::enabled(loaded) {
                            let Some(context_selection) =
                                super::contextual::identity(loaded, agent_name, task, &identity)?
                            else {
                                continue;
                            };
                            identity["contextSelection"] = context_selection;
                        }
                        unavailable.push(json!({
                            "identity": identity,
                            "reason": "no_longer_eligible"
                        }));
                    }
                }
                continue;
            }
            if is_expired(item)? {
                continue;
            }
            let source_path = item["source"]["path"]
                .as_str()
                .ok_or("invalid memory source path")?;
            let source_sha = item["source"]["sha256"]
                .as_str()
                .ok_or("invalid memory source hash")?;
            if reserved
                .iter()
                .any(|(path, digest)| path == source_path || digest == source_sha)
            {
                continue;
            }
            let source_target =
                confined_target(loaded.product_root.as_path(), source_path, "memory source")?;
            let source_path = relative_project_path(&loaded.product_root, &source_target)?;
            if scope == super::lessons::SCOPE {
                super::validate_history_current(loaded, &ledger.snapshot)?;
            }
            let mut reference = json!({
                "itemId": item["itemId"],
                "scope": scope,
                "sourcePath": source_path,
                "sourceSha256": source_sha,
                "ledgerPath": ledger.path,
                "ledgerHeadSha256": ledger.snapshot.last_event_sha256,
                "state": "accepted",
            });
            if !candidates && super::contextual::enabled(loaded) {
                let Some(identity) =
                    super::contextual::identity(loaded, agent_name, task, &reference)?
                else {
                    continue;
                };
                reference["contextSelection"] = identity;
            }
            let source = if tolerate_source_unavailable {
                match optional_current_source(loaded, &source_path, source_sha)? {
                    Some(source) => source,
                    None => {
                        let identity = reference.clone();
                        unavailable.push(json!({
                            "identity": identity,
                            "reason": "source_unavailable_or_changed"
                        }));
                        continue;
                    }
                }
            } else {
                current_source(loaded, &source_path, source_sha)?
            };
            if crate::project::specialists::reserved_source(&source) {
                continue;
            }
            if scope == super::lessons::SCOPE {
                let lesson = match super::lessons::parse(loaded, &source) {
                    Ok(lesson) => lesson,
                    Err(error) if tolerate_source_unavailable => {
                        let identity = reference.clone();
                        unavailable.push(json!({
                            "identity": identity,
                            "reason": "source_unavailable_or_changed"
                        }));
                        let _ = error;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let eligible = if candidates || super::contextual::enabled(loaded) {
                    super::lessons::eligible(loaded, &lesson, agent_name)?
                } else {
                    super::lessons::applicable(loaded, &lesson, agent_name, task)?
                };
                if !eligible {
                    continue;
                }
            }
            reference["byteLength"] = json!(source.len());
            let attempted_items = references.len().saturating_add(1);
            let attempted_bytes = bytes.saturating_add(source.len());
            check_budget(loaded, attempted_items, attempted_bytes, item_id)?;
            bytes = attempted_bytes;
            references.push(reference);
        }
    }
    Ok(OptionalResolution {
        references,
        unavailable,
    })
}

fn scope_allowed_by_cross_context(
    scope: &str,
    cross_context: &str,
    policy: &policy::MemoryPolicy,
) -> bool {
    match cross_context {
        "same-scope" => true,
        "protocol-only" => policy
            .protocol_scopes
            .iter()
            .any(|allowed| allowed == scope),
        "synthetic-only" => policy
            .synthetic_scopes
            .iter()
            .any(|allowed| allowed == scope),
        _ => false,
    }
}

fn optional_current_source(
    loaded: &config::Loaded,
    requested: &str,
    expected_hash: &str,
) -> Result<Option<Vec<u8>>, String> {
    use crate::project::path::SecureBytesResult;
    let bytes = match project_path::secure_bytes_observation(
        &loaded.product_root,
        requested,
        "memory source",
    ) {
        SecureBytesResult::Bytes(bytes) => bytes,
        SecureBytesResult::Absent(_) | SecureBytesResult::Unreadable(_) => return Ok(None),
        SecureBytesResult::Unsafe(error) => return Err(error),
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => return Err(error),
    };
    if hash::bytes(&bytes) != expected_hash || std::str::from_utf8(&bytes).is_err() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

pub(crate) fn check_budget(
    loaded: &config::Loaded,
    items: usize,
    bytes: usize,
    item_id: &str,
) -> Result<(), String> {
    if let Some(policy) = policy::get(&loaded.config) {
        if items > policy.max_items || bytes > policy.max_bytes {
            return Err(format!("memory_budget_exceeded: itemId={item_id} attemptedItems={items} attemptedBytes={bytes} maxItems={} maxBytes={}", policy.max_items, policy.max_bytes));
        }
    }
    Ok(())
}

pub(crate) fn validate_references(value: &Value) -> Result<(), String> {
    let references = value
        .as_array()
        .ok_or("invalid memory references: expected an array")?;
    let mut item_ids = BTreeSet::new();
    let mut ledger_paths = BTreeSet::new();
    for reference in references {
        let object = reference
            .as_object()
            .ok_or("invalid memory reference: expected an object")?;
        if !(object.len() == REFERENCE_FIELDS.len()
            || (object.len() == REFERENCE_FIELDS.len() + 1
                && reference
                    .get("contextSelection")
                    .is_some_and(super::contextual::valid_identity)))
            || REFERENCE_FIELDS
                .iter()
                .any(|field| !object.contains_key(*field))
            || reference["state"] != "accepted"
            || !is_hash(reference["itemId"].as_str())
            || reference["scope"].as_str().map_or(true, |value| {
                value.trim().is_empty()
                    || value != value.trim()
                    || value == "*"
                    || value.contains('\0')
            })
            || !relative(reference["sourcePath"].as_str())
            || !relative(reference["ledgerPath"].as_str())
            || !is_hash(reference["sourceSha256"].as_str())
            || !is_hash(reference["ledgerHeadSha256"].as_str())
            || reference["byteLength"]
                .as_u64()
                .map_or(true, |length| length > MAX_REFERENCE_BYTES)
        {
            return Err("invalid memory reference".into());
        }
        let item_id = reference["itemId"]
            .as_str()
            .ok_or("invalid memory reference item ID")?;
        let ledger_path = reference["ledgerPath"]
            .as_str()
            .ok_or("invalid memory reference ledger path")?;
        if !item_ids.insert(item_id) || !ledger_paths.insert(ledger_path) {
            return Err("invalid memory reference: duplicate item or ledger".into());
        }
    }
    Ok(())
}

pub(crate) fn diagnostics(loaded: &config::Loaded, name: &str) -> Result<Value, String> {
    let agent = loaded
        .agent(name)
        .ok_or_else(|| format!("unknown agent '{name}'"))?;
    let policy = policy::get(&loaded.config);
    let scopes = policy
        .as_ref()
        .map(|p| eligible_scopes(agent, p))
        .unwrap_or_default();
    let reason = if policy.is_none() {
        "memory_disabled"
    } else if agent.memory_read.is_empty() {
        "no_read_rights"
    } else if agent.cross_context == "none" {
        "cross_context_disabled"
    } else if scopes.is_empty() {
        "no_eligible_scopes"
    } else {
        "eligible_scopes_require_current_applicable_items"
    };
    Ok(
        json!({"crossContext":agent.cross_context,"effectiveReadScopes":scopes,"reason":reason,
        "meaning":"Storage and approval do not imply selection or recipient delivery. Empty recall remains valid for intentional storage-only use.",
        "nextAction":if scopes.is_empty() {"Have the project owner review the intended read policy; preserve prior configuration for memory correct-policy if correction is needed."} else {"Use --task with the actual goal, then inspect the current recipient context to verify delivery."}}),
    )
}

fn eligible_scopes(
    agent: &crate::config::types::AgentConfig,
    policy: &policy::MemoryPolicy,
) -> Vec<String> {
    let rights = agent.memory_read.iter().filter(|scope| *scope != "*");
    match agent.cross_context.as_str() {
        "same-scope" => rights.cloned().collect(),
        "protocol-only" => rights
            .filter(|scope| policy.protocol_scopes.contains(*scope))
            .cloned()
            .collect(),
        "synthetic-only" => rights
            .filter(|scope| policy.synthetic_scopes.contains(*scope))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn current_source(
    loaded: &config::Loaded,
    requested: &str,
    expected_hash: &str,
) -> Result<Vec<u8>, String> {
    let bytes = project_path::secure_bytes(&loaded.product_root, requested, "memory source")?;
    if hash::bytes(&bytes) != expected_hash {
        return Err(format!("memory source changed since proposal: {requested}"));
    }
    if std::str::from_utf8(&bytes).is_err() {
        return Err(format!("memory source is not UTF-8: {requested}"));
    }
    Ok(bytes)
}

fn is_expired(item: &Value) -> Result<bool, String> {
    let Some(expiry) = item["expiresAt"].as_str() else {
        return Ok(false);
    };
    let expiry = chrono::DateTime::parse_from_rfc3339(expiry)
        .map_err(|_| "invalid memory expiry".to_owned())?;
    Ok(expiry <= chrono::Utc::now().fixed_offset())
}

fn relative(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let path = Path::new(value);
    !value.is_empty()
        && !value.contains('\0')
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn is_hash(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

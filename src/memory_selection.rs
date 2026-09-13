//! Eligibility filtering and deterministic, content-free memory references.

use crate::{config, hash, memory, memory_discovery, memory_policy, project_path};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

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
    let agent = loaded
        .agent(agent_name)
        .ok_or_else(|| format!("unknown agent '{agent_name}'"))?;
    let Some(policy) = memory_policy::get(&loaded.config) else {
        return Ok(Vec::new());
    };
    let Some(ledgers) = memory_discovery::discover(loaded)? else {
        return Ok(Vec::new());
    };
    let scopes = eligible_scopes(agent, &policy);
    let mut references = Vec::new();
    let mut bytes = 0usize;
    let mut item_ids = BTreeSet::new();
    for ledger in ledgers {
        for item in ledger.snapshot.items.values() {
            let item_id = item["itemId"].as_str().ok_or("invalid memory item id")?;
            if !item_ids.insert(item_id.to_owned()) {
                return Err(format!("duplicate memory item id: {item_id}"));
            }
            if item["state"] != "accepted" {
                continue;
            }
            let scope = item["scope"].as_str().ok_or("invalid memory item scope")?;
            if !scopes.iter().any(|candidate| candidate == scope) {
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
            let source_target = memory::confined_target(
                loaded.product_root.as_path(),
                source_path,
                "memory source",
            )?;
            let source_path = memory::relative_project_path(&loaded.product_root, &source_target)?;
            let source = current_source(loaded, &source_path, source_sha)?;
            let attempted_items = references.len().saturating_add(1);
            let attempted_bytes = bytes.saturating_add(source.len());
            if attempted_items > policy.max_items || attempted_bytes > policy.max_bytes {
                return Err(format!(
                    "memory_budget_exceeded: itemId={} attemptedItems={} attemptedBytes={} maxItems={} maxBytes={}",
                    item_id, attempted_items, attempted_bytes, policy.max_items, policy.max_bytes
                ));
            }
            bytes = attempted_bytes;
            references.push(json!({
                "itemId": item["itemId"],
                "scope": scope,
                "sourcePath": source_path,
                "sourceSha256": source_sha,
                "ledgerPath": ledger.path,
                "ledgerHeadSha256": ledger.snapshot.last_event_sha256,
                "state": "accepted",
                "byteLength": source.len(),
            }));
        }
    }
    Ok(references)
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
        if object.len() != REFERENCE_FIELDS.len()
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

fn eligible_scopes(
    agent: &crate::config_types::AgentConfig,
    policy: &memory_policy::MemoryPolicy,
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

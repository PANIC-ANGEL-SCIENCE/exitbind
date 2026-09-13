use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use crate::{config::Loaded, hash};

use super::memory_ledger::{
    confined_target, project_file, relative_project_path, stable_bytes, stable_text, LedgerSnapshot,
};

pub(crate) const ACTIONS: [&str; 6] =
    ["propose", "review", "promote", "reject", "revoke", "expire"];
const SHA256: &str = "0123456789abcdef";

pub(crate) fn validate_event(
    event: &Value,
    source_root: &Path,
    profile_root: &Path,
    line: usize,
    previous_hash: Option<&str>,
    previous_timestamp: Option<&str>,
    items: &BTreeMap<String, Value>,
) -> Result<(), String> {
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid memory ledger line {line}: event must be an object"))?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "itemId",
        "scope",
        "actor",
        "source",
        "configSha256",
        "actorProfile",
        "previousEventSha256",
        "timestamp",
        "expiresAt",
        "eventSha256",
    ];
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "invalid memory ledger line {line}: unknown field '{key}'"
            ));
        }
    }
    if event["version"] != json!(1)
        || event["kind"] != json!("memory")
        || !event["action"]
            .as_str()
            .is_some_and(|a| ACTIONS.contains(&a))
    {
        return Err(format!(
            "invalid memory ledger line {line}: invalid event header"
        ));
    }
    if event
        .get("producer")
        .is_some_and(|producer| !crate::producer::valid(producer))
    {
        return Err(format!(
            "invalid memory ledger line {line}: invalid producer"
        ));
    }
    for field in [
        "itemId",
        "scope",
        "actor",
        "configSha256",
        "timestamp",
        "eventSha256",
    ] {
        if event[field].as_str().map_or(true, str::is_empty) {
            return Err(format!(
                "invalid memory ledger line {line}: {field} is required"
            ));
        }
    }
    let scope = event["scope"]
        .as_str()
        .ok_or_else(|| format!("invalid memory ledger line {line}: scope is required"))?;
    if scope.trim().is_empty() || scope.contains('\0') {
        return Err(format!("invalid memory ledger line {line}: invalid scope"));
    }
    for field in ["itemId", "configSha256", "eventSha256"] {
        if !event[field].as_str().is_some_and(valid_hash) {
            return Err(format!("invalid memory ledger line {line}: invalid hash"));
        }
    }
    let recorded_previous = object
        .get("previousEventSha256")
        .ok_or_else(|| format!("invalid memory ledger line {line}: broken event chain"))?;
    if recorded_previous != &previous_hash.map_or(Value::Null, |v| json!(v)) {
        return Err(format!(
            "invalid memory ledger line {line}: broken event chain"
        ));
    }
    let timestamp = event["timestamp"]
        .as_str()
        .ok_or_else(|| format!("invalid memory ledger line {line}: invalid timestamp"))?;
    let parsed_timestamp = parse_timestamp(timestamp)
        .ok_or_else(|| format!("invalid memory ledger line {line}: invalid timestamp"))?;
    if let Some(previous) = previous_timestamp {
        let parsed_previous = parse_timestamp(previous).ok_or_else(|| {
            format!("invalid memory ledger line {line}: invalid previous timestamp")
        })?;
        if parsed_timestamp < parsed_previous {
            return Err(format!(
                "invalid memory ledger line {line}: timestamp is earlier than previous event"
            ));
        }
    }
    if let Some(expiry) = object.get("expiresAt") {
        if expiry.as_str().and_then(parse_timestamp).is_none() {
            return Err(format!(
                "invalid memory ledger line {line}: invalid expiresAt"
            ));
        }
    }
    validate_evidence(event, source_root, profile_root, line)?;
    let source = event["source"]
        .as_object()
        .ok_or_else(|| format!("invalid memory ledger line {line}: invalid source evidence"))?;
    let expected_id = item_id_for(
        scope,
        source["path"]
            .as_str()
            .ok_or_else(|| format!("invalid memory ledger line {line}: invalid source path"))?,
        source["sha256"]
            .as_str()
            .ok_or_else(|| format!("invalid memory ledger line {line}: invalid source hash"))?,
    );
    if event["itemId"] != expected_id {
        return Err(format!(
            "invalid memory ledger line {line}: itemId does not match source evidence"
        ));
    }
    if hash::value(&without(event, "eventSha256")) != event["eventSha256"] {
        return Err(format!(
            "invalid memory ledger line {line}: event hash mismatch"
        ));
    }
    let action = event["action"]
        .as_str()
        .ok_or_else(|| format!("invalid memory ledger line {line}: invalid action"))?;
    if action == "propose" {
        if !items.is_empty() {
            return Err(format!(
                "invalid memory ledger line {line}: memory ledger may contain only one item"
            ));
        }
        if event["previousEventSha256"] != Value::Null {
            return Err(format!(
                "invalid memory ledger line {line}: proposal must start a chain"
            ));
        }
        return Ok(());
    }
    let current = items
        .get(
            event["itemId"]
                .as_str()
                .ok_or_else(|| format!("invalid memory ledger line {line}: invalid itemId"))?,
        )
        .ok_or_else(|| format!("invalid memory ledger line {line}: transition has no proposal"))?;
    if current["scope"] != event["scope"]
        || current["source"] != event["source"]
        || current.get("expiresAt") != object.get("expiresAt")
    {
        return Err(format!(
            "invalid memory ledger line {line}: item evidence changed"
        ));
    }
    next_state_for(
        action,
        current["state"]
            .as_str()
            .ok_or_else(|| format!("invalid memory ledger line {line}: invalid item state"))?,
    )?;
    if action == "expire" {
        let expiry = event["expiresAt"]
            .as_str()
            .ok_or_else(|| format!("invalid memory ledger line {line}: expiry is required"))?;
        let parsed_expiry = parse_timestamp(expiry)
            .ok_or_else(|| format!("invalid memory ledger line {line}: invalid expiresAt"))?;
        if parsed_timestamp < parsed_expiry {
            return Err(format!(
                "invalid memory ledger line {line}: expiry occurred before expiresAt"
            ));
        }
    }
    Ok(())
}

fn validate_evidence(
    event: &Value,
    source_root: &Path,
    profile_root: &Path,
    line: usize,
) -> Result<(), String> {
    for field in ["source", "actorProfile"] {
        let value = event[field].as_object().ok_or_else(|| {
            format!("invalid memory ledger line {line}: invalid {field} evidence")
        })?;
        if value.len() != 2
            || !value.contains_key("path")
            || !value.contains_key("sha256")
            || value["path"].as_str().map_or(true, str::is_empty)
            || !value["sha256"].as_str().is_some_and(valid_hash)
        {
            return Err(format!(
                "invalid memory ledger line {line}: invalid {field} evidence"
            ));
        }
        let path = value["path"]
            .as_str()
            .ok_or_else(|| format!("invalid memory ledger line {line}: invalid {field} path"))?;
        let root = if field == "source" {
            source_root
        } else {
            profile_root
        };
        if confined_target(root, path, field).is_err() || Path::new(path).is_absolute() {
            return Err(format!("path escapes project root: {field} on line {line}"));
        }
    }
    if let Some(previous) = event["previousEventSha256"].as_str() {
        if !valid_hash(previous) {
            return Err(format!(
                "invalid memory ledger line {line}: invalid previous event hash"
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply_event(
    items: &mut BTreeMap<String, Value>,
    event: &Value,
) -> Result<(), String> {
    let id = event["itemId"]
        .as_str()
        .ok_or("memory event has no itemId")?
        .to_owned();
    if event["action"] == "propose" {
        let mut item = json!({"itemId": id.clone(), "scope": event["scope"], "state": "proposed", "source": event["source"], "lastEventSha256": event["eventSha256"]});
        if let Some(expiry) = event.get("expiresAt") {
            item["expiresAt"] = expiry.clone();
        }
        items.insert(id, item);
    } else if let Some(item) = items.get_mut(&id) {
        let action = event["action"]
            .as_str()
            .ok_or("memory event has no action")?;
        item["state"] = json!(state_for_action(action));
        item["lastEventSha256"] = event["eventSha256"].clone();
    } else {
        return Err("memory transition has no proposal".into());
    }
    Ok(())
}

pub(crate) fn source_evidence(root: &Path, requested: &str) -> Result<Value, String> {
    let path = project_file(root, requested, "source")?;
    let bytes = stable_bytes(&path, "source")?;
    Ok(json!({"path": relative_project_path(root, &path)?, "sha256": hash_bytes(&bytes)}))
}

pub(crate) fn ensure_config_current(loaded: &Loaded) -> Result<(), String> {
    let current = std::fs::read_to_string(&loaded.path).map_err(|e| e.to_string())?;
    if current != loaded.source {
        return Err("configuration changed since load".into());
    }
    Ok(())
}

pub(crate) fn validate_history_current(
    loaded: &Loaded,
    ledger: &LedgerSnapshot,
) -> Result<(), String> {
    let config_hash = hash::text(&loaded.source);
    for event in &ledger.events {
        if event["configSha256"] != config_hash {
            return Err("configuration changed since memory event".into());
        }
        let actor = event["actor"].as_str().ok_or("invalid memory actor")?;
        let agent = loaded.config["agents"]
            .get(actor)
            .ok_or_else(|| format!("memory actor configuration changed: {actor}"))?;
        let profile_name = agent["profile"]
            .as_str()
            .ok_or_else(|| format!("memory actor configuration changed: {actor}"))?;
        let profile = project_file(&loaded.control_root, profile_name, "actor profile")?;
        let current_path = relative_project_path(&loaded.control_root, &profile)?;
        let current_hash = hash::text(&stable_text(&profile, "actor profile")?);
        if event["actorProfile"]["path"] != current_path
            || event["actorProfile"]["sha256"] != current_hash
        {
            return Err(format!("memory actor profile changed: {actor}"));
        }
    }
    Ok(())
}

pub(crate) fn validate_source_current(
    loaded: &Loaded,
    ledger: &LedgerSnapshot,
) -> Result<(), String> {
    let item = ledger
        .items
        .values()
        .next()
        .ok_or("memory ledger has no items")?;
    let source_path = item["source"]["path"]
        .as_str()
        .ok_or("invalid memory source")?;
    let recorded_hash = item["source"]["sha256"]
        .as_str()
        .ok_or("invalid memory source")?;
    let source = project_file(&loaded.product_root, source_path, "source")?;
    let current_hash = hash_bytes(&stable_bytes(&source, "source")?);
    if current_hash != recorded_hash {
        return Err("memory source changed since proposal".into());
    }
    Ok(())
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    hash::bytes(bytes)
}

pub(crate) fn right_for(action: &str) -> &str {
    match action {
        "propose" => "memoryWrite",
        "review" => "memoryReview",
        "promote" => "memoryPromote",
        "reject" => "memoryReject",
        "revoke" => "memoryRevoke",
        _ => "memoryExpire",
    }
}

pub(crate) fn authorized(agent: &Value, right: &str, scope: &str) -> bool {
    agent[right]
        .as_array()
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(scope)))
}

pub(crate) fn assert_scope(scope: &str) -> Result<(), String> {
    if scope.trim().is_empty() || scope.contains('\0') {
        Err("scope must be a non-empty string".into())
    } else {
        Ok(())
    }
}

pub(crate) fn state_for_action(action: &str) -> &str {
    match action {
        "propose" => "proposed",
        "review" => "reviewed",
        "promote" => "accepted",
        "reject" => "rejected",
        "revoke" => "revoked",
        "expire" => "expired",
        _ => "unknown",
    }
}

pub(crate) fn next_state_for(action: &str, current: &str) -> Result<String, String> {
    let allowed = match current {
        "proposed" => ["review", "reject"].as_slice(),
        "reviewed" => ["promote", "reject"].as_slice(),
        "accepted" => ["revoke", "expire"].as_slice(),
        _ => [].as_slice(),
    };
    if !allowed.contains(&action) {
        return Err(format!(
            "invalid memory transition: cannot {action} from {current}"
        ));
    }
    Ok(state_for_action(action).to_owned())
}

pub(crate) fn without(value: &Value, key: &str) -> Value {
    let mut copy = value.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove(key);
    }
    copy
}

pub(crate) fn item_id_for(scope: &str, source_path: &str, source_sha256: &str) -> String {
    hash::value(&json!({"scope": scope, "sourcePath": source_path, "sourceSha256": source_sha256}))
}

pub(crate) fn parse_timestamp(value: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    if !value.contains('T') {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

pub(crate) fn normalize_timestamp(value: &str, label: &str) -> Result<String, String> {
    let parsed =
        parse_timestamp(value).ok_or_else(|| format!("{label} must be an ISO timestamp"))?;
    Ok(parsed
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| SHA256.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_comparison_is_chronological_and_rejects_invalid_values() {
        let earlier = parse_timestamp("2026-08-29T00:00:00.000Z").unwrap();
        let later = parse_timestamp("2026-08-29T00:00:01.000Z").unwrap();
        assert!(earlier < later);
        assert!(parse_timestamp("2026-08-29").is_none());
        assert!(parse_timestamp("not-a-time").is_none());
    }
}

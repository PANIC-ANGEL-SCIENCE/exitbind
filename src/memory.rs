use serde_json::{json, Value};

use crate::{config::Loaded, hash};

#[path = "memory_ledger.rs"]
mod memory_ledger;
pub(crate) use memory_ledger::*;

#[path = "memory_state.rs"]
mod memory_state;
pub(crate) use memory_state::*;

pub fn resolve(loaded: &Loaded, agent: &str) -> Result<Value, String> {
    let references = crate::memory_selection::resolve(loaded, agent)?;
    Ok(json!({
        "valid": true,
        "agent": agent,
        "references": references,
    }))
}

pub fn action(
    loaded: &Loaded,
    actor: &str,
    act: &str,
    source: Option<&str>,
    scope: Option<&str>,
    ledger: &str,
    expires: Option<&str>,
) -> Result<Value, String> {
    if !ACTIONS.contains(&act) {
        return Err(format!("unknown memory action '{act}'"));
    }
    ensure_config_current(loaded)?;
    let agent = loaded.config["agents"]
        .get(actor)
        .ok_or_else(|| format!("unknown agent '{actor}'"))?;
    let profile_name = agent["profile"]
        .as_str()
        .ok_or_else(|| format!("agent '{actor}' has no profile"))?;
    let resolved_profile = project_file(&loaded.control_root, profile_name, "actor profile")?;

    let (requested_scope, requested_expiry) = if act == "propose" {
        let scope = scope.ok_or("scope must be a non-empty string")?;
        assert_scope(scope)?;
        (
            scope.to_owned(),
            expires
                .map(|value| normalize_timestamp(value, "expires-at"))
                .transpose()?,
        )
    } else {
        if source.is_some() || scope.is_some() || expires.is_some() {
            return Err(format!(
                "{act} does not accept source, scope, or expires-at"
            ));
        }
        (String::new(), None)
    };

    let ledger_state = read_ledger(loaded, ledger, act == "propose")?;
    let targets = [ledger_state.path.as_path()];
    crate::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    validate_history_current(loaded, &ledger_state)?;
    if act != "propose" {
        validate_source_current(loaded, &ledger_state)?;
    }
    let (item_id, item_scope, item_source, item_expiry, next_state) = if act == "propose" {
        let source_name = source.ok_or("source path is required")?;
        let evidence = source_evidence(&loaded.product_root, source_name)?;
        let evidence_path = evidence["path"]
            .as_str()
            .ok_or("invalid memory source path")?;
        let evidence_hash = evidence["sha256"]
            .as_str()
            .ok_or("invalid memory source hash")?;
        let item_id = item_id_for(&requested_scope, evidence_path, evidence_hash);
        if ledger_state.items.contains_key(&item_id) {
            return Err(format!("memory item already exists: {item_id}"));
        }
        if !ledger_state.items.is_empty() {
            return Err("memory ledger already contains a different item".into());
        }
        (
            item_id,
            requested_scope,
            evidence,
            requested_expiry,
            "proposed".to_owned(),
        )
    } else {
        if ledger_state.items.is_empty() {
            return Err("memory ledger has no items".into());
        }
        if ledger_state.items.len() != 1 {
            return Err("memory transition requires a ledger with exactly one item".into());
        }
        let (id, item) = ledger_state
            .items
            .iter()
            .next()
            .ok_or("memory ledger has no item")?;
        let current = item["state"].as_str().ok_or("invalid memory item state")?;
        let next = next_state_for(act, current)?;
        let expiry = item["expiresAt"].as_str().map(str::to_owned);
        (
            id.clone(),
            item["scope"]
                .as_str()
                .ok_or("invalid memory item scope")?
                .to_owned(),
            item["source"].clone(),
            expiry,
            next,
        )
    };

    let right = right_for(act);
    if !authorized(agent, right, &item_scope) {
        return Err(format!(
            "agent is not authorized for {right} scope '{item_scope}'"
        ));
    }
    let action_time = normalize_timestamp(&now(), "clock")?;
    let parsed_action_time = parse_timestamp(&action_time).ok_or("clock timestamp is invalid")?;
    if let Some(previous) = ledger_state.last_timestamp.as_deref() {
        let parsed_previous =
            parse_timestamp(previous).ok_or("previous memory timestamp is invalid")?;
        if parsed_action_time < parsed_previous {
            return Err("memory event timestamp must be nondecreasing".into());
        }
    }
    if act == "expire" {
        let expiry = item_expiry.as_deref().ok_or("memory item has no expiry")?;
        let parsed_expiry = parse_timestamp(expiry).ok_or("memory expiry is invalid")?;
        if parsed_action_time < parsed_expiry {
            return Err("memory item has not expired".into());
        }
    }

    let profile_bytes = stable_text(&resolved_profile, "actor profile")?;
    let mut event = json!({
        "version": 1,
        "kind": "memory",
        "producer": crate::producer::evidence(),
        "action": act,
        "itemId": item_id,
        "scope": item_scope,
        "actor": actor,
        "source": item_source,
        "configSha256": hash::text(&loaded.source),
        "actorProfile": {
            "path": relative_project_path(&loaded.control_root, &resolved_profile)?,
            "sha256": hash::text(&profile_bytes),
        },
        "previousEventSha256": ledger_state.last_event_sha256.clone(),
        "timestamp": action_time,
    });
    if let Some(expiry) = item_expiry {
        event["expiresAt"] = json!(expiry);
    }
    event["eventSha256"] = json!(hash::value(&event));
    append_event(&ledger_state, &event)?;
    Ok(json!({"event": event, "state": next_state}))
}

pub fn inspect(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let snapshot = read_ledger(loaded, ledger, false)?;
    Ok(json!({
        "valid": true,
        "events": snapshot.events,
        "items": snapshot.items.values().cloned().collect::<Vec<_>>(),
        "lastTimestamp": snapshot.last_timestamp,
    }))
}

pub(crate) fn snapshot(loaded: &Loaded, ledger: &str) -> Result<LedgerSnapshot, String> {
    read_ledger(loaded, ledger, false)
}

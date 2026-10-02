//! Product-owned, exact grouped consumption for the current Work.
//!
//! The legacy section reader remains hex encoded for compatibility. This
//! versioned reader resolves and validates the same current binding, then
//! presents bounded UTF-8 artifacts directly so consumers do not implement
//! paging, decoding, or integrity checks themselves.

use crate::{config::Loaded, run::RunSnapshot};
use serde_json::{json, Map, Value};

pub(crate) const PREFIX: &str = "ref:work-detail:v1:";
pub(crate) const MAX_GROUPED_BYTES: usize = 256 * 1024;

pub(crate) fn is_grouped(reference: &str) -> bool {
    reference.starts_with(PREFIX)
}

fn token(binding: &str) -> String {
    format!("{PREFIX}{binding}")
}

fn route(loaded: &Loaded, work: &str, binding: &str) -> Value {
    let command = super::response_recovery::bounded_argv(
        vec![
            "work".into(),
            "expand".into(),
            work.into(),
            token(binding),
            "--json".into(),
            "--config".into(),
        ],
        loaded.path.to_str(),
        1024,
    );
    json!({
        "version": 1,
        "kind": "work_detail",
        "reference": token(binding),
        "command": command.argv,
        "sameConfigRequired": command.same_config,
        "sameExecutableRequired": command.same_executable,
        "readOnly": true,
    })
}

pub(crate) fn attach(loaded: &Loaded, work: &str, binding: &str, current: &mut Value) {
    if let Some(details) = current.get_mut("details").and_then(Value::as_object_mut) {
        details.insert("grouped".into(), route(loaded, work, binding));
    }
}

pub(crate) fn read(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    let binding = super::details::binding(loaded, work, snapshot, &canonical)?;
    let sections = grouped_sections(loaded, work, snapshot, &canonical, &binding)?;
    super::details::ensure_current_config(loaded)?;
    let fresh_snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let fresh = super::next_from(loaded, work, &fresh_snapshot)?;
    let mut fresh_canonical = fresh.clone();
    fresh_canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if super::details::binding(loaded, work, &fresh_snapshot, &fresh_canonical)? != binding {
        return Err("work detail changed during expansion; refresh with work next".into());
    }
    response(loaded, work, &binding, &canonical, sections)
}

pub(crate) fn current(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = super::resolve(loaded, work)?;
    let snapshot = RunSnapshot::capture(loaded, &ledger)?;
    let next = super::next_from(loaded, work, &snapshot)?;
    read(loaded, work, &snapshot, &next)
}

pub(crate) fn expand(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    requested: &str,
) -> Result<Value, String> {
    super::details::ensure_current_config(loaded)?;
    let binding = requested
        .strip_prefix(PREFIX)
        .filter(|value| !value.is_empty() && !value.contains(':'))
        .ok_or("malformed grouped work detail reference; refresh with work next")?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    let expected = super::details::binding(loaded, work, snapshot, &canonical)?;
    if binding != expected {
        return Err("work detail reference is stale, revoked or belongs to another project/Work/recipient; refresh with work next".into());
    }
    let sections = grouped_sections(loaded, work, snapshot, &canonical, &binding)?;
    super::details::ensure_current_config(loaded)?;
    let fresh_snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let fresh = super::next_from(loaded, work, &fresh_snapshot)?;
    let mut fresh_canonical = fresh.clone();
    fresh_canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if super::details::binding(loaded, work, &fresh_snapshot, &fresh_canonical)? != expected {
        return Err("work detail changed during expansion; refresh with work next".into());
    }
    response(loaded, work, expected.as_str(), &canonical, sections)
}

fn grouped_sections(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    binding: &str,
) -> Result<Map<String, Value>, String> {
    let mut sections = Map::new();
    let mut assignment = super::details::section_value(loaded, work, snapshot, next, "assignment")?;
    // This is a delivery projection, not a replacement canonical packet.
    // Only byte-equal aliases are removed; every unequal fact is retained.
    let mut aliases = Vec::new();
    for (residual_key, context_key) in [
        ("context", None),
        ("goal", Some("goal")),
        ("currentSubject", Some("subject")),
        ("remaining", Some("obligations")),
    ] {
        let original = &assignment["residual"][residual_key];
        let context = &assignment["assignment"]["context"];
        let replacement = context_key.map_or(context, |key| &context[key]);
        if !original.is_null() && original == replacement {
            assignment["residual"]
                .as_object_mut()
                .unwrap()
                .remove(residual_key);
            aliases.push(json!({"removed": format!("residual.{residual_key}"),
                "source": context_key.map_or("assignment.context".to_owned(), |key| format!("assignment.context.{key}"))}));
        }
    }
    if assignment["assignment"].is_object() {
        assignment["assignment"] = super::action::delivery_packet(&assignment["assignment"]);
    }
    assignment["projection"] = json!({"equalAliases": aliases,
        "recoveryAliases": "equal recovery goal/scope/subject/current/missing/loop/next refer to context goal/scope/subject/evidence/obligations/loop/next"});
    sections.insert("assignment".into(), assignment);
    let mut evidence = super::details::readable_evidence(loaded, snapshot, next)?;
    for item in evidence["items"].as_array_mut().unwrap() {
        for field in ["artifact", "stdout", "stderr"] {
            if item[field].is_object() && item[field]["readable"] == false {
                item[field]["access"] = json!({"currentMetadata": route(loaded, work, binding),
                    "legacySection": super::details::legacy_evidence_route(loaded, work, binding),
                    "limitation": "non-UTF-8 material remains exact bytes in the legacy section; oversized material exposes verified metadata only"});
            }
        }
    }
    sections.insert("evidence".into(), evidence);
    sections.insert(
        "tasks".into(),
        super::details::section_value(loaded, work, snapshot, next, "tasks")?,
    );
    Ok(sections)
}

fn response(
    loaded: &Loaded,
    work: &str,
    binding: &str,
    next: &Value,
    sections: Map<String, Value>,
) -> Result<Value, String> {
    let readable_complete = evidence_complete(&sections["evidence"]);
    let value = json!({
        "compact": true,
        "valid": true,
        "version": 1,
        "kind": "work_detail",
        "reference": token(binding),
        "work": work,
        "binding": binding,
        "recipient": {"role": next["role"], "agent": next["agent"], "assignment": next["assignment"], "actor": next["resolvedActor"]},
        "complete": readable_complete,
        "sections": sections,
        "actionForms": super::action_forms::full(loaded, work, next, binding),
        "transport": {"encoding": "utf-8", "exact": readable_complete, "modelPaging": false},
    });
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    if bytes.len() + 1 > MAX_GROUPED_BYTES {
        return Err("grouped work detail exceeds the bounded readable channel; use the current section routes".into());
    }
    Ok(value)
}

fn evidence_complete(value: &Value) -> bool {
    value["items"].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            ["artifact", "stdout", "stderr"].iter().all(|key| {
                !item[*key].is_object()
                    || (item[*key]["readable"] == true && item[*key]["complete"] == true)
            })
        })
    })
}

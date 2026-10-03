//! Typed approved role facts, parsed once and applied to validated agents.
use crate::{
    cli::args::Arguments,
    config::{self, Loaded},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct RoleFacts {
    pub(super) observe: Option<Vec<String>>,
    pub(super) write: Option<Vec<String>>,
    pub(super) commands: Option<Vec<String>>,
}

pub(super) const OPTIONS: &[&str] = &[
    "lead-observe",
    "lead-write",
    "lead-commands",
    "worker-observe",
    "worker-write",
    "worker-commands",
    "reviewer-observe",
    "reviewer-write",
    "reviewer-commands",
];

pub(super) fn parse(arguments: &Arguments) -> Result<BTreeMap<String, RoleFacts>, String> {
    let mut roles = BTreeMap::new();
    for role in ["lead", "worker", "reviewer"] {
        let key = |field: &str| format!("{role}-{field}");
        let facts = RoleFacts {
            observe: split_list(arguments.options.get(&key("observe")), &key("observe"))?,
            write: split_list(arguments.options.get(&key("write")), &key("write"))?,
            commands: single_command(arguments.options.get(&key("commands")), &key("commands"))?,
        };
        if facts.observe.is_some() || facts.write.is_some() || facts.commands.is_some() {
            roles.insert(role.to_owned(), facts);
        }
    }
    if !roles.is_empty()
        && ["scope", "observe", "write", "commands"]
            .iter()
            .any(|key| arguments.options.contains_key(*key))
    {
        return Err("role-specific setup facts cannot be combined with --scope/--observe/--write/--commands; choose one explicit representation".into());
    }
    Ok(roles)
}

pub(super) fn single_command(
    value: Option<&String>,
    label: &str,
) -> Result<Option<Vec<String>>, String> {
    let Some(value) = value else { return Ok(None) };
    if value.trim().is_empty() || value.contains('\0') {
        return Err(format!(
            "--{label} must be a non-empty value without NUL bytes"
        ));
    }
    if label.ends_with("-commands") && value == "none" {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(vec![value.clone()]))
}

pub(super) fn split_list(
    value: Option<&String>,
    label: &str,
) -> Result<Option<Vec<String>>, String> {
    let Some(value) = value else { return Ok(None) };
    if (label.ends_with("-observe") || label.ends_with("-write")) && value == "none" {
        return Ok(Some(Vec::new()));
    }
    let mut result = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() || item.contains('\0') {
            return Err(format!("--{label} contains an empty or NUL value"));
        }
        result.push(item.to_owned());
    }
    let mut unique = BTreeSet::new();
    result.retain(|item| unique.insert(item.clone()));
    Ok(Some(result))
}

pub(super) fn updated_config(
    loaded: &Loaded,
    roles: &BTreeMap<String, RoleFacts>,
    hosts: &[String],
) -> Result<(Value, bool), String> {
    let mut next = loaded.config.clone();
    let mut changed = false;
    for (role, facts) in roles {
        let configured = loaded
            .agent(role)
            .ok_or_else(|| format!("configuration has no '{role}' agent"))?;
        let agent = next["agents"]
            .get_mut(role)
            .and_then(Value::as_object_mut)
            .ok_or("validated agent representation is unavailable")?;
        for (name, supplied, existing) in [
            ("observe", &facts.observe, &configured.observe),
            ("write", &facts.write, &configured.write),
            ("commands", &facts.commands, &configured.commands),
        ] {
            if let Some(value) = supplied {
                if value != existing {
                    agent.insert(name.to_owned(), json!(value));
                    changed = true;
                }
            }
        }
        if hosts.len() == 1 {
            let runtime = agent.entry("runtime").or_insert_with(|| json!({}));
            let runtime = runtime
                .as_object_mut()
                .ok_or_else(|| format!("agents.{role}.runtime must be an object"))?;
            if configured.runtime.host.as_ref() != Some(&hosts[0]) {
                runtime.insert("host".into(), json!(hosts[0]));
                changed = true;
            }
        }
    }
    let errors = config::validate(&next);
    if !errors.is_empty() {
        return Err(format!(
            "approved setup facts would invalidate configuration:\n- {}",
            errors.join("\n- ")
        ));
    }
    Ok((next, changed))
}

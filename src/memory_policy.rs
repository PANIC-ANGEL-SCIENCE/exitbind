//! Validation and projection of the optional project recall policy.

use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

const MAX_MEMORY_ITEMS: u64 = 256;
const MAX_MEMORY_BYTES: u64 = 4 * 1024 * 1024;
const FIELDS: &[&str] = &[
    "root",
    "maxItems",
    "maxBytes",
    "protocolScopes",
    "syntheticScopes",
];

#[derive(Clone, Debug)]
pub(crate) struct MemoryPolicy {
    pub(crate) root: String,
    pub(crate) max_items: usize,
    pub(crate) max_bytes: usize,
    pub(crate) protocol_scopes: BTreeSet<String>,
    pub(crate) synthetic_scopes: BTreeSet<String>,
}

pub(crate) fn validate(value: &Value, errors: &mut Vec<String>) {
    let Some(memory) = value.as_object() else {
        if !value.is_null() {
            errors.push("memory must be an object".into());
        }
        return;
    };
    for key in memory.keys() {
        if !FIELDS.contains(&key.as_str()) {
            errors.push(format!("memory.{key} is not allowed"));
        }
    }
    match memory.get("root").and_then(Value::as_str) {
        None => errors.push("memory.root must be a non-empty relative path".into()),
        Some(root) if root.trim().is_empty() || root.contains('\0') => {
            errors.push("memory.root must be a non-empty relative path".into())
        }
        Some(root) if !valid_relative_path(root) => {
            errors.push("memory.root must stay inside the project".into())
        }
        _ => {}
    }
    validate_bounded_positive(memory, "maxItems", MAX_MEMORY_ITEMS, errors);
    validate_bounded_positive(memory, "maxBytes", MAX_MEMORY_BYTES, errors);
    for field in ["protocolScopes", "syntheticScopes"] {
        let Some(value) = memory.get(field) else {
            errors.push(format!("memory.{field} must be an array of scopes"));
            continue;
        };
        if !valid_scope_array(value) {
            errors.push(format!(
                "memory.{field} must be an array of unique, non-empty exact scopes"
            ));
        }
    }
    if valid_scope_array(&memory["protocolScopes"]) && valid_scope_array(&memory["syntheticScopes"])
    {
        let protocol = scopes(memory.get("protocolScopes"));
        let synthetic = scopes(memory.get("syntheticScopes"));
        if protocol.intersection(&synthetic).next().is_some() {
            errors.push("memory protocolScopes and syntheticScopes must not overlap".into());
        }
    }
}

pub(crate) fn get(config: &Value) -> Option<MemoryPolicy> {
    let memory = config.get("memory")?.as_object()?;
    Some(MemoryPolicy {
        root: memory["root"].as_str()?.to_owned(),
        max_items: memory["maxItems"].as_u64()? as usize,
        max_bytes: memory["maxBytes"].as_u64()? as usize,
        protocol_scopes: scopes(memory.get("protocolScopes")),
        synthetic_scopes: scopes(memory.get("syntheticScopes")),
    })
}

pub(crate) fn valid_scope_array(value: &Value) -> bool {
    let mut seen = BTreeSet::new();
    value.as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item.as_str().is_some_and(|scope| {
                !scope.trim().is_empty()
                    && scope == scope.trim()
                    && scope != "*"
                    && !scope.contains('\0')
                    && seen.insert(scope)
            })
        })
    })
}

fn validate_bounded_positive(
    object: &Map<String, Value>,
    field: &str,
    maximum: u64,
    errors: &mut Vec<String>,
) {
    match object.get(field).and_then(Value::as_u64) {
        Some(value) if (1..=maximum).contains(&value) => {}
        _ => errors.push(format!(
            "memory.{field} must be a positive integer no greater than {maximum}"
        )),
    }
}

fn valid_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    let has_named_component = path
        .components()
        .any(|component| matches!(component, std::path::Component::Normal(_)));
    has_named_component
        && !path.is_absolute()
        && path.components().all(|component| {
            !matches!(component, std::path::Component::ParentDir)
                && !matches!(
                    component,
                    std::path::Component::RootDir | std::path::Component::Prefix(_)
                )
        })
}

fn scopes(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

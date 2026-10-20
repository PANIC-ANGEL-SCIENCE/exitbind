//! Owner-declared guidance bodies (for example a skill's SKILL.md) that an
//! agent receives whole when its assignment enters a phase.
//!
//! Declaring an item grants nothing: it is read-only guidance delivered to the
//! one agent whose configuration names it, never to another role.
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) const FIELD: &str = "phaseGuidance";
pub(crate) const PHASES: &[&str] = &["implementation", "test", "repair", "review", "advice"];
const MAX_ITEMS: usize = 16;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct PhaseGuidance {
    pub id: String,
    pub phase: String,
    pub path: String,
}

impl PhaseGuidance {
    /// Absolute paths are used as written; relative paths resolve against the
    /// configuration's directory (the control root).
    pub(crate) fn source(&self, control_root: &Path) -> PathBuf {
        let path = Path::new(&self.path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            control_root.join(path)
        }
    }
}

/// The phases an assignment of `role` at `attempt` is in when it is delivered.
/// A worker implements and runs tests, and repairs after its first attempt; a
/// reviewer reviews and may run checks; an adviser advises.
pub(crate) fn phases_for(role: &str, attempt: u64) -> Vec<&'static str> {
    match role {
        "worker" if attempt > 1 => vec!["implementation", "test", "repair"],
        "worker" => vec!["implementation", "test"],
        "reviewer" => vec!["review", "test"],
        "adviser" => vec!["advice"],
        _ => Vec::new(),
    }
}

pub(crate) fn validate(name: &str, value: Option<&Value>, errors: &mut Vec<String>) {
    let Some(value) = value else {
        return;
    };
    let prefix = format!("agents.{name}.{FIELD}");
    let Some(items) = value.as_array() else {
        errors.push(format!("{prefix} must be an array"));
        return;
    };
    if items.len() > MAX_ITEMS {
        errors.push(format!("{prefix} accepts at most {MAX_ITEMS} items"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        let at = format!("{prefix}[{index}]");
        let Some(object) = item.as_object() else {
            errors.push(format!("{at} must be an object"));
            continue;
        };
        if let Some(unknown) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "id" | "phase" | "path"))
        {
            errors.push(format!("{at} has unknown field '{unknown}'"));
        }
        match object.get("id").and_then(Value::as_str) {
            Some(id) if valid_id(id) => {
                if !ids.insert(id.to_owned()) {
                    errors.push(format!("{at}.id '{id}' is duplicated"));
                }
            }
            _ => errors.push(format!("{at}.id must match /^[a-z0-9][a-z0-9_-]{{0,63}}$/")),
        }
        if !object
            .get("phase")
            .and_then(Value::as_str)
            .is_some_and(|phase| PHASES.contains(&phase))
        {
            errors.push(format!("{at}.phase must be one of: {}", PHASES.join(", ")));
        }
        if !object
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|path| {
                !path.trim().is_empty() && path.len() <= 4096 && !path.contains('\0')
            })
        {
            errors.push(format!("{at}.path must be a non-empty file path"));
        }
    }
}

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declarations_are_typed_and_strict() {
        let mut errors = Vec::new();
        validate(
            "worker",
            Some(&json!([{"id":"failing-test-triage","phase":"test","path":"/skills/SKILL.md"}])),
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:?}");
        for bad in [
            json!({}),
            json!([{"id":"x","phase":"lunch","path":"a"}]),
            json!([{"id":"Bad","phase":"test","path":"a"}]),
            json!([{"id":"x","phase":"test","path":""}]),
            json!([{"id":"x","phase":"test","path":"a","extra":1}]),
            json!([{"id":"x","phase":"test","path":"a"},{"id":"x","phase":"review","path":"b"}]),
        ] {
            let mut errors = Vec::new();
            validate("worker", Some(&bad), &mut errors);
            assert!(!errors.is_empty(), "accepted {bad}");
        }
        assert_eq!(phases_for("worker", 1), ["implementation", "test"]);
        assert!(phases_for("worker", 2).contains(&"repair"));
        assert!(phases_for("lead", 1).is_empty());
    }
}

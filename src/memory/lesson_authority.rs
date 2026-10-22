//! Which agent configuration fields carry lesson authority.
//!
//! Every comparison that decides whether carried project lessons stay
//! compatible with a changed configuration reads this one list: `memory
//! revalidate`, the owner policy correction and the contextual opt-in. A
//! recipient's identity (`profile`, `nativeName`, `displayName`) and its
//! read-only `phaseGuidance` grant no memory right, so a change there is
//! recorded as a difference instead of refused. Every other agent field stays
//! exact; `memoryRead` keeps its own additive-only rule at each caller.
use serde_json::{json, Map, Value};

/// How lesson compatibility treats one agent field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Grants no memory right and carries no lesson authority; a change is
    /// recorded as a difference.
    NotAuthority,
    /// A recipient read right: compatible only when additive, checked by the
    /// caller that owns that rule.
    ReadRight,
    /// Rights, workflow, runtime, retention and descriptive policy: exact.
    Authority,
}

/// Every property of `$defs.agent` in `schema/exitbind.schema.json`.
pub(crate) const AGENT_FIELDS: &[(&str, Kind)] = &[
    ("profile", Kind::NotAuthority),
    ("displayName", Kind::NotAuthority),
    ("nativeName", Kind::NotAuthority),
    (crate::config::phase_guidance::FIELD, Kind::NotAuthority),
    ("memoryRead", Kind::ReadRight),
    ("purpose", Kind::Authority),
    ("observe", Kind::Authority),
    ("write", Kind::Authority),
    ("commands", Kind::Authority),
    ("skills", Kind::Authority),
    ("memoryWrite", Kind::Authority),
    ("memoryReview", Kind::Authority),
    ("memoryPromote", Kind::Authority),
    ("memoryReject", Kind::Authority),
    ("memoryRevoke", Kind::Authority),
    ("memoryExpire", Kind::Authority),
    ("memoryForget", Kind::Authority),
    ("retention", Kind::Authority),
    ("crossContext", Kind::Authority),
    ("runtime", Kind::Authority),
];

/// The agent fields that are not lesson authority, in list order.
pub(crate) fn not_authority() -> impl Iterator<Item = &'static str> {
    AGENT_FIELDS
        .iter()
        .filter(|(_, kind)| *kind == Kind::NotAuthority)
        .map(|(field, _)| *field)
}

fn agents_mut(config: &mut Value) -> Result<&mut Map<String, Value>, String> {
    config
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "agents missing".to_owned())
}

/// The configuration without schema URL metadata and without the fields that
/// are not lesson authority; with `strip_reads`, also without `memoryRead`.
pub(crate) fn projection(source: &str, strip_reads: bool) -> Result<Value, String> {
    let mut value: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("configuration is not an object")?
        .remove("$schema");
    for agent in agents_mut(&mut value)?.values_mut() {
        let agent = agent.as_object_mut().ok_or("agent is not an object")?;
        for field in not_authority() {
            agent.remove(field);
        }
        if strip_reads {
            agent.remove("memoryRead");
        }
    }
    Ok(value)
}

/// One `{agent, field, before, after}` entry per changed field that is not
/// lesson authority, for each agent present in both configurations.
pub(crate) fn differences(previous: &Value, current: &Value) -> Vec<Value> {
    let mut found = Vec::new();
    let Some(agents) = previous["agents"].as_object() else {
        return found;
    };
    for name in agents.keys() {
        if current["agents"].get(name).is_none() {
            continue;
        }
        for field in not_authority() {
            let before = &previous["agents"][name][field];
            let after = &current["agents"][name][field];
            if before != after {
                found.push(json!({"agent":name,"field":field,"before":before,"after":after}));
            }
        }
    }
    found
}

/// Copy every field that is not lesson authority from `current` onto
/// `expected`, so that an exact comparison then covers only authority.
pub(crate) fn adopt(expected: &mut Value, current: &Value) -> Result<(), String> {
    let names: Vec<String> = agents_mut(expected)?.keys().cloned().collect();
    for name in names {
        let Some(source) = current["agents"].get(&name).and_then(Value::as_object) else {
            continue;
        };
        let target = expected["agents"][&name]
            .as_object_mut()
            .ok_or("agent is not an object")?;
        for field in not_authority() {
            match source.get(field) {
                Some(value) => target.insert(field.to_owned(), value.clone()),
                None => target.remove(field),
            };
        }
    }
    Ok(())
}

/// Whether one recorded difference names a field that is not lesson authority.
pub(crate) fn is_recorded_difference(difference: &Value) -> bool {
    difference["agent"].as_str().is_some_and(|s| !s.is_empty())
        && difference["field"]
            .as_str()
            .is_some_and(|field| not_authority().any(|known| known == field))
}

#[cfg(test)]
#[path = "lesson_authority_tests.rs"]
mod tests;

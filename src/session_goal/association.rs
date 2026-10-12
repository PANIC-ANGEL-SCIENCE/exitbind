//! Bounded, context-only Work membership on the existing canonical Goal.

use serde_json::{json, Value};

pub(crate) const PROTOCOL: u64 = 1;
const MAX_ASSOCIATIONS: usize = 128;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    goal_id: String,
    revision: u64,
    event_sha256: String,
}

fn has_extension(record: &Value) -> bool {
    record.get("workAssociationProtocol").is_some() || record.get("workAssociations").is_some()
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

fn valid_work(value: &str) -> bool {
    value.strip_prefix("smw_").is_some_and(valid_sha)
}

fn entries(record: &Value) -> Result<Vec<Value>, String> {
    if !has_extension(record) {
        return Ok(Vec::new());
    }
    if record["workAssociationProtocol"].as_u64() != Some(PROTOCOL) {
        return Err("unsupported context-only Work association protocol".into());
    }
    let values = record["workAssociations"]
        .as_array()
        .ok_or("context-only Work associations are malformed")?;
    if values.is_empty() || values.len() > MAX_ASSOCIATIONS {
        return Err("context-only Work association bound is invalid".into());
    }
    let mut works = std::collections::BTreeSet::new();
    for value in values {
        let object = value
            .as_object()
            .ok_or("context-only Work association entry is malformed")?;
        if object.len() != 3 || object.get("kind").and_then(Value::as_str) != Some("context_only") {
            return Err("context-only Work association entry is malformed".into());
        }
        let work = object
            .get("work")
            .and_then(Value::as_str)
            .filter(|value| valid_work(value))
            .ok_or("context-only Work association has an invalid Work identity")?;
        let start = object
            .get("startEventSha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha(value))
            .ok_or("context-only Work association has an invalid start identity")?;
        if !works.insert(work.to_owned()) || start.trim().is_empty() {
            return Err("context-only Work association identity is duplicated".into());
        }
    }
    Ok(values.clone())
}

pub(super) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    entries(record)
        .map(|_| ())
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))
}

/// Retain the extension only while ordinary updates continue the same Goal ID.
pub(super) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] != next["goalId"] || !has_extension(previous) || has_extension(next) {
        return;
    }
    next["workAssociationProtocol"] = previous["workAssociationProtocol"].clone();
    next["workAssociations"] = previous["workAssociations"].clone();
}

/// Read-only selection before a Work start. Legacy single-Work continuation
/// records keep their established exclusive binding behavior.
pub(crate) fn prepare(loaded: &crate::config::Loaded) -> Result<Option<Candidate>, String> {
    let Some(record) = super::read(&loaded.state_root)? else {
        return Ok(None);
    };
    match record["closure"]["closed"].as_bool() {
        Some(false) => {}
        Some(true) => return Ok(None),
        None => return Err("canonical Goal closure state is invalid; no Work was started".into()),
    }
    if record["continuation"]["work"].is_string() {
        return Ok(None);
    }
    let work_associations = entries(&record)?;
    if work_associations.len() >= MAX_ASSOCIATIONS {
        return Err("context-only Work association bound reached; no Work was started".into());
    }
    Ok(Some(Candidate {
        goal_id: record["goalId"]
            .as_str()
            .ok_or("canonical Goal identity is missing")?
            .to_owned(),
        revision: record["revision"]
            .as_u64()
            .ok_or("canonical Goal revision is missing")?,
        event_sha256: record["eventSha256"]
            .as_str()
            .ok_or("canonical Goal event identity is missing")?
            .to_owned(),
    }))
}

/// Bind a started Work only if the canonical Goal still matches its preflight
/// snapshot. This function never changes named mappings or requirement state.
pub(crate) fn record(
    loaded: &crate::config::Loaded,
    candidate: &Candidate,
    work: &str,
    start_event_sha256: &str,
) -> Result<Value, String> {
    if !valid_work(work) || !valid_sha(start_event_sha256) {
        return Err("Work start identity is malformed".into());
    }
    super::mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("canonical Goal disappeared after Work start")?;
        if previous["goalId"].as_str() != Some(candidate.goal_id.as_str())
            || previous["revision"].as_u64() != Some(candidate.revision)
            || previous["eventSha256"].as_str() != Some(candidate.event_sha256.as_str())
            || previous["closure"]["closed"] != false
            || previous["continuation"]["work"].is_string()
        {
            return Err("canonical Goal changed after Work start".into());
        }
        let mut associations = entries(previous)?;
        if let Some(existing) = associations
            .iter()
            .find(|entry| entry["work"].as_str() == Some(work))
        {
            if existing["startEventSha256"].as_str() == Some(start_event_sha256) {
                return Ok(previous.clone());
            }
            return Err("Work identity is already associated with another start".into());
        }
        if associations.len() >= MAX_ASSOCIATIONS {
            return Err("context-only Work association bound reached".into());
        }
        associations.push(json!({
            "kind": "context_only",
            "work": work,
            "startEventSha256": start_event_sha256,
        }));
        let revision = previous["revision"]
            .as_u64()
            .ok_or("canonical Goal revision is missing")?
            .checked_add(1)
            .ok_or("canonical Goal revision exhausted")?;
        let mut next = previous.clone();
        next.as_object_mut()
            .ok_or("canonical Goal record is malformed")?
            .remove("eventSha256");
        next["revision"] = json!(revision);
        next["predecessor"] = json!({
            "goalId": candidate.goal_id,
            "revision": candidate.revision,
        });
        next["successorOf"] = Value::Null;
        next["workAssociationProtocol"] = json!(PROTOCOL);
        next["workAssociations"] = json!(associations);
        Ok(super::sealed(next, Some(previous)))
    })
}

pub(super) fn for_work(record: &Value, work: &str) -> Option<Value> {
    entries(record)
        .ok()?
        .into_iter()
        .find(|entry| entry["work"].as_str() == Some(work))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    fn goal(goal_id: &str, revision: u64) -> Value {
        json!({
            "kind": "lead_session_goal",
            "version": 2,
            "goalId": goal_id,
            "revision": revision,
            "predecessor": Value::Null,
            "successorOf": Value::Null,
            "closure": {"closed": false, "revision": Value::Null, "resultRefs": []},
            "continuation": Value::Null,
        })
    }

    fn sealed(mut value: Value, previous: Option<&Value>) -> Value {
        super::super::sealed(value.take(), previous)
    }

    #[test]
    fn legacy_records_remain_readable_and_unknown_protocols_refuse() {
        let legacy = sealed(goal("legacy", 1), None);
        let raw = format!("{}\n", legacy);
        assert!(super::super::records_from_raw(Some(&raw)).is_ok());

        let mut current = goal("current", 1);
        current["workAssociationProtocol"] = json!(super::PROTOCOL);
        current["workAssociations"] = json!([{
            "kind": "context_only",
            "work": format!("smw_{}", "a".repeat(64)),
            "startEventSha256": "b".repeat(64),
        }]);
        let current = sealed(current, None);
        let raw = format!("{}\n", current);
        assert!(super::super::records_from_raw(Some(&raw)).is_ok());

        let mut unknown = current;
        unknown["workAssociationProtocol"] = json!(super::PROTOCOL + 1);
        let unknown = sealed(unknown, None);
        let raw = format!("{}\n", unknown);
        assert!(super::super::records_from_raw(Some(&raw)).is_err());
    }

    #[test]
    fn same_goal_sealing_preserves_associations_but_successors_start_unbound() {
        let mut previous = goal("same", 1);
        previous["workAssociationProtocol"] = json!(super::PROTOCOL);
        previous["workAssociations"] = json!([{
            "kind": "context_only",
            "work": format!("smw_{}", "a".repeat(64)),
            "startEventSha256": "b".repeat(64),
        }]);
        let previous = sealed(previous, None);

        let mut same = goal("same", 2);
        same["predecessor"] = json!({"goalId":"same", "revision":1});
        let same = sealed(same, Some(&previous));
        assert_eq!(same["workAssociationProtocol"], super::PROTOCOL);
        assert_eq!(same["workAssociations"], previous["workAssociations"]);

        let mut successor = goal("next", 2);
        successor["predecessor"] = json!({"goalId":"same", "revision":1});
        successor["successorOf"] = json!({"goalId":"same", "revision":1});
        let successor = sealed(successor, Some(&previous));
        assert!(successor.get("workAssociationProtocol").is_none());
        assert!(successor.get("workAssociations").is_none());
    }
}

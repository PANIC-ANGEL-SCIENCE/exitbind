//! The `sessionAdoptions` extension of a canonical Goal record: each session
//! that adopted this journal, bounded, validated, and carried across
//! same-Goal rewrites.
use super::super::native::{valid_hash, Owner, Session};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

const FIELD: &str = "sessionAdoptions";
const MAX_ADOPTIONS: usize = 64;

pub(crate) struct Appended {
    pub(crate) record: Value,
    pub(crate) appended: bool,
}

/// Append one revision of the open Goal `goal_id` that lists `session` as
/// an adopter; unchanged when it is already listed.
pub(super) fn append(
    root: &Path,
    goal_id: &Value,
    session: &Session,
    owner: &Owner,
) -> Result<Appended, String> {
    let mut appended = false;
    let record = super::super::mutate(root, |previous| {
        let previous =
            previous.ok_or("the adopted Goal journal disappeared; nothing was written")?;
        if previous["goalId"] != *goal_id
            || super::super::goal_summary::closed_revision(previous).is_some()
        {
            return Err("the adopted Goal changed before adoption; nothing was written".into());
        }
        let mut adoptions = previous[FIELD].as_array().cloned().unwrap_or_default();
        if adoptions
            .iter()
            .any(|item| item["sessionKey"] == session.selector_key.as_str())
        {
            return Ok(previous.clone());
        }
        if adoptions.len() >= MAX_ADOPTIONS {
            return Err(format!(
                "the Goal journal already lists {MAX_ADOPTIONS} adopting sessions; nothing was written"
            ));
        }
        let revision = previous["revision"]
            .as_u64()
            .ok_or("canonical Goal revision is missing")?
            .checked_add(1)
            .ok_or("canonical Goal revision exhausted")?;
        adoptions.push(json!({
            "host":session.host,
            "sessionKey":session.selector_key,
            "ownerIdentitySha256":owner.identity_sha256,
            "ownerProjectId":owner.project_id,
            "revision":revision,
        }));
        let mut next = previous.clone();
        next.as_object_mut()
            .ok_or("canonical Goal record is malformed")?
            .remove("eventSha256");
        next["revision"] = json!(revision);
        next["predecessor"] = json!({"goalId":previous["goalId"],"revision":revision - 1});
        next["successorOf"] = Value::Null;
        next[FIELD] = json!(adoptions);
        appended = true;
        Ok(super::super::sealed(next, Some(previous)))
    })?;
    Ok(Appended { record, appended })
}

pub(crate) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    let Some(value) = record.get(FIELD) else {
        return Ok(());
    };
    let invalid =
        || format!("invalid canonical session goal line {line}: session adoptions are invalid");
    let items = value.as_array().ok_or_else(invalid)?;
    if items.is_empty() || items.len() > MAX_ADOPTIONS {
        return Err(invalid());
    }
    let mut keys = BTreeSet::new();
    for item in items {
        let valid = item.as_object().is_some_and(|object| object.len() == 5)
            && matches!(item["host"].as_str(), Some("codex" | "claude"))
            && item["sessionKey"].as_str().is_some_and(valid_hash)
            && item["ownerIdentitySha256"].as_str().is_some_and(valid_hash)
            && (item["ownerProjectId"].is_null() || item["ownerProjectId"].is_string())
            && item["revision"]
                .as_u64()
                .is_some_and(|revision| revision > 1)
            && keys.insert(item["sessionKey"].as_str().unwrap_or_default().to_owned());
        if !valid {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] == next["goalId"]
        && previous.get(FIELD).is_some()
        && next.get(FIELD).is_none()
    {
        next[FIELD] = previous[FIELD].clone();
    }
}

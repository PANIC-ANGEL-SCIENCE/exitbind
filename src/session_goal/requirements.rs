//! Agreed named requirements in the existing canonical goal stream.
//!
//! Coverage is a Lead decision. Satisfaction is derived from current observed
//! checks and accepted Works; neither a mapping nor source excerpts prove it.

mod model;
mod mutation;
mod view;
mod work;

use crate::{config::Loaded, evidence::hash};
use model::*;
use serde_json::{json, Value};

pub(crate) use mutation::{assign, cover, require};
pub(crate) use view::{all_current, projection};
pub(crate) use work::prepare;

pub(crate) fn named(record: &Value) -> bool {
    record["continuation"]["kind"] == "goal_requirements"
}

pub(crate) fn bound(record: &Value, work: &str) -> bool {
    named(record)
        && record["continuation"]["mappings"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["work"] == work))
}

pub(crate) fn unresolved(record: &Value) -> bool {
    let Ok(goal) = Goal::read(record) else {
        return true;
    };
    !goal.source.coverage_confirmed
        || goal.requirements.is_empty()
        || goal.requirements.iter().any(|requirement| {
            !goal
                .mappings
                .iter()
                .any(|mapping| mapping.active && mapping.requirement == requirement.reference())
        })
}

fn identity(loaded: &Loaded) -> Result<String, String> {
    let root = loaded
        .product_root
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = root
        .to_str()
        .ok_or("project identity path is not valid UTF-8")?;
    Ok(hash::value(&json!({"root":root,"id":loaded.project_id})))
}

fn source(loaded: &Loaded, path: &str) -> Result<Source, String> {
    use crate::project::path::SecureBytesResult;
    let bytes = match crate::project::path::secure_bytes_observation(
        &loaded.product_root,
        path,
        "requirement source",
    ) {
        SecureBytesResult::Bytes(bytes) => bytes,
        _ => {
            return Err(
                "requirement source must be a readable regular project file without symlinks"
                    .into(),
            )
        }
    };
    if bytes.is_empty() || bytes.len() > MAX_SOURCE {
        return Err("requirement source must contain 1–32768 bytes".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "requirement source is not valid UTF-8")?;
    if text.contains('\0') {
        return Err("requirement source contains NUL".into());
    }
    Ok(Source {
        reference: path.into(),
        sha256: hash::text(&text),
        text,
        coverage_confirmed: false,
    })
}

fn selected(goal: &Goal, ids: &str) -> Result<Vec<RequirementRef>, String> {
    let mut result = Vec::new();
    for id in ids.split(',') {
        if result.iter().any(|item: &RequirementRef| item.id == id) {
            return Err("requirement selection contains a duplicate ID".into());
        }
        let requirement = goal
            .requirements
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("requirement {id} is not in the agreed goal"))?;
        result.push(requirement.reference());
    }
    if result.is_empty() {
        return Err("select at least one agreed requirement".into());
    }
    Ok(result)
}

fn next_record(previous: &Value, goal_id: &str) -> Result<Value, String> {
    if previous["goalId"] != goal_id {
        return Err("canonical goal identity does not match".into());
    }
    let revision = previous["revision"]
        .as_u64()
        .ok_or("goal revision is missing")?;
    let mut record = previous.clone();
    record
        .as_object_mut()
        .ok_or("goal record malformed")?
        .remove("eventSha256");
    record["version"] = json!(4);
    record["revision"] = json!(revision.checked_add(1).ok_or("goal revision exhausted")?);
    record["predecessor"] = json!({"goalId":goal_id,"revision":revision});
    record["successorOf"] = Value::Null;
    record["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
    Ok(record)
}

pub(crate) fn close(loaded: &Loaded, goal_id: &str, result_ref: &str) -> Result<Value, String> {
    super::mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical goal is open")?;
        let goal = Goal::read(previous)?;
        if previous["goalId"] != goal_id {
            return Err("canonical goal identity does not match".into());
        }
        if !all_current(loaded, previous)? {
            return Err("required goal coverage is missing, failed, stale or unknown".into());
        }
        let contract = work::contract(loaded, result_ref)?;
        if !contract.integration
            || contract.goal_id != goal_id
            || contract.project_identity != identity(loaded)?
            || goal
                .requirements
                .iter()
                .any(|requirement| !contract.covers(&requirement.reference()))
        {
            return Err("closure needs a checked integration Work covering every current requirement revision".into());
        }
        if work::evidence(loaded, result_ref, &contract)?["state"] != "current" {
            return Err("integration Work lacks current observed checks, required review or Lead acceptance".into());
        }
        // Preserve conventional goal obligations and decisions as additional gates.
        if super::CATEGORIES
            .iter()
            .any(|category| !super::category_resolved(previous, category))
        {
            return Err("additional goal obligations or decisions remain unresolved".into());
        }
        for category in super::CATEGORIES {
            for item in previous[category]
                .as_array()
                .ok_or("goal category malformed")?
            {
                for reference in item["resultRefs"]
                    .as_array()
                    .ok_or("goal result references malformed")?
                {
                    let reference = reference
                        .as_str()
                        .ok_or("goal result reference malformed")?;
                    let evidence = crate::run::result_ref_evidence(loaded, reference)?
                        .ok_or("goal result is unavailable")?;
                    if !evidence.accepted
                        || !evidence.artifact_current
                        || evidence.drift.is_some()
                        || evidence.inputs_current != Some(true)
                    {
                        return Err("additional goal result is stale or unknown".into());
                    }
                }
                if item["disposition"] == "direct"
                    && !super::direct_completion_inputs(loaded, previous, item)?
                        .is_some_and(|(a, b)| a == b)
                {
                    return Err("additional direct completion is stale or unknown".into());
                }
            }
        }
        if previous["closure"]["closed"] == true
            && previous["closure"]["resultRefs"] == json!([result_ref])
        {
            return Ok(previous.clone());
        }
        let mut record = next_record(previous, goal_id)?;
        record["closure"] = json!({"closed":true,"revision":record["revision"],"resultRefs":[result_ref],"owner":"lead"});
        Ok(super::sealed(record, Some(previous)))
    })
}

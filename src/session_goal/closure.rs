//! Canonical goal closure through existing accepted-result authority.

use super::*;
use std::path::Path;

pub(crate) fn close(
    root: &Path,
    loaded: &crate::config::Loaded,
    goal_id: &str,
    result_ref: &str,
) -> Result<Value, String> {
    if read(root)?.as_ref().is_some_and(requirements::named) {
        return requirements::close(root, loaded, goal_id, result_ref);
    }
    let result_ref = text(Some(result_ref), "result-ref")?;
    mutate(root, |previous| {
        let previous = previous.ok_or("no canonical session goal is open")?;
        validate_governed_candidate(loaded, root, previous, goal_id, &result_ref)?;
        for category in CATEGORIES {
            for item in previous[category].as_array().into_iter().flatten() {
                if item["disposition"] == "direct" {
                    if let Some((recorded, current)) =
                        direct_completion_inputs(loaded, previous, item)?
                    {
                        warn_direct_completion_drift(category, &recorded, &current);
                    }
                }
            }
        }
        if previous["closure"]["closed"] == true
            && previous["closure"]["kind"] != "direct"
            && previous["closure"]["resultRefs"] == json!([result_ref])
        {
            return Ok(previous.clone());
        }
        let revision = previous["revision"]
            .as_u64()
            .ok_or("session goal revision is missing")?;
        let mut record = previous.clone();
        record
            .as_object_mut()
            .expect("validated session goal record")
            .remove("eventSha256");
        record["revision"] = json!(revision + 1);
        record["predecessor"] = json!({"goalId": goal_id, "revision": revision});
        record["successorOf"] = Value::Null;
        record["closure"] = json!({
            "closed": true,
            "revision": revision + 1,
            "resultRefs": [result_ref],
            "owner": "lead",
        });
        Ok(sealed(record, Some(previous)))
    })
}

/// Read-only counterpart to ordinary governed close; the status projection
/// uses this exact candidate predicate before showing its route.
pub(crate) fn validate_governed_candidate(
    loaded: &crate::config::Loaded,
    root: &Path,
    previous: &Value,
    goal_id: &str,
    result_ref: &str,
) -> Result<(), String> {
    if previous["goalId"].as_str() != Some(goal_id) {
        return Err("session goal identity does not match the current revision".into());
    }
    super::journey::require_close_ready(loaded, root, previous)?;
    if unresolved(previous) {
        return Err("session goal has unresolved or unconsidered obligations, findings, blockers, decisions, or external actions".into());
    }
    if !continuity::support_current(loaded, previous)? {
        return Err("continuation support is stale or incomplete".into());
    }
    for category in CATEGORIES {
        for item in previous[category].as_array().into_iter().flatten() {
            if item["disposition"] == "open" {
                continue;
            }
            if item["disposition"] == "direct" {
                if direct_completion_inputs(loaded, previous, item)?.is_none() {
                    return Err(format!("{category} direct completion is invalid"));
                }
            } else {
                let refs = item["resultRefs"]
                    .as_array()
                    .ok_or_else(|| format!("{category} has malformed result references"))?;
                if refs.is_empty() {
                    return Err(format!("{category} has no governed result evidence"));
                }
                for reference in refs {
                    let reference = reference
                        .as_str()
                        .ok_or_else(|| format!("{category} has malformed result references"))?;
                    close_result_ref(loaded, reference).map_err(|error| {
                        format!("{category} result reference is stale or unavailable: {error}")
                    })?;
                }
            }
        }
    }
    close_result_ref(loaded, result_ref)
}

/// Close a goal from explicit Lead-reported direct facts.  A governed result
/// may be supplied for a mixed goal; when it is absent, no run is required.
pub(crate) fn close_direct(
    root: &Path,
    loaded: &crate::config::Loaded,
    goal_id: &str,
    result_ref: Option<&str>,
) -> Result<Value, String> {
    if read(root)?.as_ref().is_some_and(requirements::named) {
        return Err(
            "named requirements need observed checked Work evidence; direct closure is unavailable"
                .into(),
        );
    }
    let result_ref = result_ref
        .map(|value| text(Some(value), "result-ref"))
        .transpose()?;
    mutate(root, |previous| {
        let previous = previous.ok_or("no canonical session goal is open")?;
        let inputs_sha256 =
            validate_direct_candidate(loaded, root, previous, goal_id, result_ref.as_deref())?;
        for category in CATEGORIES {
            for item in previous[category].as_array().into_iter().flatten() {
                if item["disposition"] == "direct" {
                    if let Some((recorded, current)) =
                        direct_completion_inputs(loaded, previous, item)?
                    {
                        warn_direct_completion_drift(category, &recorded, &current);
                    }
                }
            }
        }
        if previous["closure"]["closed"] == true
            && previous["closure"]["kind"] == "direct"
            && previous["closure"]["inputsSha256"] == inputs_sha256
            && previous["closure"]["resultRefs"]
                == result_ref
                    .as_ref()
                    .map_or_else(|| json!([]), |value| json!([value]))
        {
            return Ok(previous.clone());
        }
        let revision = previous["revision"]
            .as_u64()
            .ok_or("session goal revision is missing")?;
        let mut record = previous.clone();
        record
            .as_object_mut()
            .expect("validated session goal record")
            .remove("eventSha256");
        record["revision"] = json!(revision + 1);
        record["predecessor"] = json!({"goalId": goal_id, "revision": revision});
        record["successorOf"] = Value::Null;
        record["closure"] = json!({
            "closed": true,
            "revision": revision + 1,
            "resultRefs": result_ref.map_or_else(|| json!([]), |value| json!([value])),
            "owner": "lead",
            "kind": "direct",
            "inputsSha256": inputs_sha256,
        });
        Ok(sealed(record, Some(previous)))
    })
}

/// Read-only counterpart to the direct close mutation. Goal progress uses this
/// exact predicate before offering an executable close route.
pub(crate) fn validate_direct_candidate(
    loaded: &crate::config::Loaded,
    root: &Path,
    previous: &Value,
    goal_id: &str,
    result_ref: Option<&str>,
) -> Result<String, String> {
    if requirements::named(previous) {
        return Err("named requirements need observed checked Work evidence".into());
    }
    if previous["goalId"].as_str() != Some(goal_id) {
        return Err("session goal identity does not match the current revision".into());
    }
    super::journey::require_close_ready(loaded, root, previous)?;
    if unresolved(previous) {
        return Err("session goal has unresolved or unconsidered categories".into());
    }
    if !continuity::support_current(loaded, previous)? {
        return Err("continuation support is stale or incomplete".into());
    }
    let mut has_governed_evidence = false;
    for category in CATEGORIES {
        for item in previous[category].as_array().into_iter().flatten() {
            if item["disposition"] == "open" {
                continue;
            }
            if item["disposition"] == "direct" {
                if direct_completion_inputs(loaded, previous, item)?.is_none() {
                    return Err(format!("{category} direct completion is invalid"));
                }
            } else {
                has_governed_evidence = true;
                let refs = item["resultRefs"]
                    .as_array()
                    .ok_or_else(|| format!("{category} has malformed result references"))?;
                if refs.is_empty() {
                    return Err(format!("{category} has no governed result evidence"));
                }
                for reference in refs {
                    close_result_ref(
                        loaded,
                        reference
                            .as_str()
                            .ok_or_else(|| format!("{category} has malformed result references"))?,
                    )?;
                }
            }
        }
    }
    if has_governed_evidence && result_ref.is_none() {
        return Err(
            "mixed session goal requires --result-ref for governed closure evidence".into(),
        );
    }
    if let Some(result_ref) = result_ref {
        close_result_ref(loaded, result_ref)?;
    }
    current_inputs(loaded)
}

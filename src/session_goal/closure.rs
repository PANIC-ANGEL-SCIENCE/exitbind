//! Canonical goal closure through existing accepted-result authority.

use super::*;

pub(crate) fn close(
    loaded: &crate::config::Loaded,
    goal_id: &str,
    result_ref: &str,
) -> Result<Value, String> {
    if read(&loaded.state_root)?
        .as_ref()
        .is_some_and(requirements::named)
    {
        return requirements::close(loaded, goal_id, result_ref);
    }
    let result_ref = text(Some(result_ref), "result-ref")?;
    mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical session goal is open")?;
        if previous["goalId"].as_str() != Some(goal_id) {
            return Err("session goal identity does not match the current revision".into());
        }
        if unresolved(previous) {
            return Err("session goal has unresolved or unconsidered obligations, findings, blockers, decisions, or external actions".into());
        }
        if !continuity::support_current(loaded, previous)? {
            return Err("continuation support is stale or incomplete".into());
        }
        for category in CATEGORIES {
            if let Some(items) = previous[category].as_array() {
                for item in items {
                    if item["disposition"] == "open" {
                        continue;
                    }
                    if item["disposition"] == "direct" {
                        let Some((recorded, current)) =
                            direct_completion_inputs(loaded, previous, item)?
                        else {
                            return Err(format!("{category} direct completion is invalid"));
                        };
                        warn_direct_completion_drift(category, &recorded, &current);
                        continue;
                    }
                    let Some(refs) = item["resultRefs"].as_array() else {
                        return Err(format!("{category} has malformed result references"));
                    };
                    if refs.is_empty() {
                        return Err(format!("{category} has no governed result evidence"));
                    }
                    for reference in refs {
                        let Some(reference) = reference.as_str() else {
                            return Err(format!("{category} has malformed result references"));
                        };
                        close_result_ref(loaded, reference)?;
                    }
                }
            }
        }
        close_result_ref(loaded, &result_ref)?;
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

/// Close a goal from explicit Lead-reported direct facts.  A governed result
/// may be supplied for a mixed goal; when it is absent, no run is required.
pub(crate) fn close_direct(
    loaded: &crate::config::Loaded,
    goal_id: &str,
    result_ref: Option<&str>,
) -> Result<Value, String> {
    if read(&loaded.state_root)?
        .as_ref()
        .is_some_and(requirements::named)
    {
        return Err(
            "named requirements need observed checked Work evidence; direct closure is unavailable"
                .into(),
        );
    }
    let result_ref = result_ref
        .map(|value| text(Some(value), "result-ref"))
        .transpose()?;
    mutate(&loaded.state_root, |previous| {
        let previous = previous.ok_or("no canonical session goal is open")?;
        if previous["goalId"].as_str() != Some(goal_id) {
            return Err("session goal identity does not match the current revision".into());
        }
        if unresolved(previous) {
            return Err("session goal has unresolved or unconsidered obligations, findings, blockers, decisions, or external actions".into());
        }
        if !continuity::support_current(loaded, previous)? {
            return Err("continuation support is stale or incomplete".into());
        }
        let mut has_governed_evidence = false;
        for category in CATEGORIES {
            if let Some(items) = previous[category].as_array() {
                for item in items {
                    if item["disposition"] == "open" {
                        continue;
                    }
                    if item["disposition"] == "direct" {
                        let Some((recorded, current)) =
                            direct_completion_inputs(loaded, previous, item)?
                        else {
                            return Err(format!("{category} direct completion is invalid"));
                        };
                        warn_direct_completion_drift(category, &recorded, &current);
                        continue;
                    }
                    has_governed_evidence = true;
                    let Some(refs) = item["resultRefs"].as_array() else {
                        return Err(format!("{category} has malformed result references"));
                    };
                    if refs.is_empty() {
                        return Err(format!("{category} has no governed result evidence"));
                    }
                    for reference in refs {
                        let Some(reference) = reference.as_str() else {
                            return Err(format!("{category} has malformed result references"));
                        };
                        close_result_ref(loaded, reference)?;
                    }
                }
            }
        }
        if has_governed_evidence && result_ref.is_none() {
            return Err(
                "mixed session goal requires --result-ref for governed closure evidence".into(),
            );
        }
        if let Some(result_ref) = result_ref.as_deref() {
            close_result_ref(loaded, result_ref)?;
        }
        let inputs_sha256 = current_inputs(loaded)?;
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

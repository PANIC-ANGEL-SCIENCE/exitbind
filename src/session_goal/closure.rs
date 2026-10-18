//! Canonical goal closure through existing accepted-result authority.

use super::{
    continuity, current_inputs, items, journey, mutate, read, recovery, requirements, sealed, text,
    CATEGORIES,
};
use serde_json::{json, Value};
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
                    close_result_ref(loaded, previous, reference).map_err(|error| {
                        format!("{category} result reference is stale or unavailable: {error}")
                    })?;
                }
            }
        }
    }
    close_result_ref(loaded, previous, result_ref)
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
                        previous,
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
        close_result_ref(loaded, previous, result_ref)?;
    }
    current_inputs(loaded)
}

/// Record one explicit Lead-reported direct completion.  This is deliberately
/// separate from governed result references: the claim is bound to the
/// current product inputs and goal revision, but it is not check evidence.
pub(crate) struct DirectCompletionRequest<'a> {
    pub(crate) goal_id: &'a str,
    pub(crate) goal: &'a str,
    pub(crate) category: &'a str,
    pub(crate) item: &'a str,
    pub(crate) external_scope: Option<&'a str>,
    pub(crate) native_session: bool,
}

pub(crate) fn direct_complete(
    root: &Path,
    loaded: &crate::config::Loaded,
    request: DirectCompletionRequest<'_>,
) -> Result<Value, String> {
    let DirectCompletionRequest {
        goal_id,
        goal,
        category,
        item,
        external_scope,
        native_session,
    } = request;
    let goal_id = text(Some(goal_id), "goal-id")?;
    let goal = text(Some(goal), "goal")?;
    let item = text(Some(item), category)?;
    if !CATEGORIES.contains(&category) {
        return Err(format!("unknown session-goal category '{category}'"));
    }
    mutate(root, |previous| {
        let revision = previous
            .and_then(|value| value["revision"].as_u64())
            .unwrap_or(0)
            .saturating_add(1);
        let inputs_sha256 = current_inputs(loaded)?;
        let mut obligations = items(previous, "obligations");
        let mut findings = items(previous, "findings");
        let mut blockers = items(previous, "blockers");
        let mut decisions = items(previous, "decisions");
        let mut external_actions = items(previous, "externalActions");
        match category {
            "obligations" => direct_item(
                &mut obligations,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "findings" => direct_item(
                &mut findings,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "blockers" => direct_item(
                &mut blockers,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "decisions" => direct_item(
                &mut decisions,
                item.clone(),
                item.clone(),
                &goal_id,
                revision,
                &inputs_sha256,
                category,
            )?,
            "externalActions" => direct_external_item(
                &mut external_actions,
                item.clone(),
                external_scope.unwrap_or("in_scope"),
                &goal_id,
                revision,
                &inputs_sha256,
            )?,
            _ => unreachable!(),
        }
        let mut category_state = previous
            .and_then(|value| value["categories"].as_object().cloned())
            .unwrap_or_default();
        category_state.insert(category.into(), json!("considered"));
        let predecessor =
            previous.map(|value| json!({"goalId": value["goalId"], "revision": value["revision"]}));
        let successor_of = previous
            .filter(|value| value["goalId"] != goal_id)
            .map(|value| json!({"goalId": value["goalId"], "revision": value["revision"]}));
        let mut next = json!({
            "kind": "lead_session_goal",
            "version": if previous.is_some_and(|record| record["goalId"] == goal_id && !record["continuation"].is_null()) { 3 } else { 2 },
            "goalId": goal_id,
            "revision": revision,
            "goal": goal,
            "source": {"owner": "lead", "kind": "explicit_direct_completion"},
            "predecessor": predecessor,
            "successorOf": successor_of,
            "obligations": obligations,
            "findings": findings,
            "blockers": blockers,
            "decisions": decisions,
            "externalActions": external_actions,
            "categories": category_state,
            "closure": {"closed": false, "revision": Value::Null, "resultRefs": []},
            "continuation": previous.filter(|record| record["goalId"] == goal_id).map_or(Value::Null, |record| record["continuation"].clone()),
        });
        journey::transition(previous, &mut next, native_session)?;
        Ok(sealed(next, previous))
    })
}

pub(super) fn category_resolved(record: &Value, key: &str) -> bool {
    if !matches!(
        record["categories"][key].as_str(),
        Some("considered") | Some("none_applicable")
    ) {
        return false;
    }
    record[key].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            if item["disposition"].as_str() == Some("direct") {
                item["resultRefs"].as_array().is_some_and(Vec::is_empty)
                    && item["directCompletion"]["kind"] == "lead_reported"
                    && item["directCompletion"]["goalId"].is_string()
                    && item["directCompletion"]["revision"].is_u64()
                    && item["directCompletion"]["inputsSha256"].is_string()
            } else {
                matches!(
                    item["disposition"].as_str(),
                    Some("accepted") | Some("outside_scope") | Some("successor")
                ) && item["resultRefs"]
                    .as_array()
                    .is_some_and(|refs| !refs.is_empty())
            }
        })
    })
}

pub(super) fn unresolved(record: &Value) -> bool {
    if continuity::has_unresolved(record) {
        return true;
    }
    CATEGORIES
        .iter()
        .any(|category| !category_resolved(record, category))
}

pub(super) fn direct_completion_inputs(
    loaded: &crate::config::Loaded,
    record: &Value,
    item: &Value,
) -> Result<Option<(String, String)>, String> {
    let completion = &item["directCompletion"];
    if item["disposition"] != "direct"
        || !item["resultRefs"].as_array().is_some_and(Vec::is_empty)
        || completion["kind"] != "lead_reported"
        || completion["goalId"] != record["goalId"]
    {
        return Ok(None);
    }
    let Some(revision) = completion["revision"].as_u64() else {
        return Ok(None);
    };
    let Some(current_revision) = record["revision"].as_u64() else {
        return Ok(None);
    };
    let Some(expected_inputs) = completion["inputsSha256"].as_str() else {
        return Ok(None);
    };
    if revision == 0 || revision > current_revision {
        return Ok(None);
    }
    Ok(Some((expected_inputs.to_owned(), current_inputs(loaded)?)))
}

fn close_result_ref(
    loaded: &crate::config::Loaded,
    record: &Value,
    reference: &str,
) -> Result<(), String> {
    let (accepted, artifact_current, drift, inputs_current) =
        if let Some(evidence) = recovery::result_ref_evidence(loaded, record, reference)? {
            (
                evidence.accepted,
                evidence.artifact_current,
                evidence.drift,
                evidence.inputs_current,
            )
        } else {
            let Some(evidence) = crate::run::result_ref_evidence(loaded, reference)? else {
                return Err("session goal result reference is not a governed result".into());
            };
            (
                evidence.accepted,
                evidence.artifact_current,
                evidence.drift,
                evidence.inputs_current,
            )
        };
    if !accepted || !artifact_current {
        return Err("session goal result reference is not a valid accepted result".into());
    }
    warn_result_ref_drift(reference, drift.as_ref(), inputs_current);
    Ok(())
}

fn warn_result_ref_drift(reference: &str, drift: Option<&Value>, inputs_current: Option<bool>) {
    if let Some(warning) = drift {
        let classification = warning["classification"].as_str().unwrap_or("input_drift");
        eprintln!(
            "warning: {classification} detected for accepted result {reference}; retaining its recorded evidence"
        );
    }
    if inputs_current == Some(false) {
        eprintln!(
            "warning: tested_inputs_drift detected for accepted result {reference}; retaining recorded tested-input identity"
        );
    } else if inputs_current.is_none() {
        eprintln!(
            "warning: tested_inputs_currentness unknown for historical result {reference}; retaining recorded evidence"
        );
    }
}

fn warn_direct_completion_drift(
    category: &str,
    recorded_inputs_sha256: &str,
    current_inputs_sha256: &str,
) {
    if recorded_inputs_sha256 != current_inputs_sha256 {
        eprintln!(
            "warning: tested_inputs_drift detected for {category}; retaining its recorded completion"
        );
    }
}

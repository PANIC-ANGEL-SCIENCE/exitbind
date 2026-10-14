//! Current bindings and read-only, recipient-bound grouped section expansion.
//!
//! A reference selects bytes derived from the current validated snapshot. It
//! conveys no authority and is never interpreted as a filesystem path.

use crate::{config::Loaded, evidence::hash, run::RunSnapshot};
use serde_json::{json, Value};

const PAGE_BYTES: usize = 24 * 1024;
const SMALL_PAGE_BYTES: usize = 4 * 1024;
pub(crate) const MAX_DETAIL_BYTES: usize = 64 * 1024;
const PREFIX: &str = "ref:work-section:";
const SMALL_PREFIX: &str = "ref:work-section:v2:";
const SECTIONS: [&str; 4] = ["assignment", "evidence", "tasks", "recipient"];

pub(crate) fn binding(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<String, String> {
    // The loaded roots/config and canonical recipient are part of the digest,
    // rather than ambient CWD/PATH or a caller-selected role. A new grant,
    // repair, goal revision, profile/boundary change or assignment revokes it.
    Ok(hash::value(&json!({
        "version": 1, "work": work,
        "config": loaded.path, "configBytes": loaded.source,
        "product": loaded.product_root, "state": loaded.state_root,
        "project": loaded.project_id,
        "snapshot": snapshot.inspect_view()["ledgerSha256"],
        "recipient": {"role": next["role"], "agent": next["agent"],
            "assignment": next["assignment"]},
        "packet": next["packet"],
        "taskConditions": crate::session_goal::progress_detail_for_work(loaded, work, &next["progress"] )?,
        "goalRevision": crate::session_goal::record_for_work(loaded, work)
            .as_ref()
            .map(hash::value),
        "earlyEvidence": super::early_evidence::projection(loaded, work, snapshot)?,
    })))
}

pub(crate) fn ensure_current_config(loaded: &Loaded) -> Result<(), String> {
    let path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let fresh = crate::config::load(Some(path))?;
    if fresh.source != loaded.source
        || fresh.product_root != loaded.product_root
        || fresh.state_root != loaded.state_root
        || fresh.control_root != loaded.control_root
        || fresh.project_id != loaded.project_id
    {
        return Err(
            "configuration changed during work detail consumption; refresh with work next".into(),
        );
    }
    Ok(())
}

pub(crate) fn ensure_action_binding(
    loaded: &Loaded,
    work: &str,
    expected: Option<&str>,
) -> Result<(), String> {
    let Some(expected) = expected else {
        return Ok(());
    };
    ensure_current_config(loaded)?;
    let next = super::next_for(loaded, work, &super::resolve(loaded, work)?)?;
    if next["current"]["binding"] != expected {
        return Err(format!("current action form binding is stale; refresh with work next {work} --json and follow current.details.grouped"));
    }
    Ok(())
}

fn token(identity: &str, section: &str, page: usize) -> String {
    format!("{SMALL_PREFIX}{identity}:{section}:{page}")
}

fn route(loaded: &Loaded, work: &str, id: &str) -> Value {
    let command = super::response_recovery::bounded_argv(
        vec![
            "work".into(),
            "expand".into(),
            work.into(),
            id.into(),
            "--json".into(),
            "--config".into(),
        ],
        loaded.path.to_str(),
        // Current section routes share the bounded response with the
        // grouped route and its effective-action projection. Preserve every
        // route and require exact invocation arguments when paths are long.
        768,
    );
    json!({"reference": id, "command": command.argv,
        "sameConfigRequired": command.same_config,
        "sameExecutableRequired": command.same_executable,
        "readOnly": true})
}

pub(crate) fn legacy_evidence_route(loaded: &Loaded, work: &str, binding: &str) -> Value {
    route(loaded, work, &token(binding, "evidence", 0))
}

pub(super) fn attach(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &mut Value,
) -> Result<(), String> {
    let progress = crate::session_goal::progress_for_work(loaded, work, &next["progress"])?;
    next["goalProgress"] = progress.clone();
    next["goalProgress"]["usageAccount"] = crate::session_goal::usage::for_work(loaded, work);
    match crate::session_goal::project_goal_progress_for_work(loaded, work) {
        Ok(Some(project_progress)) => next["projectGoalProgress"] = project_progress,
        Ok(None) => {}
        Err(error) => {
            // A named-contract locator is optional display metadata for Work
            // recovery. Keep mandatory Work detail readable if it is damaged.
            next["projectGoalProgress"] = json!({
                "kind":"project_named_contract",
                "state":"unavailable",
                "reason":"named_goal_projection_unavailable",
                "detail":error,
                "readOnly":true,
            });
        }
    }
    let view = snapshot.inspect_view();
    let subject = if next["packet"]["context"]["subject"].is_object() {
        next["packet"]["context"]["subject"].clone()
    } else {
        view["subject"].clone()
    };
    let binding = binding(loaded, work, snapshot, next)?;
    let mut routes = json!({});
    for section in SECTIONS {
        routes[section] = route(loaded, work, &token(&binding, section, 0));
    }
    let residual = super::packet::project(work, snapshot, next)?;
    let events = view["events"].as_array().cloned().unwrap_or_default();
    let repair = events
        .iter()
        .rev()
        .find(|event| matches!(event["outcome"].as_str(), Some("rework")))
        .map(|event| {
            json!({
                "eventSha256": event["eventSha256"], "reason": event["reason"],
                "outcome": event["outcome"], "priorRecords": "retained_history",
            })
        });
    let last = events
        .iter()
        .rev()
        .find(|event| event["stage"].is_number())
        .cloned()
        .unwrap_or(Value::Null);
    let stage = if next["packet"]["stage"].is_number() {
        next["packet"]["stage"].clone()
    } else if view["currentStage"].is_number() {
        view["currentStage"].clone()
    } else {
        last["stage"].clone()
    };
    let attempt = if next["packet"]["attempt"].is_number() {
        next["packet"]["attempt"].clone()
    } else if view["attempt"].is_number() {
        view["attempt"].clone()
    } else {
        last["attempt"].clone()
    };
    let artifact_exists = subject["workerArtifactSha256"].is_string();
    let current_attempt = subject["attempt"] == attempt;
    next["current"] = json!({
        "version": 1, "work": work,
        "binding": binding,
        "goal": {"workGoalSha256": subject["goalSha256"],
            "overallGoalId": if next["goalProgress"]["goal"].is_string() { crate::session_goal::record_for_work(loaded, work).map(|record| hash::value(&record["goalId"])) } else { None },
            "overallAvailable": next["goalProgress"]["goal"].is_string()},
        "result": {"available": subject.is_object(),
            "exists": if subject.is_object() { json!(artifact_exists && current_attempt) } else { Value::Null },
            "retainedPreviousResult": artifact_exists && !current_attempt,
            "subject": {"sha256":subject["sha256"], "attempt":subject["attempt"], "workerArtifactSha256":subject["workerArtifactSha256"]}, "historical": view["status"] != "running" || (artifact_exists && !current_attempt)},
        "phase": {"stage": stage, "attempt": attempt, "status": view["status"]},
        "recipient": {"role": next["role"], "agent": next["agent"],
            "assignment": next["assignment"], "actor": next["resolvedActor"]},
        "action": next["action"],
        "missing": {"count": residual["remaining"].as_array().map_or(0, Vec::len),
            "items": residual["remaining"].as_array().map(|items| items.iter().take(8).map(|item| json!({"obligation":item["obligation"], "requirementIdSha256":item["requirementId"].as_str().map(hash::text)})).collect::<Vec<_>>()),
            "omitted": residual["remaining"].as_array().map_or(0, |items| items.len().saturating_sub(8)), "detail": "assignment"},
        "readiness": next["progress"]["state"],
        "repair": repair,
        "completeness": {"binding": "complete", "instructions": "requires_detail",
            "evidence": "requires_detail", "tasks": "requires_detail"},
        "details": routes,
    });
    crate::work::readable::attach(loaded, work, &binding, &mut next["current"]);
    if let Some(relation) = replacement_relation(loaded, work, &events)? {
        next["current"]["checkReplacement"] = relation;
    }
    next["current"]["actionForm"] =
        crate::work::action_forms::project(loaded, work, next, &binding);
    if next["action"] == "done" {
        next["current"]["completion"] = super::closeout::projection(loaded, work, snapshot, next);
    }
    Ok(())
}

fn replacement_policy_projection(marker: &Value, mut relation: Value) -> Value {
    if marker["target"] == "pre_return" {
        relation["target"] = json!("pre_return");
        for side in ["predecessor", "successor"] {
            if let Some(object) = relation[side].as_object_mut() {
                if let Some(policy) = object.remove("checkPolicy") {
                    object.insert("preReturnPolicy".into(), policy);
                }
            }
        }
        relation["finalCheckPolicyPreserved"] = json!(true);
        relation["meaning"] = json!("The predecessor remains immutable. Only the explicitly defective pre-return policy changed; the successor gets no old grant, diagnostic pass, final check or review credit.");
    }
    relation
}

pub(super) fn replacement_relation(
    loaded: &Loaded,
    work: &str,
    events: &[Value],
) -> Result<Option<Value>, String> {
    let proposed_start = events.first().ok_or("Work ledger has no start")?;
    if proposed_start.get("checkReplacement").is_some() {
        let ledger = super::resolve(loaded, work)?;
        let (_, current_events, _) = crate::run::ledger::load(loaded, &ledger)?;
        if !events_match_committed_or_submit_preview(events, &current_events) {
            return Err(
                "check replacement changed during Work detail; refresh the current Work view"
                    .into(),
            );
        }
        let start = current_events.first().ok_or("Work ledger has no start")?;
        let marker = start
            .get("checkReplacement")
            .ok_or("check replacement changed during Work detail; refresh the current Work view")?;
        if proposed_start.get("checkReplacement") != Some(marker) {
            return Err(
                "check replacement changed during Work detail; refresh the current Work view"
                    .into(),
            );
        }
        let predecessor = start["supersedes"]["ledgerPath"]
            .as_str()
            .unwrap_or_default();
        let previous_work = crate::work::locator_for_ledger(predecessor);
        return Ok(Some(replacement_policy_projection(
            marker,
            json!({"state":"verified","direction":"successor",
            "predecessor":{"work":previous_work,"ledger":predecessor,
                "runId":start["supersedes"]["runId"],"headEventSha256":start["supersedes"]["headEventSha256"],
                "checkPolicy":marker["oldPolicy"]},
            "successor":{"work":work,"runId":start["runId"],"checkPolicy":marker["newPolicy"]},
            "source":marker["source"],"reason":marker["reason"],"classification":marker["classification"],"leadOwner":marker["leadOwner"],
            "meaning":"The predecessor is immutable; the successor has a new check contract and receives no prior check, review or grant credit."}),
        )));
    }
    let ledger = super::resolve(loaded, work)?;
    let ledger_path = crate::run::ledger::load(loaded, &ledger)?.0;
    let Some(claim) =
        crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&ledger_path))?
    else {
        return Ok(None);
    };
    let Some(marker) = claim.get("checkReplacement") else {
        return Ok(None);
    };
    let successor_ledger = claim["newLedgerPath"].as_str().unwrap_or_default();
    let verified = crate::run::ledger::load(loaded, successor_ledger)
        .ok()
        .is_some_and(|(_, successor_events, _)| {
            successor_events.first().is_some_and(|successor| {
                successor["checkReplacement"] == *marker
                    && successor["supersedes"]["ledgerPath"] == claim["oldLedgerPath"]
                    && successor["supersedes"]["headEventSha256"] == claim["oldHeadEventSha256"]
            })
        });
    if !verified {
        return Ok(Some(json!({"state":"unverified","direction":"predecessor",
            "predecessor":{"work":work,"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"]},
            "successor":{"work":crate::work::locator_for_ledger(successor_ledger),"ledger":successor_ledger,"runId":claim["newRunId"]},
            "reason":"successor ledger or provenance could not be verified; no continuation is authorized"})));
    }
    Ok(Some(replacement_policy_projection(
        marker,
        json!({"state":"verified","direction":"predecessor",
        "predecessor":{"work":work,"ledger":ledger,"runId":claim["oldRunId"],"headEventSha256":claim["oldHeadEventSha256"],"checkPolicy":marker["oldPolicy"]},
        "successor":{"work":crate::work::locator_for_ledger(successor_ledger),"ledger":successor_ledger,"runId":claim["newRunId"],"checkPolicy":marker["newPolicy"]},
        "source":marker["source"],"reason":marker["reason"],"classification":marker["classification"],"leadOwner":marker["leadOwner"],
        "meaning":"This predecessor remains immutable; its successor has a new check contract and receives no prior check, review or grant credit."}),
    )))
}

/// A preflight projection may contain the one submit transaction being
/// considered, while the ledger on disk is still the checked committed base.
/// Every other consumer must present that exact base; no arbitrary suffix or
/// changed committed history is a valid preview.
fn events_match_committed_or_submit_preview(candidate: &[Value], committed: &[Value]) -> bool {
    if candidate == committed {
        return true;
    }
    if candidate.len() != committed.len().saturating_add(1)
        || candidate.get(..committed.len()) != Some(committed)
    {
        return false;
    }
    let Some(previous) = committed.last() else {
        return false;
    };
    let Some(submission) = candidate.last() else {
        return false;
    };
    submission["kind"] == "run"
        && submission["action"] == "submit"
        && submission["version"] == previous["version"]
        && submission["runId"] == previous["runId"]
        && submission["previousEventSha256"] == previous["eventSha256"]
        && crate::run::state::validate_event(submission, Some(previous), committed.len() + 1)
            .is_ok()
}

pub(crate) fn section_value(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    name: &str,
) -> Result<Value, String> {
    match name {
        "assignment" => Ok(json!({"available": next["packet"].is_object(),
            "assignment": next["packet"], "outcomes": next["outcomes"],
            "residual": super::packet::project(work, snapshot, next)?})),
        "tasks" => crate::session_goal::progress_detail_for_work(loaded, work, &next["progress"]),
        "evidence" => evidence(loaded, snapshot, next, false),
        "recipient" => super::readable::recipient_context::section(loaded, next),
        _ => Err("unknown work section".into()),
    }
}

fn verified(
    loaded: &Loaded,
    artifact: &Value,
    label: &str,
    readable: bool,
) -> Result<Value, String> {
    let read = crate::run::artifact::read(loaded, artifact, label)?;
    let complete = read.bytes as usize == read.preview.len();
    if !readable {
        if !complete {
            return Err(format!("{label} exceeds the existing verified artifact bound; complete evidence is unavailable"));
        }
        return Ok(json!({"sha256": read.sha256, "bytes": read.bytes,
            "encoding": "hex", "contentHex": hex(&read.preview), "complete": true}));
    }
    let mut value = json!({"sha256": read.sha256, "bytes": read.bytes,
        "verified": true, "complete": complete, "exact": complete, "readable": false});
    if !complete {
        value["reason"] = json!("oversized");
    } else if let Ok(text) = std::str::from_utf8(&read.preview) {
        value["readable"] = json!(true);
        value["encoding"] = json!("utf-8");
        value["content"] = json!(text);
    } else {
        value["reason"] = json!("non_utf8");
    }
    Ok(value)
}

pub(crate) fn readable_evidence(
    loaded: &Loaded,
    snapshot: &RunSnapshot,
    next: &Value,
) -> Result<Value, String> {
    evidence(loaded, snapshot, next, true)
}

fn evidence(
    loaded: &Loaded,
    snapshot: &RunSnapshot,
    next: &Value,
    readable: bool,
) -> Result<Value, String> {
    let view = snapshot.inspect_view();
    let events = view["events"]
        .as_array()
        .ok_or("current events unavailable")?;
    let historical = view["status"] != "running";
    let mut selected = if historical {
        let attempt = view["subject"]["attempt"].as_u64().or_else(|| {
            events
                .iter()
                .rev()
                .find(|event| event["action"] == "submit")
                .and_then(|event| event["attempt"].as_u64())
        });
        let submissions = events
            .iter()
            .filter(|event| {
                event["action"] == "submit"
                    && attempt.is_some_and(|attempt| event["attempt"] == attempt)
            })
            .collect::<Vec<_>>();
        events
            .iter()
            .filter(|event| {
                submissions.iter().any(|submission| {
                    event["eventSha256"] == submission["eventSha256"]
                        || (event["action"] == "check"
                            && event["targetEventSha256"] == submission["eventSha256"])
                })
            })
            .map(|event| {
                json!({"rawEventRef":{"selector":event["eventSha256"]},
            "kind":event["action"]})
            })
            .collect::<Vec<_>>()
    } else {
        next["packet"]["context"]["evidence"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    // Declared upstream results include the scope and repair instructions from
    // prior attempts. Select only the exact results named by this recipient,
    // never every historical artifact or a caller-provided path.
    for upstream in next["packet"]["upstreamArtifacts"]
        .as_array()
        .unwrap_or(&Vec::new())
    {
        if let Some(event) = events.iter().find(|event| {
            event["action"] == "submit"
                && event["artifact"]["sha256"] == upstream["sha256"]
                && event["stage"] == upstream["stage"]
                && event["attempt"] == upstream["attempt"]
                && event["role"] == upstream["role"]
                && event["agent"] == upstream["agent"]
        }) {
            if !selected
                .iter()
                .any(|item| item["rawEventRef"]["selector"] == event["eventSha256"])
            {
                selected.push(json!({"kind":"upstream_submission", "rawEventRef":{"selector":event["eventSha256"]}}));
            }
        }
    }
    let mut items = Vec::new();
    for item in &selected {
        let reference = if item["checkEventRef"].is_object() {
            &item["checkEventRef"]
        } else {
            &item["rawEventRef"]
        };
        let selector = &reference["selector"];
        let Some(event) = events
            .iter()
            .find(|event| &event["eventSha256"] == selector)
        else {
            // Evidence without an event reference is metadata, not an invented
            // artifact route. The canonical assignment keeps it intact.
            continue;
        };
        let mut detail = json!({"reference": reference["id"],
            "kind": item["kind"], "eventSha256": event["eventSha256"],
            "historical": historical || (event["attempt"].is_number() && event["attempt"] != view["attempt"])});
        if event["artifact"].is_object() {
            detail["artifact"] =
                verified(loaded, &event["artifact"], "assignment evidence", readable)?;
        }
        for stream in ["stdout", "stderr"] {
            if event[stream].is_object() {
                detail[stream] = verified(loaded, &event[stream], "check evidence", readable)?;
            }
        }
        detail["result"] = event["result"].clone();
        detail["outcome"] = event["outcome"].clone();
        items.push(detail);
    }
    Ok(
        json!({"complete": true, "historical": historical, "items": items, "checkEvidence": next["packet"]["checkEvidence"]}),
    )
}

/// Revalidate and expand a current section, with explicit exact-byte paging.
pub(super) fn expand(
    loaded: &Loaded,
    work: &str,
    snapshot: &RunSnapshot,
    next: &Value,
    requested: &str,
) -> Result<Value, String> {
    ensure_current_config(loaded)?;
    let small = requested.starts_with(SMALL_PREFIX);
    let prefix = if small { SMALL_PREFIX } else { PREFIX };
    let page_bytes = if small { SMALL_PAGE_BYTES } else { PAGE_BYTES };
    let suffix = requested
        .strip_prefix(prefix)
        .ok_or("not a work section reference")?;
    let parts = suffix.split(':').collect::<Vec<_>>();
    if parts.len() != 3 || !SECTIONS.contains(&parts[1]) || (!small && parts[1] == "recipient") {
        return Err("malformed work section reference; refresh with work next".into());
    }
    let page: usize = parts[2].parse().map_err(|_| "invalid work section page")?;
    let mut canonical = next.clone();
    canonical
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    let expected_binding = binding(loaded, work, snapshot, &canonical)?;
    if parts[0] != expected_binding
        || requested != format!("{prefix}{expected_binding}:{}:{page}", parts[1])
    {
        return Err("work section reference is stale, revoked or belongs to another project/Work/recipient; refresh with work next".into());
    }
    let value = section_value(loaded, work, snapshot, &canonical, parts[1])?;
    let fresh_snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let mut fresh = super::next_from(loaded, work, &fresh_snapshot)?;
    fresh
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if binding(loaded, work, &fresh_snapshot, &fresh)? != expected_binding {
        return Err("work section changed during expansion; refresh with work next".into());
    }
    if parts[1] == "recipient"
        && section_value(loaded, work, &fresh_snapshot, &fresh, "recipient")? != value
    {
        return Err(
            "recipient instructions changed during expansion; refresh with work next".into(),
        );
    }
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    let start = page
        .checked_mul(page_bytes)
        .ok_or("work section page overflow")?;
    if start >= bytes.len() {
        return Err("work section page is outside the current section".into());
    }
    let end = bytes.len().min(start.saturating_add(page_bytes));
    let more = end < bytes.len();
    let response = json!({"valid": true, "kind": "work_section", "reference": requested,
        "work": work, "binding": expected_binding, "section": parts[1],
        "encoding": "hex", "sectionSha256": hash::bytes(&bytes), "totalBytes": bytes.len(),
        "offset": start, "bytes": end-start, "contentHex": hex(&bytes[start..end]),
        "complete": !more && page == 0, "pageComplete": true,
        "next": if more { route(loaded, work, &format!("{prefix}{expected_binding}:{}:{}",parts[1],page+1)) } else { Value::Null },
    });
    if serde_json::to_vec(&response)
        .map_err(|error| error.to_string())?
        .len()
        + 1
        > MAX_DETAIL_BYTES
    {
        return Err("work section metadata exceeds the detail budget; retain the exact executable/config and refresh".into());
    }
    Ok(response)
}

pub(super) fn is_section(reference: &str) -> bool {
    reference.starts_with(PREFIX)
}

/// Native delivery consumes the same canonical section as the public scoped
/// reader. It requires complete instructions; compact status never grants work.
pub(crate) fn required_assignment(
    loaded: &Loaded,
    work: &str,
    current: &Value,
) -> Result<Value, String> {
    let ledger = super::resolve(loaded, work)?;
    let snapshot = RunSnapshot::capture(loaded, &ledger)?;
    let fresh = super::next_from(loaded, work, &snapshot)?;
    if current["current"]["binding"] != fresh["current"]["binding"] {
        return Err("native current binding changed; refresh before execution".into());
    }
    let detail = super::readable::read(loaded, work, &snapshot, &fresh)?;
    // Native request assembly owns its separate, validated profile read.
    // A grouped inline-profile fallback is not missing upstream evidence.
    if !super::readable::evidence_complete(&detail["sections"]["evidence"]) {
        let item = detail["sections"]["evidence"]["items"]
            .as_array()
            .and_then(|items| {
                items.iter().find_map(|item| {
                    ["artifact", "stdout", "stderr"].iter().find_map(|key| {
                        (item[*key].is_object() && item[*key]["readable"] != true).then(|| {
                            format!(
                                "{} {} ({})",
                                item["eventSha256"].as_str().unwrap_or("unknown event"),
                                key,
                                item[*key]["reason"].as_str().unwrap_or("unavailable")
                            )
                        })
                    })
                })
            })
            .unwrap_or_else(|| "required evidence".into());
        return Err(format!("native required evidence {item} is not completely readable; use work detail {work} --json with the same executable/config before provider launch"));
    }
    let mut sections = detail["sections"].clone();
    if let Some(tasks) = sections["tasks"].as_object_mut() {
        tasks.remove("systemText");
    }
    Ok(json!({
        "canonicalAssignment": fresh["packet"],
        "sections": sections,
        "binding": detail["binding"],
    }))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").expect("string write");
    }
    text
}

/// One automatic line, composed from validated current state rather than a
/// percentage difference. No raw rework artifact is rendered as terminal text.
pub(super) fn automatic_progress(progress: &mut Value, current: &Value) {
    let Some(text) = progress["systemText"].as_str() else {
        return;
    };
    let mut line = text.to_owned();
    if let Some(action) = current["action"].as_str() {
        if matches!(action, "spawn" | "check" | "lead_decision") {
            line.push_str(" | Next: ");
            line.push_str(action);
        }
    }
    if current["phase"]["status"] == "running" && current["repair"].is_object() {
        line.push_str(" | Repair: rework recorded; history retained");
    }
    progress["systemText"] = json!(line);
}

#[cfg(test)]
mod replacement_relation_preview_tests {
    use super::*;

    fn committed() -> Vec<Value> {
        vec![json!({
            "version": 8,
            "runId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "eventSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "timestamp": "2026-10-14T00:00:00Z"
        })]
    }

    fn submit(previous: &Value) -> Value {
        crate::run::state::make_event(json!({
            "version": 8,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(8),
            "action": "submit",
            "runId": previous["runId"],
            "stage": 1,
            "attempt": 1,
            "agent": "lead",
            "role": "lead",
            "outcome": "scoped",
            "artifact": {
                "root": "state",
                "path": "result.json",
                "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            },
            "previousEventSha256": previous["eventSha256"],
            "timestamp": "2026-10-14T00:00:01Z"
        }))
    }

    #[test]
    fn replacement_relation_accepts_only_exact_current_or_one_valid_linked_submit() {
        let committed = committed();
        assert!(events_match_committed_or_submit_preview(
            &committed, &committed
        ));

        let transaction = submit(&committed[0]);
        let mut preview = committed.clone();
        preview.push(transaction.clone());
        assert!(events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut changed_prefix = preview.clone();
        changed_prefix[0]["timestamp"] = json!("2026-10-14T00:00:00Z");
        changed_prefix[0]["eventSha256"] =
            json!("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");
        assert!(!events_match_committed_or_submit_preview(
            &changed_prefix,
            &committed
        ));

        let mut wrong_previous = transaction.clone();
        wrong_previous["previousEventSha256"] =
            json!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        wrong_previous
            .as_object_mut()
            .unwrap()
            .remove("eventSha256");
        let wrong_previous = crate::run::state::make_event(wrong_previous);
        let mut preview = committed.clone();
        preview.push(wrong_previous);
        assert!(!events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut wrong_run = transaction.clone();
        wrong_run["runId"] =
            json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        wrong_run.as_object_mut().unwrap().remove("eventSha256");
        let wrong_run = crate::run::state::make_event(wrong_run);
        let mut preview = committed.clone();
        preview.push(wrong_run);
        assert!(!events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut non_submit = transaction.clone();
        non_submit["action"] = json!("check");
        non_submit.as_object_mut().unwrap().remove("eventSha256");
        let non_submit = crate::run::state::make_event(non_submit);
        let mut preview = committed.clone();
        preview.push(non_submit);
        assert!(!events_match_committed_or_submit_preview(
            &preview, &committed
        ));

        let mut multiple = committed.clone();
        multiple.push(transaction.clone());
        multiple.push(transaction.clone());
        assert!(!events_match_committed_or_submit_preview(
            &multiple, &committed
        ));

        let mut invalid_hash = committed.clone();
        invalid_hash.push(submit(&invalid_hash[0]));
        invalid_hash[1]["eventSha256"] =
            json!("0000000000000000000000000000000000000000000000000000000000000000");
        assert!(!events_match_committed_or_submit_preview(
            &invalid_hash,
            &committed
        ));
    }
}

//! Current bindings and read-only, recipient-bound grouped section expansion.
//!
//! A reference selects bytes derived from the current validated snapshot. It
//! conveys no authority and is never interpreted as a filesystem path.

use crate::{config::Loaded, evidence::hash, run::RunSnapshot};
use serde_json::{json, Value};

mod native_snapshot;
pub(super) use native_snapshot::native_snapshot_facts;
pub(crate) use native_snapshot::{required_assignment, required_assignment_with_check};

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
        let route = crate::emitted::refresh(&crate::emitted::slot("WORK"), &loaded.path);
        return Err(format!(
            "configuration changed during work detail consumption; {route}"
        ));
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
        return Err(stale_action_binding(loaded, work));
    }
    Ok(())
}

fn stale_action_binding(loaded: &Loaded, work: &str) -> String {
    let route = crate::emitted::refresh(work, &loaded.path);
    format!("current action form binding is stale; {route} and follow current.details.grouped")
}

fn check_preparation(loaded: &Loaded, snapshot: &RunSnapshot) -> Value {
    crate::project::readiness::checker_preparation(
        &loaded.product_root,
        snapshot.state()["checkPolicy"]["command"].as_str(),
    )
}

/// Host delivery reads the frozen policy from its validated Work snapshot;
/// the recipient packet intentionally omits this internal policy object.
pub(crate) fn current_check_preparation(
    loaded: &Loaded,
    work: &str,
    current: &Value,
) -> Result<Value, String> {
    let snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let fresh = super::next_from(loaded, work, &snapshot)?;
    if current["current"]["binding"] != fresh["current"]["binding"] {
        return Err("check preparation changed with the current assignment".into());
    }
    ensure_current_config(loaded)?;
    Ok(check_preparation(loaded, &snapshot))
}

fn token(identity: &str, section: &str, page: usize) -> String {
    format!("{SMALL_PREFIX}{identity}:{section}:{page}")
}

fn route(loaded: &Loaded, work: &str, id: &str) -> Value {
    // Current section routes share the bounded response with the grouped
    // route and its effective-action projection. Preserve every route and
    // require exact invocation arguments when paths are long.
    let command = crate::emitted::Route::new(["work", "expand", work, id, "--json"])
        .config(&loaded.path)
        .within(768);
    json!({"reference": id, "command": command.argv_value(),
        "sameConfigRequired": command.needs_config(),
        "sameExecutableRequired": command.needs_executable(),
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
    if let Some(marker) = view.get("quarantineVerification") {
        next["packet"]["quarantineVerification"] = marker.clone();
    }
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
    let residual = super::packet::project(work, snapshot, next, &loaded.path)?;
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
        "recipient": super::readable::recipient_context::identity(loaded, next),
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
    if let Some(raw) = crate::session_goal::latest_raw_result(loaded, work) {
        next["current"]["uncreditedChild"] = raw;
    }
    crate::work::readable::attach(loaded, work, &binding, &mut next["current"]);
    if let Some(relation) = super::replacement_relation::relation(loaded, work, &events)? {
        next["current"]["checkReplacement"] = relation;
    }
    next["current"]["actionForm"] =
        crate::work::action_forms::project(loaded, work, next, &binding);
    if next["action"] == "done" {
        next["current"]["completion"] = super::closeout::projection(loaded, work, snapshot, next);
    }
    Ok(())
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
            "memoryQuery": if crate::memory::policy::get(&loaded.config).is_some() {
                crate::memory::query::route(loaded, work)
            } else { Value::Null },
            "checkPreparation": check_preparation(loaded, snapshot),
            "residual": super::packet::project(work, snapshot, next, &loaded.path)?})),
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
            "encoding": "hex", "contentHex": hex(&read.preview), "complete": true,
            "contentText": super::text_page::text(&read.preview)}));
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
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!("malformed work section reference; {route}"));
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
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!("work section reference is stale, revoked or belongs to another project/Work/recipient; {route}"));
    }
    let value = section_value(loaded, work, snapshot, &canonical, parts[1])?;
    let fresh_snapshot = RunSnapshot::capture(loaded, &super::resolve(loaded, work)?)?;
    let mut fresh = super::next_from(loaded, work, &fresh_snapshot)?;
    fresh
        .as_object_mut()
        .ok_or("current action unavailable")?
        .remove("current");
    if binding(loaded, work, &fresh_snapshot, &fresh)? != expected_binding {
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!("work section changed during expansion; {route}"));
    }
    if parts[1] == "recipient"
        && section_value(loaded, work, &fresh_snapshot, &fresh, "recipient")? != value
    {
        let route = crate::emitted::refresh(work, &loaded.path);
        return Err(format!(
            "recipient instructions changed during expansion; {route}"
        ));
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
    let mut response = json!({"valid": true, "kind": "work_section", "reference": requested,
        "work": work, "binding": expected_binding, "section": parts[1],
        "encoding": "hex", "sectionSha256": hash::bytes(&bytes), "totalBytes": bytes.len(),
        "offset": start, "bytes": end-start, "contentHex": hex(&bytes[start..end]),
        "complete": !more && page == 0, "pageComplete": true,
        "next": if more { route(loaded, work, &format!("{prefix}{expected_binding}:{}:{}",parts[1],page+1)) } else { Value::Null },
    });
    super::text_page::prefer_text(&mut response, &bytes[start..end]);
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

/// Managed launch and saved-result validation consume the same complete
/// recipient packet, including the existing project continuation when any.
pub(crate) fn managed_required_assignment(
    loaded: &Loaded,
    work: &str,
    current: &Value,
) -> Result<Value, String> {
    let mut selected = required_assignment(loaded, work, current)?;
    if let Some(delivery) = crate::project::continuation::runtime::delivery(loaded, work)? {
        selected["sections"]["projectContinuation"] = delivery;
    }
    selected["requiredFactsSha256"] = json!(hash::value(&selected["sections"]));
    Ok(selected)
}

fn native_sections(mut sections: Value, forms: &Value, binding: &Value) -> Value {
    if let Some(tasks) = sections["tasks"].as_object_mut() {
        tasks.remove("systemText");
    }
    let choices = if forms["leadChoiceRequired"] == true {
        Vec::new()
    } else {
        forms["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|choice| choice["label"].as_str())
            .collect::<Vec<_>>()
    };
    // The actor receives the actual form, not a newly constructed permit.
    // Complete return forms remain at the same bound grouped-detail route;
    // host and managed execution already supply the typed result contract.
    let mut actor_action = json!({
        "binding": binding,
        "returnChoices": choices,
    });
    if !forms["beforeEditing"].is_null() {
        actor_action["beforeEditing"] = forms["beforeEditing"].clone();
    }
    if !forms["currentGrant"].is_null() {
        actor_action["currentGrant"] = forms["currentGrant"].clone();
    }
    if !forms["nextRequest"].is_null() {
        actor_action["nextRequest"] = forms["nextRequest"].clone();
    }
    if forms["leadChoiceRequired"] == true {
        actor_action["leadChoiceRequired"] = json!(true);
    }
    sections["currentAction"] = actor_action;
    sections
}

/// The same recipient owner rechecks rules, profile, rights and guidance at
/// return; the hash is evidence of currentness, not an authority grant.
pub(crate) fn recipient_fingerprint(loaded: &Loaded, current: &Value) -> Result<String, String> {
    ensure_current_config(loaded)?;
    let recipient = super::readable::recipient_context::read(loaded, current)?;
    if recipient["available"] != true || !recipient["rules"].is_array() {
        return Err("current recipient instructions are unavailable".into());
    }
    Ok(hash::value(&recipient))
}

/// A changed delivered recipient cannot confer credit on a saved result.
pub(crate) fn recipient_drift(loaded: &Loaded, work: &str, bound: &str, current: &str) -> String {
    crate::drift::refusal(
        crate::drift::Class::RecipientContext,
        bound,
        Some(current),
        None,
        crate::drift::Route::WorkDetail {
            work,
            config: &loaded.path,
        },
    )
}

/// Managed execution uses the same host-specific guidance and specialist
/// owners as its request builder, in addition to the shared recipient view.
pub(crate) fn managed_recipient_fingerprint(
    loaded: &Loaded,
    current: &Value,
) -> Result<String, String> {
    let recipient = recipient_fingerprint(loaded, current)?;
    let packet = &current["packet"];
    let agent = current["agent"]
        .as_str()
        .ok_or("native agent unavailable")?;
    let role = current["role"].as_str().ok_or("native role unavailable")?;
    let goal = packet["goal"].as_str();
    let guidance = crate::project::agent_context::guidance_for_assignment(
        loaded,
        agent,
        goal,
        "codex",
        crate::project::agent_context::NATIVE_GUIDANCE_BYTES,
        packet,
    )?;
    let architecture = crate::project::architecture::delivery(loaded, packet)?;
    let capabilities = crate::project::capabilities::delivery(loaded, packet, "codex")?;
    let specialist =
        crate::project::specialists::delivery(loaded, agent, role, goal.unwrap_or(""), "codex")?;
    Ok(hash::value(
        &json!({"recipient":recipient,"guidance":guidance,
        "architecture":architecture,"capabilities":capabilities,"specialist":specialist}),
    ))
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

//! Agent-facing work façade over the strict run protocol.

pub(crate) mod packet;

use crate::{config::Loaded, evidence::hash, run};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Read;

const WORK_PREFIX: &str = "smw_";

fn runs_dir() -> String {
    format!("{}/runs", crate::project::layout_types::state_namespace())
}

fn artifacts_dir() -> String {
    format!(
        "{}/artifacts",
        crate::project::layout_types::state_namespace()
    )
}

pub(crate) struct BeginOptions<'a> {
    pub(crate) workflow: &'a str,
    pub(crate) goal: &'a str,
    pub(crate) check_command: &'a str,
    pub(crate) boundary: Option<&'a str>,
    pub(crate) harness_receipt: Option<&'a str>,
    pub(crate) proof_origin: Option<&'a str>,
    pub(crate) preserve_requirement: Option<&'a str>,
    pub(crate) preservation_check_command: Option<&'a str>,
    pub(crate) preservation_proof_origin: Option<&'a str>,
}

pub(crate) fn begin(loaded: &Loaded, options: BeginOptions<'_>) -> Result<Value, String> {
    // `work begin` is the governed activation boundary. Direct, harmless work
    // never enters this path. Availability is read from the loaded project
    // preconditions before any ledger mutation; the final boolean means those
    // preconditions have been activated by this boundary, not that a prior run
    // already existed.
    let material_consequence = !options.goal.trim().is_empty();
    let promotion_required = !options.check_command.trim().is_empty();
    let activation_available = loaded.path.is_file() && loaded.state_root.is_dir();
    let managed_namespace = loaded
        .state_root
        .join(crate::project::layout_types::state_namespace());
    let activation_ready = activation_available
        && managed_namespace.is_dir()
        && managed_namespace.join("runs").is_dir()
        && managed_namespace.join("locks").is_dir();
    if crate::host::runtime::classify_activation(
        material_consequence,
        promotion_required,
        activation_available,
        activation_ready,
    ) != crate::host::runtime::Activation::Governed
    {
        return Err("governed activation is unavailable".into());
    }
    let token = hash::text(&format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        options.workflow,
        options.goal,
        options.check_command,
        loaded.source,
        std::process::id(),
        timestamp_nanos()
    ));
    let ledger = format!("{}/work-{token}.jsonl", runs_dir());
    let _started = run::start_with_policy(
        loaded,
        options.workflow,
        options.goal,
        &ledger,
        options.boundary,
        options.harness_receipt,
        Some(options.check_command),
        options.proof_origin,
        options.preserve_requirement,
        options.preservation_check_command,
        options.preservation_proof_origin,
    )?;
    let work = format!("{WORK_PREFIX}{token}");
    let next = next_for(loaded, &work, &ledger)?;
    Ok(json!({"work": work, "next": next}))
}

pub(crate) fn next(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let (next, residual, presentation) = next_and_residual(loaded, work, &ledger, false)?;
    Ok(json!({"work": work, "next": next, "residual": residual, "presentation": presentation}))
}

/// Atomically authorize and record one cooperative product-mutation unit for
/// the exact current assignment.  Callers must perform their product edit
/// only after this succeeds; the ledger lock and replay reducer own the
/// refusal boundary.
pub(crate) fn permit(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    operation: &str,
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" || action["role"] != "worker" {
        return Err("a worker assignment is required for a governed mutation".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let expected = run::AssignmentIdentity::from_action(&action)?;
    let permission =
        run::permit_for_assignment(loaded, &ledger, work, assignment, expected, operation)?;
    Ok(json!({
        "work": work,
        "allowed": permission["allowed"],
        "event": permission["event"],
        "governor": permission["governor"],
        "next": next_for(loaded, work, &ledger)?,
    }))
}

pub(crate) fn replan(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    hypothesis: Option<&str>,
    evidence_request: Option<&str>,
    scope_decision: Option<&str>,
    blocker: Option<&str>,
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" || action["role"] != "worker" {
        return Err("a worker assignment is required for re-plan".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let expected = run::AssignmentIdentity::from_action(&action)?;
    let result = run::replan_for_assignment(
        loaded,
        &ledger,
        assignment,
        expected,
        hypothesis,
        evidence_request,
        scope_decision,
        blocker,
    )?;
    Ok(json!({
        "work": work,
        "event": result["event"],
        "governor": result["governor"],
        "next": next_for(loaded, work, &ledger)?,
    }))
}

pub(crate) fn evidence(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    artifact_root: Option<&str>,
    artifact_path: &str,
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" || action["role"] != "worker" {
        return Err("a worker assignment is required for evidence".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let expected = run::AssignmentIdentity::from_action(&action)?;
    let result = run::evidence_for_assignment(
        loaded,
        &ledger,
        assignment,
        expected,
        artifact_root,
        artifact_path,
    )?;
    Ok(json!({
        "work": work,
        "event": result["event"],
        "governor": result["governor"],
        "next": next_for(loaded, work, &ledger)?,
    }))
}

pub(crate) fn sensor_request(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" || action["role"] != "worker" {
        return Err("a worker assignment is required for a sensor request".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let result = run::sensor_request_for_assignment(
        loaded,
        &ledger,
        assignment,
        run::AssignmentIdentity::from_action(&action)?,
    )?;
    Ok(
        json!({"work": work, "event": result["event"], "governor": result["governor"], "next": next_for(loaded, work, &ledger)?}),
    )
}

pub(crate) fn sensor_result(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    assessment: &str,
    confidence: Option<&str>,
    input_digest: &str,
    identity_source: &str,
) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" || action["role"] != "worker" {
        return Err("a worker assignment is required for a sensor result".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let result = run::sensor_result_for_assignment(
        loaded,
        &ledger,
        assignment,
        run::AssignmentIdentity::from_action(&action)?,
        assessment,
        confidence,
        input_digest,
        identity_source,
    )?;
    Ok(
        json!({"work": work, "event": result["event"], "governor": result["governor"], "next": next_for(loaded, work, &ledger)?}),
    )
}

pub(crate) fn return_result(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    outcome: &str,
    reason: Option<&str>,
) -> Result<Value, String> {
    if outcome.trim().is_empty() {
        return Err("work return requires --outcome OUTCOME".into());
    }
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "spawn" && action["action"] != "lead_decision" {
        return Err("an assignment is not the current work action".into());
    }
    if action["assignment"] != assignment {
        return Err("assignment is not the current pending work action".into());
    }
    let expected = run::AssignmentIdentity::from_action(&action)?;
    // `unavailable` is admissible only for a reviewer, and only to report that
    // the target could not execute — never to escape an adverse verdict, which
    // the reducer refuses independently.
    let allowed = action["role"] == "lead"
        && action["outcomes"]
            .as_array()
            .is_some_and(|outcomes| outcomes.iter().any(|item| item == outcome))
        || action["role"] == "reviewer"
            && ["approved", "rework", "blocked", "unavailable"].contains(&outcome)
        || matches!(action["role"].as_str(), Some("worker" | "adviser"))
            && ["completed", "blocked"].contains(&outcome);
    if !allowed {
        return Err(format!(
            "outcome '{outcome}' is not allowed for this assignment"
        ));
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("work result could not be read: {error}"))?;
    let _submitted = run::submit_for_assignment(
        loaded,
        &ledger,
        assignment,
        expected,
        outcome,
        reason,
        || write_artifact(loaded, work, assignment, &bytes),
    )?;
    // The decision that reaches a terminal state is exactly where its display
    // belongs: returning it here means the caller copies the product's own
    // wording instead of assembling a sentence from status and progress.
    let (next, _residual, presentation) = next_and_residual(loaded, work, &ledger, false)?;
    Ok(json!({"work": work, "next": next, "presentation": presentation}))
}

/// Decide whether a previously issued residual packet still applies to the
/// current ledger and tested inputs. Never executes anything from the packet.
pub(crate) fn validate(loaded: &Loaded, work: &str, packet_path: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let packet = crate::work::packet::read_bounded(packet_path)?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let next = next_from(loaded, work, &snapshot)?;
    crate::work::packet::validate(work, &snapshot, &next, &packet)
}

/// Expand one opaque reference emitted by the current context projection.
/// Paths and hashes supplied by a caller are never accepted as a substitute
/// for the packet-bound reference.
pub(crate) fn expand(loaded: &Loaded, work: &str, reference: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let next = next_from(loaded, work, &snapshot)?;
    let packet = crate::work::packet::project(work, &snapshot, &next)?;
    let context = packet
        .get("context")
        .ok_or("current packet has no context projection")?;
    let canonical = find_reference(context, reference)
        .ok_or("reference is not an opaque reference from the current context")?;
    if canonical["exact"] != true
        || canonical["root"] != "state"
        || canonical["path"].as_str().is_none()
    {
        return Err("reference is malformed".into());
    }
    let (_path, events, source) = crate::run::ledger::load(loaded, &ledger)?;
    let raw_sha = hash::bytes(source.as_bytes());
    let expected_path = expected_history_path(work);
    if canonical["path"] != expected_path
        || canonical["sha256"] != raw_sha
        || canonical["headEventSha256"]
            != events
                .last()
                .and_then(|event| event["eventSha256"].as_str())
                .unwrap_or_default()
        || canonical["eventCount"].as_u64() != Some(events.len() as u64)
    {
        return Err("reference is stale or ledger bytes changed".into());
    }
    let kind = canonical["kind"].as_str().unwrap_or_default();
    match kind {
        "ledger_history" => Ok(expansion_response(
            &canonical,
            "history",
            source.as_bytes(),
            json!({"events": events}),
        )),
        "ledger_event" => {
            let wanted = canonical["selector"]
                .as_str()
                .ok_or("event reference has no selected event")?;
            let (index, event) = events
                .iter()
                .enumerate()
                .find(|(_, event)| event["eventSha256"].as_str() == Some(wanted))
                .ok_or("selected event is not in the referenced ledger")?;
            let line = source
                .split('\n')
                .filter(|line| !line.is_empty())
                .nth(index)
                .ok_or("selected event bytes are unavailable")?;
            Ok(expansion_response(
                &canonical,
                "event",
                line.as_bytes(),
                json!({"event": event, "eventIndex": index}),
            ))
        }
        "check_log" => expand_check_log(loaded, &canonical, &events),
        _ => Err("reference kind is unsupported".into()),
    }
}

fn find_reference(value: &Value, requested: &str) -> Option<Value> {
    if let Some(object) = value.as_object() {
        let matches = object.get("id").and_then(Value::as_str) == Some(requested);
        if matches && object.contains_key("kind") && object.contains_key("exact") {
            return Some(value.clone());
        }
        for child in object.values() {
            if let Some(found) = find_reference(child, requested) {
                return Some(found);
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            if let Some(found) = find_reference(child, requested) {
                return Some(found);
            }
        }
    }
    None
}

fn expected_history_path(work: &str) -> String {
    work.strip_prefix(WORK_PREFIX).map_or_else(
        || {
            format!(
                "{}/runs/{work}.jsonl",
                crate::project::layout_types::state_namespace()
            )
        },
        |token| {
            format!(
                "{}/runs/work-{token}.jsonl",
                crate::project::layout_types::state_namespace()
            )
        },
    )
}

fn expansion_response(reference: &Value, kind: &str, bytes: &[u8], metadata: Value) -> Value {
    let mut response = json!({
        "valid": true,
        "kind": kind,
        "reference": reference,
        "encoding": "hex",
        "bytes": bytes.len(),
        "contentHex": hex(bytes),
    });
    if let (Some(object), Some(extra)) = (response.as_object_mut(), metadata.as_object()) {
        object.extend(
            extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    response
}

fn expand_check_log(loaded: &Loaded, reference: &Value, events: &[Value]) -> Result<Value, String> {
    let wanted = reference["checkEventSha256"]
        .as_str()
        .ok_or("check-log reference has no check event")?;
    let event = events
        .iter()
        .find(|event| event["eventSha256"].as_str() == Some(wanted))
        .ok_or("check-log event is not in the referenced ledger")?;
    if event["action"] != "check" || event["acquisition"] != "observed" {
        return Err("check-log reference does not name an observed check".into());
    }
    let stdout = crate::run::artifact::read(loaded, &event["stdout"], "stdout")?;
    let stderr = crate::run::artifact::read(loaded, &event["stderr"], "stderr")?;
    if reference["stdout"] != event["stdout"] || reference["stderr"] != event["stderr"] {
        return Err("check-log artifact reference is stale or tampered".into());
    }
    Ok(json!({
        "valid": true,
        "kind": "check_log",
        "reference": reference,
        "event": event,
        "stdout": {"encoding":"hex", "bytes":stdout.len(), "contentHex":hex(&stdout)},
        "stderr": {"encoding":"hex", "bytes":stderr.len(), "contentHex":hex(&stderr)},
    }))
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

pub(crate) fn check(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let pending = current_check_target(&snapshot)?.ok_or("no current worker check is pending")?;
    let _observed = run::observe_check_for_requirement(
        loaded,
        &ledger,
        &pending.target_event_sha256,
        pending.requirement_id.as_deref(),
        None,
    )?;
    Ok(json!({"work": work, "next": next_for(loaded, work, &ledger)?}))
}

pub(crate) fn resume(loaded: &Loaded) -> Result<Value, String> {
    let directory = loaded.state_root.join(runs_dir());
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return none_result(loaded, Vec::new())
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut candidates = Vec::new();
    let mut finished: Vec<(String, String, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let info = entry.file_type().map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or("work ledger filename is not valid UTF-8")?
            .to_owned();
        let Some(token) = name
            .strip_prefix("work-")
            .and_then(|name| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        if !info.is_file() || !valid_token(token) {
            continue;
        }
        let ledger = format!("{}/{}", runs_dir(), name);
        let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
        let view = snapshot.inspect_view();
        if view["status"] == "running" {
            let progress = snapshot.next_view(loaded)?["progress"].clone();
            candidates.push((
                format!("{WORK_PREFIX}{token}"),
                view["workflow"].clone(),
                view["events"][0]["goal"].clone(),
                ledger,
                progress,
            ));
        } else if let Some(at) = view["events"]
            .as_array()
            .and_then(|events| events.last())
            .and_then(|event| event["timestamp"].as_str())
        {
            finished.push((at.to_owned(), format!("{WORK_PREFIX}{token}"), ledger));
        }
    }
    match candidates.len() {
        0 => none_result(loaded, finished),
        1 => {
            let (work, _, _, ledger, _) = candidates.pop().expect("one candidate exists");
            let (next, residual, presentation) = next_and_residual(loaded, &work, &ledger, true)?;
            Ok(json!({
                "status": "resumed",
                "work": work,
                "next": next,
                "residual": residual,
                "presentation": presentation
            }))
        }
        _ => Ok(json!({
            "status": "ambiguous",
            "works": candidates.into_iter().map(|(work, workflow, goal, _, progress)| json!({
                "work": work,
                "workflow": workflow,
                "goal": goal,
                "progress": progress
            })).collect::<Vec<_>>()
        })),
    }
}

/// No work is active. The most recently finished run is still reported, with
/// the presentation the product would print for it, so the answer to "where
/// does this stand" comes from recorded state rather than from a reader's
/// summary of the ledger. Its terminal block appears only while that decision
/// still describes the files present now.
fn none_result(
    loaded: &Loaded,
    mut finished: Vec<(String, String, String)>,
) -> Result<Value, String> {
    let mut result =
        json!({"status": "none", "next": {"action": "none", "reason": "no_active_work"}});
    finished.sort_by(|left, right| left.0.cmp(&right.0));
    if let Some((_, work, ledger)) = finished.pop() {
        let (next, _residual, presentation) = next_and_residual(loaded, &work, &ledger, false)?;
        // `presentation` sits where every other work response carries it, so
        // one documented path holds for every answer a host has to render.
        result["recent"] = json!({"work": work, "exitState": next["progress"]["state"]});
        result["presentation"] = presentation;
    }
    Ok(result)
}

/// Next action and residual packet derived from one captured revision, plus the
/// conversational presentation that speaks only when the state moved.
fn next_and_residual(
    loaded: &Loaded,
    work: &str,
    ledger: &str,
    resumed: bool,
) -> Result<(Value, Value, Value), String> {
    let snapshot = run::RunSnapshot::capture(loaded, ledger)?;
    let next = next_from(loaded, work, &snapshot)?;
    let residual = crate::work::packet::project(work, &snapshot, &next)?;
    let mut next = next;
    next["resolvedActor"] = residual["humanHelp"]["nextAction"]["actor"].clone();
    // The fingerprint is computed only when a terminal acceptance has to be
    // compared with the tree; a running run already carries its own.
    let facts = crate::work::packet::facts(&snapshot, &next, || {
        crate::run::inputs::fingerprint(loaded).ok()
    })?;
    let presentation = crate::presentation_events::project(
        &loaded.state_root,
        work,
        &residual,
        &next["progress"],
        &facts,
        resumed,
    );
    Ok((next, residual, presentation))
}

fn next_for(loaded: &Loaded, work: &str, ledger: &str) -> Result<Value, String> {
    next_from(loaded, work, &run::RunSnapshot::capture(loaded, ledger)?)
}

fn next_from(loaded: &Loaded, work: &str, snapshot: &run::RunSnapshot) -> Result<Value, String> {
    let value = snapshot.next_view(loaded)?;
    let progress = value["progress"].clone();
    if value["status"] != "running" {
        return Ok(json!({"action": "done", "status": value["status"], "progress": progress}));
    }
    if let Some(pending) = current_check_target(snapshot)? {
        let mut result = json!({
            "action": "check",
            "check": pending.value(),
            "progress": progress,
        });
        result["resolvedActor"] = crate::work::packet::resolved_actor(work, snapshot, &result)?;
        let view = snapshot.inspect_view();
        let status = snapshot.status_view()?;
        if let Ok(context) = crate::context::project(work, &view, &status, &result) {
            result["packet"] = json!({"context": context});
        }
        return Ok(result);
    }
    let assignment = value["assignments"]
        .as_array()
        .and_then(|items| items.first())
        .ok_or("validated work has no next action")?;
    let assignment_handle = assignment_handle(work, assignment)?;
    let role = assignment["role"].as_str().unwrap_or("");
    let action = if role == "lead" {
        "lead_decision"
    } else {
        "spawn"
    };
    let mut packet = assignment.clone();
    if let Some(object) = packet.as_object_mut() {
        for field in [
            "artifactRootHint",
            "artifactPathHint",
            "checkPolicy",
            "profilePath",
        ] {
            object.remove(field);
        }
        if let Some(summary) = check_summary(snapshot)? {
            object.insert("checkEvidence".into(), summary);
        }
    }
    let mut result = json!({
        "action": action,
        "assignment": assignment_handle,
        "role": assignment["role"],
        "agent": assignment["agent"],
        "packet": packet,
        "progress": progress
    });
    let resolved_actor = crate::work::packet::resolved_actor(work, snapshot, &result)?;
    result["resolvedActor"] = resolved_actor;
    let view = snapshot.inspect_view();
    let status = if view["status"] == "running" {
        snapshot.status_view()?
    } else {
        Value::Null
    };
    if let Ok(context) = crate::context::project(work, &view, &status, &result) {
        result["packet"]["context"] = context;
    }
    if role == "lead" {
        result["outcomes"] = if value["currentStage"] == 1 {
            json!(["scoped", "blocked"])
        } else {
            json!(["accepted", "rework", "blocked"])
        };
    }
    Ok(result)
}

#[derive(Clone, Debug)]
struct PendingCheck {
    target_event_sha256: String,
    requirement_id: Option<String>,
}

impl PendingCheck {
    fn value(&self) -> Value {
        let mut value = json!({});
        if let Some(id) = &self.requirement_id {
            value["kind"] = json!("preservation");
            value["requirementId"] = json!(id);
        } else {
            value["kind"] = json!("check");
        }
        value
    }
}

fn current_check_target(snapshot: &run::RunSnapshot) -> Result<Option<PendingCheck>, String> {
    if snapshot.status() != "running" {
        return Ok(None);
    }
    let status = snapshot.status_view()?;
    Ok(status["checks"]["targets"].as_array().and_then(|targets| {
        targets.iter().find_map(|item| {
            (item["status"] == "missing").then(|| PendingCheck {
                target_event_sha256: item["targetEventSha256"].as_str().unwrap_or("").to_owned(),
                requirement_id: item["requirementId"].as_str().map(str::to_owned),
            })
        })
    }))
}

fn check_summary(snapshot: &run::RunSnapshot) -> Result<Option<Value>, String> {
    let status = snapshot.status_view()?;
    let Some(targets) = status["checks"]["targets"].as_array() else {
        return Ok(None);
    };
    if targets.is_empty() {
        return Ok(None);
    }
    Ok(Some(Value::Array(
        targets
            .iter()
            .map(|item| {
                let mut summary = json!({"status": item["status"]});
                if let Some(value) = item.get("kind") {
                    summary["kind"] = value.clone();
                }
                if let Some(value) = item.get("requirementId") {
                    summary["requirementId"] = value.clone();
                }
                if let Some(value) = item.get("requirementText") {
                    summary["requirementText"] = value.clone();
                }
                if let Some(value) = item.get("acquisition") {
                    summary["acquisition"] = value.clone();
                }
                if let Some(value) = item.get("result") {
                    summary["result"] = value.clone();
                }
                summary
            })
            .collect(),
    )))
}

fn resolve(loaded: &Loaded, work: &str) -> Result<String, String> {
    let token = work
        .strip_prefix(WORK_PREFIX)
        .filter(|token| valid_token(token))
        .ok_or("work handle is invalid")?;
    let ledger = format!("{}/work-{token}.jsonl", runs_dir());
    let path = loaded.state_root.join(&ledger);
    let info = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "work handle was not found".to_owned()
        } else {
            error.to_string()
        }
    })?;
    if info.file_type().is_symlink() || !info.is_file() {
        return Err("work handle does not identify a regular ledger".into());
    }
    Ok(ledger)
}

fn write_artifact(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let directory = loaded.state_root.join(artifacts_dir());
    crate::project::managed_files::ensure_managed_directory(&loaded.state_root, &directory)?;
    let suffix = &hash::bytes(bytes)[..16];
    let name = format!("{work}-{assignment}-{suffix}.md");
    let path = directory.join(&name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut file, bytes).map_err(|error| error.to_string())?;
    Ok(format!("{}/{}", artifacts_dir(), name))
}

fn assignment_handle(work: &str, assignment: &Value) -> Result<String, String> {
    crate::run::assignment::handle(work, assignment)
}

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

//! Agent-facing work façade over the strict run protocol.

pub(crate) mod compact;
mod held;
mod mutation_response;
pub(crate) mod packet;
mod recovery;
mod response_recovery;
mod return_result_impl;

use crate::{config::Loaded, evidence::hash, run};
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::Path;

pub(crate) use return_result_impl::return_result;

use mutation_response::{
    bounded as bounded_mutation, recorded_projection_failure, recorded_protection_reference,
    recorded_reference,
};
use recovery::{
    add_identity, candidate_command, compact_progress, unreadable_candidate, work_identity,
    Candidate,
};

const WORK_PREFIX: &str = "smw_";
pub(crate) const DIAGNOSTIC_PREFIX: &str = "EXITBIND_WORK_DIAGNOSTIC:";

fn runs_dir() -> String {
    format!("{}/runs", crate::project::layout_types::state_namespace())
}

/// Read-only activation assessment used by the explicit `work classify`
/// bridge. The caller supplies both typed consequence facts; local availability
/// and activation are derived from the loaded project or current directory.
pub(crate) fn classify(
    loaded: Option<&Loaded>,
    current_dir: &Path,
    material_consequence: bool,
    promotion_required: bool,
) -> Value {
    let state_root = loaded.map_or(current_dir, |value| value.state_root.as_path());
    let activation_available =
        loaded.is_some_and(|value| value.path.is_file() && value.state_root.is_dir());
    let managed_namespace = state_root.join(crate::project::layout_types::state_namespace());
    let activation_ready = activation_available
        && managed_namespace.is_dir()
        && managed_namespace.join("runs").is_dir()
        && managed_namespace.join("locks").is_dir();
    let assessment =
        crate::host::runtime::assess_activation(crate::host::runtime::ActivationFacts {
            material_consequence,
            promotion_required,
            available: activation_available,
            activated: activation_ready,
        });
    json!({
        "facts": {
            "materialConsequence": material_consequence,
            "promotionRequired": promotion_required,
        },
        "availability": {
            "configuration": activation_available,
            "managedNamespace": activation_ready,
        },
        "assessment": assessment.value(),
        "provenance": "host_reported",
    })
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
    pub(crate) basis: Option<&'a str>,
    pub(crate) review_policy: Option<&'a str>,
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
        options.basis,
        options.review_policy,
    )?;
    let work = format!("{WORK_PREFIX}{token}");
    let next = next_for(loaded, &work, &ledger)?;
    Ok(json!({"work": work, "next": next}))
}

pub(crate) fn next(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work).map_err(|_| {
        diagnostic_error(
            "work handle is stale or unknown. Inspect the handle before continuing.",
            "stale_handle",
            "no-change",
            reference(work),
            safe_action("inspect"),
        )
    })?;
    let (next, residual, presentation) = next_and_residual(loaded, work, &ledger, false)
        .map_err(|error| discovery_error(error, work))?;
    if let Some(warnings) = next["warnings"]
        .as_array()
        .filter(|warnings| !warnings.is_empty())
    {
        let classification = warnings[0]["classification"].as_str().unwrap_or("drift");
        let warnings = warnings
            .iter()
            .map(|warning| {
                json!({
                    "classification": warning["classification"],
                    "error": warning["error"],
                })
            })
            .collect::<Vec<_>>();
        let mut response = json!({
            "work": work,
            "next": {"warnings": warnings},
            "presentation": presentation,
            "reason": {"code": classification},
            "effect": "no-change",
            "reference": reference(work),
            "nextAction": safe_action(if next["action"] == "done" { "none" } else { "continue" }),
        });
        add_identity(&mut response, &work_identity(loaded, Some(&ledger))?);
        attach_continuation(loaded, work, &mut response)?;
        return Ok(response);
    }
    let mut response = json!({
        "work": work,
        "next": next,
        "residual": residual,
        "presentation": presentation,
        "reason": {"code": "explicit_handle"},
        "effect": "no-change",
        "reference": reference(work),
        "nextAction": safe_action(if next["action"] == "done" { "none" } else { "continue" }),
    });
    add_identity(&mut response, &work_identity(loaded, Some(&ledger))?);
    attach_continuation(loaded, work, &mut response)?;
    Ok(response)
}

fn attach_continuation(loaded: &Loaded, work: &str, response: &mut Value) -> Result<(), String> {
    let Some(goal) = crate::session_goal::read(&loaded.state_root)? else {
        return Ok(());
    };
    if goal["continuation"]["work"] == work {
        response["continuation"] = crate::session_goal::continuation_view(loaded, work)?;
    }
    Ok(())
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
    request_id: Option<&str>,
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
    let permission = run::permit_for_assignment(
        loaded, &ledger, work, assignment, expected, operation, request_id,
    )?;
    let mut response = json!({
        "work": work,
        "allowed": permission["allowed"],
        "event": permission["event"],
        "governor": permission["governor"],
        "next": next_for(loaded, work, &ledger)?,
    });
    if let Some(idempotent) = permission.get("idempotent") {
        response["idempotent"] = idempotent.clone();
    }
    Ok(agent_response(response))
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
    Ok(agent_response(json!({
        "work": work,
        "event": result["event"],
        "governor": result["governor"],
        "next": next_for(loaded, work, &ledger)?,
    })))
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
    Ok(agent_response(json!({
        "work": work,
        "event": result["event"],
        "governor": result["governor"],
        "next": next_for(loaded, work, &ledger)?,
    })))
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
    Ok(agent_response(
        json!({"work": work, "event": result["event"], "governor": result["governor"], "next": next_for(loaded, work, &ledger)?}),
    ))
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
    Ok(agent_response(
        json!({"work": work, "event": result["event"], "governor": result["governor"], "next": next_for(loaded, work, &ledger)?}),
    ))
}

/// Decide whether a previously issued residual packet still applies to the
/// current ledger and tested inputs. Never executes anything from the packet.
pub(crate) fn validate(loaded: &Loaded, work: &str, packet_path: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let packet = match crate::work::packet::read_bounded(packet_path) {
        Ok(packet) => packet,
        Err(error) if error == "packet is not valid JSON" => Value::Null,
        Err(error) => return Err(error),
    };
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
    if canonical["exact"] != true || canonical["id"].as_str().is_none() {
        return Err("reference is malformed".into());
    }
    let (_path, events, source) = crate::run::ledger::load(loaded, &ledger)?;
    let raw_sha = hash::bytes(source.as_bytes());
    if canonical["sha256"] != raw_sha
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
        let expandable = matches
            && object.get("exact") == Some(&Value::Bool(true))
            && object
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    matches!(kind, "ledger_history" | "ledger_event" | "check_log")
                });
        if expandable {
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
    if reference["stdout"] != crate::context::artifact_reference(&event["stdout"], "stdout")
        || reference["stderr"] != crate::context::artifact_reference(&event["stderr"], "stderr")
    {
        return Err("check-log artifact reference is stale or tampered".into());
    }
    Ok(json!({
        "valid": true,
        "kind": "check_log",
        "reference": reference,
        "event": event,
        "stdout": bounded_log_value(&stdout),
        "stderr": bounded_log_value(&stderr),
    }))
}

const MAX_LOG_DISPLAY_BYTES: usize = 64 * 1024;

fn bounded_log_value(artifact: &crate::run::artifact::VerifiedArtifact) -> Value {
    let displayed = artifact.preview.len().min(MAX_LOG_DISPLAY_BYTES);
    json!({
        "encoding": "hex",
        "bytes": artifact.bytes,
        "totalBytes": artifact.bytes,
        "sha256": artifact.sha256,
        "displayedBytes": displayed,
        "truncated": artifact.bytes > displayed as u64,
        "contentHex": hex(&artifact.preview[..displayed]),
    })
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
    let config_path = loaded
        .path
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
    let pending = retry_check_target(&snapshot)?.ok_or("no current worker check is pending")?;
    let observed = run::observe_check_for_requirement(
        loaded,
        &ledger,
        &pending.target_event_sha256,
        pending.requirement_id.as_deref(),
        None,
    )?;
    let mut response = json!({"work": work, "event": observed["event"]});
    if let Some(error) = observed.get("projectionError") {
        response["projectionError"] = error.clone();
    }
    if let Some(error) = observed.get("cleanupError") {
        response["cleanupError"] = error.clone();
    }
    match next_for(loaded, work, &ledger) {
        Ok(next) => response["next"] = next,
        Err(error) => response["projectionError"] = json!(error),
    }
    Ok(bounded_mutation(
        &response,
        work,
        None,
        &ledger,
        config_path,
    ))
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
    let mut unreadable = Vec::new();
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
        let work = format!("{WORK_PREFIX}{token}");
        let discovered = (|| -> Result<Option<Candidate>, String> {
            if superseded_by_valid_claim(loaded, &ledger)? {
                return Ok(None);
            }
            let snapshot = run::RunSnapshot::capture(loaded, &ledger)?;
            let view = snapshot.inspect_view();
            if view["status"] == "running" {
                let progress = snapshot.next_view(loaded)?["progress"].clone();
                let identity = work_identity(loaded, Some(&ledger))?;
                return Ok(Some((
                    work.clone(),
                    view["workflow"].clone(),
                    view["events"][0]["goal"].clone(),
                    ledger.clone(),
                    progress,
                    identity,
                )));
            }
            if let Some(at) = view["events"]
                .as_array()
                .and_then(|events| events.last())
                .and_then(|event| event["timestamp"].as_str())
            {
                finished.push((at.to_owned(), work.clone(), ledger.clone()));
            }
            Ok(None)
        })();
        match discovered {
            Ok(Some(candidate)) => candidates.push(candidate),
            Ok(None) => {}
            Err(error) => unreadable.push(unreadable_candidate(loaded, &work, &ledger, &error)?),
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    if !unreadable.is_empty() {
        let status = if candidates.is_empty() {
            "unresolved"
        } else {
            "ambiguous"
        };
        let works = candidates
            .iter()
            .map(|(work, workflow, goal, _, progress, identity)| {
                Ok(json!({
                    "work": work,
                    "workflow": workflow,
                    "goal": goal,
                    "progress": compact_progress(progress),
                    "command": candidate_command(loaded, work)?,
                    "ledgerProducer": identity["ledgerProducer"],
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut result = json!({
            "status": status,
            "reason": {"code": "unreadable_candidate"},
            "effect": "no-change",
            "compact": true,
            "omitted": ["candidate progress detail"],
            "nextAction": safe_action("inspect_candidates"),
            "works": works,
            "unreadable": unreadable,
        });
        add_identity(&mut result, &work_identity(loaded, None)?);
        return Ok(result);
    }
    match candidates.len() {
        0 => none_result(loaded, finished),
        1 => {
            let (work, _, _, ledger, _, _) = candidates.pop().expect("one candidate exists");
            let (next, residual, presentation) = next_and_residual(loaded, &work, &ledger, true)
                .map_err(|error| discovery_error(error, &work))?;
            let mut result = json!({
                "status": "resumed",
                "work": work,
                "next": next,
                "residual": residual,
                "presentation": presentation
            });
            add_identity(&mut result, &work_identity(loaded, Some(&ledger))?);
            attach_continuation(loaded, &work, &mut result)?;
            Ok(result)
        }
        _ => {
            let works = candidates
                .iter()
                .map(|(work, workflow, goal, _, progress, identity)| {
                    Ok(json!({
                        "work": work.clone(),
                        "workflow": workflow,
                        "goal": goal,
                        "progress": compact_progress(progress),
                        "command": candidate_command(loaded, work)?,
                        "ledgerProducer": identity["ledgerProducer"],
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let mut result = json!({
            "status": "ambiguous",
            "reason": {"code": "ambiguous_candidates"},
            "effect": "no-change",
            "compact": true,
            "omitted": ["candidate progress detail"],
            "reference": {"works": candidates.iter().map(|(work, _, _, _, _, _)| work).collect::<Vec<_>>()},
            "nextAction": safe_action("choose_explicit_handle"),
            "works": works
            });
            add_identity(&mut result, &work_identity(loaded, None)?);
            Ok(result)
        }
    }
}

fn superseded_by_valid_claim(loaded: &Loaded, ledger: &str) -> Result<bool, String> {
    let claim_path = loaded.state_root.join(format!("{ledger}.supersede"));
    let Some(claim) = crate::run::ledger::valid_claim(&claim_path)? else {
        return Ok(false);
    };
    if claim["oldLedgerPath"] != ledger {
        return Ok(false);
    }
    let (_, old_events, old_source) = crate::run::ledger::load(loaded, ledger)?;
    let Some(old_head) = old_events.last() else {
        return Ok(false);
    };
    if claim["oldLedgerSha256"] != hash::bytes(old_source.as_bytes())
        || claim["oldRunId"] != old_events[0]["runId"]
        || claim["oldHeadEventSha256"] != old_head["eventSha256"]
        || claim["oldConfigSha256"] != old_events[0]["configSha256"]
    {
        return Ok(false);
    }
    let successor = claim["newLedgerPath"].as_str().unwrap_or_default();
    let (_, successor_events, _) = match crate::run::ledger::load(loaded, successor) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    let Some(start) = successor_events.first() else {
        return Ok(false);
    };
    let link = &start["supersedes"];
    Ok(start["runId"] == claim["newRunId"]
        && start["workflow"] == claim["workflow"]
        && link["ledgerPath"] == claim["oldLedgerPath"]
        && link["ledgerSha256"] == claim["oldLedgerSha256"]
        && link["runId"] == claim["oldRunId"]
        && link["headEventSha256"] == claim["oldHeadEventSha256"]
        && link["configSha256"] == claim["oldConfigSha256"])
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
        let (next, _residual, presentation) = next_and_residual(loaded, &work, &ledger, false)
            .map_err(|error| discovery_error(error, &work))?;
        // `presentation` sits where every other work response carries it, so
        // one documented path holds for every answer a host has to render.
        result["recent"] = json!({"work": work, "exitState": next["progress"]["state"]});
        result["presentation"] = presentation;
        add_identity(&mut result, &work_identity(loaded, Some(&ledger))?);
    } else {
        add_identity(&mut result, &work_identity(loaded, None)?);
    }
    Ok(result)
}

fn reference(work: &str) -> Value {
    json!({"work": work})
}

fn agent_response(mut value: Value) -> Value {
    crate::work::packet::sanitize_assignment(&mut value);
    value
}
fn safe_action(kind: &str) -> Value {
    json!({"type": kind, "safe": true})
}

fn diagnostic_error(
    error: &str,
    reason: &str,
    effect: &str,
    reference: Value,
    next_action: Value,
) -> String {
    format!(
        "{DIAGNOSTIC_PREFIX}{}",
        serde_json::to_string(&json!({
            "error": error,
            "reason": {"code": reason},
            "effect": effect,
            "reference": reference,
            "nextAction": next_action,
        }))
        .expect("work diagnostic is serializable")
    )
}

fn discovery_error(error: String, work: &str) -> String {
    if error.starts_with("artifact drift detected:") {
        return diagnostic_error(
            "artifact drift detected after run start. Inspect the recorded result and restore the original artifact before continuing.",
            "artifact_drift",
            "no-change",
            reference(work),
            safe_action("inspect"),
        );
    }
    if let Some(machine) = error
        .strip_prefix(crate::run::error::DRIFT_PREFIX)
        .or_else(|| error.strip_prefix(crate::run::error::LEGACY_DRIFT_PREFIX))
    {
        if let Ok(value) = serde_json::from_str::<Value>(machine) {
            let classification = match value["classification"].as_str() {
                Some("profile_drift") => "profile_drift",
                Some("memory_drift") => "memory_drift",
                Some("boundary_drift") => "boundary_drift",
                Some("harness_receipt_drift") => "harness_receipt_drift",
                _ => "config_drift",
            };
            let human = match classification {
                "profile_drift" => format!(
                    "profile drift detected after run start for {} (expected {}, current {}); continue with the recorded plan. Supersede only to bind a new run to changed inputs.",
                    value["agent"].as_str().unwrap_or("unknown agent"),
                    value["expectedProfileSha256"].as_str().unwrap_or("unknown"),
                    value["currentProfileSha256"].as_str().unwrap_or("unknown")
                ),
                "boundary_drift" => {
                    if value["currentBoundaryState"] == "absent" {
                        format!(
                            "run boundary manifest is absent after run start (expected hash {}); continue with the recorded plan. Supersede only to bind a new run to changed inputs.",
                            value["expectedBoundarySha256"].as_str().unwrap_or("unknown")
                        )
                    } else {
                        format!(
                            "run boundary manifest drift detected after run start (expected {}, current {}); continue with the recorded plan. Supersede only to bind a new run to changed inputs.",
                            value["expectedBoundarySha256"].as_str().unwrap_or("unknown"),
                            value["currentBoundarySha256"].as_str().unwrap_or("unknown")
                        )
                    }
                }
                "harness_receipt_drift" => format!(
                    "semantic harness drift detected after run start (expected {}, current {}); continue with the recorded plan. Exact receipt integrity failures are refused separately.",
                    value["expectedHarnessReceiptSha256"].as_str().unwrap_or("unknown"),
                    value["currentHarnessReceiptSha256"].as_str().unwrap_or("unknown")
                ),
                "memory_drift" => format!(
                    "memory drift detected after run start for {} (expected set {}, current set {}); continue with the recorded plan. Supersede only to bind a new run to changed inputs.",
                    value["agent"].as_str().unwrap_or("unknown agent"),
                    value["expectedMemorySetSha256"].as_str().unwrap_or("unknown"),
                    value["currentMemorySetSha256"].as_str().unwrap_or("unknown")
                ),
                _ => format!(
                    "configuration drift detected after run start (expected {}, current {}); continue with the recorded plan. Supersede only to bind a new run to changed inputs.",
                    value["expectedConfigSha256"].as_str().unwrap_or("unknown"),
                    value["currentConfigSha256"].as_str().unwrap_or("unknown")
                ),
            };
            return diagnostic_error(
                &human,
                classification,
                "no-change",
                reference(work),
                safe_action("continue"),
            );
        }
    }
    diagnostic_error(
        "work discovery failed. Inspect the recorded run before taking further action.",
        "corrupt_ledger",
        "no-change",
        reference(work),
        safe_action("inspect"),
    )
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
    next_and_residual_from_snapshot(loaded, work, &snapshot, resumed, true)
}

fn preflight_submission(
    loaded: &Loaded,
    work: &str,
    events: &[Value],
    source: &str,
) -> Result<(), String> {
    let snapshot = run::RunSnapshot::from_events(loaded, events, source)?;
    let _ = next_and_residual_from_snapshot(loaded, work, &snapshot, false, false)?;
    Ok(())
}

fn next_and_residual_from_snapshot(
    loaded: &Loaded,
    work: &str,
    snapshot: &run::RunSnapshot,
    resumed: bool,
    remember_presentation: bool,
) -> Result<(Value, Value, Value), String> {
    let mut next = next_from(loaded, work, snapshot)?;
    if let Some(assignment) = next["assignment"].as_str().map(str::to_owned) {
        attach_held(loaded, work, &assignment, &mut next)?;
    }
    let residual = crate::work::packet::project(work, snapshot, &next)?;
    next["resolvedActor"] = residual["humanHelp"]["nextAction"]["actor"].clone();
    // The fingerprint is computed only when a terminal acceptance has to be
    // compared with the tree; a running run already carries its own.
    let facts = crate::work::packet::facts(snapshot, &next, || {
        crate::run::inputs::fingerprint(loaded).ok()
    })?;
    let presentation = if remember_presentation {
        crate::presentation_events::project(
            &loaded.state_root,
            work,
            &residual,
            &next["progress"],
            &facts,
            resumed,
        )
    } else {
        crate::presentation_events::project_without_memory(
            &loaded.state_root,
            work,
            &residual,
            &next["progress"],
            &facts,
            resumed,
        )
    };
    Ok((next, residual, presentation))
}

fn next_for(loaded: &Loaded, work: &str, ledger: &str) -> Result<Value, String> {
    let mut next = next_from(loaded, work, &run::RunSnapshot::capture(loaded, ledger)?)?;
    if let Some(assignment) = next["assignment"].as_str().map(str::to_owned) {
        attach_held(loaded, work, &assignment, &mut next)?;
    }
    Ok(next)
}

fn attach_held(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    next: &mut Value,
) -> Result<(), String> {
    let held = held::discover(loaded, work, assignment)?;
    match held.len() {
        0 => {}
        1 => next["held"] = held[0].clone(),
        _ => next["heldResults"] = json!(held),
    }
    Ok(())
}

fn next_from(loaded: &Loaded, work: &str, snapshot: &run::RunSnapshot) -> Result<Value, String> {
    let value = snapshot.next_view(loaded)?;
    let progress = value["progress"].clone();
    let warnings = value["warnings"].clone();
    if value["status"] != "running" {
        return Ok(
            json!({"action": "done", "status": value["status"], "progress": progress, "warnings": warnings}),
        );
    }
    if let Some(pending) = current_check_target(snapshot)? {
        let mut result = json!({
            "action": "check",
            "check": pending.value(),
            "progress": progress,
            "warnings": warnings,
        });
        result["resolvedActor"] = crate::work::packet::resolved_actor(work, snapshot, &result)?;
        let view = snapshot.inspect_view();
        let status = snapshot.status_view()?;
        if let Ok(context) = crate::context::project(work, &view, &status, &result) {
            result["packet"] = json!({"context": context});
        }
        return Ok(agent_response(result));
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
    crate::work::packet::sanitize_assignment(&mut packet);
    let mut result = json!({
        "action": action,
        "assignment": assignment_handle,
        "role": assignment["role"],
        "agent": assignment["agent"],
        "packet": packet,
        "progress": progress,
        "warnings": warnings
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
            json!(["accepted", "rework", "blocked", "disposition"])
        };
    }
    Ok(agent_response(result))
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
    check_target(snapshot, false)
}

fn retry_check_target(snapshot: &run::RunSnapshot) -> Result<Option<PendingCheck>, String> {
    if let Some(target) = check_target(snapshot, false)? {
        return Ok(Some(target));
    }
    check_target(snapshot, true)
}

fn check_target(
    snapshot: &run::RunSnapshot,
    include_failed: bool,
) -> Result<Option<PendingCheck>, String> {
    if snapshot.status() != "running" {
        return Ok(None);
    }
    let status = snapshot.status_view()?;
    Ok(status["checks"]["targets"].as_array().and_then(|targets| {
        targets.iter().find_map(|item| {
            let pending =
                item["status"] == "missing" || (include_failed && item["status"] == "failed");
            pending.then(|| PendingCheck {
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

pub(crate) fn resolve(loaded: &Loaded, work: &str) -> Result<String, String> {
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
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, &directory)?;
    let suffix = &hash::bytes(bytes)[..16];
    let name = format!("{work}-{assignment}-{suffix}.md");
    let path = directory.join(&name);
    crate::project::managed_files::write_state_exclusive(&path, bytes)?;
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

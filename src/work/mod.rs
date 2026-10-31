//! Governed work façade.

pub(crate) mod action;
pub(crate) mod action_forms;
mod begin;
mod check;
pub(crate) mod check_recovery;
pub(crate) mod check_replacement;
#[cfg(test)]
pub(crate) use begin::begin;
pub(crate) use begin::{begin_with_pre_return, BasisOrigin, BeginOptions};
pub(crate) mod action_text;
pub(crate) mod closeout;
pub(crate) mod compact;
pub(crate) mod details;
mod disposition;
pub(crate) mod early_evidence;
pub(crate) mod effective_action;
mod expansion;
pub(crate) mod file_effect;
pub(crate) mod focus;
mod held;
mod locator;
pub(crate) mod managed_edit;
mod mutation_response;
mod next_snapshot;
pub(crate) mod packet;
mod permit_response;
pub(crate) mod pre_return_recovery;
pub(crate) mod quarantine;
pub(crate) mod readable;
mod recovery;
mod refreshed_action;
mod replacement_relation;
pub(crate) mod response_recovery;
mod resume;
mod return_result_impl;
mod stale_check;
pub(crate) mod start_delivery;
mod submission_preflight;
mod text_page;

use self::packet::attach_continuation;
use crate::{config::Loaded, evidence::hash, run};
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::Path;

pub(crate) use action::{act, ActOptions};
pub(crate) use check::check_with_timeout;
pub(crate) use disposition::{dispose, DispositionOptions};
pub(crate) use expansion::expand;
pub(crate) use focus::valid_work_handle;
pub(crate) use permit_response::permit;
pub(crate) use resume::resume;
pub(crate) use return_result_impl::return_result;

use locator::{runs_dir, valid_token, WORK_PREFIX};
use mutation_response::{
    bounded as bounded_mutation, recorded_projection_failure, recorded_protection_reference,
    recorded_reference,
};
use recovery::{
    add_identity, candidate_command, compact_progress, unreadable_candidate, work_identity,
    Candidate,
};

pub(crate) const DIAGNOSTIC_PREFIX: &str = "EXITBIND_WORK_DIAGNOSTIC:";
pub(crate) use locator::locator_for_ledger;

/// Read-only assessment; caller facts do not establish local activation.
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

/// Point navigation at an explicit, existing work. This changes no ledger.
pub(crate) fn set_focus(loaded: &Loaded, work: &str) -> Result<Value, String> {
    resolve(loaded, work).map_err(|_| "work handle is stale or unknown; focus unchanged")?;
    focus::write(loaded, work)?;
    Ok(
        json!({"work": work, "focus": {"updated": true, "authority": "none"}, "effect": "focus-only"}),
    )
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
    let (next, residual, presentation) =
        next_snapshot::next_and_residual(loaded, work, &ledger, false)
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
            "next": {"warnings": warnings, "current": next["current"]},
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
    crate::work::packet::validate(work, &snapshot, &next, &packet, &loaded.path)
}

fn reference(work: &str) -> Value {
    json!({"work": work})
}

fn agent_response(mut value: Value) -> Value {
    crate::work::packet::sanitize_assignment(&mut value);
    value
}
use effective_action::safe_action;

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
    // A typed changed-after-binding refusal already names its route.
    if crate::drift::split(&error).is_some() {
        return error;
    }
    if error.starts_with("artifact drift detected:") {
        return diagnostic_error(
            "artifact drift detected after run start. Inspect the recorded result and restore the original artifact before continuing.",
            "artifact_drift",
            "no-change",
            reference(work),
            safe_action("inspect"),
        );
    }
    if let Some(machine) = error.strip_prefix(crate::run::error::DRIFT_PREFIX) {
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
    let mut next = next_from_base(loaded, work, snapshot)?;
    let view = snapshot.inspect_view();
    crate::project::continuation::attach_validated_action_association(loaded, &view, &mut next)?;
    details::attach(loaded, work, snapshot, &mut next)?;
    Ok(next)
}

fn next_from_base(
    loaded: &Loaded,
    work: &str,
    snapshot: &run::RunSnapshot,
) -> Result<Value, String> {
    let value = snapshot.next_view(loaded)?;
    let progress = value["progress"].clone();
    let warnings = value["warnings"].clone();
    if value["status"] != "running" {
        let mut done = json!({"action": "done", "status": value["status"], "progress": progress, "warnings": warnings});
        for field in ["checkObservation", "quarantineVerification"] {
            if let Some(value) = snapshot.inspect_view().get(field) {
                done["packet"][field] = value.clone();
            }
        }
        return Ok(done);
    }
    let pending_check = current_check_target(snapshot)?;
    let assignment = value["assignments"]
        .as_array()
        .and_then(|items| items.first());
    let stale_check = match (&pending_check, assignment) {
        (Some(pending), Some(assignment)) => stale_check::for_assignment(
            loaded,
            work,
            snapshot,
            &pending.target_event_sha256,
            pending.requirement_id.as_deref(),
            assignment["role"].as_str().unwrap_or(""),
        )?,
        _ => None,
    };
    if let Some(pending) = pending_check.as_ref().filter(|_| stale_check.is_none()) {
        let mut result = json!({
            "action": "check",
            "check": pending.value(),
            "progress": progress,
            "warnings": warnings,
        });
        result["resolvedActor"] =
            crate::work::packet::resolved_actor(work, snapshot, &result, &loaded.path)?;
        let view = snapshot.inspect_view();
        let status = snapshot.status_view()?;
        if let Ok(context) = crate::context::project(work, &view, &status, &result) {
            result["packet"] = json!({"context": context});
        }
        return Ok(agent_response(result));
    }
    let assignment = assignment.ok_or("validated work has no next action")?;
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
    if let Some(stale_check) = stale_check {
        result["staleEndedCheck"] = json!({
            "status": if stale_check { "passed" } else { "failed" },
            "targetEventSha256": pending_check.as_ref().map(|check| check.target_event_sha256.as_str()),
            "requirementId": pending_check.as_ref().and_then(|check| check.requirement_id.as_deref()),
            "staleReason": "tested_inputs_changed",
        });
    }
    let resolved_actor =
        crate::work::packet::resolved_actor(work, snapshot, &result, &loaded.path)?;
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
        result["outcomes"] = if check::unresolved(&result) {
            // The timed-out observation's owned group is observed ended, so
            // the one existing environmental decision re-pends its check.
            if crate::run::environmental_check::closed_owned_group(&result["packet"]) {
                json!(["blocked", "rejected", "disposition"])
            } else {
                json!(["blocked", "rejected"])
            }
        } else if value["currentStage"] == 1 {
            json!(["scoped", "blocked"])
        } else if assignment.get("pendingDisposition").is_some() {
            json!(crate::run::disposition::PENDING_LEAD_OUTCOMES)
        } else if stale_check.is_some() {
            json!(["rework", "blocked"])
        } else {
            json!(["accepted", "rework", "blocked", "disposition"])
        };
    } else if role == "reviewer" && stale_check.is_some() {
        result["outcomes"] = json!(["rework", "blocked"]);
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
    if snapshot.status() != "running" {
        return Ok(None);
    }
    let status = snapshot.status_view()?;
    if status["checkObservation"].is_object() {
        return Ok(None);
    }
    Ok(status["checks"]["targets"].as_array().and_then(|targets| {
        targets.iter().find_map(|item| {
            (item["status"] == "missing").then(|| PendingCheck {
                target_event_sha256: item["targetEventSha256"].as_str().unwrap_or("").to_owned(),
                requirement_id: item["requirementId"].as_str().map(str::to_owned),
            })
        })
    }))
}

fn retry_check_target(snapshot: &run::RunSnapshot) -> Result<Option<PendingCheck>, String> {
    if let Some(target) = current_check_target(snapshot)? {
        return Ok(Some(target));
    }
    if snapshot.status() != "running" {
        return Ok(None);
    }
    let status = snapshot.status_view()?;
    Ok(status["checks"]["targets"].as_array().and_then(|targets| {
        targets.iter().find_map(|item| {
            (item["status"] == "failed").then(|| PendingCheck {
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
) -> Result<(String, bool), String> {
    let directory = loaded.state_root.join(artifacts_dir());
    crate::project::managed_files::ensure_state_directory(&loaded.state_root, &directory)?;
    let suffix = &hash::bytes(bytes)[..16];
    let name = format!("{work}-{assignment}-{suffix}.md");
    let path = directory.join(&name);
    let relative = format!("{}/{}", artifacts_dir(), name);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            // A held result may already have prepared these exact bytes before
            // a governor refusal. Reuse only verified immutable content; never
            // overwrite it or treat it as a newly owned cleanup target.
            run::artifact::read(
                loaded,
                &json!({"root":"state", "path":relative,
                    "sha256":hash::bytes(bytes), "bytes":bytes.len()}),
                "retained worker result",
            )?;
            return Ok((relative, false));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    crate::project::managed_files::write_state_exclusive(&path, bytes)?;
    Ok((relative, true))
}

fn assignment_handle(work: &str, assignment: &Value) -> Result<String, String> {
    crate::run::assignment::handle(work, assignment)
}

fn timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

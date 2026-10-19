//! Validation of retained custody receipts for known-ended quarantine steps.

use crate::{config::Loaded, evidence::hash, run::artifact};
use serde_json::Value;
use std::path::Path;

fn valid_sha(sha: &str) -> bool {
    sha.len() == 64
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate(loaded: &Loaded, descriptor: &Value, expected_exit: i64) -> Result<(), String> {
    if !super::exact(descriptor, &["root", "path", "sha256", "bytes"])
        || descriptor["root"] != "state"
        || descriptor["sha256"]
            .as_str()
            .map_or(true, |sha| !super::sha(&Value::String(sha.into())))
        || descriptor["bytes"].as_u64().is_none()
    {
        return Err("retained custody evidence reference is invalid".into());
    }
    let receipt = read(loaded, descriptor)?;
    if !valid_sha(receipt["adapterSha256"].as_str().unwrap_or_default())
        || !receipt["requestId"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
        || receipt["rootIdentityPreserved"] != true
        || receipt["ownedExecutionEnded"] != true
        || receipt["namespaceReaperWaitedByPidfd"] != true
        || receipt["remainingNamespaceMembers"]
            .as_array()
            .map_or(true, |members| !members.is_empty())
        || !receipt.get("failure").is_some_and(Value::is_null)
        || receipt["timedOut"] != false
        || receipt["cancelled"] != false
        || receipt["launcherExit"].as_i64() != Some(expected_exit)
        || expected_exit == 0
    {
        return Err(
            "retained custody receipt does not prove a known-ended failed execution".into(),
        );
    }
    Ok(())
}

pub(crate) fn validate_against_first_check(
    loaded: &Loaded,
    descriptor: &Value,
    first_marker: &Value,
    check: &Value,
    expected_exit: i64,
) -> Result<(), String> {
    validate(loaded, descriptor, expected_exit)?;
    let receipt = read(loaded, descriptor)?;
    let review = &first_marker["verification"]["isolationReview"];
    let feature_work = review["work"]
        .as_str()
        .ok_or("first verification's accepted plan Work is missing")?;
    if !crate::work::valid_work_handle(feature_work) {
        return Err("first verification's accepted plan Work is invalid".into());
    }
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &feature_work[4..]);
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let feature_start = events
        .first()
        .ok_or("first verification's accepted plan Work is empty")?;
    let feature_head = events
        .last()
        .ok_or("first verification's accepted plan Work has no head")?;
    let feature_state = crate::run::state::reduce(&events)?;
    let reviewer = events.iter().find(|event| {
        event["action"] == "submit"
            && event["role"] == "reviewer"
            && event["outcome"] == "approved"
            && event["eventSha256"] == review["reviewEventSha256"]
    });
    let accepted = events.iter().find(|event| {
        event["action"] == "submit"
            && event["role"] == "lead"
            && event["outcome"] == "accepted"
            && event["eventSha256"] == review["acceptanceEventSha256"]
    });
    let worker = events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
        })
        .ok_or("first verification's accepted plan artifact is missing")?;
    if feature_start["eventSha256"] != review["startEventSha256"]
        || feature_head["eventSha256"] != review["headEventSha256"]
        || feature_state["status"] != "accepted"
        || reviewer.is_none()
        || accepted.is_none()
    {
        return Err(
            "first verification's accepted plan Work is no longer exact and accepted".into(),
        );
    }
    let bytes = artifact::read_exact(
        loaded,
        &worker["artifact"],
        "first verification plan",
        256 * 1024,
    )?;
    let report: Value =
        serde_json::from_slice(&bytes).map_err(|_| "first verification plan is not valid JSON")?;
    let plan = &report["plan"];
    let environment = &plan["environment"];
    let planned_uid = environment["executorUid"].as_u64();
    let receipt_uid = receipt["uid"].as_u64();
    let planned_parent = environment["freshParent"].as_str();
    let planned_manifest = environment["inputManifest"].as_str();
    let planned_manifest_sha = environment["inputManifestSha256"].as_str();
    let receipt_job_root = receipt["jobRoot"].as_str();
    let receipt_working_directory = receipt["inside"]["environment"]["PWD"].as_str();
    let receipt_argv = receipt["argv"].as_array();
    let inside_argv = receipt["inside"]["argv"].as_array();
    if report["kind"] != "quarantine-verification-plan"
        || report["protocol"] != 1
        || !plan.is_object()
        || hash::value(plan) != review["planSha256"]
        || hash::value(&plan["effectInventory"]) != review["effectInventorySha256"]
        || hash::value(environment) != first_marker["verification"]["environmentSha256"]
        || plan["artifactSha256"] != first_marker["verification"]["artifactSha256"]
        || plan["inputsSha256"] != feature_state["inputsSha256"]
        || hash::text(plan["command"].as_str().unwrap_or_default())
            != first_marker["verification"]["commandSha256"]
        // The plan records the accepting feature Work's state-aware digest;
        // the marker and the first verification's check both stay full-domain
        // because a quarantine verification keeps every guidance path strict.
        || check["inputsSha256"] != first_marker["verification"]["inputsSha256"]
        || check["configSha256"] != first_marker["verification"]["configSha256"]
        || check["checkCommandSha256"] != hash::text(plan["command"].as_str().unwrap_or_default())
        || environment["observerSha256"] != receipt["adapterSha256"]
        || planned_uid.is_none()
        || receipt_uid.is_none()
        || planned_uid != receipt_uid
        || planned_parent.is_none()
        || receipt_job_root.is_none()
        || receipt_working_directory != receipt_job_root
        || !Path::new(receipt_job_root.unwrap_or_default())
            .starts_with(planned_parent.unwrap_or_default())
        || receipt_job_root == planned_parent
        || planned_manifest.map_or(true, str::is_empty)
        || planned_manifest_sha.map_or(true, |sha| !valid_sha(sha))
        || receipt_argv.is_none()
        || inside_argv != receipt_argv
        || !receipt_argv
            .into_iter()
            .flatten()
            .any(|arg| arg.as_str() == planned_manifest)
        || !receipt_argv
            .into_iter()
            .flatten()
            .any(|arg| arg.as_str() == planned_manifest_sha)
    {
        return Err("retained custody receipt does not match the first verification's reviewed observer, inputs, and command".into());
    }
    Ok(())
}

fn read(loaded: &Loaded, descriptor: &Value) -> Result<Value, String> {
    let bytes = artifact::read_exact(loaded, descriptor, "retained custody receipt", 256 * 1024)?;
    serde_json::from_slice(&bytes).map_err(|_| "retained custody receipt is not valid JSON".into())
}

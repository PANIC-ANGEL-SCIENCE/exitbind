//! Agent-facing work façade over the strict run protocol.

use crate::{config::Loaded, hash, run};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Read;

const WORK_PREFIX: &str = "smw_";
const ASSIGNMENT_PREFIX: &str = "sma_";

fn runs_dir() -> String {
    format!("{}/runs", crate::project_layout::state_namespace())
}

fn artifacts_dir() -> String {
    format!("{}/artifacts", crate::project_layout::state_namespace())
}

pub(crate) fn begin(
    loaded: &Loaded,
    workflow: &str,
    goal: &str,
    check_command: &str,
    boundary: Option<&str>,
    harness_receipt: Option<&str>,
    proof_origin: Option<&str>,
) -> Result<Value, String> {
    let token = hash::text(&format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        workflow,
        goal,
        check_command,
        loaded.source,
        std::process::id(),
        timestamp_nanos()
    ));
    let ledger = format!("{}/work-{token}.jsonl", runs_dir());
    let _started = run::start_with_policy(
        loaded,
        workflow,
        goal,
        &ledger,
        boundary,
        harness_receipt,
        Some(check_command),
        proof_origin,
    )?;
    let work = format!("{WORK_PREFIX}{token}");
    let next = next_for(loaded, &work, &ledger)?;
    Ok(json!({"work": work, "next": next}))
}

pub(crate) fn next(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    Ok(json!({
        "work": work,
        "next": next_for(loaded, work, &ledger)?,
        "residual": residual_packet(loaded, work, &ledger)?
    }))
}

pub(crate) fn return_result(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    outcome: &str,
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
    let allowed = action["role"] == "lead"
        && action["outcomes"]
            .as_array()
            .is_some_and(|outcomes| outcomes.iter().any(|item| item == outcome))
        || action["role"] == "reviewer" && ["approved", "rework", "blocked"].contains(&outcome)
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
    let _submitted = run::submit_for_assignment(loaded, &ledger, expected, outcome, || {
        write_artifact(loaded, work, assignment, &bytes)
    })?;
    Ok(json!({"work": work, "next": next_for(loaded, work, &ledger)?}))
}

pub(crate) fn check(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let target =
        current_check_target(loaded, &ledger)?.ok_or("no current worker check is pending")?;
    let _observed = run::observe_check(loaded, &ledger, &target, None)?;
    Ok(json!({"work": work, "next": next_for(loaded, work, &ledger)?}))
}

pub(crate) fn resume(loaded: &Loaded) -> Result<Value, String> {
    let directory = loaded.state_root.join(runs_dir());
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return none_result(),
        Err(error) => return Err(error.to_string()),
    };
    let mut candidates = Vec::new();
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
        let view = run::inspect(loaded, &ledger)?;
        if view["status"] == "running" {
            let progress = run::next(loaded, &ledger)?["progress"].clone();
            candidates.push((
                format!("{WORK_PREFIX}{token}"),
                view["workflow"].clone(),
                view["goal"].clone(),
                ledger,
                progress,
            ));
        }
    }
    match candidates.len() {
        0 => none_result(),
        1 => {
            let (work, _, _, ledger, _) = candidates.pop().expect("one candidate exists");
            Ok(json!({
                "status": "resumed",
                "work": work.clone(),
                "next": next_for(loaded, &work, &ledger)?,
                "residual": residual_packet(loaded, &work, &ledger)?
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

fn none_result() -> Result<Value, String> {
    Ok(json!({"status": "none", "next": {"action": "none", "reason": "no_active_work"}}))
}

fn next_for(loaded: &Loaded, work: &str, ledger: &str) -> Result<Value, String> {
    let value = run::next(loaded, ledger)?;
    let progress = value["progress"].clone();
    if value["status"] != "running" {
        return Ok(json!({"action": "done", "status": value["status"], "progress": progress}));
    }
    if current_check_target(loaded, ledger)?.is_some() {
        return Ok(json!({"action": "check", "progress": progress}));
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
        if let Some(summary) = check_summary(loaded, ledger)? {
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
    if role == "lead" {
        result["outcomes"] = if value["currentStage"] == 1 {
            json!(["scoped", "blocked"])
        } else {
            json!(["accepted", "rework", "blocked"])
        };
    }
    Ok(result)
}

fn residual_packet(loaded: &Loaded, work: &str, ledger: &str) -> Result<Value, String> {
    let view = run::inspect(loaded, ledger)?;
    let status = run::status(loaded, ledger)?;
    let next = next_for(loaded, work, ledger)?;
    let mut established = Vec::new();
    let mut still_valid = Vec::new();
    let mut remaining = Vec::new();
    let mut do_not_repeat = Vec::new();
    let mut check_obligation_recorded = false;

    if view["goal"].as_str().is_some_and(|goal| !goal.is_empty()) {
        established.push(json!({"fact": "work_identity_recorded", "status": "completed"}));
    }
    if next["progress"]["weights"]["lead"]
        .as_u64()
        .is_some_and(|value| value > 0)
        || next["progress"]["percent"]
            .as_u64()
            .is_some_and(|value| value > 0)
    {
        established.push(json!({"fact": "scope_recorded", "status": "completed"}));
        do_not_repeat.push(json!("scope"));
    }

    if let Some(targets) = status["checks"]["targets"].as_array() {
        let passed = targets
            .iter()
            .filter(|target| target["status"] == "passed")
            .count();
        let missing = targets
            .iter()
            .filter(|target| target["status"] == "missing")
            .count();
        let failed = targets
            .iter()
            .filter(|target| target["status"] == "failed")
            .count();
        if passed > 0 {
            still_valid.push(json!({
                "evidence": "current_check",
                "status": "passed",
                "count": passed,
                "subject": "current"
            }));
            do_not_repeat.push(json!("passed_check"));
        }
        if missing > 0 {
            remaining.push(json!({"obligation": "check", "count": missing}));
            check_obligation_recorded = true;
        }
        if failed > 0 {
            remaining.push(json!({"obligation": "rework_after_failed_check", "count": failed}));
        }
    }

    if next["action"] == "lead_decision" {
        still_valid.push(json!({"evidence": "current_review", "status": "approved"}));
        do_not_repeat.push(json!("review"));
        remaining.push(json!({"obligation": "lead_acceptance"}));
    } else if next["role"] == "reviewer" {
        remaining.push(json!({"obligation": "review"}));
    } else if next["role"] == "worker" {
        remaining.push(json!({"obligation": "implementation"}));
    } else if next["action"] == "check" && !check_obligation_recorded {
        remaining.push(json!({"obligation": "check"}));
    }

    Ok(json!({
        "work": work,
        "workflow": view["workflow"],
        "goal": view["goal"],
        "currentSubject": view["subject"],
        "alreadyEstablished": established,
        "stillValid": still_valid,
        "remaining": remaining,
        "next": next["action"],
        "doNotRepeat": do_not_repeat,
        "invalidation": {
            "rule": "reuse only when exact subject and governing evidence identity remain current",
            "sessionRestartInvalidates": false,
            "subjectChangeInvalidates": true
        }
    }))
}

fn current_check_target(loaded: &Loaded, ledger: &str) -> Result<Option<String>, String> {
    let state = run::inspect(loaded, ledger)?;
    if state["status"] != "running" {
        return Ok(None);
    }
    let status = run::status(loaded, ledger)?;
    Ok(status["checks"]["targets"].as_array().and_then(|targets| {
        targets.iter().find_map(|item| {
            (item["status"] == "missing")
                .then(|| item["targetEventSha256"].as_str().map(str::to_owned))
                .flatten()
        })
    }))
}

fn check_summary(loaded: &Loaded, ledger: &str) -> Result<Option<Value>, String> {
    let status = run::status(loaded, ledger)?;
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
    crate::managed_files::ensure_managed_directory(&loaded.state_root, &directory)?;
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
    let stage = assignment["stage"]
        .as_u64()
        .ok_or("assignment stage is invalid")?;
    let attempt = assignment["attempt"]
        .as_u64()
        .ok_or("assignment attempt is invalid")?;
    let agent = assignment["agent"]
        .as_str()
        .ok_or("assignment agent is invalid")?;
    Ok(format!(
        "{ASSIGNMENT_PREFIX}{}",
        hash::text(&format!("{work}\n{stage}\n{attempt}\n{agent}"))
    ))
}

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

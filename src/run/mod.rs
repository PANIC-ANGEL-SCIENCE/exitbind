//! Run orchestration for bounded agent workflows.
//!
//! Ledger storage and locking live in `ledger`; event validation and state
//! transitions live in `state`. This module coordinates those boundaries
//! and checks live configuration/artifact evidence before mutation.

pub(crate) mod artifact;
pub(crate) mod assignment;
pub(crate) mod capture;
pub(crate) mod disposition;
pub(crate) mod error;
mod event_detail;
mod event_evidence;
mod evidence_identity;
mod governor;
pub(crate) mod inputs;
pub(crate) mod ledger;
mod lifecycle;
mod observed_check;
pub(crate) mod preservation_assignment;
mod reducer;
mod repair_recovery;
mod result_reference;
pub(crate) mod state;
mod submission;
mod view;

use self::governor::{canonical_work_for_ledger, GOVERNOR_COMPLETION_REFUSAL};
pub(crate) use self::governor::{
    evidence_for_assignment, governor_request_digest, permit_for_assignment, replan_for_assignment,
    sensor_request_for_assignment, sensor_result_for_assignment, submission_refusal,
    AssignmentIdentity, RecordedProtection, SubmissionRefusal, REQUEST_ID_MAX_BYTES,
};
pub use self::lifecycle::{next, review_policy, start, start_with_policy};
use self::reducer::{
    action_drift_warning, artifact_current, extension_from_cli, fallback_provenance,
    has_worker_stage, live_inputs, nondecreasing, selected_plan,
};
pub(crate) use self::reducer::{
    assert_current_for_receipt, assert_no_drift, drift_value, drift_warning,
    input_drift_warning_for, reduce_live, result, subject, warn_drift, InputContext, RunSnapshot,
};
pub use self::submission::submit;
pub(crate) use self::submission::submit_for_assignment;
pub use self::view::{
    explain, inspect, report, report_markdown, status, supersede, supersede_with_policy,
};
pub(crate) use self::view::{human_explain, human_status, terminal_display};

pub use self::error::DriftError;
pub use self::event_detail::inspect_event;
pub(crate) use self::evidence_identity::validate_governor_identity as validate_evidence_governor_identity;
use self::ledger::{
    append, claim_path, ledger_path, load, load_at, obtain_claim, predecessor, rollback_claim,
    with_lock,
};
pub use self::observed_check::{observe_check, observe_check_for_requirement};
pub(crate) use self::result_reference::result_ref_evidence;
use self::{error as run_error, ledger as run_ledger, state as run_state};
use crate::{
    config::{self, Loaded},
    envelope,
    evidence::hash,
};
use serde_json::{json, Value};
use std::fs;
const DEFAULT_OBSERVE_TIMEOUT_MS: u64 = 1_800_000;

pub fn record_check(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    check_command: &str,
    exit_code: &str,
    duration_ms: Option<&str>,
) -> Result<Value, String> {
    record_check_for_requirement(
        loaded,
        ledger,
        target,
        None,
        check_command,
        exit_code,
        duration_ms,
    )
}

/// Record a caller-supplied result for either the normal deterministic check or
/// a named preservation requirement on the exact current worker completion.
pub fn record_check_for_requirement(
    loaded: &Loaded,
    ledger: &str,
    target: &str,
    requirement_id: Option<&str>,
    check_command: &str,
    exit_code: &str,
    duration_ms: Option<&str>,
) -> Result<Value, String> {
    if check_command.trim().is_empty() {
        return Err("--check-command requires a non-empty value".into());
    }
    if check_command.contains('\0') {
        return Err("--check-command must not contain NUL bytes".into());
    }
    let exit_code = parse_nonnegative("--exit-code", exit_code)?;
    let duration_ms = duration_ms
        .map(|value| parse_nonnegative("--duration-ms", value))
        .transpose()?;
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    let targets = [path.path.as_path(), path.lock.as_path()];
    crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
    with_lock(&path, || {
        crate::project::git_preflight::refuse_tracked_targets(&loaded.state_root, &targets)?;
        if claim_path(&path).exists() {
            return Err("run has been superseded; no mutation was made".into());
        }
        let (_, events, source) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        if state["status"] != "running" {
            return Err("run has already reached a terminal state; no mutation was made".into());
        }
        let warning = action_drift_warning(loaded, &state)?;
        crate::run::artifact::assert_current(loaded, &state)?;
        let policy = active_check_policy(&state, requirement_id)?;
        if policy.command != check_command
            || policy.command_sha256 != crate::evidence::hash::text(check_command)
        {
            return Err("check command does not match the configured run policy".into());
        }
        crate::run_value::validate_check_target(&state, target)?;
        let last = events.last().ok_or("run ledger has no event head")?;
        let version = state["version"].as_u64().unwrap_or(3);
        let mut value = if version >= 4 {
            json!({
            "version": version,
            "kind": "run",
            "producer": crate::producer::evidence_for_version(version),
            "action": "check",
            "runId": state["runId"],
            "targetEventSha256": target,
            "checkCommand": check_command,
            "checkCommandSha256": crate::evidence::hash::text(check_command),
            "origin": policy.origin.as_str(),
            "acquisition": "reported",
            "result": {"kind": "exit", "code": exit_code},
            "previousEventSha256": last["eventSha256"],
            "timestamp": nondecreasing(&last["timestamp"])?
            })
        } else {
            json!({
                "version": 3,
                "kind": "run",
                "producer": crate::producer::evidence_for_version(3),
                "action": "check",
                "runId": state["runId"],
                "targetEventSha256": target,
                "checkCommand": check_command,
            "checkCommandSha256": crate::evidence::hash::text(check_command),
                "origin": policy.origin.as_str(),
                "exitCode": exit_code,
                "previousEventSha256": last["eventSha256"],
                "timestamp": nondecreasing(&last["timestamp"])?
            })
        };
        if version >= 5 {
            value["subjectSha256"] = state["subject"]["sha256"].clone();
        }
        if version >= 6 {
            value["inputsSha256"] = live_inputs(&state)?;
            if let Some(id) = requirement_id {
                value["requirementId"] = json!(id);
            }
        }
        value["durationMs"] = duration_ms.map_or(Value::Null, |duration| json!(duration));
        let event = run_state::make_event(value);
        let mut all = events.clone();
        all.push(event.clone());
        let next_state = run_state::reduce(&all)?;
        append(&path, &event, false, &source)?;
        Ok(json!({
            "valid": true,
            "event": event,
            "runId": next_state["runId"],
            "status": next_state["status"],
            "checks": crate::run_value::status(&next_state, Some(true))?["checks"],
            "warnings": warning.map_or_else(|| json!([]), |warning| json!([warning]))
        }))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveCheckPolicy {
    command: String,
    command_sha256: String,
    origin: crate::run_value::ProofOrigin,
}

fn active_check_policy(
    state: &Value,
    requirement_id: Option<&str>,
) -> Result<ActiveCheckPolicy, String> {
    if let Some(id) = requirement_id {
        if state["version"].as_u64() < Some(6) {
            return Err("preservation checks require an Exitbind v6 run".into());
        }
        let preservation = state
            .get("preservation")
            .ok_or("run has no configured preservation policy")?;
        let preservation = crate::run_value::preservation_from_value(preservation, 0)
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        let requirement = preservation
            .requirement(id)
            .ok_or("preservation requirement is not configured")?;
        Ok(ActiveCheckPolicy {
            command: requirement.command.clone(),
            command_sha256: requirement.command_sha256.clone(),
            origin: requirement.origin,
        })
    } else {
        let policy = state
            .get("checkPolicy")
            .ok_or("run has no configured check policy")?;
        let policy = crate::run_value::policy_from_value(policy, 0)
            .map_err(|error| error.replacen("line 0", "state", 1))?;
        Ok(ActiveCheckPolicy {
            command: policy.command,
            command_sha256: policy.command_sha256,
            origin: policy.origin,
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::canonical_work_for_ledger;
    use super::capture::{process_group_exists, terminate_process_group};
    use crate::run::ledger::LedgerPath;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };

    #[test]
    fn canonical_work_binding_rejects_cross_run_ledger_pairs() {
        let root = PathBuf::from("/fixture");
        let token_a = "a".repeat(64);
        let token_b = "b".repeat(64);
        let path = root.join(format!(
            "{}/runs/work-{token_a}.jsonl",
            crate::project::layout_types::state_namespace()
        ));
        let ledger = LedgerPath {
            path: path.clone(),
            lock: root.join(".exitbind/locks/permit.lock"),
            root,
            expected: path,
        };
        assert_eq!(
            canonical_work_for_ledger(&ledger).unwrap(),
            format!("smw_{token_a}")
        );
        assert_ne!(
            canonical_work_for_ledger(&ledger).unwrap(),
            format!("smw_{token_b}")
        );
    }

    #[test]
    fn cleanup_reports_reap_failure_after_still_killing_the_group() {
        let marker =
            std::env::temp_dir().join(format!("soulmate-cleanup-{}.ready", std::process::id()));
        let _ = fs::remove_file(&marker);
        let command = format!(
            "sh -c 'trap : TERM HUP; echo ready > {}; while :; do :; done' & exit 0",
            marker.display()
        );
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .process_group(0)
            .spawn()
            .expect("cleanup fixture should launch");
        let child_pid = i32::try_from(child.id()).expect("fixture pid should fit POSIX");
        let ready_started = Instant::now();
        while !marker.exists() && ready_started.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(1));
        }
        let waited = unsafe { libc::waitpid(child_pid, std::ptr::null_mut(), 0) };
        let group_before = process_group_exists(child_pid).unwrap_or(false);

        let started = Instant::now();
        let failure = terminate_process_group(&mut child).expect_err("external reap is an error");
        let group_after = process_group_exists(child_pid);
        let ready = marker.exists();
        let _ = fs::remove_file(marker);
        assert_eq!(waited, child_pid);
        assert!(ready, "cleanup fixture did not become ready");
        assert!(
            group_before,
            "cleanup fixture process group was not present"
        );
        assert!(
            matches!(group_after, Ok(false)),
            "cleanup left the process group behind: {group_after:?}"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "cleanup skipped its bounded TERM grace"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cleanup exceeded its bounded budget"
        );
        assert!(failure.contains("could not be reaped"), "{failure}");
    }
}

fn parse_nonnegative(option: &str, value: &str) -> Result<u64, String> {
    if value.trim().is_empty() || value.starts_with('+') || value.starts_with('-') {
        return Err(format!("{option} must be a non-negative integer"));
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{option} must be a non-negative integer"))
}

fn parse_positive(option: &str, value: &str) -> Result<u64, String> {
    let parsed = parse_nonnegative(option, value)
        .map_err(|_| format!("{option} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{option} must be a positive integer"));
    }
    Ok(parsed)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

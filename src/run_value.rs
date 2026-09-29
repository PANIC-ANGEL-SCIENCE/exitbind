//! Typed value-proof records derived from a run ledger.
//!
//! The checked-run value boundary validates both caller-reported and locally
//! observed results.  Observation execution itself lives in `run.rs`.

use crate::run_exit::{CheckAssessment, CheckTarget};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub(crate) const CHECK_POLICY_VERSION: u64 = 1;
pub(crate) const VALUE_REPORT_VERSION: u64 = 1;

const SHA_LEN: usize = 64;

mod human;
mod report;
mod validation;
pub(crate) use human::*;
pub(crate) use report::{aggregate, markdown};
pub(crate) use validation::{
    protection_event, validate_check_against_state, validate_check_event, validate_check_target,
    validate_protection_against_state, validate_protection_event, MAX_CAPTURE_BYTES,
};

#[cfg(test)]
#[path = "run_value/progress_tests.rs"]
mod progress_tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckPolicy {
    pub(crate) command: String,
    pub(crate) command_sha256: String,
    pub(crate) origin: ProofOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreservationRequirement {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) command: String,
    pub(crate) command_sha256: String,
    pub(crate) origin: ProofOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreservationPolicy {
    pub(crate) requirements: Vec<PreservationRequirement>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProofOrigin {
    LocalReport,
    Synthetic,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CheckAction {
    Check,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProtectionReason {
    CheckMissing,
    CheckFailed,
    PreservationMissing,
    PreservationFailed,
}

impl ProtectionReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::CheckMissing => "check_missing",
            Self::CheckFailed => "check_failed",
            Self::PreservationMissing => "preservation_missing",
            Self::PreservationFailed => "preservation_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EvidenceStatus {
    Missing,
    Failed,
}

pub(crate) fn check_result(event: &Value) -> Option<Value> {
    event.get("result").cloned()
}

pub(crate) fn check_acquisition(event: &Value) -> Option<String> {
    Some(
        event["acquisition"]
            .as_str()
            .unwrap_or("reported")
            .to_owned(),
    )
}

pub(crate) fn check_exit_code(event: &Value) -> Option<u64> {
    event["exitCode"].as_u64().or_else(|| {
        (event["result"]["kind"] == "exit")
            .then(|| event["result"]["code"].as_u64())
            .flatten()
    })
}

pub(crate) fn check_passed(event: &Value) -> bool {
    check_exit_code(event) == Some(0)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckObservation {
    version: u64,
    kind: String,
    producer: Value,
    action: CheckAction,
    #[serde(rename = "runId")]
    run_id: String,
    #[serde(rename = "targetEventSha256")]
    target_event_sha256: String,
    #[serde(rename = "checkCommand")]
    check_command: String,
    #[serde(rename = "checkCommandSha256")]
    check_command_sha256: String,
    origin: ProofOrigin,
    #[serde(rename = "exitCode")]
    _exit_code: u64,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
    #[serde(rename = "previousEventSha256")]
    previous_event_sha256: Option<String>,
    timestamp: String,
    #[serde(rename = "eventSha256")]
    event_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum CheckResult {
    #[serde(rename = "exit")]
    Exit { code: u64 },
    #[serde(rename = "signal")]
    Signal { signal: u64 },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckObservationV4 {
    version: u64,
    kind: String,
    producer: Value,
    action: CheckAction,
    #[serde(rename = "runId")]
    run_id: String,
    #[serde(rename = "targetEventSha256")]
    target_event_sha256: String,
    #[serde(rename = "requirementId")]
    requirement_id: Option<String>,
    #[serde(rename = "checkCommand")]
    check_command: String,
    #[serde(rename = "checkCommandSha256")]
    check_command_sha256: String,
    origin: ProofOrigin,
    acquisition: String,
    #[serde(rename = "configSha256", default)]
    config_sha256: Option<String>,
    result: CheckResult,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
    #[serde(default)]
    stdout: Option<Value>,
    #[serde(default)]
    stderr: Option<Value>,
    #[serde(rename = "previousEventSha256")]
    previous_event_sha256: Option<String>,
    timestamp: String,
    #[serde(rename = "eventSha256")]
    event_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckEvidence {
    #[serde(rename = "targetEventSha256")]
    target_event_sha256: String,
    #[serde(rename = "requirementId")]
    requirement_id: Option<String>,
    status: EvidenceStatus,
    #[serde(rename = "checkEventSha256")]
    check_event_sha256: Option<String>,
    #[serde(rename = "exitCode")]
    exit_code: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectionRecord {
    version: u64,
    kind: String,
    producer: Value,
    action: ProtectionAction,
    #[serde(rename = "runId")]
    run_id: String,
    stage: u64,
    attempt: u64,
    actor: String,
    role: String,
    #[serde(rename = "attemptedOutcome")]
    attempted_outcome: String,
    reason: ProtectionReason,
    #[serde(rename = "checkEvidence")]
    check_evidence: Vec<CheckEvidence>,
    #[serde(rename = "origin")]
    _origin: ProofOrigin,
    #[serde(rename = "previousEventSha256")]
    previous_event_sha256: Option<String>,
    timestamp: String,
    #[serde(rename = "eventSha256")]
    event_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProtectionAction {
    Protect,
}

impl ProofOrigin {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "local_report" => Ok(Self::LocalReport),
            "synthetic" => Ok(Self::Synthetic),
            _ => Err("proof origin must be local_report or synthetic".into()),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LocalReport => "local_report",
            Self::Synthetic => "synthetic",
        }
    }
}

impl CheckPolicy {
    pub(crate) fn value(&self) -> Value {
        json!({
            "version": CHECK_POLICY_VERSION,
            "command": self.command,
            "commandSha256": self.command_sha256,
            "origin": self.origin.as_str(),
        })
    }
}

impl PreservationRequirement {
    pub(crate) fn value(&self) -> Value {
        json!({
            "id": self.id,
            "text": self.text,
            "command": self.command,
            "commandSha256": self.command_sha256,
            "origin": self.origin.as_str(),
        })
    }
}

impl PreservationPolicy {
    pub(crate) fn value(&self) -> Value {
        json!({
            "version": CHECK_POLICY_VERSION,
            "requirements": self
                .requirements
                .iter()
                .map(PreservationRequirement::value)
                .collect::<Vec<_>>(),
        })
    }

    pub(crate) fn requirement(&self, id: &str) -> Option<&PreservationRequirement> {
        self.requirements.iter().find(|item| item.id == id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckPolicyRecord {
    version: u64,
    command: String,
    #[serde(rename = "commandSha256")]
    command_sha256: String,
    origin: ProofOrigin,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreservationPolicyRecord {
    version: u64,
    requirements: Vec<PreservationRequirementRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreservationRequirementRecord {
    id: String,
    text: String,
    command: String,
    #[serde(rename = "commandSha256")]
    command_sha256: String,
    origin: ProofOrigin,
}

/// Parse the optional start-time policy.  A proof origin without a command is
/// intentionally rejected so `--proof-origin` cannot opt a run into a weaker
/// or ambiguous checked state.
pub(crate) fn policy_from_cli(
    command: Option<&str>,
    origin: Option<&str>,
) -> Result<Option<CheckPolicy>, String> {
    let Some(command) = command else {
        if origin.is_some() {
            return Err("--proof-origin requires --check-command".into());
        }
        return Ok(None);
    };
    if command.trim().is_empty() {
        return Err("--check-command requires a non-empty value".into());
    }
    if command.contains('\0') {
        return Err("--check-command must not contain NUL bytes".into());
    }
    let origin = origin
        .map(ProofOrigin::parse)
        .transpose()?
        .unwrap_or(ProofOrigin::LocalReport);
    Ok(Some(CheckPolicy {
        command: command.to_owned(),
        command_sha256: crate::evidence::hash::text(command),
        origin,
    }))
}

pub(crate) fn policy_from_value(value: &Value, line: usize) -> Result<CheckPolicy, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: checkPolicy must be an object"))?;
    let allowed = ["version", "command", "commandSha256", "origin"];
    reject_unknown(object, &allowed, line, "checkPolicy")?;
    let record: CheckPolicyRecord = serde_json::from_value(value.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed checkPolicy"))?;
    if object.len() != allowed.len()
        || record.version != CHECK_POLICY_VERSION
        || record.command.trim().is_empty()
        || record.command.contains('\0')
        || !is_sha(Some(&record.command_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed checkPolicy"
        ));
    }
    if crate::evidence::hash::text(&record.command) != record.command_sha256 {
        return Err(format!(
            "invalid run ledger line {line}: checkPolicy command hash mismatch"
        ));
    }
    Ok(CheckPolicy {
        command: record.command,
        command_sha256: record.command_sha256,
        origin: record.origin,
    })
}

pub(crate) fn preservation_from_cli(
    requirement: Option<&str>,
    command: Option<&str>,
    origin: Option<&str>,
) -> Result<Option<PreservationPolicy>, String> {
    let Some(requirement) = requirement else {
        if command.is_some() {
            return Err("--preservation-check-command requires --preserve-requirement".into());
        }
        if origin.is_some() {
            return Err("--preservation-proof-origin requires --preserve-requirement".into());
        }
        return Ok(None);
    };
    let Some(command) = command else {
        return Err("--preserve-requirement requires --preservation-check-command".into());
    };
    let (id, text) = requirement
        .split_once(':')
        .ok_or("--preserve-requirement must be ID:TEXT")?;
    validate_requirement_id(id).map_err(|error| format!("--preserve-requirement {error}"))?;
    validate_requirement_text(text).map_err(|error| format!("--preserve-requirement {error}"))?;
    if command.trim().is_empty() {
        return Err("--preservation-check-command requires a non-empty value".into());
    }
    if command.contains('\0') {
        return Err("--preservation-check-command must not contain NUL bytes".into());
    }
    let origin = origin
        .map(ProofOrigin::parse)
        .transpose()?
        .unwrap_or(ProofOrigin::LocalReport);
    Ok(Some(PreservationPolicy {
        requirements: vec![PreservationRequirement {
            id: id.to_owned(),
            text: text.to_owned(),
            command: command.to_owned(),
            command_sha256: crate::evidence::hash::text(command),
            origin,
        }],
    }))
}

pub(crate) fn preservation_from_value(
    value: &Value,
    line: usize,
) -> Result<PreservationPolicy, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: preservation must be an object"))?;
    reject_unknown(object, &["version", "requirements"], line, "preservation")?;
    let record: PreservationPolicyRecord = serde_json::from_value(value.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed preservation"))?;
    if record.version != CHECK_POLICY_VERSION || record.requirements.is_empty() {
        return Err(format!(
            "invalid run ledger line {line}: malformed preservation"
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut requirements = Vec::with_capacity(record.requirements.len());
    for item in record.requirements {
        validate_requirement_id(&item.id)
            .map_err(|_| format!("invalid run ledger line {line}: malformed preservation"))?;
        validate_requirement_text(&item.text)
            .map_err(|_| format!("invalid run ledger line {line}: malformed preservation"))?;
        if item.command.trim().is_empty()
            || item.command.contains('\0')
            || !is_sha(Some(&item.command_sha256))
            || crate::evidence::hash::text(&item.command) != item.command_sha256
            || !seen.insert(item.id.clone())
        {
            return Err(format!(
                "invalid run ledger line {line}: malformed preservation"
            ));
        }
        requirements.push(PreservationRequirement {
            id: item.id,
            text: item.text,
            command: item.command,
            command_sha256: item.command_sha256,
            origin: item.origin,
        });
    }
    Ok(PreservationPolicy { requirements })
}

fn validate_requirement_id(id: &str) -> Result<(), &'static str> {
    if id.is_empty()
        || id.len() > 64
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err("id must be 1-64 lowercase letters, digits, '-' or '_'");
    }
    Ok(())
}

fn validate_requirement_text(text: &str) -> Result<(), &'static str> {
    if text.trim().is_empty() || text.len() > 512 || text.contains('\0') {
        return Err("text must be non-empty, <=512 bytes, and contain no NUL bytes");
    }
    Ok(())
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    line: usize,
    kind: &str,
) -> Result<(), String> {
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown {kind} field '{key}'"
            ))
        })
}

fn is_sha(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == SHA_LEN
            && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            && value.bytes().all(|byte| !byte.is_ascii_uppercase())
    })
}

fn valid_timestamp(value: &str) -> bool {
    value.contains('T') && chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

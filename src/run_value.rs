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

#[cfg(test)]
mod progress_tests {
    use crate::{run_exit, run_progress};
    use serde_json::json;

    #[test]
    fn progress_distinguishes_current_refusal_and_block() {
        let base = json!({
            "version": 5,
            "status":"running",
            "attempt":2,
            "subject":{"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
            "checkPolicy": {
                "version": 1,
                "command": "true",
                "commandSha256": crate::hash::text("true"),
                "origin": "local_report"
            },
            "plan":{"stages":[{"agents":[{"role":"worker"}]}]},
            "submissions":[],
            "checks":[],
            "protections":[]
        });
        assert_eq!(run_progress::project(&base)["state"], "IN_PROGRESS");
        let mut failed = base.clone();
        failed["submissions"] = json!([{
            "attempt":2,
            "role":"worker",
            "outcome":"completed",
            "eventSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        }]);
        failed["checks"] = json!([{
            "targetEventSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "subjectSha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "exitCode":1
        }]);
        assert_eq!(run_progress::project(&failed)["state"], "REFUSED");
        assert_eq!(
            run_progress::project(&failed)["reason"]["code"],
            "check_failed"
        );
        let mut missing = base;
        missing["submissions"] = failed["submissions"].clone();
        assert_eq!(run_progress::project(&missing)["state"], "BLOCKED");
        let mut reworked = missing;
        reworked["attempt"] = json!(3);
        assert_eq!(run_progress::project(&reworked)["state"], "IN_PROGRESS");
    }

    #[test]
    fn historical_and_unchecked_acceptance_are_not_exit_ready_progress() {
        let historical = json!({
            "version": 4,
            "status": "accepted",
            "attempt": 1,
            "submissions": [{"role": "lead", "outcome": "accepted"}]
        });
        let unchecked = {
            let mut value = historical.clone();
            value["version"] = json!(5);
            value
        };
        for (state, code) in [
            (&historical, "historical_run"),
            (&unchecked, "unchecked_run"),
        ] {
            let progress = run_progress::project(state);
            assert_eq!(progress["applicable"], false);
            assert!(progress["percent"].is_null());
            assert_eq!(progress["state"], "NOT_APPLICABLE");
            assert_eq!(progress["reason"]["code"], code);
            assert_eq!(progress["runStatus"], "accepted");
        }
    }

    #[test]
    fn stale_subject_checks_are_missing_and_old_refusal_recovers() {
        let old_subject = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let current_subject = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let worker_a = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let worker_b = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let mut state = json!({
            "version": 5,
            "status": "running",
            "attempt": 1,
            "subject": {"sha256": current_subject},
            "checkPolicy": {
                "version": 1,
                "command": "true",
                "commandSha256": crate::hash::text("true"),
                "origin": "local_report"
            },
            "plan": {"stages": [{"agents": [
                {"role": "worker"}, {"role": "worker"}
            ]}]},
            "submissions": [
                {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_a},
                {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_b}
            ],
            "checks": [
                {"targetEventSha256": worker_a, "subjectSha256": old_subject, "exitCode": 0},
                {"targetEventSha256": worker_b, "subjectSha256": current_subject, "exitCode": 0}
            ],
            "protections": [{"attempt": 1, "reason": "check_failed"}]
        });
        let assessment = run_exit::assess(&state).unwrap();
        assert_eq!(assessment.reason(), Some("check_missing"));
        assert_eq!(run_progress::project(&state)["state"], "BLOCKED");
        assert_eq!(run_progress::project(&state)["percent"], 32);

        state["checks"] = json!([
            {"targetEventSha256": worker_a, "subjectSha256": current_subject, "exitCode": 0},
            {"targetEventSha256": worker_b, "subjectSha256": current_subject, "exitCode": 0}
        ]);
        let assessment = run_exit::assess(&state).unwrap();
        assert_eq!(assessment.reason(), None);
        assert_eq!(run_progress::project(&state)["state"], "IN_PROGRESS");
        assert_eq!(run_progress::project(&state)["percent"], 40);
    }

    #[test]
    fn progress_allocates_named_weights_across_planned_workers_reviewers_and_lead() {
        let worker_a = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let worker_b = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let state = json!({
            "version": 5,
            "status": "running",
            "attempt": 1,
            "subject": {"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
            "checkPolicy": {
                "version": 1,
                "command": "true",
                "commandSha256": crate::hash::text("true"),
                "origin": "local_report"
            },
            "plan": {"stages": [{"agents": [
                {"role": "lead"}, {"role": "worker"}, {"role": "worker"},
                {"role": "reviewer"}, {"role": "reviewer"}
            ]}]},
            "submissions": [
                {"attempt": 1, "role": "lead", "outcome": "scoped"},
                {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_a},
                {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_b},
                {"attempt": 1, "role": "reviewer", "outcome": "approved"}
            ],
            "checks": [
                {"targetEventSha256": worker_a, "subjectSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "exitCode": 0}
            ],
            "protections": []
        });
        let progress = run_progress::project(&state);
        assert_eq!(progress["applicable"], true);
        assert_eq!(progress["percent"], 57);
        assert_eq!(progress["weights"]["worker"], 25);
        assert_eq!(progress["weights"]["review"], 20);
        assert_eq!(progress["components"]["worker"]["total"], 2);
        assert_eq!(progress["components"]["check"]["completed"], 1);
        assert_eq!(progress["components"]["review"]["total"], 2);
        assert_eq!(progress["components"]["lead"]["earned"], 0);
    }
}

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
    result: CheckResult,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
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
        command_sha256: crate::hash::text(command),
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
    if crate::hash::text(&record.command) != record.command_sha256 {
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
            command_sha256: crate::hash::text(command),
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
            || crate::hash::text(&item.command) != item.command_sha256
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

pub(crate) fn validate_check_event(event: &Value, line: usize) -> Result<(), String> {
    if matches!(event["version"].as_u64(), Some(4..=7)) {
        return validate_check_event_v4(event, line);
    }
    let record: CheckObservation = serde_json::from_value(event.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed check event"))?;
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: check event must be an object"))?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "targetEventSha256",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "exitCode",
        "durationMs",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "check")?;
    let required = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "targetEventSha256",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "exitCode",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    if !required.iter().all(|key| object.contains_key(*key))
        || record.version != 3
        || record.kind != "run"
        || record.action != CheckAction::Check
        || !crate::producer::valid(&record.producer)
        || !is_sha(Some(&record.run_id))
        || !is_sha(Some(&record.target_event_sha256))
        || record.check_command.trim().is_empty()
        || record.check_command.contains('\0')
        || !is_sha(Some(&record.check_command_sha256))
        || crate::hash::text(&record.check_command) != record.check_command_sha256
        || !valid_timestamp(&record.timestamp)
        || !is_sha(Some(&record.event_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed check event"
        ));
    }
    if record.origin != ProofOrigin::LocalReport && record.origin != ProofOrigin::Synthetic {
        return Err(format!(
            "invalid run ledger line {line}: proof origin is invalid"
        ));
    }
    if record.previous_event_sha256.is_none() && event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: previous event hash is malformed"
        ));
    }
    if let Some(duration) = event.get("durationMs") {
        if !duration.is_null() && record.duration_ms.is_none() {
            return Err(format!(
                "invalid run ledger line {line}: durationMs must be a non-negative integer or null"
            ));
        }
    }
    Ok(())
}

fn validate_check_event_v4(event: &Value, line: usize) -> Result<(), String> {
    let mut parsed = event.clone();
    if parsed["version"].as_u64() >= Some(5) {
        parsed
            .as_object_mut()
            .map(|object| object.remove("subjectSha256"));
    }
    parsed
        .as_object_mut()
        .map(|object| object.remove("inputsSha256"));
    let record: CheckObservationV4 = serde_json::from_value(parsed)
        .map_err(|_| format!("invalid run ledger line {line}: malformed check event"))?;
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: check event must be an object"))?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "subjectSha256",
        "inputsSha256",
        "targetEventSha256",
        "requirementId",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "acquisition",
        "result",
        "durationMs",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "check")?;
    if event["version"].as_u64() >= Some(6) && !is_sha(event["inputsSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: check event requires tested input identity"
        ));
    }
    if event["version"].as_u64() < Some(6)
        && (object.contains_key("inputsSha256") || object.contains_key("requirementId"))
    {
        return Err(format!(
            "invalid run ledger line {line}: input and preservation bindings require v6"
        ));
    }
    if event["version"].as_u64() >= Some(5) && !is_sha(event["subjectSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: malformed subject binding"
        ));
    }
    if let Some(id) = record.requirement_id.as_deref() {
        validate_requirement_id(id).map_err(|_| {
            format!("invalid run ledger line {line}: malformed preservation requirement binding")
        })?;
    }
    let required = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "targetEventSha256",
        "checkCommand",
        "checkCommandSha256",
        "origin",
        "acquisition",
        "result",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    let result_valid = match &record.result {
        CheckResult::Exit { code } => event["result"].as_object().is_some_and(|result| {
            result.len() == 2
                && result.get("kind") == Some(&json!("exit"))
                && result["code"].as_u64() == Some(*code)
        }),
        CheckResult::Signal { signal } => event["result"].as_object().is_some_and(|result| {
            result.len() == 2 && result.get("kind") == Some(&json!("signal")) && *signal > 0
        }),
    };
    if !required.iter().all(|key| object.contains_key(*key))
        || (event["version"].as_u64() >= Some(5) && !object.contains_key("subjectSha256"))
        || !matches!(record.version, 4..=7)
        || record.kind != "run"
        || record.action != CheckAction::Check
        || !crate::producer::valid(&record.producer)
        || !is_sha(Some(&record.run_id))
        || !is_sha(Some(&record.target_event_sha256))
        || record.check_command.trim().is_empty()
        || record.check_command.contains('\0')
        || !is_sha(Some(&record.check_command_sha256))
        || crate::hash::text(&record.check_command) != record.check_command_sha256
        || record.origin.as_str() != event["origin"]
        || !matches!(record.acquisition.as_str(), "reported" | "observed")
        || !result_valid
        || !valid_timestamp(&record.timestamp)
        || !is_sha(Some(&record.event_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed check event"
        ));
    }
    if record.previous_event_sha256.is_none() && event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: previous event hash is malformed"
        ));
    }
    if let Some(duration) = event.get("durationMs") {
        if !duration.is_null() && record.duration_ms.is_none() {
            return Err(format!(
                "invalid run ledger line {line}: durationMs must be a non-negative integer or null"
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_protection_event(event: &Value, line: usize) -> Result<(), String> {
    if matches!(event["version"].as_u64(), Some(4..=7)) {
        return validate_protection_event_v4(event, line);
    }
    let record: ProtectionRecord = serde_json::from_value(event.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed protection event"))?;
    let object = event.as_object().ok_or_else(|| {
        format!("invalid run ledger line {line}: protection event must be an object")
    })?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "stage",
        "attempt",
        "actor",
        "role",
        "attemptedOutcome",
        "reason",
        "checkEvidence",
        "origin",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "protection")?;
    if object.len() != allowed.len()
        || record.version != 3
        || record.kind != "run"
        || record.action != ProtectionAction::Protect
        || !crate::producer::valid(&record.producer)
        || !is_sha(Some(&record.run_id))
        || record.stage < 1
        || record.attempt < 1
        || record.actor.trim().is_empty()
        || record.role != "lead"
        || record.attempted_outcome != "accepted"
        || record.check_evidence.is_empty()
        || !valid_timestamp(&record.timestamp)
        || !is_sha(Some(&record.event_sha256))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection event"
        ));
    }
    if record.previous_event_sha256.is_none() && event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: previous event hash is malformed"
        ));
    }
    let evidence = event["checkEvidence"]
        .as_array()
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    for item in evidence {
        validate_check_evidence(item, line)?;
    }
    let has_missing = record
        .check_evidence
        .iter()
        .any(|item| item.status == EvidenceStatus::Missing);
    let has_failed = record
        .check_evidence
        .iter()
        .any(|item| item.status == EvidenceStatus::Failed);
    let reason = record.reason.as_str();
    if (!has_missing && matches!(reason, "check_missing" | "preservation_missing"))
        || (!has_failed && matches!(reason, "check_failed" | "preservation_failed"))
    {
        return Err(format!(
            "invalid run ledger line {line}: protection reason does not match evidence"
        ));
    }
    Ok(())
}

fn validate_protection_event_v4(event: &Value, line: usize) -> Result<(), String> {
    let object = event.as_object().ok_or_else(|| {
        format!("invalid run ledger line {line}: protection event must be an object")
    })?;
    let allowed = [
        "version",
        "kind",
        "producer",
        "action",
        "runId",
        "subjectSha256",
        "inputsSha256",
        "stage",
        "attempt",
        "actor",
        "role",
        "attemptedOutcome",
        "reason",
        "checkEvidence",
        "origin",
        "previousEventSha256",
        "timestamp",
        "eventSha256",
    ];
    reject_unknown(object, &allowed, line, "protection")
        .map_err(|_| format!("invalid run ledger line {line}: malformed protection event"))?;
    if (event["version"].as_u64() >= Some(6)) != is_sha(event["inputsSha256"].as_str())
        || (event["version"].as_u64() < Some(6) && object.contains_key("inputsSha256"))
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed tested input binding"
        ));
    }
    if event["version"].as_u64() >= Some(5) && !is_sha(event["subjectSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: malformed subject binding"
        ));
    }
    let evidence = object
        .get("checkEvidence")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    if !matches!(event["version"].as_u64(), Some(4..=7))
        || event["kind"] != "run"
        || event["action"] != "protect"
        || !crate::producer::valid(&event["producer"])
        || !is_sha(event["runId"].as_str())
        || event["stage"].as_u64().map_or(true, |x| x < 1)
        || event["attempt"].as_u64().map_or(true, |x| x < 1)
        || event["actor"].as_str().map_or(true, str::is_empty)
        || event["role"] != "lead"
        || event["attemptedOutcome"] != "accepted"
        || evidence.is_empty()
        || !matches!(
            event["reason"].as_str(),
            Some("check_missing" | "check_failed" | "preservation_missing" | "preservation_failed")
        )
        || !matches!(event["origin"].as_str(), Some("local_report" | "synthetic"))
        || !event["timestamp"].as_str().is_some_and(valid_timestamp)
        || !is_sha(event["eventSha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection event"
        ));
    }
    for item in evidence {
        validate_check_evidence_v4(item, line)?;
    }
    let has_missing = evidence.iter().any(|item| item["status"] == "missing");
    let has_failed = evidence.iter().any(|item| item["status"] == "failed");
    if (!has_missing
        && matches!(
            event["reason"].as_str(),
            Some("check_missing" | "preservation_missing")
        ))
        || (!has_failed
            && matches!(
                event["reason"].as_str(),
                Some("check_failed" | "preservation_failed")
            ))
    {
        return Err(format!(
            "invalid run ledger line {line}: protection reason does not match evidence"
        ));
    }
    Ok(())
}

fn validate_check_evidence_v4(value: &Value, line: usize) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    if !is_sha(value["targetEventSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection evidence"
        ));
    }
    match value["status"].as_str() {
        Some("missing")
            if matches!(object.len(), 2 | 3) && object.contains_key("targetEventSha256") =>
        {
            if object.len() == 3 {
                value["requirementId"]
                    .as_str()
                    .filter(|id| validate_requirement_id(id).is_ok())
                    .map(|_| ())
                    .ok_or_else(|| {
                        format!("invalid run ledger line {line}: malformed protection evidence")
                    })
            } else {
                Ok(())
            }
        }
        Some("failed")
            if matches!(object.len(), 4 | 5)
                && object.contains_key("checkEventSha256")
                && is_sha(value["checkEventSha256"].as_str())
                && object.contains_key("result") =>
        {
            if object.len() == 5
                && value["requirementId"]
                    .as_str()
                    .filter(|id| validate_requirement_id(id).is_ok())
                    .is_none()
            {
                return Err(format!(
                    "invalid run ledger line {line}: malformed protection evidence"
                ));
            }
            let result = &value["result"];
            let valid = result.as_object().is_some_and(|result| {
                result.len() == 2
                    && matches!(result["kind"].as_str(), Some("exit" | "signal"))
                    && (result["kind"] == "exit" && result["code"].is_u64()
                        || result["kind"] == "signal"
                            && result["signal"].as_u64().is_some_and(|signal| signal > 0))
            });
            if valid {
                Ok(())
            } else {
                Err(format!(
                    "invalid run ledger line {line}: malformed protection evidence"
                ))
            }
        }
        _ => Err(format!(
            "invalid run ledger line {line}: malformed protection evidence"
        )),
    }
}

fn validate_check_evidence(value: &Value, line: usize) -> Result<(), String> {
    let record: CheckEvidence = serde_json::from_value(value.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: malformed protection evidence"))?;
    if !is_sha(Some(&record.target_event_sha256)) {
        return Err(format!(
            "invalid run ledger line {line}: malformed protection evidence"
        ));
    }
    if record.status == EvidenceStatus::Missing {
        if !(object.len() == 2
            || (object.len() == 3
                && record
                    .requirement_id
                    .as_deref()
                    .is_some_and(|id| validate_requirement_id(id).is_ok())))
        {
            return Err(format!(
                "invalid run ledger line {line}: missing check evidence has extra fields"
            ));
        }
    } else {
        let has_valid_requirement = record
            .requirement_id
            .as_deref()
            .is_some_and(|id| validate_requirement_id(id).is_ok());
        if !((object.len() == 4 && record.requirement_id.is_none())
            || (object.len() == 5 && has_valid_requirement))
            || !is_sha(record.check_event_sha256.as_deref())
            || record.exit_code.is_none()
        {
            return Err(format!(
                "invalid run ledger line {line}: failed check evidence is malformed"
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_check_target(state: &Value, target: &str) -> Result<Value, String> {
    if !is_sha(Some(target)) {
        return Err("--target must be a lowercase SHA-256 event hash".into());
    }
    let attempt = state["attempt"].as_u64().ok_or("run attempt is invalid")?;
    state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?
        .iter()
        .find(|submission| {
            submission["eventSha256"].as_str() == Some(target)
                && submission["attempt"] == attempt
                && submission["role"] == "worker"
                && submission["outcome"] == "completed"
        })
        .cloned()
        .ok_or_else(|| "check target is not a current worker completion".into())
}

pub(crate) fn validate_check_against_state(
    state: &Value,
    event: &Value,
    line: usize,
) -> Result<(), String> {
    let target = event["targetEventSha256"]
        .as_str()
        .ok_or_else(|| format!("invalid run ledger line {line}: check target is malformed"))?;
    validate_check_target(state, target)
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    let version = state["version"].as_u64().unwrap_or(0);
    let requirement_id = event["requirementId"].as_str();
    let (command, command_sha256, origin) = if let Some(id) = requirement_id {
        if version < 6 {
            return Err(format!(
                "invalid run ledger line {line}: preservation checks require v6"
            ));
        }
        let preservation = state.get("preservation").ok_or_else(|| {
            format!("invalid run ledger line {line}: preservation is not configured")
        })?;
        let preservation = preservation_from_value(preservation, line)?;
        let requirement = preservation.requirement(id).ok_or_else(|| {
            format!("invalid run ledger line {line}: preservation requirement is not configured")
        })?;
        (
            requirement.command.clone(),
            requirement.command_sha256.clone(),
            requirement.origin,
        )
    } else {
        let policy_value = state.get("checkPolicy").ok_or_else(|| {
            format!("invalid run ledger line {line}: check policy is not configured")
        })?;
        let policy = policy_from_value(policy_value, line)?;
        (policy.command, policy.command_sha256, policy.origin)
    };
    let policy_matches = event["checkCommand"] == command
        && event["checkCommandSha256"] == command_sha256
        && event["origin"] == origin.as_str();
    let shape_matches = if matches!(version, 4..=7) {
        event["version"] == version
            && matches!(event["acquisition"].as_str(), Some("reported" | "observed"))
    } else {
        event["version"] == 3 && event.get("acquisition").is_none()
    };
    if !policy_matches || !shape_matches {
        return Err(format!(
            "invalid run ledger line {line}: check report does not match configured policy"
        ));
    }
    Ok(())
}

pub(crate) fn validate_protection_against_state(
    state: &Value,
    event: &Value,
    line: usize,
) -> Result<(), String> {
    if state["status"] != "running" {
        return Err(format!(
            "invalid run ledger line {line}: protection event requires a running run"
        ));
    }
    let assignments = crate::run_assignment::pending(state);
    let assignment = assignments
        .iter()
        .find(|assignment| assignment["agent"] == event["actor"])
        .ok_or_else(|| {
            format!("invalid run ledger line {line}: protection actor is not currently pending")
        })?;
    if assignment["stage"] != event["stage"]
        || assignment["attempt"] != event["attempt"]
        || assignment["role"] != event["role"]
    {
        return Err(format!(
            "invalid run ledger line {line}: protection target is out of order"
        ));
    }
    let assessment = crate::run_exit::assess(state)
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    if !assessment.has_refusal_evidence() {
        return Err(format!(
            "invalid run ledger line {line}: protection has no current complete check refusal"
        ));
    }
    let expected = assessment
        .targets
        .iter()
        .filter(|target| target.is_missing() || target.is_failed())
        .map(|target| {
            if matches!(state["version"].as_u64(), Some(4..=7)) {
                target.protection_value()
            } else {
                target.value()
            }
        })
        .collect::<Vec<_>>();
    if event["reason"] != assessment.reason().unwrap_or_default()
        || event["origin"]
            != assessment.policy.as_ref().map_or(Value::Null, |policy| {
                Value::String(policy.origin.as_str().to_owned())
            })
        || event["checkEvidence"] != Value::Array(expected)
    {
        return Err(format!(
            "invalid run ledger line {line}: protection evidence does not match current checks"
        ));
    }
    Ok(())
}

/// Build the factual refusal event.  It contains only the failing/missing
/// check references; it never claims avoided loss or invents human time.
pub(crate) fn protection_event(
    state: &Value,
    actor: &str,
    stage: &Value,
    attempt: &Value,
    previous_event: &Value,
    timestamp: &str,
) -> Result<Value, String> {
    let assessment = crate::run_exit::assess(state)?;
    let reason = assessment
        .reason()
        .ok_or("acceptance is not blocked by a configured check")?;
    let origin = assessment
        .policy
        .as_ref()
        .ok_or("acceptance is not blocked by a configured check")?
        .origin
        .as_str();
    let version = state["version"].as_u64().unwrap_or(3);
    let evidence = assessment
        .targets
        .iter()
        .filter(|target| target.is_missing() || target.is_failed())
        .map(|target| {
            if matches!(version, 4..=7) {
                target.protection_value()
            } else {
                target.value()
            }
        })
        .collect::<Vec<_>>();
    let mut value = json!({
        "version": version,
        "kind": "run",
        "producer": crate::producer::evidence_for_version(version),
        "action": "protect",
        "runId": state["runId"],
        "stage": stage,
        "attempt": attempt,
        "actor": actor,
        "role": "lead",
        "attemptedOutcome": "accepted",
        "reason": reason,
        "checkEvidence": evidence,
        "origin": origin,
        "previousEventSha256": previous_event["eventSha256"],
        "timestamp": timestamp,
    });
    if version >= 5 {
        value["subjectSha256"] = state["subject"]["sha256"].clone();
    }
    if version >= 6 {
        value["inputsSha256"] = state
            .get("inputsSha256")
            .filter(|inputs| inputs.is_string())
            .cloned()
            .ok_or("tested inputs cannot be established; no protection was recorded")?;
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanIdentity {
    pub(crate) stage: u64,
    pub(crate) name: String,
    pub(crate) display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanRecord {
    pub(crate) identity: HumanIdentity,
    pub(crate) role: String,
    pub(crate) attempt: u64,
    pub(crate) outcome: String,
    pub(crate) event_sha256: String,
    pub(crate) artifact_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanWorker {
    pub(crate) identity: HumanIdentity,
    pub(crate) attempt: u64,
    pub(crate) current: Option<HumanRecord>,
    pub(crate) history: Vec<HumanRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanReviewer {
    pub(crate) identity: HumanIdentity,
    pub(crate) current: Option<HumanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HumanLeadState {
    Pending,
    Accepted,
    Rejected,
    Blocked,
    TerminalWithoutLeadDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanLeadDecision {
    pub(crate) state: HumanLeadState,
    pub(crate) current: Vec<HumanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HumanCheckState {
    Unconfigured,
    NotObserved,
    Blocked,
    Passed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanCheckTarget {
    pub(crate) worker: Option<HumanIdentity>,
    pub(crate) target_event_sha256: Option<String>,
    pub(crate) status: String,
    pub(crate) check_event_sha256: Option<String>,
    pub(crate) exit_code: Option<u64>,
    pub(crate) acquisition: Option<String>,
    pub(crate) result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanChecks {
    pub(crate) state: HumanCheckState,
    pub(crate) observed_capable: bool,
    pub(crate) command: Option<String>,
    pub(crate) command_sha256: Option<String>,
    pub(crate) origin: Option<String>,
    pub(crate) targets: Vec<HumanCheckTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanProtection {
    pub(crate) attempt: u64,
    pub(crate) actor: String,
    pub(crate) attempted_outcome: String,
    pub(crate) reason: String,
    pub(crate) origin: String,
    pub(crate) event_sha256: String,
    pub(crate) current: bool,
    pub(crate) evidence: Vec<HumanCheckTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanStatus {
    pub(crate) run_id: String,
    pub(crate) status: String,
    pub(crate) stage: u64,
    pub(crate) attempt: u64,
    pub(crate) artifact_status: String,
    pub(crate) workers: Vec<HumanWorker>,
    pub(crate) checks: HumanChecks,
    pub(crate) reviewers: Vec<HumanReviewer>,
    pub(crate) lead: HumanLeadDecision,
    pub(crate) history: Vec<HumanRecord>,
    pub(crate) protections: Vec<HumanProtection>,
    pub(crate) guidance: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanExplanation {
    pub(crate) status: HumanStatus,
    pub(crate) protection: Option<HumanProtection>,
    pub(crate) guidance: &'static str,
}

fn required_str(value: &Value, key: &str, context: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("validated {context} is missing {key}"))
}

fn required_u64(value: &Value, key: &str, context: &str) -> Result<u64, String> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("validated {context} is missing {key}"))
}

fn planned_identities(state: &Value, role: &str) -> Result<Vec<HumanIdentity>, String> {
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("validated run state plan stages are invalid")?;
    stages
        .iter()
        .flat_map(|stage| {
            let stage_number = stage["stage"].as_u64();
            stage["agents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(move |agent| agent["role"] == role)
                .map(move |agent| (stage_number, agent))
        })
        .map(|(stage, agent)| {
            Ok(HumanIdentity {
                stage: stage.ok_or("validated run stage is missing stage")?,
                name: required_str(agent, "name", "planned agent")?,
                display_name: required_str(agent, "displayName", "planned agent")?,
            })
        })
        .collect()
}

fn identity_for_submission(state: &Value, submission: &Value) -> Result<HumanIdentity, String> {
    let stage = required_u64(submission, "stage", "submission")?;
    let agent = required_str(submission, "agent", "submission")?;
    let role = required_str(submission, "role", "submission")?;
    planned_identities(state, &role)?
        .into_iter()
        .find(|identity| identity.stage == stage && identity.name == agent)
        .ok_or_else(|| format!("validated submission has no planned identity for {role} '{agent}'"))
}

fn record_from_submission(state: &Value, submission: &Value) -> Result<HumanRecord, String> {
    Ok(HumanRecord {
        identity: identity_for_submission(state, submission)?,
        role: required_str(submission, "role", "submission")?,
        attempt: required_u64(submission, "attempt", "submission")?,
        outcome: required_str(submission, "outcome", "submission")?,
        event_sha256: required_str(submission, "eventSha256", "submission")?,
        artifact_sha256: required_str(&submission["artifact"], "sha256", "submission artifact")?,
    })
}

fn current_submission<'a>(
    submissions: &'a [Value],
    identity: &HumanIdentity,
    role: &str,
    attempt: u64,
) -> Option<&'a Value> {
    submissions.iter().rev().find(|submission| {
        submission["stage"] == identity.stage
            && submission["attempt"] == attempt
            && submission["agent"] == identity.name
            && submission["role"] == role
    })
}

fn history_for_identity(
    state: &Value,
    submissions: &[Value],
    identity: &HumanIdentity,
    role: &str,
    attempt: u64,
) -> Result<Vec<HumanRecord>, String> {
    submissions
        .iter()
        .filter(|submission| {
            submission["stage"] == identity.stage
                && submission["agent"] == identity.name
                && submission["role"] == role
                && submission["attempt"]
                    .as_u64()
                    .is_some_and(|value| value < attempt)
        })
        .map(|submission| record_from_submission(state, submission))
        .collect()
}

fn target_for_worker<'a>(
    worker: &HumanWorker,
    assessment: &'a CheckAssessment,
) -> Option<&'a CheckTarget> {
    let event = worker.current.as_ref()?.event_sha256.as_str();
    assessment
        .targets
        .iter()
        .find(|target| target.target_event_sha256 == event)
}

fn human_check_target(
    worker: Option<HumanIdentity>,
    target_event_sha256: Option<String>,
    status: &str,
    check_event_sha256: Option<String>,
    exit_code: Option<u64>,
    acquisition: Option<String>,
    result: Option<Value>,
) -> HumanCheckTarget {
    HumanCheckTarget {
        worker,
        target_event_sha256,
        status: status.to_owned(),
        check_event_sha256,
        exit_code,
        acquisition,
        result: result.map(|value| value.to_string()),
    }
}

fn human_checks(
    workers: &[HumanWorker],
    assessment: &CheckAssessment,
    version: u64,
) -> HumanChecks {
    let Some(policy) = assessment.policy.as_ref() else {
        return HumanChecks {
            state: HumanCheckState::Unconfigured,
            observed_capable: false,
            command: None,
            command_sha256: None,
            origin: None,
            targets: Vec::new(),
        };
    };

    let mut targets = Vec::new();
    for worker in workers {
        let current_event = worker
            .current
            .as_ref()
            .map(|record| record.event_sha256.clone());
        if let Some(target) = target_for_worker(worker, assessment) {
            targets.push(human_check_target(
                Some(worker.identity.clone()),
                Some(target.target_event_sha256.clone()),
                target.status.as_str(),
                target.check_event_sha256.clone(),
                target.exit_code,
                target.acquisition.clone(),
                target.result.clone(),
            ));
        } else {
            targets.push(human_check_target(
                Some(worker.identity.clone()),
                current_event,
                "missing",
                None,
                None,
                None,
                None,
            ));
        }
    }
    for target in &assessment.targets {
        if !targets.iter().any(|item| {
            item.target_event_sha256.as_deref() == Some(target.target_event_sha256.as_str())
        }) {
            targets.push(human_check_target(
                None,
                Some(target.target_event_sha256.clone()),
                target.status.as_str(),
                target.check_event_sha256.clone(),
                target.exit_code,
                target.acquisition.clone(),
                target.result.clone(),
            ));
        }
    }

    let state = if targets.iter().any(|target| target.status == "failed") {
        HumanCheckState::Blocked
    } else if assessment.incomplete
        || targets.is_empty()
        || targets.iter().any(|target| target.status == "missing")
    {
        HumanCheckState::NotObserved
    } else {
        HumanCheckState::Passed
    };
    if !matches!(version, 4..=7) {
        for target in &mut targets {
            target.acquisition = Some("reported".to_owned());
        }
    }
    HumanChecks {
        state,
        observed_capable: matches!(version, 4..=7),
        command: Some(policy.command.clone()),
        command_sha256: Some(policy.command_sha256.clone()),
        origin: Some(policy.origin.as_str().to_owned()),
        targets,
    }
}

fn check_guidance(state: &Value) -> &'static str {
    if matches!(state["version"].as_u64(), Some(4..=7)) {
        "observe the frozen check locally with run observe-check, or report the actual result from the host with run record-check, for every current worker target"
    } else {
        "run the configured check in its host and report the actual result for every current worker target with run record-check"
    }
}

fn protection_from_value(
    event: &Value,
    state: &Value,
    workers: &[HumanWorker],
    current_attempt: u64,
) -> Result<HumanProtection, String> {
    let evidence = event["checkEvidence"]
        .as_array()
        .ok_or("validated protection evidence is invalid")?
        .iter()
        .map(|item| {
            let target = required_str(item, "targetEventSha256", "protection evidence")?;
            let worker = workers
                .iter()
                .find(|candidate| {
                    candidate
                        .current
                        .as_ref()
                        .is_some_and(|record| record.event_sha256 == target)
                        || candidate
                            .history
                            .iter()
                            .any(|record| record.event_sha256 == target)
                })
                .map(|candidate| candidate.identity.clone());
            let check_event_sha256 = item["checkEventSha256"].as_str().map(str::to_owned);
            let acquisition = check_event_sha256
                .as_deref()
                .and_then(|wanted| {
                    state["checks"].as_array()?.iter().find_map(|check| {
                        (check["eventSha256"].as_str() == Some(wanted)).then(|| {
                            check["acquisition"]
                                .as_str()
                                .unwrap_or("reported")
                                .to_owned()
                        })
                    })
                })
                .or_else(|| (state["version"] == 3).then(|| "reported".to_owned()));
            Ok(human_check_target(
                worker,
                Some(target),
                &required_str(item, "status", "protection evidence")?,
                check_event_sha256,
                item["exitCode"].as_u64(),
                acquisition,
                item["result"].as_object().map(|_| item["result"].clone()),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(HumanProtection {
        attempt: required_u64(event, "attempt", "protection")?,
        actor: required_str(event, "actor", "protection")?,
        attempted_outcome: required_str(event, "attemptedOutcome", "protection")?,
        reason: required_str(event, "reason", "protection")?,
        origin: required_str(event, "origin", "protection")?,
        event_sha256: required_str(event, "eventSha256", "protection")?,
        current: event["attempt"].as_u64() == Some(current_attempt),
        evidence,
    })
}

pub(crate) fn human_status_from_kernel(
    state: &Value,
    artifact_current: bool,
    kernel: &crate::run_exit::ExitState,
) -> Result<HumanStatus, String> {
    let current_attempt = kernel.attempt;
    let submissions = state["submissions"]
        .as_array()
        .ok_or("validated run state submissions are invalid")?;
    let worker_identities = planned_identities(state, "worker")?;
    let workers = worker_identities
        .into_iter()
        .map(|identity| {
            let current = current_submission(submissions, &identity, "worker", current_attempt)
                .map(|submission| record_from_submission(state, submission))
                .transpose()?;
            Ok(HumanWorker {
                history: history_for_identity(
                    state,
                    submissions,
                    &identity,
                    "worker",
                    current_attempt,
                )?,
                identity,
                attempt: current_attempt,
                current,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let reviewer_identities = planned_identities(state, "reviewer")?;
    let reviewers = reviewer_identities
        .into_iter()
        .map(|identity| {
            let current = current_submission(submissions, &identity, "reviewer", current_attempt)
                .map(|submission| record_from_submission(state, submission))
                .transpose()?;
            Ok(HumanReviewer { identity, current })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let current_lead = submissions
        .iter()
        .filter(|submission| {
            submission["attempt"] == current_attempt && submission["role"] == "lead"
        })
        .map(|submission| record_from_submission(state, submission))
        .collect::<Result<Vec<_>, String>>()?;
    let lead_state = current_lead
        .iter()
        .rev()
        .find_map(|record| match record.outcome.as_str() {
            "accepted" => Some(HumanLeadState::Accepted),
            "rejected" => Some(HumanLeadState::Rejected),
            "blocked" => Some(HumanLeadState::Blocked),
            _ => None,
        })
        .unwrap_or_else(|| {
            if matches!(
                state["status"].as_str(),
                Some("accepted" | "rejected" | "blocked")
            ) {
                HumanLeadState::TerminalWithoutLeadDecision
            } else {
                HumanLeadState::Pending
            }
        });
    let history = submissions
        .iter()
        .filter(|submission| {
            submission["attempt"]
                .as_u64()
                .is_some_and(|value| value < current_attempt)
        })
        .map(|submission| record_from_submission(state, submission))
        .collect::<Result<Vec<_>, String>>()?;
    let assessment = kernel.assessment.clone();
    let checks = human_checks(
        &workers,
        &assessment,
        state["version"].as_u64().unwrap_or(3),
    );
    let empty_protections = Vec::new();
    let protections = state["protections"]
        .as_array()
        .unwrap_or(&empty_protections)
        .iter()
        .map(|event| protection_from_value(event, state, &workers, current_attempt))
        .collect::<Result<Vec<_>, String>>()?;
    let guidance = if state["status"] != "running" {
        Some("this run is terminal; inspect its recorded outcome before choosing an explicit successor where supported")
    } else if !artifact_current {
        Some(
            "artifact drift prevents progression; restore the exact recorded bytes before retrying",
        )
    } else {
        match checks.state {
            HumanCheckState::NotObserved | HumanCheckState::Blocked => Some(check_guidance(state)),
            HumanCheckState::Passed => Some(
                "checks passed after recording the actual result; reviewer approval and lead acceptance remain separate authority steps",
            ),
            HumanCheckState::Unconfigured => None,
        }
    };
    Ok(HumanStatus {
        run_id: required_str(state, "runId", "run state")?,
        status: required_str(state, "status", "run state")?,
        stage: required_u64(state, "currentStage", "run state")?,
        attempt: current_attempt,
        artifact_status: if artifact_current {
            "current"
        } else {
            "drifted"
        }
        .to_owned(),
        workers,
        checks,
        reviewers,
        lead: HumanLeadDecision {
            state: lead_state,
            current: current_lead,
        },
        history,
        protections,
        guidance,
    })
}

/// A status view intentionally reports only ledger-derived facts.  Disk
/// revalidation is performed by `run`; callers that have not revalidated bytes
/// must not label an artifact "current".
pub(crate) fn status(state: &Value, artifact_current: Option<bool>) -> Result<Value, String> {
    let assessment = crate::run_exit::reduce(state)?.assessment;
    let submissions = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?;
    let current_attempt = state["attempt"].as_u64().unwrap_or_default();
    let claim = submissions.iter().rev().find(|submission| {
        submission["attempt"] == current_attempt && submission["role"] == "worker"
    });
    let review = submissions.iter().rev().find(|submission| {
        submission["attempt"] == current_attempt && submission["role"] == "reviewer"
    });
    let acceptance = submissions.iter().rev().find(|submission| {
        submission["attempt"] == current_attempt && submission["outcome"] == "accepted"
    });
    let mut checks = json!({
        "configured": assessment.policy.is_some(),
        "origin": assessment
            .policy
            .as_ref()
            .map_or("none", |policy| policy.origin.as_str()),
        "status": if assessment.policy.is_none() {
            "not_configured"
        } else if assessment.incomplete
            || assessment
                .targets
                .iter()
            .any(CheckTarget::is_missing)
            || assessment.targets.is_empty()
        {
            "not_observed"
        } else if assessment.is_blocked() {
            "blocked"
        } else {
            "passed"
        },
        "targets": assessment.targets.iter().map(CheckTarget::value).collect::<Vec<_>>(),
    });
    if let Some(object) = checks.as_object_mut() {
        object.insert("targetCount".into(), json!(assessment.targets.len()));
        object.insert(
            "observedCount".into(),
            json!(assessment
                .targets
                .iter()
                .filter(|target| target.is_observed())
                .count()),
        );
        object.insert(
            "failedCount".into(),
            json!(assessment
                .targets
                .iter()
                .filter(|target| target.is_failed())
                .count()),
        );
        object.insert(
            "missingCount".into(),
            json!(assessment
                .targets
                .iter()
                .filter(|target| target.is_missing())
                .count()),
        );
    }
    Ok(json!({
        "version": VALUE_REPORT_VERSION,
        "runId": state["runId"],
        "workflow": state["workflow"],
        "status": state["status"],
        "stage": state["currentStage"],
        "attempt": state["attempt"],
        "claim": claim.map_or_else(|| json!({"status":"absent"}), |submission| json!({
            "status": submission["outcome"],
            "agent": submission["agent"],
            "eventSha256": submission["eventSha256"],
            "artifactSha256": submission["artifact"]["sha256"],
        })),
        "artifact": artifact_current.map_or_else(|| json!({"status":"not_revalidated"}), |current| json!({"status": if current {"current"} else {"drifted"}})),
        "checks": checks,
        "review": review.map_or_else(|| json!({"status":"absent"}), |submission| json!({
            "status": submission["outcome"],
            "eventSha256": submission["eventSha256"],
            "artifactSha256": submission["artifact"]["sha256"],
        })),
        "acceptance": acceptance.map_or_else(|| json!({"status":"absent"}), |submission| json!({
            "status": "accepted",
            "eventSha256": submission["eventSha256"],
            "artifactSha256": submission["artifact"]["sha256"],
        })),
        "evidence": {
            "eventCount": state["events"].as_array().map_or(0, Vec::len),
            "submissionCount": submissions.len(),
            "checkCount": state["checks"].as_array().map_or(0, Vec::len),
            "protectionCount": state["protections"].as_array().map_or(0, Vec::len),
        },
    }))
}

/// Return a concise explanation for a run.  The caller may optionally identify
/// a protection event; unknown IDs remain an explicit unknown instead of being
/// silently attributed to a nearby event.
pub(crate) fn explain_with_artifact(
    state: &Value,
    event_id: Option<&str>,
    artifact_current: bool,
) -> Result<Value, String> {
    let status = status(state, Some(artifact_current))?;
    let empty_protections = Vec::new();
    let protections = state["protections"]
        .as_array()
        .unwrap_or(&empty_protections);
    let selected = event_id.and_then(|wanted| {
        protections
            .iter()
            .find(|event| event["eventSha256"].as_str() == Some(wanted))
    });
    if event_id.is_some() && selected.is_none() {
        return Err("explanation target is not a protection event in this run".into());
    }
    Ok(json!({
        "version": VALUE_REPORT_VERSION,
        "runId": state["runId"],
        "status": status,
        "protection": selected.map_or(Value::Null, Clone::clone),
        "guidance": if selected.is_some() {
            "rerun the configured check in its host and report the actual result for every current worker completion; repair or rework if needed, then request review and acceptance again; a passing report still requires authority"
        } else {
            "inspect the exact evidence references before choosing repair, rework, or acceptance"
        },
    }))
}

/// Aggregate redacted run views.  Commands, goals, prompts, profiles, paths,
/// and artifact content are intentionally absent.  Synthetic and unclassified
/// runs are grouped separately and no incident or avoided-loss count is
/// inferred from them.  Missing policy metadata remains unclassified rather
/// than being inferred as a local report.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportGroup {
    runs: u64,
    checks: u64,
    protections: u64,
    failed_checks: u64,
    missing_checks: u64,
    duration_ms_reported: u64,
    duration_ms_reported_count: u64,
}

#[derive(Debug, Default, Serialize)]
struct ReportGroups {
    local_report: ReportGroup,
    synthetic: ReportGroup,
    unclassified: ReportGroup,
}

impl ReportGroups {
    fn get_mut(&mut self, origin: &str) -> Option<&mut ReportGroup> {
        match origin {
            "local_report" => Some(&mut self.local_report),
            "synthetic" => Some(&mut self.synthetic),
            "unclassified" => Some(&mut self.unclassified),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportMetrics {
    synthetic_incident_count: &'static str,
    user_confirmed_avoided_loss_count: &'static str,
    human_time_ms: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRedaction {
    goals: &'static str,
    commands: &'static str,
    prompts: &'static str,
    profiles: &'static str,
    paths: &'static str,
    artifact_content: &'static str,
    hashes_are_not_anonymization: bool,
}

#[derive(Debug, Serialize)]
struct ValueReport {
    version: u64,
    runs: u64,
    groups: ReportGroups,
    metrics: ReportMetrics,
    redaction: ReportRedaction,
}

impl ValueReport {
    fn new() -> Self {
        Self {
            version: VALUE_REPORT_VERSION,
            runs: 0,
            groups: ReportGroups::default(),
            metrics: ReportMetrics {
                synthetic_incident_count: "unknown",
                user_confirmed_avoided_loss_count: "unknown",
                human_time_ms: "unknown",
            },
            redaction: ReportRedaction {
                goals: "omitted",
                commands: "omitted",
                prompts: "omitted",
                profiles: "omitted",
                paths: "omitted",
                artifact_content: "omitted",
                hashes_are_not_anonymization: true,
            },
        }
    }
}

pub(crate) fn aggregate(states: &[Value]) -> Result<Value, String> {
    let mut seen_runs = std::collections::BTreeMap::new();
    let mut report = ValueReport::new();
    for state in states {
        let run_id = state["runId"]
            .as_str()
            .ok_or("report run is missing a runId")?;
        let state_hash = crate::hash::value(state);
        if let Some(previous) = seen_runs.insert(run_id.to_owned(), state_hash.clone()) {
            if previous != state_hash {
                return Err("duplicate runId has conflicting ledger evidence".into());
            }
            continue;
        }
        let origin = state["checkPolicy"]["origin"]
            .as_str()
            .unwrap_or("unclassified");
        if !matches!(origin, "local_report" | "synthetic" | "unclassified") {
            return Err("invalid report origin".into());
        }
        let empty_checks = Vec::new();
        let checks = state["checks"].as_array().unwrap_or(&empty_checks);
        let empty_protections = Vec::new();
        let protections = state["protections"]
            .as_array()
            .unwrap_or(&empty_protections);
        let assessment = crate::run_exit::assess(state)?;
        let group = report
            .groups
            .get_mut(origin)
            .ok_or("report groups are invalid")?;
        add_counter(&mut group.runs, 1)?;
        add_counter(&mut group.checks, checks.len() as u64)?;
        add_counter(&mut group.protections, protections.len() as u64)?;
        add_counter(
            &mut group.failed_checks,
            checks
                .iter()
                .filter(|check| check_exit_code(check) != Some(0))
                .count() as u64,
        )?;
        add_counter(
            &mut group.missing_checks,
            assessment
                .targets
                .iter()
                .filter(|target| target.is_missing())
                .count() as u64,
        )?;
        for check in checks {
            if check["acquisition"] != "observed" {
                if let Some(duration) = check["durationMs"].as_u64() {
                    add_counter(&mut group.duration_ms_reported, duration)?;
                    add_counter(&mut group.duration_ms_reported_count, 1)?;
                }
            }
        }
    }
    report.runs = seen_runs.len() as u64;
    serde_json::to_value(report).map_err(|error| error.to_string())
}

pub(crate) fn markdown(report: &Value) -> String {
    let groups = &report["groups"];
    format!(
        "Value proof report v{}\n\nRuns: {}\n\nLocal reports: {} runs, {} checks, {} protections\nSynthetic scenarios: {} runs, {} checks, {} protections\nUnclassified runs: {} runs, {} checks, {} protections\n\nSynthetic incident frequency, user-confirmed avoided loss, and human time are unknown.\nGoals, commands, prompts, profiles, paths, and artifact content are omitted. Hashes are not anonymization.\n",
        report["version"],
        report["runs"],
        groups["local_report"]["runs"],
        groups["local_report"]["checks"],
        groups["local_report"]["protections"],
        groups["synthetic"]["runs"],
        groups["synthetic"]["checks"],
        groups["synthetic"]["protections"],
        groups["unclassified"]["runs"],
        groups["unclassified"]["checks"],
        groups["unclassified"]["protections"],
    )
}

fn add_counter(current: &mut u64, amount: u64) -> Result<(), String> {
    let total = (*current)
        .checked_add(amount)
        .ok_or("report metric overflow")?;
    *current = total;
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

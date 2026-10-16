//! Typed value-proof records derived from a run ledger.
//!
//! The checked-run value boundary validates both caller-reported and locally
//! observed results.  Observation execution itself lives in `run.rs`.

use crate::run_exit::{CheckAssessment, CheckTarget};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{fs, path::Path};

pub(crate) const CHECK_POLICY_VERSION: u64 = 1;
pub(crate) const VALUE_REPORT_VERSION: u64 = 1;
pub(crate) const PRE_RETURN_POLICY_VERSION: u64 = 1;
pub(crate) const PRE_RETURN_TIMEOUT_MAX_MS: u64 = 300_000;
pub(crate) const PRE_RETURN_CAPTURE_MAX_BYTES: u64 = 1_048_576;

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
pub(crate) struct PreReturnPolicy {
    pub(crate) command: String,
    pub(crate) command_sha256: String,
    pub(crate) timeout_ms: u64,
    pub(crate) capture_limit_bytes: u64,
    pub(crate) checker_sources: Vec<CheckerSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreReturnOptions {
    pub(crate) command: String,
    pub(crate) timeout_ms: u64,
    pub(crate) capture_limit_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckerSource {
    pub(crate) argument_path: Option<String>,
    pub(crate) path: String,
    pub(crate) sha256: String,
}

impl PreReturnPolicy {
    pub(crate) fn from_cli(
        loaded: &crate::config::Loaded,
        command: Option<&str>,
        timeout_ms: Option<&str>,
        capture_bytes: Option<&str>,
    ) -> Result<Option<Self>, String> {
        let Some(options) = PreReturnOptions::from_cli(command, timeout_ms, capture_bytes)? else {
            return Ok(None);
        };
        Ok(Some(options.into_policy(loaded)?))
    }

    pub(crate) fn value(&self) -> Value {
        json!({"version": PRE_RETURN_POLICY_VERSION, "command": self.command, "commandSha256": self.command_sha256,
            "timeoutMs": self.timeout_ms, "captureLimitBytes": self.capture_limit_bytes, "checkerSources": self.checker_sources.iter().map(|s| {
                let mut value=json!({"path":s.path,"sha256":s.sha256});
                if let Some(argument_path)=&s.argument_path { value["argumentPath"]=json!(argument_path); }
                value
            }).collect::<Vec<_>>()})
    }
}

impl PreReturnOptions {
    /// Validate caller-selected policy values without needing project state.
    /// Setup uses this before any write; Work begin later adds source pinning
    /// from the loaded project configuration.
    pub(crate) fn from_cli(
        command: Option<&str>,
        timeout_ms: Option<&str>,
        capture_bytes: Option<&str>,
    ) -> Result<Option<Self>, String> {
        let (Some(command), timeout_ms, capture_bytes) = (command, timeout_ms, capture_bytes)
        else {
            if timeout_ms.is_some() || capture_bytes.is_some() {
                return Err("--pre-return-timeout-ms and --pre-return-capture-bytes require --pre-return-command".into());
            }
            return Ok(None);
        };
        let command = command.trim();
        if command.is_empty() || command.contains('\0') {
            return Err("--pre-return-command requires non-empty text without NUL bytes".into());
        }
        let timeout_ms = timeout_ms
            .map(|v| {
                parse_bounded_policy_u64("--pre-return-timeout-ms", v, 1, PRE_RETURN_TIMEOUT_MAX_MS)
            })
            .transpose()?
            .unwrap_or(120_000);
        let capture_limit_bytes = capture_bytes
            .map(|v| {
                parse_bounded_policy_u64(
                    "--pre-return-capture-bytes",
                    v,
                    1,
                    PRE_RETURN_CAPTURE_MAX_BYTES,
                )
            })
            .transpose()?
            .unwrap_or(262_144);
        Ok(Some(Self {
            command: command.to_owned(),
            timeout_ms,
            capture_limit_bytes,
        }))
    }

    fn into_policy(self, loaded: &crate::config::Loaded) -> Result<PreReturnPolicy, String> {
        let checker_sources = checker_sources(loaded, &self.command)?;
        Ok(PreReturnPolicy {
            command_sha256: crate::evidence::hash::text(&self.command),
            command: self.command,
            timeout_ms: self.timeout_ms,
            capture_limit_bytes: self.capture_limit_bytes,
            checker_sources,
        })
    }
}

pub(crate) fn checker_sources(
    loaded: &crate::config::Loaded,
    command: &str,
) -> Result<Vec<CheckerSource>, String> {
    // Pin ordinary file arguments, including `python3 check.py` and `sh -c
    // 'python3 check.py'`. Product-root inputs are also fingerprinted; this
    // separate list covers external wrappers explicitly named by the command.
    let words = command.split_whitespace().collect::<Vec<_>>();
    let mut sources = std::collections::BTreeMap::new();
    for argument in words {
        let argument = argument.trim_matches(['\'', '"', ';', ')', '(']);
        if argument.is_empty()
            || argument.starts_with('-')
            || argument.contains('$')
            || argument.contains('*')
        {
            continue;
        }
        let path = Path::new(argument);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            loaded.product_root.join(path)
        };
        let canonical = match fs::canonicalize(&resolved) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let metadata = fs::symlink_metadata(&canonical).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        let managed_state = loaded
            .state_root
            .join(crate::project::layout_types::state_namespace());
        if canonical.starts_with(managed_state) {
            continue;
        }
        // Interpreters are runtime tools; their explicit script arguments are the
        // checker source and are recorded separately when they resolve to files.
        let basename = canonical.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if matches!(
            basename,
            "sh" | "bash"
                | "dash"
                | "zsh"
                | "python"
                | "python2"
                | "python3"
                | "node"
                | "ruby"
                | "perl"
        ) {
            continue;
        }
        let path = canonical
            .to_str()
            .ok_or("pre-return checker path is not valid UTF-8")?
            .to_owned();
        let argument_path = resolved
            .to_str()
            .ok_or("pre-return checker argument path is not valid UTF-8")?
            .to_owned();
        let sha256 = crate::evidence::hash::file(&canonical)?;
        sources.insert(
            argument_path.clone(),
            CheckerSource {
                argument_path: Some(argument_path),
                path,
                sha256,
            },
        );
    }
    if sources.len() > CHECKER_SOURCE_COUNT_MAX {
        return Err("checker command names too many source files to bind".into());
    }
    Ok(sources.into_values().collect())
}

pub(crate) fn verification_checker_sources(
    loaded: &crate::config::Loaded,
    policy: Option<&CheckPolicy>,
    preservation: Option<&PreservationPolicy>,
) -> Result<Vec<CheckerSource>, String> {
    let mut sources = std::collections::BTreeMap::new();
    let mut commands = Vec::new();
    if let Some(policy) = policy {
        commands.push(policy.command.as_str());
    }
    if let Some(preservation) = preservation {
        commands.extend(
            preservation
                .requirements
                .iter()
                .map(|item| item.command.as_str()),
        );
    }
    for command in commands {
        for source in checker_sources(loaded, command)? {
            let key = source
                .argument_path
                .clone()
                .unwrap_or_else(|| source.path.clone());
            if let Some(previous) = sources.get(&key) {
                if previous != &source {
                    return Err(
                        "verification policies resolve one checker argument to different sources"
                            .into(),
                    );
                }
            } else {
                sources.insert(key, source);
            }
        }
    }
    if sources.len() > CHECKER_SOURCE_COUNT_MAX {
        return Err("verification policies name too many checker source files to bind".into());
    }
    Ok(sources.into_values().collect())
}

pub(crate) const CHECKER_SOURCE_COUNT_MAX: usize = 32;

pub(crate) fn checker_sources_value(sources: &[CheckerSource]) -> Value {
    Value::Array(
        sources
            .iter()
            .map(|source| {
                let mut value = json!({"path": source.path, "sha256": source.sha256});
                if let Some(argument_path) = &source.argument_path {
                    value["argumentPath"] = json!(argument_path);
                }
                value
            })
            .collect(),
    )
}

pub(crate) fn checker_sources_from_value(
    value: &Value,
    line: usize,
) -> Result<Vec<CheckerSource>, String> {
    let invalid =
        || format!("invalid run ledger line {line}: malformed verificationCheckerSources");
    let entries = value
        .as_array()
        .filter(|entries| entries.len() <= CHECKER_SOURCE_COUNT_MAX)
        .ok_or_else(invalid)?;
    let mut sources = Vec::with_capacity(entries.len());
    let mut seen = std::collections::BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(invalid)?;
        if !(object.len() == 2 || object.len() == 3)
            || !object.contains_key("path")
            || !object.contains_key("sha256")
            || object
                .keys()
                .any(|key| !["path", "sha256", "argumentPath"].contains(&key.as_str()))
        {
            return Err(invalid());
        }
        let path = entry["path"]
            .as_str()
            .filter(|path| {
                !path.is_empty()
                    && path.len() <= 1024
                    && !path.contains('\0')
                    && Path::new(path).is_absolute()
            })
            .ok_or_else(invalid)?
            .to_owned();
        let sha256 = entry["sha256"]
            .as_str()
            .filter(|sha| is_sha(Some(sha)))
            .ok_or_else(invalid)?
            .to_owned();
        let argument_path = entry
            .get("argumentPath")
            .map(|value| {
                value
                    .as_str()
                    .filter(|path| {
                        !path.is_empty()
                            && path.len() <= 1024
                            && !path.contains('\0')
                            && Path::new(path).is_absolute()
                    })
                    .map(str::to_owned)
                    .ok_or_else(invalid)
            })
            .transpose()?;
        let key = argument_path.clone().unwrap_or_else(|| path.clone());
        if !seen.insert(key) {
            return Err(invalid());
        }
        sources.push(CheckerSource {
            argument_path,
            path,
            sha256,
        });
    }
    Ok(sources)
}

pub(crate) fn validate_checker_sources(
    sources: &[CheckerSource],
    label: &str,
) -> Result<(), String> {
    for source in sources {
        let argument_path = source.argument_path.as_deref().unwrap_or(&source.path);
        let canonical = fs::canonicalize(argument_path)
            .map_err(|error| format!("{label} checker source is unavailable: {error}"))?;
        if canonical.to_str() != Some(source.path.as_str())
            || crate::evidence::hash::file(&canonical)? != source.sha256
        {
            return Err(format!("{label} checker source changed; use authorized supersession to bind the changed checker"));
        }
    }
    Ok(())
}

fn parse_bounded_policy_u64(
    name: &str,
    value: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an integer"))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(format!("{name} must be between {minimum} and {maximum}"));
    }
    Ok(parsed)
}

pub(crate) fn pre_return_policy_from_value(
    value: &Value,
    line: usize,
) -> Result<PreReturnPolicy, String> {
    let policy: PreReturnPolicyValue = serde_json::from_value(value.clone())
        .map_err(|_| format!("invalid run ledger line {line}: malformed preReturnPolicy"))?;
    if policy.version != PRE_RETURN_POLICY_VERSION
        || policy.command.trim().is_empty()
        || policy.command.contains('\0')
        || policy.command_sha256 != crate::evidence::hash::text(&policy.command)
        || !(1..=PRE_RETURN_TIMEOUT_MAX_MS).contains(&policy.timeout_ms)
        || !(1..=PRE_RETURN_CAPTURE_MAX_BYTES).contains(&policy.capture_limit_bytes)
    {
        return Err(format!(
            "invalid run ledger line {line}: malformed preReturnPolicy"
        ));
    }
    let checker_sources = policy
        .checker_sources
        .into_iter()
        .map(|source| CheckerSource {
            argument_path: source.argument_path,
            path: source.path,
            sha256: source.sha256,
        })
        .collect::<Vec<_>>();
    if checker_sources.iter().any(|source| {
        !is_sha(Some(&source.sha256))
            || source.path.is_empty()
            || source.argument_path.as_ref().is_some_and(|path| {
                path.is_empty() || path.contains('\0') || !Path::new(path).is_absolute()
            })
    }) {
        return Err(format!(
            "invalid run ledger line {line}: malformed preReturnPolicy"
        ));
    }
    Ok(PreReturnPolicy {
        command: policy.command,
        command_sha256: policy.command_sha256,
        timeout_ms: policy.timeout_ms,
        capture_limit_bytes: policy.capture_limit_bytes,
        checker_sources,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreReturnPolicyValue {
    version: u64,
    command: String,
    #[serde(rename = "commandSha256")]
    command_sha256: String,
    #[serde(rename = "timeoutMs")]
    timeout_ms: u64,
    #[serde(rename = "captureLimitBytes")]
    capture_limit_bytes: u64,
    #[serde(default)]
    #[serde(rename = "checkerSources")]
    checker_sources: Vec<CheckerSourceValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckerSourceValue {
    #[serde(default)]
    #[serde(rename = "argumentPath")]
    argument_path: Option<String>,
    path: String,
    sha256: String,
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

use super::receipt_path::{is_relative_path, receipt_path, state_relative};
use super::{harness, hash};
use crate::config::{self, Loaded};
use crate::{project::path as project_path, run::error as run_error};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

pub(crate) enum ExitPathError {
    Failure(String),
    Decision(crate::run_exit::ExitDecision),
}

impl From<String> for ExitPathError {
    fn from(error: String) -> Self {
        Self::Failure(error)
    }
}

impl From<&str> for ExitPathError {
    fn from(error: &str) -> Self {
        Self::Failure(error.to_owned())
    }
}

impl fmt::Display for ExitPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure(error) => formatter.write_str(error),
            Self::Decision(decision) => formatter.write_str(decision.wire().2),
        }
    }
}

pub fn write(
    path: &str,
    loaded: &Loaded,
    artifact: &Value,
    harness_manifest: Option<&str>,
) -> Result<Value, String> {
    let mut names = BTreeSet::new();
    if let Some(agent) = artifact["agent"].as_str() {
        names.insert(agent.to_owned());
    }
    for stage in artifact["stages"].as_array().into_iter().flatten() {
        for agent in stage["agents"].as_array().into_iter().flatten() {
            if let Some(name) = agent["name"].as_str() {
                names.insert(name.to_owned());
            }
        }
    }

    let mut profiles = Vec::new();
    for name in names {
        let agent = loaded
            .agent(&name)
            .ok_or_else(|| format!("receipt references unknown agent '{name}'"))?;
        let profile_path = config::file(&loaded.control_root, &agent.profile)?;
        profiles.push(json!({
            "agent": name,
            "path": config::rel(&loaded.control_root, &profile_path)?,
            "sha256": hash::file(&profile_path)?,
            "requestedRuntime": agent.runtime_value(),
        }));
    }

    let requested = profiles
        .iter()
        .map(|profile| {
            json!({
                "agent": profile["agent"],
                "host": profile["requestedRuntime"]["host"],
                "model": profile["requestedRuntime"]["model"],
                "reasoningEffort": profile["requestedRuntime"]["reasoningEffort"],
                "fallback": profile["requestedRuntime"]["fallback"],
            })
        })
        .collect::<Vec<_>>();
    let mut receipt = json!({
        "version": 1,
        "producer": crate::producer::evidence(),
        "evidence": "selected-config-and-profile-bytes",
        "createdAt": now(),
        "config": {
            "path": config::rel(&loaded.control_root, &loaded.path)?,
            "sha256": hash::text(&loaded.source),
        },
        "profiles": profiles,
        "runtime": { "requested": requested, "observed": Value::Null },
        "limitations": [
            "does not prove the model read or followed the profile",
            "does not prove filesystem, process, command, or memory isolation",
            "does not contain the task, goal, prompt, transcript, environment, or command output",
        ],
    });
    if let Some(path) = harness_manifest {
        receipt["version"] = json!(2);
        receipt["evidence"] = json!("selected-config-profile-and-harness-manifest-bytes");
        receipt["harness"] = harness::load(loaded, path)?;
        receipt["limitations"]
            .as_array_mut()
            .ok_or("internal receipt limitations must be an array")?
            .push(json!(
                "binds hashed harness claims with raw manifest strings omitted; does not authenticate them or prove activation or compliance"
            ));
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(receipt_path(&loaded.state_root, path)?)
        .map_err(|error| error.to_string())?;
    let serialized = serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?;
    file.write_all(format!("{serialized}\n").as_bytes())
        .map_err(|error| error.to_string())?;
    Ok(receipt)
}

pub fn verify(path: &str, loaded: &Loaded) -> Result<Value, String> {
    let (_, source) = read_state_bytes(loaded, path)?;
    let receipt = parse(&source)?;
    let mismatches = verify_value(loaded, &receipt)?;
    Ok(json!({
        "valid": mismatches.is_empty(),
        "mismatches": mismatches,
        "evidence": receipt["evidence"],
    }))
}

/// Emit the Exit Path receipt for a fully accepted v5 checked run.
pub(crate) fn exit_path(loaded: &Loaded, ledger: &str) -> Result<Value, ExitPathError> {
    let (path, events, source) = crate::run::ledger::load(loaded, ledger)?;
    let state = crate::run::reduce_live(loaded, &events)?;
    crate::run::assert_current_for_receipt(loaded, &state)?;
    let kernel = crate::run_exit::reduce(&state)?;
    let decision = kernel.decision();
    if !matches!(decision, crate::run_exit::ExitDecision::Ready) {
        return Err(ExitPathError::Decision(decision));
    }
    let (outcome, reason, detail) = decision.wire();
    let assessment = kernel.assessment.clone();
    let current = kernel.current_submissions.iter().collect::<Vec<_>>();
    let reviewer = if kernel.review_required {
        current
            .iter()
            .rev()
            .find(|event| event["role"] == "reviewer" && event["outcome"] == "approved")
            .copied()
    } else {
        None
    };
    if kernel.review_required && reviewer.is_none() {
        return Err("Exit Path receipt requires reviewer approval".into());
    }
    let acceptance = current
        .iter()
        .rev()
        .find(|event| event["role"] == "lead" && event["outcome"] == "accepted")
        .ok_or("Exit Path receipt requires lead acceptance")?;
    if !current
        .iter()
        .any(|event| event["role"] == "worker" && event["outcome"] == "completed")
    {
        return Err("Exit Path receipt requires worker completion".into());
    }
    let artifacts = current
        .iter()
        .filter(|event| {
            (event["role"] == "worker" && event["outcome"] == "completed")
                || (kernel.review_required
                    && event["role"] == "reviewer"
                    && event["outcome"] == "approved")
                || (event["role"] == "lead" && event["outcome"] == "accepted")
        })
        .map(|event| event["artifact"].clone())
        .collect::<Vec<_>>();
    let marked_policy = state.get("basisProtocol").is_some();
    let review = reviewer.map_or_else(
        || {
            if marked_policy {
                json!({
                    "status": "omitted",
                    "decisionSha256": kernel.review_decision_sha256,
                    "source": "owner-reported",
                })
            } else {
                json!({})
            }
        },
        |reviewer| {
            if marked_policy {
                json!({
                    "status": "approved",
                    "eventSha256": reviewer["eventSha256"],
                    "artifactSha256": reviewer["artifact"]["sha256"],
                })
            } else {
                json!({
                    "eventSha256": reviewer["eventSha256"],
                    "artifactSha256": reviewer["artifact"]["sha256"],
                })
            }
        },
    );
    Ok(json!({
        "version": 1,
        "format": "exit-path-v1",
        "product": "exitbind",
        "outcome": outcome,
        "reason": {"code": reason, "detail": detail},
        "producer": crate::producer::evidence(),
        "run": {
            "path": config::rel(&loaded.state_root, &path.expected)?,
            "sha256": hash::bytes(source.as_bytes()),
            "runId": state["runId"],
            "headEventSha256": events.last().map(|event| event["eventSha256"].clone()).unwrap_or(Value::Null),
            "configSha256": state["configSha256"],
        },
        "subject": state["subject"],
        "artifacts": artifacts,
        "check": {"status": "passed", "targets": assessment.targets_for_receipt()},
        "review": review,
        "acceptance": {"eventSha256": acceptance["eventSha256"], "artifactSha256": acceptance["artifact"]["sha256"]},
    }))
}

pub(crate) fn verify_exit_path(path: &str, loaded: &Loaded) -> Result<Value, String> {
    let (_, source) = read_state_bytes(loaded, path)?;
    let receipt: Value = serde_json::from_slice(&source)
        .map_err(|error| format!("invalid Exit Path receipt JSON: {error}"))?;
    let mut mismatches = Vec::new();
    if receipt["version"] != 1
        || receipt["format"] != "exit-path-v1"
        || receipt["product"] != "exitbind"
    {
        mismatches.push("receipt format or product changed".to_owned());
    }
    if !receipt.get("producer").is_some_and(crate::producer::valid)
        || receipt["producer"]["name"] != "exitbind"
    {
        mismatches.push("receipt producer is not Exitbind".to_owned());
    }
    let ledger = receipt["run"]["path"].as_str().unwrap_or("");
    match crate::run::ledger::load(loaded, ledger) {
        Ok((_, events, ledger_source)) => match crate::run::reduce_live(loaded, &events) {
            Ok(state) => {
                let kernel = crate::run_exit::reduce(&state);
                if !kernel.as_ref().is_ok_and(|kernel| {
                    matches!(kernel.decision(), crate::run_exit::ExitDecision::Ready)
                }) {
                    mismatches.push("run is not an accepted v5 checked run".into());
                }
                if kernel
                    .as_ref()
                    .is_ok_and(|kernel| kernel.assessment.policy.is_none())
                {
                    mismatches.push("run has no configured check policy".into());
                }
                if receipt["run"]["runId"] != state["runId"] {
                    mismatches.push("run subject changed".into());
                }
                if receipt["run"]["configSha256"] != state["configSha256"] {
                    mismatches.push("configuration binding changed".into());
                }
                if receipt["run"]["sha256"] != hash::bytes(ledger_source.as_bytes()) {
                    mismatches.push("ledger bytes changed".into());
                }
                if receipt["run"]["headEventSha256"]
                    != events
                        .last()
                        .map(|event| event["eventSha256"].clone())
                        .unwrap_or(Value::Null)
                {
                    mismatches.push("ledger head changed".into());
                }
                if receipt["subject"] != state["subject"] {
                    mismatches.push("accepted subject changed".into());
                }
                if let Err(error) = crate::run::assert_current_for_receipt(loaded, &state) {
                    mismatches.push(error);
                }
                match exit_path(loaded, ledger) {
                    Ok(expected) => {
                        for key in [
                            "run",
                            "subject",
                            "artifacts",
                            "check",
                            "review",
                            "acceptance",
                        ] {
                            if receipt[key] != expected[key] {
                                mismatches.push(format!("receipt {key} binding changed"));
                            }
                        }
                    }
                    Err(error) => {
                        mismatches.push(format!("canonical receipt reconstruction failed: {error}"))
                    }
                }
                if let Ok(kernel) = kernel {
                    let assessment = kernel.assessment;
                    if assessment.is_blocked() {
                        mismatches.push("check evidence is incomplete or failed".into());
                    }
                }
            }
            Err(error) => mismatches.push(error),
        },
        Err(error) => mismatches.push(format!("ledger unavailable: {error}")),
    }
    let valid = mismatches.is_empty();
    let outcome = crate::run_exit::receipt_decision(valid).wire().0;
    Ok(json!({
        "valid": valid,
        "outcome": outcome,
        "reason": if valid {
            json!({"code":"verified", "detail":"receipt matches the exact current ledger state"})
        } else {
            json!({"code":"receipt_mismatch", "detail": mismatches})
        },
        "mismatches": mismatches,
        "format": receipt["format"]
    }))
}

pub(crate) fn exit_receipt_path(
    loaded: &Loaded,
    requested: &str,
) -> Result<std::path::PathBuf, String> {
    let path = receipt_path(&loaded.state_root, requested)?;
    if path.exists() {
        return Err("receipt output already exists; refusing to overwrite".into());
    }
    Ok(path)
}

/// Validate a v2 harness receipt before binding it to a run start event.
pub(crate) fn for_run(loaded: &Loaded, requested: &str, plan: &Value) -> Result<Value, String> {
    let (relative, source) = read_state_bytes(loaded, requested)?;
    let receipt = parse(&source)?;
    if receipt["version"] != 2 {
        return Err("harness receipt must be a version-2 receipt".into());
    }
    let mismatches = verify_value(loaded, &receipt)?;
    if !mismatches.is_empty() {
        return Err("harness receipt is not current".into());
    }
    assert_plan_coverage(&receipt, plan)?;
    Ok(json!({
        "path": relative,
        "sha256": hash::bytes(&source),
        "version": 2,
    }))
}

/// Revalidate the exact receipt reference persisted by a bound run.
pub(crate) fn assert_current(
    loaded: &Loaded,
    reference: &Value,
    plan: &Value,
) -> Result<(), String> {
    let expected = reference["sha256"].as_str().unwrap_or_default();
    let (source, receipt) = exact_reference(loaded, reference)?;
    let current = hash::bytes(&source);
    let mismatches = verify_value(loaded, &receipt)?;
    if !mismatches.is_empty() {
        return Err(harness_drift(expected, &current));
    }
    assert_plan_coverage(&receipt, plan)
}

/// Validate the persisted receipt's own identity independently of whether its
/// recorded config and profiles still match today's project inputs.
pub(crate) fn assert_exact_reference(loaded: &Loaded, reference: &Value) -> Result<(), String> {
    exact_reference(loaded, reference).map(|_| ())
}

/// Identify an absent v2 receipt for a historical read-only run. A present
/// receipt still has to pass the full exact-reference check.
pub(crate) fn is_missing_historical_reference(
    loaded: &Loaded,
    reference: &Value,
) -> Result<bool, String> {
    let expected = reference["sha256"].as_str().unwrap_or_default();
    let relative = state_relative(
        &loaded.state_root,
        reference["path"].as_str().unwrap_or_default(),
    )?;
    if reference["version"] != 2 || reference["path"] != relative || !is_sha_text(expected) {
        return Err("harness receipt reference is not exact".into());
    }
    if matches!(
        crate::project::path::secure_bytes_observation(&loaded.state_root, &relative, "receipt"),
        crate::project::path::SecureBytesResult::Absent(_)
    ) {
        return Ok(true);
    }
    assert_exact_reference(loaded, reference)?;
    Ok(false)
}

/// Read an exact persisted receipt for an away prompt while preserving it as
/// historical evidence when current config/profile bytes have drifted. The
/// path, symlink, byte/hash, version, and JSON checks remain mandatory; only
/// semantic currentness against today's inputs is deferred to the run warning.
pub(crate) fn historical_manifest_for_reference(
    loaded: &Loaded,
    reference: &Value,
) -> Result<(Option<String>, Option<Value>), String> {
    let (_, receipt) = exact_reference(loaded, reference)?;
    let (manifest, current) = harness::raw_for_receipt(loaded, &receipt["harness"])?;
    let expected = receipt["harness"]["manifestSha256"]
        .as_str()
        .unwrap_or_default();
    let warning = (manifest.is_none() || current != expected).then(|| {
        json!({
            "error": "harness receipt drift detected after run start",
            "classification": "harness_receipt_drift",
            "expectedHarnessReceiptSha256": expected,
            "currentHarnessReceiptSha256": current
        })
    });
    Ok((manifest, warning))
}

fn exact_reference(loaded: &Loaded, reference: &Value) -> Result<(Vec<u8>, Value), String> {
    let expected = reference["sha256"].as_str().unwrap_or_default();
    let (relative, source) =
        read_state_bytes(loaded, reference["path"].as_str().unwrap_or_default())?;
    if reference["version"] != 2
        || reference["path"] != relative
        || !is_sha_text(expected)
        || hash::bytes(&source) != expected
    {
        return Err("harness receipt reference is not exact".into());
    }
    let receipt = parse(&source)?;
    if receipt["version"] != 2 {
        return Err("harness receipt must be a version-2 receipt".into());
    }
    validate_receipt_shape(&receipt)?;
    Ok((source, receipt))
}

fn validate_receipt_shape(receipt: &Value) -> Result<u64, String> {
    let version = receipt["version"]
        .as_u64()
        .filter(|version| matches!(version, 1 | 2))
        .ok_or("unsupported or malformed receipt")?;
    let expected_evidence = if version == 1 {
        "selected-config-and-profile-bytes"
    } else {
        "selected-config-profile-and-harness-manifest-bytes"
    };
    if receipt["evidence"] != expected_evidence
        || !receipt["profiles"].is_array()
        || receipt
            .get("producer")
            .is_some_and(|producer| !crate::producer::valid(producer))
        || !is_sha256(&receipt["config"]["sha256"])
        || receipt.get("taskOrGoalSha256").is_some()
        || receipt.get("artifactSha256").is_some()
    {
        return Err("unsupported or malformed receipt".into());
    }
    if version == 2
        && (receipt.as_object().map_or(0, |object| object.len()) != 9
            || !receipt.get("producer").is_some_and(crate::producer::valid)
            || receipt.get("harness").is_none())
    {
        return Err("unsupported or malformed receipt".into());
    }
    if version == 2 {
        validate_v2_shape(receipt)?;
    }
    Ok(version)
}

fn verify_value(loaded: &Loaded, receipt: &Value) -> Result<Vec<String>, String> {
    let version = validate_receipt_shape(receipt)?;

    let mut mismatches = Vec::new();
    if receipt["config"]["sha256"] != hash::text(&loaded.source) {
        mismatches.push("configuration changed".to_owned());
    }
    if receipt["config"]["path"] != config::rel(&loaded.control_root, &loaded.path)? {
        mismatches.push("configuration path changed".to_owned());
    }
    if version == 2 {
        match harness::verify(loaded, &receipt["harness"])? {
            true => {}
            false => mismatches.push("harness manifest changed".to_owned()),
        }
    } else if receipt.get("harness").is_some() {
        return Err("unsupported or malformed receipt".into());
    }

    let profiles = receipt["profiles"]
        .as_array()
        .ok_or("unsupported or malformed receipt")?;
    for entry in profiles {
        let name = entry["agent"]
            .as_str()
            .ok_or("unsupported or malformed receipt profile entry")?;
        let entry_path = entry["path"]
            .as_str()
            .ok_or("unsupported or malformed receipt profile entry")?;
        if !is_sha256(&entry["sha256"]) {
            return Err("unsupported or malformed receipt profile entry".into());
        }
        let agent = loaded
            .agent(name)
            .ok_or_else(|| format!("receipt references unknown agent '{name}'"))?;
        let declared = config::file(&loaded.control_root, &agent.profile);
        let selected = config::file(&loaded.control_root, entry_path);
        match (declared, selected) {
            (Ok(declared), Ok(selected)) => {
                if declared != selected {
                    mismatches.push(format!("profile path changed: {name}"));
                }
                if entry["sha256"] != hash::file(&selected)? {
                    mismatches.push(format!("profile changed: {name}"));
                }
            }
            (Err(error), _) | (_, Err(error))
                if error.starts_with("declared file does not exist:") =>
            {
                mismatches.push(format!("profile changed: {name}"));
            }
            (Err(error), _) | (_, Err(error)) => return Err(error),
        }
        if let Some(runtime) = entry.get("requestedRuntime") {
            if runtime != &agent.runtime_value() {
                mismatches.push(format!("runtime binding changed: {name}"));
            }
        }
    }
    if version == 2 {
        let requested = profiles
            .iter()
            .map(|profile| {
                json!({
                    "agent": profile["agent"],
                    "host": profile["requestedRuntime"]["host"],
                    "model": profile["requestedRuntime"]["model"],
                    "reasoningEffort": profile["requestedRuntime"]["reasoningEffort"],
                    "fallback": profile["requestedRuntime"]["fallback"],
                })
            })
            .collect::<Vec<_>>();
        if receipt["runtime"]["requested"] != Value::Array(requested) {
            return Err("unsupported or malformed receipt".into());
        }
    }
    Ok(mismatches)
}

fn validate_v2_shape(receipt: &Value) -> Result<(), String> {
    let config = receipt["config"]
        .as_object()
        .filter(|object| {
            object.len() == 2 && object.contains_key("path") && object.contains_key("sha256")
        })
        .ok_or("unsupported or malformed receipt")?;
    if !config
        .get("path")
        .and_then(Value::as_str)
        .is_some_and(is_relative_path)
        || !is_sha256(config.get("sha256").unwrap_or(&Value::Null))
        || !receipt["createdAt"]
            .as_str()
            .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok())
    {
        return Err("unsupported or malformed receipt".into());
    }
    let runtime = receipt["runtime"]
        .as_object()
        .filter(|object| {
            object.len() == 2 && object.contains_key("requested") && object.contains_key("observed")
        })
        .ok_or("unsupported or malformed receipt")?;
    if !runtime["requested"].is_array() || !runtime["observed"].is_null() {
        return Err("unsupported or malformed receipt".into());
    }
    for requested in runtime["requested"].as_array().into_iter().flatten() {
        let object = requested.as_object().filter(|object| {
            object.len() == 5
                && object.contains_key("agent")
                && object.contains_key("host")
                && object.contains_key("model")
                && object.contains_key("reasoningEffort")
                && object.contains_key("fallback")
        });
        if object.is_none() {
            return Err("unsupported or malformed receipt".into());
        }
    }
    if receipt["limitations"]
        .as_array()
        .map_or(true, |items| items.iter().any(|item| !item.is_string()))
    {
        return Err("unsupported or malformed receipt".into());
    }
    if receipt["profiles"].as_array().is_none() {
        return Err("unsupported or malformed receipt".into());
    }
    for profile in receipt["profiles"].as_array().into_iter().flatten() {
        let object = profile.as_object().filter(|object| {
            object.len() == 4
                && object.contains_key("agent")
                && object.contains_key("path")
                && object.contains_key("sha256")
                && object.contains_key("requestedRuntime")
        });
        let Some(object) = object else {
            return Err("unsupported or malformed receipt".into());
        };
        let requested_runtime = object["requestedRuntime"].as_object();
        if !object
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(is_relative_path)
            || !is_sha256(object.get("sha256").unwrap_or(&Value::Null))
            || requested_runtime.map_or(true, |runtime| {
                runtime.len() != 4
                    || !runtime.contains_key("host")
                    || !runtime.contains_key("model")
                    || !runtime.contains_key("reasoningEffort")
                    || !runtime.contains_key("fallback")
            })
        {
            return Err("unsupported or malformed receipt".into());
        }
    }
    Ok(())
}

fn assert_plan_coverage(receipt: &Value, plan: &Value) -> Result<(), String> {
    let expected = plan_profiles(plan)?;
    let mut actual = BTreeMap::new();
    for entry in receipt["profiles"]
        .as_array()
        .ok_or("unsupported or malformed receipt")?
    {
        let name = entry["agent"]
            .as_str()
            .ok_or("unsupported or malformed receipt profile entry")?;
        if actual.insert(name.to_owned(), entry.clone()).is_some() {
            return Err("harness receipt does not exactly cover the selected run plan".into());
        }
    }
    if actual.len() != expected.len()
        || expected
            .iter()
            .any(|(name, wanted)| actual.get(name) != Some(wanted))
    {
        return Err("harness receipt does not exactly cover the selected run plan".into());
    }
    Ok(())
}

fn plan_profiles(plan: &Value) -> Result<BTreeMap<String, Value>, String> {
    let mut profiles = BTreeMap::new();
    for stage in plan["stages"]
        .as_array()
        .ok_or("run plan stages are missing")?
    {
        for agent in stage["agents"]
            .as_array()
            .ok_or("run plan agents are missing")?
        {
            // One selected agent is one reviewer contract. Its authorized
            // alternate binding travels inside `requestedRuntime.fallback`, so
            // the receipt binds the contract and the alternative together
            // without pretending a second contract exists.
            let name = agent["name"]
                .as_str()
                .ok_or("run plan agent name is missing")?;
            let value = json!({
                "agent": name,
                "path": agent["profile"],
                "sha256": agent["profileSha256"],
                "requestedRuntime": agent["runtime"],
            });
            if let Some(previous) = profiles.insert(name.to_owned(), value.clone()) {
                if previous != value {
                    return Err("run plan selects an agent with conflicting evidence".into());
                }
            }
        }
    }
    Ok(profiles)
}

fn harness_drift(expected: &str, current: &str) -> String {
    run_error::machine_drift(run_error::DriftError::harness_receipt(
        expected.to_owned(),
        current.to_owned(),
    ))
}

fn parse(source: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(source).map_err(|error| format!("invalid receipt JSON: {error}"))
}

fn read_state_bytes(loaded: &Loaded, requested: &str) -> Result<(String, Vec<u8>), String> {
    let relative = state_relative(&loaded.state_root, requested)?;
    let source = project_path::secure_bytes(&loaded.state_root, &relative, "receipt")?;
    Ok((relative, source))
}

fn is_sha_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256(value: &Value) -> bool {
    value.as_str().is_some_and(is_sha_text)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

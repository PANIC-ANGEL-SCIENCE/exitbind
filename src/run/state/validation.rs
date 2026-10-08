//! Validate the canonical run event shapes before replay.

use super::governor_validation::validate_governor_event;
use super::*;

pub fn validate_event(event: &Value, previous: Option<&Value>, line: usize) -> Result<(), String> {
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: event must be an object"))?;
    let version = event["version"].as_u64();
    if !matches!(version, Some(1..=8)) || event["kind"] != "run" {
        return Err(format!(
            "invalid run ledger line {line}: invalid event header"
        ));
    }
    if previous.is_some_and(|previous| previous["version"] != event["version"]) {
        return Err(format!(
            "invalid run ledger line {line}: mixed event versions"
        ));
    }
    if event
        .get("producer")
        .is_some_and(|producer| !crate::producer::valid(producer))
        || (version == Some(1)
            && event
                .get("producer")
                .is_some_and(|producer| producer["name"] != "soulmate"))
        || (matches!(version, Some(2..=4)) && event["producer"]["name"] != "soulmate")
        || (matches!(version, Some(5..=8)) && event["producer"]["name"] != "exitbind")
    {
        return Err(format!("invalid run ledger line {line}: invalid producer"));
    }
    let action = event["action"].as_str().unwrap_or_default();
    if !matches!(
        action,
        "start"
            | "submit"
            | "check"
            | "protect"
            | "govern"
            | "review_policy"
            | "check_observation"
            | "check_observation_failed"
    ) {
        return Err(format!("invalid run ledger line {line}: invalid action"));
    }
    if previous.is_some() && action == "start" {
        return Err(format!(
            "invalid run ledger line {line}: start is only valid at the beginning"
        ));
    }
    if previous.is_none() && action != "start" {
        return Err(format!(
            "invalid run ledger line {line}: ledger must begin with start"
        ));
    }
    if !matches!(version, Some(3..=8)) && matches!(action, "check" | "protect") {
        return Err(format!(
            "invalid run ledger line {line}: value-proof actions require version 3"
        ));
    }
    if !is_sha(event["runId"].as_str()) {
        return Err(format!("invalid run ledger line {line}: invalid runId"));
    }
    if !is_timestamp(event["timestamp"].as_str()) {
        return Err(format!("invalid run ledger line {line}: invalid timestamp"));
    }
    if !is_sha(event["eventSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: invalid event hash"
        ));
    }
    let previous_hash = previous
        .map(|x| x["eventSha256"].clone())
        .unwrap_or(Value::Null);
    if event["previousEventSha256"] != previous_hash {
        return Err(format!(
            "invalid run ledger line {line}: broken event chain"
        ));
    }
    if crate::evidence::hash::value(&without(event, "eventSha256")) != event["eventSha256"] {
        return Err(format!(
            "invalid run ledger line {line}: event hash mismatch"
        ));
    }
    if let Some(prev) = previous {
        if timestamp_ms(event["timestamp"].as_str()) < timestamp_ms(prev["timestamp"].as_str()) {
            return Err(format!(
                "invalid run ledger line {line}: timestamp is earlier than previous event"
            ));
        }
    }
    let shape = if event["action"] == "start" {
        validate_start_version(event, line, version.unwrap_or_default())
    } else if matches!(
        event["action"].as_str(),
        Some("check_observation" | "check_observation_failed")
    ) {
        super::check_observation::validate(event)
    } else if event["action"] == "submit" {
        validate_submission(event, line)
    } else if event["action"] == "govern" {
        validate_governor_event(event, line)
    } else if event["action"] == "review_policy" {
        validate_review_policy_event(event, line)
    } else if event["action"] == "check" {
        crate::run_value::validate_check_event(event, line)
    } else {
        crate::run_value::validate_protection_event(event, line)
    };
    let action = event["action"]
        .as_str()
        .ok_or_else(|| format!("invalid run ledger line {line}: invalid action"))?;
    shape.and_then(|_| reject_unknown(object, action, version.unwrap_or_default(), line))
}

pub fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    validate_start_version(event, line, event["version"].as_u64().unwrap_or_default())
}

fn validate_start_version(event: &Value, line: usize, version: u64) -> Result<(), String> {
    if !matches!(version, 1..=8) {
        return Err(format!(
            "invalid run ledger line {line}: invalid event version"
        ));
    }
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: start must be an object"))?;
    reject_unknown(object, "start", version, line)?;
    if event["previousEventSha256"] != Value::Null {
        return Err(format!(
            "invalid run ledger line {line}: start must begin the chain"
        ));
    }
    for field in ["workflow", "goal", "configSha256"] {
        if event[field].as_str().map_or(true, |x| x.trim().is_empty()) {
            return Err(format!(
                "invalid run ledger line {line}: {field} is required"
            ));
        }
    }
    if !is_sha(event["configSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: invalid config hash"
        ));
    }
    if version >= 5 && !valid_subject(event.get("subject"), event["runId"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: checked start requires a valid subject"
        ));
    }
    validate_basis_extension(event, line, version)?;
    let plan = &event["plan"];
    if let Some(selection) = plan.get("architectureContract") {
        crate::project::architecture::selection(selection)
            .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    }
    if plan["version"] != 1 {
        return Err(format!(
            "invalid run ledger line {line}: invalid workflow plan"
        ));
    }
    let stages = plan["stages"]
        .as_array()
        .filter(|stages| !stages.is_empty());
    let Some(stages) = stages else {
        return Err(format!(
            "invalid run ledger line {line}: invalid workflow plan"
        ));
    };
    if plan["maxParallel"].as_u64().map_or(true, |x| x < 1) {
        return Err(format!(
            "invalid run ledger line {line}: invalid maxParallel"
        ));
    }
    if let Some(boundary) = plan.get("boundaryManifest") {
        if !crate::config::boundary::validate_evidence(boundary) {
            return Err(format!(
                "invalid run ledger line {line}: invalid boundary manifest evidence"
            ));
        }
    }
    for (i, stage) in stages.iter().enumerate() {
        let agents = stage["agents"]
            .as_array()
            .filter(|agents| !agents.is_empty());
        let Some(agents) = agents else {
            return Err(format!(
                "invalid run ledger line {line}: invalid stage {}",
                i + 1
            ));
        };
        if stage["stage"] != i + 1 {
            return Err(format!(
                "invalid run ledger line {line}: invalid stage {}",
                i + 1
            ));
        }
        for agent in agents {
            if agent.as_object().is_none()
                || agent["name"].as_str().map_or(true, str::is_empty)
                || !ROLES.contains(&agent["role"].as_str().unwrap_or(""))
                || !display_name(agent["displayName"].as_str())
                || !native_name(agent["nativeTaskName"].as_str())
                || !relative(agent["profile"].as_str())
                || !is_sha(agent["profileSha256"].as_str())
                || !agent["runtime"].is_object()
                || !agent["declaredBoundary"].is_object()
            {
                return Err(format!(
                    "invalid run ledger line {line}: invalid selected agent evidence"
                ));
            }
            if let Some(binding) = agent.get("fallbackRuntime") {
                if version < 7 {
                    return Err(format!(
                        "invalid run ledger line {line}: fallback runtime requires v7"
                    ));
                }
                if agent["role"] != "reviewer" || !runtime_binding(binding) {
                    return Err(format!(
                        "invalid run ledger line {line}: invalid fallback runtime evidence"
                    ));
                }
            }
            if let Some(references) = agent.get("memoryReferences") {
                crate::memory::selection::validate_references(references)
                    .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
            }
        }
    }
    if let Some(link) = event.get("supersedes") {
        validate_supersession(link, line)?;
    }
    crate::run::carry::validate_start(event, line)?;
    if let Some(marker) = event.get("recoveryProtocol") {
        if version != 8
            || marker.as_u64() != Some(RECOVERY_PROTOCOL_VERSION)
            || event.get("basisProtocol").is_none()
            || event.get("governor").is_none()
            || event.get("checkPolicy").is_none()
        {
            return Err(format!(
                "invalid run ledger line {line}: recovery protocol requires a checked, governed v8 basis"
            ));
        }
    }
    if let Some(marker) = event.get("checkObservationProtocol") {
        if version != 8
            || marker != crate::run::check_observation::PROTOCOL
            || event.get("recoveryProtocol").is_none()
        {
            return Err("check observation protocol requires checked governed v8 recovery".into());
        }
    }
    if version == 2 {
        validate_harness_receipt(event.get("harnessReceipt"), line)?;
    } else if event.get("harnessReceipt").is_some() {
        if version == 1 {
            return Err(format!(
                "invalid run ledger line {line}: v1 start must not contain harnessReceipt"
            ));
        }
        validate_harness_receipt(event.get("harnessReceipt"), line)?;
    }
    if matches!(version, 3 | 4) {
        let policy = event.get("checkPolicy").ok_or_else(|| {
            format!("invalid run ledger line {line}: checked start requires checkPolicy")
        })?;
        crate::run_value::policy_from_value(policy, line)?;
        let has_worker = stages.iter().any(|stage| {
            stage["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == "worker"))
        });
        if !has_worker {
            return Err(format!(
                "invalid run ledger line {line}: checked run requires a worker stage"
            ));
        }
    } else if version >= 5 {
        if let Some(policy) = event.get("checkPolicy") {
            crate::run_value::policy_from_value(policy, line)?;
        }
        if let Some(preservation) = event.get("preservation") {
            if version < 6 {
                return Err(format!(
                    "invalid run ledger line {line}: preservation requires v6"
                ));
            }
            if event.get("checkPolicy").is_none() {
                return Err(format!(
                    "invalid run ledger line {line}: preservation requires checkPolicy"
                ));
            }
            crate::run_value::preservation_from_value(preservation, line)?;
            let has_worker = stages.iter().any(|stage| {
                stage["agents"]
                    .as_array()
                    .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == "worker"))
            });
            if !has_worker {
                return Err(format!(
                    "invalid run ledger line {line}: preservation requires a worker stage"
                ));
            }
        }
    } else if event.get("checkPolicy").is_some() {
        return Err(format!(
            "invalid run ledger line {line}: checkPolicy requires checked event version"
        ));
    } else if event.get("preservation").is_some() {
        return Err(format!(
            "invalid run ledger line {line}: preservation requires v6"
        ));
    }
    Ok(())
}

fn validate_harness_receipt(value: Option<&Value>, line: usize) -> Result<(), String> {
    let Some(value) = value else {
        return Err(format!(
            "invalid run ledger line {line}: v2 start requires harnessReceipt"
        ));
    };
    let object = value.as_object().ok_or_else(|| {
        format!("invalid run ledger line {line}: harnessReceipt must be an object")
    })?;
    if object.len() != 3
        || !object.contains_key("path")
        || !object.contains_key("sha256")
        || !object.contains_key("version")
        || value["version"] != 2
        || !relative(value["path"].as_str())
        || !is_sha(value["sha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid harness receipt reference"
        ));
    }
    Ok(())
}

pub fn validate_supersession(value: &Value, line: usize) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: invalid supersession evidence"))?;
    let fields = [
        "ledgerPath",
        "ledgerSha256",
        "runId",
        "headEventSha256",
        "configSha256",
    ];
    if object.len() != fields.len()
        || fields.iter().any(|x| !object.contains_key(*x))
        || !relative(value["ledgerPath"].as_str())
        || !is_sha(value["ledgerSha256"].as_str())
        || !is_sha(value["runId"].as_str())
        || !is_sha(value["headEventSha256"].as_str())
        || !is_sha(value["configSha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid supersession evidence"
        ));
    }
    Ok(())
}

pub fn validate_submission(event: &Value, line: usize) -> Result<(), String> {
    for field in ["stage", "attempt"] {
        if event[field].as_u64().map_or(true, |x| x < 1) {
            return Err(format!(
                "invalid run ledger line {line}: invalid stage or attempt"
            ));
        }
    }
    if event["agent"]
        .as_str()
        .map_or(true, |x| x.trim().is_empty())
        || !ROLES.contains(&event["role"].as_str().unwrap_or(""))
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid submission actor"
        ));
    }
    if event["outcome"]
        .as_str()
        .map_or(true, |x| x.trim().is_empty())
    {
        return Err(format!(
            "invalid run ledger line {line}: outcome is required"
        ));
    }
    let artifact = &event["artifact"];
    let Some(artifact_object) = artifact.as_object() else {
        return Err(format!(
            "invalid run ledger line {line}: invalid artifact evidence"
        ));
    };
    let has_bytes = artifact_object.contains_key("bytes");
    let bytes_valid = artifact_object
        .get("bytes")
        .map_or(true, |bytes| bytes.as_u64().is_some());
    let shape_valid = match artifact_object.len() {
        2 => !artifact_object.contains_key("root") && !has_bytes,
        3 => artifact_object.contains_key("root") ^ has_bytes,
        4 => artifact_object.contains_key("root") && has_bytes,
        _ => false,
    };
    if (!shape_valid
        || !artifact_object.contains_key("path")
        || !artifact_object.contains_key("sha256")
        || !bytes_valid)
        || artifact
            .get("root")
            .is_some_and(|root| !matches!(root.as_str(), Some("product" | "state")))
        || !relative(artifact["path"].as_str())
        || !is_sha(artifact["sha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid artifact evidence"
        ));
    }
    let binds_inputs = matches!(
        (event["role"].as_str(), event["outcome"].as_str()),
        (Some("reviewer"), Some("approved")) | (Some("lead"), Some("accepted"))
    );
    let worker_completion = matches!(
        (event["role"].as_str(), event["outcome"].as_str()),
        (Some("worker"), Some("completed"))
    );
    if event["version"].as_u64() >= Some(6) && binds_inputs {
        if !is_sha(event["inputsSha256"].as_str()) {
            return Err(format!(
                "invalid run ledger line {line}: approval requires tested input identity"
            ));
        }
    } else if worker_completion {
        if let Some(inputs) = event.get("inputsSha256") {
            if !is_sha(inputs.as_str()) {
                return Err(format!(
                    "invalid run ledger line {line}: invalid worker input identity"
                ));
            }
        }
    } else if event.get("inputsSha256").is_some() {
        return Err(format!(
            "invalid run ledger line {line}: unexpected tested input identity"
        ));
    }
    if event
        .get("assignmentSha256")
        .is_some_and(|value| !is_sha(value.as_str()))
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid assignment identity"
        ));
    }
    if event["version"].as_u64() >= Some(8)
        && event.get("basisSha256").is_some()
        && !event["basisSha256"].is_null()
        && !is_sha(event["basisSha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid basis identity"
        ));
    }
    if event["version"].as_u64() >= Some(8)
        && event.get("reviewDecisionSha256").is_some()
        && !is_sha(event["reviewDecisionSha256"].as_str())
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid review decision identity"
        ));
    }
    if event["outcome"] == "disposition" {
        if event["role"] != "lead"
            || (!event["basisSha256"].is_null() && !is_sha(event["basisSha256"].as_str()))
        {
            return Err(format!(
                "invalid run ledger line {line}: disposition requires marked Lead basis identity"
            ));
        }
        crate::kernel::disposition::parse_disposition(
            event
                .get("disposition")
                .ok_or_else(|| format!("invalid run ledger line {line}: disposition is missing"))?,
            "disposition",
        )
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    } else if event.get("disposition").is_some() {
        return Err(format!(
            "invalid run ledger line {line}: unexpected disposition"
        ));
    }
    validate_fallback_binding(event, line)?;
    if let Some(governor_event) = event.get("governorEvent") {
        if event["version"].as_u64() < Some(6)
            || event["role"] != "worker"
            || event["outcome"] != "completed"
            || governor_event.as_object().is_none()
        {
            return Err(format!(
                "invalid run ledger line {line}: governor event requires a completed worker submission"
            ));
        }
        if let Some(grants) = governor_event.get("grantEventSha256s") {
            let Some(grants) = grants.as_array() else {
                return Err(format!(
                    "invalid run ledger line {line}: mutation grant acknowledgement is malformed"
                ));
            };
            let mut seen = std::collections::BTreeSet::new();
            if grants.iter().any(|grant| {
                grant.as_str().map_or(true, |hash| {
                    hash.len() != SHA_LEN
                        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                        || !seen.insert(hash.to_owned())
                })
            }) {
                return Err(format!(
                    "invalid run ledger line {line}: mutation grant acknowledgement is malformed"
                ));
            }
            if governor_event["action"] != "mutation"
                || (grants.is_empty() && governor_event["authorizationMode"] != "implicit")
            {
                return Err(format!(
                    "invalid run ledger line {line}: grant acknowledgement requires mutation"
                ));
            }
        }
    }
    Ok(())
}

fn validate_basis_extension(event: &Value, line: usize, version: u64) -> Result<(), String> {
    let has_extended_fields = event.get("basisProtocol").is_some()
        || event.get("basis").is_some()
        || event.get("reviewPolicy").is_some();
    if !has_extended_fields {
        return Ok(());
    }
    if version != 8 || event["basisProtocol"] != crate::kernel::basis::PROTOCOL_VERSION {
        return Err(format!(
            "invalid run ledger line {line}: basis extension requires protocol marker v8"
        ));
    }
    if let Some(basis) = event.get("basis") {
        let basis = crate::kernel::basis::parse_basis(basis, "basis")
            .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
        if event["subject"]["basisSha256"] != basis.sha256 {
            return Err(format!(
                "invalid run ledger line {line}: subject is not bound to basis"
            ));
        }
    }
    let review = event.get("reviewPolicy").ok_or_else(|| {
        format!("invalid run ledger line {line}: basis extension requires reviewPolicy")
    })?;
    crate::kernel::basis::parse_review(review, "reviewPolicy")
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    Ok(())
}

fn validate_review_policy_event(event: &Value, line: usize) -> Result<(), String> {
    if event["version"].as_u64() != Some(8)
        || event["basisProtocol"] != crate::kernel::basis::PROTOCOL_VERSION
        || event["role"] != "lead"
    {
        return Err(format!(
            "invalid run ledger line {line}: review policy requires marked v8 Lead transition"
        ));
    }
    if event["stage"].as_u64().map_or(true, |value| value < 1)
        || event["attempt"].as_u64().map_or(true, |value| value < 1)
        || event["agent"]
            .as_str()
            .map_or(true, |value| value.trim().is_empty())
    {
        return Err(format!(
            "invalid run ledger line {line}: review policy actor binding is invalid"
        ));
    }
    let policy = event
        .get("reviewPolicy")
        .ok_or_else(|| format!("invalid run ledger line {line}: review policy is missing"))?;
    crate::kernel::basis::parse_review(policy, "reviewPolicy")
        .map_err(|error| format!("invalid run ledger line {line}: {error}"))?;
    if event["basisSha256"].is_null() {
        if event["basisProtocol"] != crate::kernel::basis::PROTOCOL_VERSION {
            return Err(format!(
                "invalid run ledger line {line}: review policy basis marker is invalid"
            ));
        }
    } else if !is_sha(event["basisSha256"].as_str()) {
        return Err(format!(
            "invalid run ledger line {line}: review policy binding is missing basisSha256"
        ));
    }
    if !is_sha(event["previousDecisionSha256"].as_str()) {
        return Err(format!(
                "invalid run ledger line {line}: review policy binding is missing previousDecisionSha256"
            ));
    }
    Ok(())
}

/// An alternate execution binding: host, model, and reasoning effort only.
///
/// The shape is the whole guarantee. A binding carries no name, profile,
/// purpose, or declared boundary, so nothing a fallback records can stand in
/// for a reviewer contract. A wholly unspecified binding is admissible here
/// because a primary runtime may request nothing at all, and the provenance
/// must record that honestly rather than refuse the submission; configuration
/// separately requires an authorized fallback to bind something.
fn runtime_binding(value: &Value) -> bool {
    let Some(binding) = value.as_object() else {
        return false;
    };
    binding
        .keys()
        .all(|key| RUNTIME_BINDING_FIELDS.contains(&key.as_str()))
        && binding.values().all(|value| {
            value.is_null() || value.as_str().is_some_and(|entry| !entry.trim().is_empty())
        })
}

const RUNTIME_BINDING_FIELDS: &[&str] = &["host", "model", "reasoningEffort"];

/// The single submit field carrying substitution provenance.
///
/// On an `unavailable` reviewer submission it records the bounded operational
/// reason and the execution binding that could not run. On the substitution's
/// own verdict it records the same reason and the alternate binding that
/// actually produced it. `identitySource` stays `host-reported` in both: the
/// binding is what the caller declared, never something Exitbind observed.
pub(crate) fn validate_fallback_binding(event: &Value, line: usize) -> Result<(), String> {
    let Some(fallback) = event.get("fallback") else {
        return Ok(());
    };
    if !matches!(event["version"].as_u64(), Some(7 | 8)) {
        return Err(format!(
            "invalid run ledger line {line}: fallback provenance requires v7"
        ));
    }
    let outcome = event["outcome"].as_str().unwrap_or_default();
    let allowed = ["reason", "runtime", "identitySource", "substituted"];
    let object = fallback.as_object().filter(|object| {
        object.keys().all(|key| allowed.contains(&key.as_str()))
            && object.get("reason").and_then(Value::as_str).is_some()
            && object.get("runtime").is_some_and(runtime_binding)
            && object.get("identitySource").and_then(Value::as_str) == Some("host-reported")
    });
    let Some(object) = object else {
        return Err(format!(
            "invalid run ledger line {line}: malformed fallback provenance"
        ));
    };
    let reason = object["reason"].as_str().unwrap_or_default();
    if !FALLBACK_REASONS.contains(&reason) {
        return Err(format!(
            "invalid run ledger line {line}: unbounded fallback reason"
        ));
    }
    let substituted = object.get("substituted").and_then(Value::as_bool);
    match (outcome, substituted) {
        ("unavailable", None) => {}
        ("approved" | "rework" | "blocked", Some(true)) => {}
        _ => {
            return Err(format!(
                "invalid run ledger line {line}: fallback provenance does not match its submission"
            ))
        }
    }
    if event["role"] != "reviewer" {
        return Err(format!(
            "invalid run ledger line {line}: fallback provenance requires the reviewer role"
        ));
    }
    Ok(())
}

use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const SHA_LEN: usize = 64;
const ROLES: &[&str] = &["lead", "adviser", "worker", "reviewer"];
/// Bounded operational reasons a reviewer target may fail to execute. These are
/// the only admissible causes for `unavailable`; vendor prose is never parsed.
pub(crate) const FALLBACK_REASONS: &[&str] =
    &["provider_quota", "rate_limit", "provider_unavailable"];

pub fn make_event(mut value: Value) -> Value {
    let hash = crate::evidence::hash::value(&value);
    value["eventSha256"] = json!(hash);
    value
}

pub fn reduce(events: &[Value]) -> Result<Value, String> {
    if events.is_empty() {
        return Err("run ledger has no start event".into());
    }
    validate_start(&events[0], 1)?;
    let first = &events[0];
    let mut state = json!({
        "version": first["version"],
        "runId": first["runId"], "workflow": first["workflow"], "goal": first["goal"],
        "configSha256": first["configSha256"], "plan": first["plan"], "status": "running",
        "currentStage": 1, "attempt": 1, "submissions": [], "checks": [],
        "protections": [], "events": events,
        "basisHistory": [], "dispositions": [], "pendingDisposition": Value::Null
    });
    if let Some(marker) = first.get("basisProtocol") {
        if marker != crate::kernel::basis::PROTOCOL_VERSION {
            return Err("invalid run start: unsupported basis protocol".into());
        }
        state["basisProtocol"] = marker.clone();
        if let Some(basis) = first.get("basis") {
            state["basis"] = basis.clone();
        }
        if let Some(review) = first.get("reviewPolicy") {
            state["reviewPolicy"] = review.clone();
            state["reviewDecisions"] = json!([review]);
        }
    }
    let governor_enabled = first
        .get("governor")
        .is_some_and(crate::context::validate_marker);
    if first.get("governor").is_some() && !governor_enabled {
        return Err("invalid run start: malformed governor marker".into());
    }
    if governor_enabled {
        state["governor"] = crate::context::reduce_governor(&[])?;
        state["governor"]["enabled"] = json!(true);
        state["governor"]["defaults"] = first["governor"].clone();
        if let Some(protocol) = first["governor"].get("grantProtocol") {
            state["governor"]["grantProtocol"] = protocol.clone();
        }
    }
    if let Some(receipt) = first.get("harnessReceipt") {
        state["harnessReceipt"] = receipt.clone();
    }
    if let Some(policy) = first.get("checkPolicy") {
        state["checkPolicy"] = policy.clone();
    }
    if let Some(preservation) = first.get("preservation") {
        state["preservation"] = preservation.clone();
    }
    if let Some(subject) = first.get("subject") {
        state["subject"] = subject.clone();
    }
    let mut request_scopes = BTreeSet::new();
    for (index, event) in events.iter().enumerate().skip(1) {
        validate_event(event, events.get(index - 1), index + 1)?;
        if event["runId"] != first["runId"] {
            return Err(format!(
                "invalid run ledger line {}: runId changed",
                index + 1
            ));
        }
        if event["action"] == "govern" && !governor_enabled {
            return Err(format!(
                "invalid run ledger line {}: governor action requires a v0.22 marker",
                index + 1
            ));
        }
        if let Some(request_id) = event.get("requestId").and_then(Value::as_str) {
            let scope = crate::evidence::hash::value(&json!({
                "runId": event["runId"],
                "stage": event["stage"],
                "attempt": event["attempt"],
                "agent": event["agent"],
                "role": event["role"],
                "assignmentSha256": event["assignmentSha256"],
                "requestId": request_id,
            }));
            if !request_scopes.insert(scope) {
                return Err(format!(
                    "invalid run ledger line {}: duplicate governor request identity",
                    index + 1
                ));
            }
        }
        if state["governor"]["grantProtocol"] == crate::context::GRANT_PROTOCOL_VERSION
            && event["action"] == "submit"
            && event["role"] == "worker"
            && event["outcome"] == "completed"
        {
            let governor_event = event.get("governorEvent").ok_or_else(|| {
                format!(
                    "invalid run ledger line {}: completion authorization is missing",
                    index + 1
                )
            })?;
            validate_grant_acknowledgement(&state, &events[..index], event, governor_event)?;
        }
        if let Some(governor_event) = event.get("governorEvent") {
            if !governor_enabled {
                return Err(format!(
                    "invalid run ledger line {}: governor event requires a v0.22 marker",
                    index + 1
                ));
            }
            if event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
                && (governor_event.get("authorizationMode").is_some()
                    || governor_event.get("grantEventSha256s").is_some())
            {
                validate_grant_acknowledgement(&state, &events[..index], event, governor_event)?;
            }
            let mut governor_events = state["governorEvents"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            governor_events.push(governor_event.clone());
            let grant_protocol = state["governor"]["grantProtocol"].clone();
            state["governor"] = crate::context::reduce_governor(&governor_events)?;
            state["governor"]["enabled"] = json!(true);
            if !grant_protocol.is_null() {
                state["governor"]["grantProtocol"] = grant_protocol;
            }
            state["governorEvents"] = Value::Array(governor_events);
        }
        apply_event(&mut state, event)?;
    }
    state["assignments"] = json!(crate::run::assignment::pending(&state));
    Ok(state)
}

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
        "start" | "submit" | "check" | "protect" | "govern" | "review_policy"
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

fn validate_grant_acknowledgement(
    state: &Value,
    prior_events: &[Value],
    event: &Value,
    governor_event: &Value,
) -> Result<(), String> {
    let grants = governor_event["grantEventSha256s"]
        .as_array()
        .ok_or("grant acknowledgement is missing")?;
    let sources = governor_event["sourceInputsSha256s"]
        .as_array()
        .ok_or("grant source inputs are missing")?;
    let mode = governor_event["authorizationMode"]
        .as_str()
        .ok_or("grant authorization mode is missing")?;
    if governor_event["action"] != "mutation" || event["action"] != "submit" {
        return Err("grant acknowledgement is only valid on a worker mutation completion".into());
    }
    let assignment = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| item["agent"] == event["agent"])
        .ok_or("grant acknowledgement has no current assignment")?;
    if assignment["role"] != "worker"
        || event["role"] != "worker"
        || event["outcome"] != "completed"
        || event["stage"] != assignment["stage"]
        || event["attempt"] != assignment["attempt"]
    {
        return Err("grant acknowledgement is not bound to the current worker".into());
    }
    let assignment_sha256 = event["assignmentSha256"].as_str();
    let subject = state["subject"]["sha256"].clone();
    let result_input = event["inputsSha256"].clone();
    let lineage = state["governor"]["lineageSha256"].clone();
    let mut expected = Vec::new();
    let mut already_acknowledged = std::collections::BTreeSet::new();
    for prior in prior_events {
        if let Some(previous) = prior["governorEvent"]["grantEventSha256s"].as_array() {
            already_acknowledged.extend(previous.iter().filter_map(Value::as_str));
        }
    }
    for prior in prior_events {
        let nested = &prior["governorEvent"];
        if prior["action"] == "govern"
            && prior["runId"] == state["runId"]
            && prior["subjectSha256"] == subject
            && prior["stage"] == assignment["stage"]
            && prior["attempt"] == assignment["attempt"]
            && prior["agent"] == assignment["agent"]
            && prior["role"] == assignment["role"]
            && nested["action"] == "mutation"
            && nested["runId"] == state["runId"]
            && nested["subjectSha256"] == subject
            && nested["attempt"] == state["attempt"]
            && nested["inputSha256"] == prior["inputsSha256"]
            && nested["lineageSha256"] == lineage
            && nested["carryLineage"] == true
            && assignment_sha256.map_or(true, |value| prior["assignmentSha256"] == value)
        {
            let hash = prior["eventSha256"]
                .as_str()
                .ok_or("mutation grant has no outer event identity")?;
            if !already_acknowledged.contains(hash) {
                expected.push((
                    hash.to_owned(),
                    nested["inputSha256"]
                        .as_str()
                        .ok_or("mutation grant has no source input identity")?
                        .to_owned(),
                ));
            }
        }
    }
    let mut actual = grants
        .iter()
        .zip(sources)
        .map(|(grant, source)| {
            Ok((
                grant
                    .as_str()
                    .ok_or("grant reference is malformed")?
                    .to_owned(),
                source
                    .as_str()
                    .ok_or("grant source input is malformed")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if grants.len() != sources.len() {
        return Err("grant references and source inputs have different lengths".into());
    }
    expected.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    actual.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    match mode {
        "explicit" if assignment_sha256.is_none() || expected.is_empty() || expected != actual => {
            return Err("mutation completion does not acknowledge exact outstanding grants".into())
        }
        "implicit" if !expected.is_empty() || !actual.is_empty() => {
            return Err("implicit completion has outstanding explicit grants".into())
        }
        "explicit" | "implicit" => {}
        _ => return Err("grant authorization mode is invalid".into()),
    }
    if governor_event["inputSha256"] != result_input {
        return Err("completion governor input does not match result input".into());
    }
    if state["governor"]["state"] != "ready" {
        return Err("mutation grant completion requires a ready governor".into());
    }
    Ok(())
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

fn validate_governor_event(event: &Value, line: usize) -> Result<(), String> {
    if event["version"].as_u64() < Some(6) {
        return Err(format!(
            "invalid run ledger line {line}: governor action requires v6"
        ));
    }
    for field in ["stage", "attempt"] {
        if event[field].as_u64().map_or(true, |value| value < 1) {
            return Err(format!(
                "invalid run ledger line {line}: invalid governor stage or attempt"
            ));
        }
    }
    if event["agent"]
        .as_str()
        .map_or(true, |value| value.trim().is_empty())
        || !ROLES.contains(&event["role"].as_str().unwrap_or(""))
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid governor actor"
        ));
    }
    for field in ["subjectSha256", "inputsSha256", "assignmentSha256"] {
        if !is_sha(event[field].as_str()) {
            return Err(format!(
                "invalid run ledger line {line}: governor binding is missing {field}"
            ));
        }
    }
    if event["operation"]
        .as_str()
        .map_or(true, |value| value.trim().is_empty() || value.len() > 120)
    {
        return Err(format!(
            "invalid run ledger line {line}: governor operation is invalid"
        ));
    }
    let request_id = event.get("requestId");
    let request_digest = event.get("requestDigest");
    if request_id.is_some() != request_digest.is_some()
        || request_id.is_some_and(|value| {
            value.as_str().map_or(true, |value| {
                value.trim().is_empty()
                    || value.len() > crate::run::REQUEST_ID_MAX_BYTES
                    || value.contains('\0')
            })
        })
        || request_digest.is_some_and(|value| !is_sha(value.as_str()))
    {
        return Err(format!(
            "invalid run ledger line {line}: governor request identity is malformed"
        ));
    }
    let Some(governor_event) = event.get("governorEvent") else {
        return Err(format!(
            "invalid run ledger line {line}: governor event is missing"
        ));
    };
    if governor_event.get("requestId") != request_id
        || governor_event.get("requestDigest") != request_digest
    {
        return Err(format!(
            "invalid run ledger line {line}: governor request identity does not bind to the action"
        ));
    }
    if let (Some(request_id), Some(request_digest)) = (
        request_id.and_then(Value::as_str),
        request_digest.and_then(Value::as_str),
    ) {
        let expected = crate::run::governor_request_digest(
            event["runId"].as_str().unwrap_or_default(),
            event["stage"].as_u64().unwrap_or_default(),
            event["attempt"].as_u64().unwrap_or_default(),
            event["agent"].as_str().unwrap_or_default(),
            event["role"].as_str().unwrap_or_default(),
            event["subjectSha256"].as_str().unwrap_or_default(),
            event["inputsSha256"].as_str().unwrap_or_default(),
            event["assignmentSha256"].as_str().unwrap_or_default(),
            event["operation"].as_str().unwrap_or_default(),
            request_id,
        );
        if request_digest != expected {
            return Err(format!(
                "invalid run ledger line {line}: governor request digest does not match its binding"
            ));
        }
    }
    let action = governor_event["action"].as_str().unwrap_or_default();
    if governor_event["runId"] != event["runId"]
        || governor_event["subjectSha256"] != event["subjectSha256"]
        || governor_event["attempt"] != event["attempt"]
        || governor_event["inputSha256"] != event["inputsSha256"]
    {
        return Err(format!(
            "invalid run ledger line {line}: governor event does not bind to the action"
        ));
    }
    match action {
        "mutation" => {
            if governor_event["operation"] != event["operation"]
                || governor_event["carryLineage"] != true
            {
                return Err(format!(
                    "invalid run ledger line {line}: governor mutation does not bind to the action"
                ));
            }
        }
        "replan" => {
            if ["hypothesis", "evidenceRequest", "scopeDecision", "blocker"]
                .iter()
                .all(|field| governor_event.get(*field).is_none())
            {
                return Err(format!(
                    "invalid run ledger line {line}: material re-plan is missing semantic fields"
                ));
            }
        }
        "evidence" => {
            if governor_event.get("evidence").is_none() {
                return Err(format!(
                    "invalid run ledger line {line}: governor evidence is missing"
                ));
            }
        }
        "sensor_request" => {
            if governor_event["requestDigest"].as_str().is_none()
                || governor_event["questions"].as_array().is_none()
            {
                return Err(format!(
                    "invalid run ledger line {line}: sensor request is malformed"
                ));
            }
        }
        "sensor" => {
            if governor_event["requestDigest"].as_str().is_none()
                || governor_event["assessment"].as_str().is_none()
            {
                return Err(format!(
                    "invalid run ledger line {line}: sensor result is malformed"
                ));
            }
        }
        "blocked" => {}
        _ => {
            return Err(format!(
                "invalid run ledger line {line}: unknown governor event action"
            ));
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

fn apply_event(state: &mut Value, event: &Value) -> Result<(), String> {
    match event["action"].as_str() {
        Some("submit") => apply_submission(state, event),
        Some("govern") => apply_govern(state, event),
        Some("review_policy") => apply_review_policy(state, event),
        Some("check") => apply_check(state, event),
        Some("protect") => apply_protection(state, event),
        _ => Err("run event action is invalid".into()),
    }
}

fn apply_govern(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" {
        return Err("governor action requires a running run".into());
    }
    if event["governorEvent"]["action"] == "replan" {
        // v0.22/v0.24-rc.2 ledgers did not persist the assignment packet
        // binding and may transition identity in their markerless re-plan.
        // Preserve those append-only histories; current run markers take the
        // strict path below.
        if state["governor"]["defaults"]["replanBinding"] != "assignment_packet_v1" {
            return Ok(());
        }
        if event["governorEvent"]["identityTransition"].is_null()
            && state["governor"]["defaults"]["replanBinding"] == "assignment_packet_v1"
            && (event["subjectSha256"] != state["subject"]["sha256"]
                || event["attempt"] != state["attempt"])
        {
            return Err("legacy re-plan cannot change identity under the current binding".into());
        }
        if state["governor"]["defaults"]["replanBinding"] == "assignment_packet_v1"
            && event["governorEvent"]["identityTransition"] != "carried_mutation_v1"
        {
            return Err("current re-plan is missing its identity transition binding".into());
        }
        let prior_input = state["events"].as_array().and_then(|events| {
            let current = events
                .iter()
                .position(|candidate| candidate["eventSha256"] == event["eventSha256"])?;
            events[..current]
                .iter()
                .rev()
                .find_map(|candidate| candidate["inputsSha256"].as_str())
        });
        let assignment = crate::run::assignment::pending(state)
            .into_iter()
            .find(|item| item["agent"] == event["agent"])
            .ok_or("re-plan actor is not currently assigned")?;
        if assignment["role"] != "worker"
            || event["role"] != "worker"
            || assignment["stage"] != event["stage"]
            || assignment["attempt"] != event["attempt"]
            || assignment["role"] != event["role"]
            || event["subjectSha256"] != state["subject"]["sha256"]
            || state["inputsSha256"]
                .as_str()
                .or(prior_input)
                .is_some_and(|expected| event["inputsSha256"] != expected)
        {
            return Err("re-plan is not bound to the current worker assignment".into());
        }
        if let Some(packet_sha256) = event["governorEvent"]["assignmentPacketSha256"].as_str() {
            if packet_sha256 != crate::evidence::hash::value(&assignment) {
                return Err("re-plan assignment packet is stale or mismatched".into());
            }
        } else if state["governor"]["defaults"]["replanBinding"] == "assignment_packet_v1" {
            return Err("carried re-plan is missing its assignment packet binding".into());
        }
    }
    Ok(())
}

fn apply_review_policy(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" || state.get("basisProtocol").is_none() {
        return Err("review policy requires a marked running run".into());
    }
    let current = state["reviewPolicy"]["sha256"]
        .as_str()
        .ok_or("review policy has no current decision identity")?;
    if event["previousDecisionSha256"].as_str() != Some(current)
        || event["basisSha256"] != state["basis"]["sha256"]
    {
        return Err("review policy transition is stale".into());
    }
    let policy = event
        .get("reviewPolicy")
        .ok_or("review policy transition is missing its decision")?;
    let parsed = crate::kernel::basis::parse_review(policy, "reviewPolicy")
        .map_err(|error| error.to_string())?;
    if parsed.previous_sha256.as_deref() != Some(current) {
        return Err("review policy transition does not chain its prior decision".into());
    }
    if policy["decision"] == state["reviewPolicy"]["decision"]
        && policy["reason"] == state["reviewPolicy"]["reason"]
    {
        return Err("duplicate review policy decision is refused".into());
    }
    state["reviewPolicy"] = policy.clone();
    state["reviewDecisions"]
        .as_array_mut()
        .ok_or("review decision history is invalid")?
        .push(policy.clone());
    if policy["decision"] == "omitted" && state["pendingDisposition"].is_null() {
        let current_stage = state["currentStage"].as_u64().unwrap_or_default();
        let reviewer_pending = state["plan"]["stages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|stage| {
                stage["stage"] == current_stage
                    && stage["agents"].as_array().is_some_and(|agents| {
                        agents.iter().any(|agent| agent["role"] == "reviewer")
                    })
            });
        if reviewer_pending {
            state["currentStage"] = json!(lead_stage(state)?);
        }
    } else if policy["decision"] == "required" {
        let current_stage = state["currentStage"].as_u64().unwrap_or_default();
        let lead_pending = state["plan"]["stages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|stage| {
                stage["stage"] == current_stage
                    && stage["agents"]
                        .as_array()
                        .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == "lead"))
            });
        if lead_pending
            && state["pendingDisposition"].is_null()
            && current_stage == lead_stage(state)?
        {
            state["currentStage"] = json!(reviewer_stage(state)?);
        }
    }
    Ok(())
}

fn apply_check(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" {
        return Err("run has already reached a terminal state".into());
    }
    crate::run_value::validate_check_against_state(state, event, 0)
        .map_err(|error| error.replacen("line 0", "state", 1))?;
    if !crate::run_exit::reduce(state)?.subject_is_current(&event["subjectSha256"]) {
        return Err("check is bound to a stale subject".into());
    }
    state["checks"]
        .as_array_mut()
        .ok_or("run state checks are invalid")?
        .push(event.clone());
    Ok(())
}

fn apply_protection(state: &mut Value, event: &Value) -> Result<(), String> {
    // A v6 protection is judged on the tested inputs it declares; that judgment
    // does not leak into later state.
    let live = state.get("inputsSha256").cloned();
    if let Some(inputs) = event.get("inputsSha256") {
        state["inputsSha256"] = inputs.clone();
    }
    let validated = crate::run_value::validate_protection_against_state(state, event, 0);
    match live {
        Some(live) => state["inputsSha256"] = live,
        None => {
            if let Some(object) = state.as_object_mut() {
                object.remove("inputsSha256");
            }
        }
    }
    validated.map_err(|error| error.replacen("line 0", "state", 1))?;
    if !crate::run_exit::reduce(state)?.subject_is_current(&event["subjectSha256"]) {
        return Err("protection is bound to a stale subject".into());
    }
    state["protections"]
        .as_array_mut()
        .ok_or("run state protections are invalid")?
        .push(event.clone());
    Ok(())
}

fn apply_submission(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" {
        return Err("run has already reached a terminal state".into());
    }
    let candidate = crate::run::assignment::pending(state)
        .into_iter()
        .find(|x| x["agent"] == event["agent"]);
    let Some(assignment) = candidate else {
        return Err(format!(
            "agent '{}' is not currently pending",
            event["agent"]
        ));
    };
    if event["stage"] != assignment["stage"]
        || event["attempt"] != assignment["attempt"]
        || event["role"] != assignment["role"]
    {
        return Err("submission is out of order".into());
    }
    if state.get("basisProtocol").is_some() && event["basisSha256"] != state["basis"]["sha256"] {
        return Err("submission is bound to a stale basis".into());
    }
    if state["version"].as_u64() >= Some(5) {
        let expected = if event["role"] == "worker" && event["outcome"] == "completed" {
            subject_for_submission(state, &assignment, &event["artifact"])["sha256"].clone()
        } else {
            state["subject"]["sha256"].clone()
        };
        if event["subjectSha256"] != expected {
            return Err("submission is bound to a stale subject".into());
        }
        if event["role"] == "worker" && event["outcome"] == "completed" {
            state["subject"] = subject_for_submission(state, &assignment, &event["artifact"]);
        }
    }
    let role = event["role"].as_str().ok_or("submission role is invalid")?;
    let outcome = event["outcome"]
        .as_str()
        .ok_or("submission outcome is invalid")?;
    let all = match role {
        "lead" => &[
            "scoped",
            "blocked",
            "accepted",
            "rework",
            "rejected",
            "disposition",
        ][..],
        "adviser" => &["completed", "blocked"][..],
        "worker" => &["completed", "blocked", "contradiction"][..],
        "reviewer" => &["approved", "rework", "blocked", "unavailable"][..],
        _ => &[],
    };
    if !all.contains(&event["outcome"].as_str().unwrap_or("")) {
        return Err(format!(
            "outcome '{}' is not allowed for role '{role}'",
            event["outcome"]
        ));
    }
    if outcome == "unavailable" {
        return apply_unavailable(state, event, &assignment);
    }
    if outcome == "contradiction"
        && (state.get("basisProtocol").is_none() || state["basis"].get("sha256").is_none())
    {
        return Err("contradiction requires a marked basis record".into());
    }
    if outcome == "disposition" {
        if role != "lead" || state.get("basisProtocol").is_none() {
            return Err("disposition requires the marked Lead transition".into());
        }
        return super::disposition::apply(state, event);
    }
    if role == "lead" && state["currentStage"] == 1 && !["scoped", "blocked"].contains(&outcome) {
        return Err(format!(
            "outcome '{}' is not allowed for this stage",
            event["outcome"]
        ));
    }
    if role == "lead" && outcome == "accepted" {
        if state["pendingDisposition"].is_object() {
            return Err("acceptance requires the pending Lead disposition".into());
        }
        if let Some(inputs) = event.get("inputsSha256") {
            // Acceptance is judged against the tested inputs it declares, so
            // replay and the live gate apply one rule.
            state["inputsSha256"] = inputs.clone();
        }
        crate::run_exit::reduce(state)?.acceptance_gate()?;
    }
    let mut submission = json!({"stage":event["stage"],"attempt":event["attempt"],"agent":event["agent"],"role":event["role"],"outcome":event["outcome"],"artifact":event["artifact"],"eventSha256":event["eventSha256"]});
    if let Some(inputs) = event.get("inputsSha256") {
        submission["inputsSha256"] = inputs.clone();
        state["inputsSha256"] = inputs.clone();
    }
    for field in ["basisSha256", "reviewDecisionSha256"] {
        if let Some(value) = event.get(field) {
            submission[field] = value.clone();
        }
    }
    state["submissions"]
        .as_array_mut()
        .ok_or("run state submissions are invalid")?
        .push(submission);
    if ["accepted", "rejected", "blocked"].contains(&outcome) {
        state["status"] = json!(outcome);
        return Ok(());
    }
    if outcome == "rework" {
        if role == "reviewer" && state.get("basisProtocol").is_some() {
            state["pendingDisposition"] = json!({
                "owner": "lead",
                "triggerEventSha256": event["eventSha256"],
                "basisSha256": state["basis"]["sha256"],
                "kind": "review_rework",
                "findingSha256s": [event["eventSha256"]]
            });
            state["currentStage"] = json!(lead_stage(state)?);
            return Ok(());
        }
        let stages = state["plan"]["stages"]
            .as_array()
            .ok_or("run state plan stages are invalid")?;
        let worker = stages
            .iter()
            .find(|s| {
                s["agents"]
                    .as_array()
                    .is_some_and(|agents| agents.iter().any(|a| a["role"] == "worker"))
            })
            .ok_or("rework requires a worker stage")?;
        state["currentStage"] = worker["stage"].clone();
        let attempt = state["attempt"].as_u64().ok_or("run attempt is invalid")?;
        state["attempt"] = json!(attempt + 1);
        return Ok(());
    }
    if outcome == "contradiction" {
        state["pendingDisposition"] = json!({
            "owner": "lead",
            "triggerEventSha256": event["eventSha256"],
            "basisSha256": state["basis"]["sha256"],
            "kind": "contradiction",
            "findingSha256s": [event["eventSha256"]]
        });
        state["currentStage"] = json!(lead_stage(state)?);
        return Ok(());
    }
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("run state plan stages are invalid")?;
    let required = stages
        .iter()
        .find(|s| s["stage"] == state["currentStage"])
        .ok_or("run current stage is invalid")?["agents"]
        .as_array()
        .ok_or("run current stage agents are invalid")?
        .len();
    let completed = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?
        .iter()
        .filter(|x| x["stage"] == state["currentStage"] && x["attempt"] == state["attempt"])
        // An `unavailable` reviewer recorded a failure to execute, not a stage
        // completion; counting it would advance the stage on no verdict.
        .filter(|x| x["outcome"] != "unavailable")
        .count();
    if completed == required && state["currentStage"] != stages.len() {
        let current = state["currentStage"]
            .as_u64()
            .ok_or("run current stage is invalid")?;
        state["currentStage"] = json!(current + 1);
        if state["reviewPolicy"]["decision"] == "omitted"
            && state["plan"]["stages"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|stage| {
                    stage["stage"] == state["currentStage"]
                        && stage["agents"].as_array().is_some_and(|agents| {
                            agents.iter().any(|agent| agent["role"] == "reviewer")
                        })
                })
        {
            state["currentStage"] = json!(lead_stage(state)?);
        }
    }
    Ok(())
}

/// Record that a reviewer target could not execute for a bounded operational
/// reason.  This is never a verdict: it neither completes the stage nor
/// advances it, and it is refused outright once any verdict exists for the same
/// stage and attempt, so a substitution can never escape an adverse review.
fn apply_unavailable(state: &mut Value, event: &Value, assignment: &Value) -> Result<(), String> {
    let stage = event["stage"].clone();
    let attempt = event["attempt"].clone();
    let existing = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?;
    let shopped = existing.iter().any(|submission| {
        submission["stage"] == stage
            && submission["attempt"] == attempt
            && submission["role"] == "reviewer"
            && matches!(submission["outcome"].as_str(), Some("rework" | "blocked"))
    });
    if shopped {
        return Err(
            "unavailable is not admissible after a reviewer verdict; fallback cannot re-open a decided review"
                .into(),
        );
    }
    let prior_unavailable = existing
        .iter()
        .filter(|submission| {
            submission["stage"] == stage
                && submission["attempt"] == attempt
                && submission["outcome"] == "unavailable"
        })
        .count();
    // A substitution is a packet re-issued onto the alternate binding, not one
    // that merely carries an authorized alternative.
    let is_substitution = assignment["substitution"].is_object();
    if is_substitution && prior_unavailable == 0 {
        return Err("a fallback reviewer requires a recorded primary unavailability".into());
    }
    if !is_substitution && prior_unavailable > 0 {
        return Err("the primary reviewer already reported unavailable for this attempt".into());
    }
    state["submissions"]
        .as_array_mut()
        .ok_or("run state submissions are invalid")?
        .push(json!({
            "stage": stage, "attempt": attempt, "agent": event["agent"],
            "role": "reviewer", "outcome": "unavailable",
            "artifact": event["artifact"], "eventSha256": event["eventSha256"],
            "fallback": event["fallback"],
        }));
    // The substitution is bounded: it happens at most once per stage+attempt. A
    // second operational failure — the fallback's own — ends the attempt at
    // blocked rather than searching for a third target.
    if is_substitution {
        state["status"] = json!("blocked");
        return Ok(());
    }
    // With no authorized binding left to execute the same contract, the run has
    // no admissible reviewer and stays blocked.
    if authorized_fallback_runtime(state, event["agent"].as_str().unwrap_or("")).is_none() {
        state["status"] = json!("blocked");
    }
    Ok(())
}

/// The alternate execution binding the plan authorized for `agent`, if any.
fn authorized_fallback_runtime<'a>(state: &'a Value, agent: &str) -> Option<&'a Value> {
    let current = state["currentStage"].as_u64()?;
    state["plan"]["stages"]
        .as_array()?
        .iter()
        .find(|stage| stage["stage"].as_u64() == Some(current))?["agents"]
        .as_array()?
        .iter()
        .find(|selected| selected["name"].as_str() == Some(agent))?
        .get("fallbackRuntime")
        .filter(|binding| binding.is_object())
}

fn reject_unknown(
    object: &Map<String, Value>,
    action: &str,
    version: u64,
    line: usize,
) -> Result<(), String> {
    let allowed: &[&str] = if action == "start" {
        let mut allowed = vec![
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "workflow",
            "goal",
            "configSha256",
            "plan",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
            "supersedes",
        ];
        if version == 2 {
            allowed.push("harnessReceipt");
        }
        if matches!(version, 3..=8) {
            allowed.push("harnessReceipt");
            allowed.push("checkPolicy");
        }
        if version >= 5 {
            allowed.push("subject");
        }
        if version >= 6 {
            allowed.push("preservation");
        }
        if version >= 6 {
            allowed.push("governor");
        }
        if version >= 8 {
            allowed.push("basisProtocol");
            allowed.push("basis");
            allowed.push("reviewPolicy");
        }
        return object
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
            .map_or(Ok(()), |key| {
                Err(format!(
                    "invalid run ledger line {line}: unknown field '{key}'"
                ))
            });
    } else if action == "submit" {
        let mut allowed = vec![
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "stage",
            "attempt",
            "agent",
            "role",
            "outcome",
            "artifact",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ];
        if version >= 5 {
            allowed.push("subjectSha256");
        }
        if version >= 6 {
            allowed.push("inputsSha256");
        }
        if version >= 6 {
            allowed.push("assignmentSha256");
        }
        if version >= 7 {
            allowed.push("fallback");
        }
        if version >= 8 {
            allowed.push("basisSha256");
            allowed.push("reviewDecisionSha256");
            allowed.push("disposition");
        }
        if version >= 6 {
            allowed.push("governorEvent");
        }
        return reject_unknown_fields(object, &allowed, line);
    } else if action == "review_policy" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "basisProtocol",
                "basisSha256",
                "previousDecisionSha256",
                "reviewPolicy",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "govern" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "subjectSha256",
                "inputsSha256",
                "assignmentSha256",
                "assignmentPacketSha256",
                "operation",
                "requestId",
                "requestDigest",
                "governorEvent",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "check" {
        if version == 8 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "configSha256",
                    "targetEventSha256",
                    "requirementId",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "stdout",
                    "stderr",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version >= 6 {
            return reject_unknown_fields(
                object,
                &[
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
                ],
                line,
            );
        }
        if version == 5 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "targetEventSha256",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 4 {
            return reject_unknown_fields(
                object,
                &[
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
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        &[
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
        ]
    } else {
        if version >= 5 {
            return reject_unknown_fields(
                object,
                &[
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
                ],
                line,
            );
        }
        if version == 4 {
            return reject_unknown_fields(
                object,
                &[
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
                ],
                line,
            );
        }
        &[
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
        ]
    };
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown field '{key}'"
            ))
        })
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    line: usize,
) -> Result<(), String> {
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown field '{key}'"
            ))
        })
}
fn without(value: &Value, key: &str) -> Value {
    let mut copy = value.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove(key);
    }
    copy
}
fn relative(value: Option<&str>) -> bool {
    let Some(v) = value else { return false };
    if v.contains('\\') {
        return false;
    }
    !v.trim().is_empty()
        && !v.contains('\0')
        && !v.starts_with('/')
        && !v.contains(":/")
        && !v.split('/').any(|x| x.is_empty() || x == "." || x == "..")
}
fn is_sha(value: Option<&str>) -> bool {
    value.is_some_and(|x| {
        x.len() == SHA_LEN
            && x.bytes().all(|b| b.is_ascii_hexdigit())
            && x.bytes().all(|b| !b.is_ascii_uppercase())
    })
}
fn display_name(value: Option<&str>) -> bool {
    value.is_some_and(|x| {
        !x.is_empty() && x.len() <= 80 && x.trim() == x && !x.bytes().any(|b| b < 0x20 || b == 0x7f)
    })
}
fn native_name(value: Option<&str>) -> bool {
    value.is_some_and(|x| {
        !x.is_empty()
            && x.len() <= 64
            && x.bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_lowercase() || b.is_ascii_digit() || (b == b'_' && i > 0))
    })
}
fn is_timestamp(value: Option<&str>) -> bool {
    value.is_some_and(|x| x.contains('T') && timestamp_ms(Some(x)) != i64::MIN)
}
fn valid_subject(value: Option<&Value>, event_run_id: Option<&str>) -> bool {
    let Some(value) = value else { return false };
    let Some(object) = value.as_object() else {
        return false;
    };
    (object.len() == 10 || object.len() == 11)
        && value["version"] == 1
        && is_sha(value["runId"].as_str())
        && value["runId"].as_str() == event_run_id
        && is_sha(value["goalSha256"].as_str())
        && is_sha(value["planSha256"].as_str())
        && is_sha(value["configSha256"].as_str())
        && (value.get("basisSha256").is_none() || is_sha(value["basisSha256"].as_str()))
        && value["attempt"].as_u64().is_some()
        && (value["previousSubjectSha256"].is_null()
            || is_sha(value["previousSubjectSha256"].as_str()))
        && (value["workerArtifactSha256"].is_null()
            || is_sha(value["workerArtifactSha256"].as_str()))
        && (value["transitionSha256"].is_null() || is_sha(value["transitionSha256"].as_str()))
        && is_sha(value["sha256"].as_str())
        && crate::evidence::hash::value(&without(value, "sha256")) == value["sha256"]
}

pub(crate) fn subject_for_submission(state: &Value, assignment: &Value, artifact: &Value) -> Value {
    let previous = state["subject"]["sha256"].clone();
    let artifact_sha = artifact["sha256"].clone();
    let attempt = assignment["attempt"].as_u64().unwrap_or_default();
    let transition = crate::evidence::hash::value(&json!({
        "runId": state["runId"], "previousSubjectSha256": previous, "stage": assignment["stage"], "attempt": attempt,
        "agent": assignment["agent"], "artifactSha256": artifact_sha,
    }));
    let mut subject = state["subject"].clone();
    subject["attempt"] = json!(attempt);
    subject["previousSubjectSha256"] = previous;
    subject["workerArtifactSha256"] = artifact_sha;
    subject["transitionSha256"] = json!(transition);
    let sha = crate::evidence::hash::value(&without(&subject, "sha256"));
    subject["sha256"] = json!(sha);
    subject
}

fn stage_for_role(state: &Value, role: &str, last: bool) -> Result<u64, String> {
    let mut stages = state["plan"]["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|stage| {
            stage["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == role))
        })
        .filter_map(|stage| stage["stage"].as_u64());
    let stage = if last {
        stages.next_back()
    } else {
        stages.next()
    };
    stage.ok_or_else(|| format!("run has no {role} stage"))
}

pub(super) fn lead_stage(state: &Value) -> Result<u64, String> {
    stage_for_role(state, "lead", true)
}
fn reviewer_stage(state: &Value) -> Result<u64, String> {
    stage_for_role(state, "reviewer", false)
}
pub(super) fn worker_stage(state: &Value) -> Result<u64, String> {
    stage_for_role(state, "worker", false)
}

pub(super) fn subject_for_basis_revision(
    state: &Value,
    basis_sha256: &Value,
    disposition_sha256: &str,
) -> Value {
    let mut subject = state["subject"].clone();
    let previous = subject["sha256"].clone();
    subject["basisSha256"] = basis_sha256.clone();
    subject["attempt"] = json!(state["attempt"].as_u64().unwrap_or_default() + 1);
    subject["previousSubjectSha256"] = previous.clone();
    subject["workerArtifactSha256"] = Value::Null;
    subject["transitionSha256"] = json!(crate::evidence::hash::value(&json!({
        "runId": state["runId"],
        "previousSubjectSha256": previous,
        "dispositionSha256": disposition_sha256,
        "basisSha256": basis_sha256,
    })));
    subject["sha256"] = json!(crate::evidence::hash::value(&without(&subject, "sha256")));
    subject
}
fn timestamp_ms(value: Option<&str>) -> i64 {
    value
        .and_then(|x| chrono::DateTime::parse_from_rfc3339(x).ok())
        .map(|x| x.timestamp_millis())
        .unwrap_or(i64::MIN)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_govern, apply_unavailable, subject_for_submission, valid_subject,
        validate_submission, without,
    };
    use serde_json::json;

    const SHA: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    /// The alternate execution binding: where a review runs, and nothing that
    /// could carry a second reviewer contract.
    fn binding() -> serde_json::Value {
        json!({"host": "claude", "model": "alternate-review", "reasoningEffort": "high"})
    }

    fn reviewer_state() -> serde_json::Value {
        json!({
            "status": "running",
            "currentStage": 2,
            "attempt": 1,
            "plan": {"version":1,"maxParallel":1,"stages":[
                {"stage":1,"agents":[{"role":"worker","name":"worker"}]},
                {"stage":2,"agents":[{"role":"reviewer","name":"reviewer","fallbackRuntime":binding()}]},
            ]},
            "submissions": []
        })
    }

    fn strict_replan_fixture() -> (serde_json::Value, serde_json::Value) {
        let state = json!({
            "status": "running",
            "currentStage": 1,
            "attempt": 1,
            "runId": "run",
            "subject": {"sha256": SHA},
            "inputsSha256": SHA,
            "events": [],
            "plan": {"version":1,"maxParallel":1,"stages":[
                {"stage":1,"agents":[{"role":"worker","name":"worker","displayName":"worker","nativeTaskName":"worker","purpose":"test","profile":"worker.md","profileSha256":SHA,"runtime":{"host":null,"model":null,"reasoningEffort":null},"declaredBoundary":{}}]}
            ]},
            "submissions": [],
            "governor": {"defaults": {"replanBinding":"assignment_packet_v1"}}
        });
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .next()
            .unwrap();
        let event = json!({
            "action":"govern", "governorEvent": {
                "action":"replan", "identityTransition":"carried_mutation_v1",
                "assignmentPacketSha256": crate::evidence::hash::value(&assignment)
            },
            "agent":"worker", "role":"worker", "stage":1, "attempt":1,
            "subjectSha256":SHA, "inputsSha256":SHA, "eventSha256":"event"
        });
        (state, event)
    }

    #[test]
    fn strict_replan_rejects_forged_input_packet_and_actor_bindings() {
        for label in ["input", "packet", "actor", "markers"] {
            let (mut state, mut event) = strict_replan_fixture();
            match label {
                "input" => event["inputsSha256"] = json!("0".repeat(64)),
                "packet" => {
                    event["governorEvent"]["assignmentPacketSha256"] = json!("0".repeat(64))
                }
                "actor" => {
                    event["agent"] = json!("reviewer");
                    event["role"] = json!("reviewer");
                }
                "markers" => {
                    event["governorEvent"]
                        .as_object_mut()
                        .unwrap()
                        .remove("identityTransition");
                    event["governorEvent"]
                        .as_object_mut()
                        .unwrap()
                        .remove("assignmentPacketSha256");
                }
                _ => unreachable!(),
            }
            assert!(
                apply_govern(&mut state, &event).is_err(),
                "{label} bypassed"
            );
        }
    }

    fn unavailable(agent: &str) -> serde_json::Value {
        json!({
            "stage":2,"attempt":1,"agent":agent,"role":"reviewer","outcome":"unavailable",
            "artifact":{"path":"a.md","sha256":SHA},"eventSha256":SHA,
            "fallback":{"reason":"provider_quota","runtime":binding(),"identitySource":"host-reported"},
        })
    }

    /// D and E, enforced in the reducer rather than the façade: once any
    /// reviewer verdict exists, an operational failure cannot re-open the
    /// review to seek a different answer.
    #[test]
    fn a_reviewer_verdict_refuses_a_later_unavailability() {
        for verdict in ["rework", "blocked"] {
            let mut state = reviewer_state();
            state["submissions"] = json!([{
                "stage":2,"attempt":1,"agent":"worker","role":"worker","outcome":"completed",
            }, {
                "stage":2,"attempt":1,"agent":"reviewer","role":"reviewer","outcome":verdict,
            }]);
            let primary = json!({"stage":2,"attempt":1,"agent":"reviewer","role":"reviewer","fallbackRuntime":binding()});
            let error = apply_unavailable(&mut state, &unavailable("reviewer"), &primary)
                .expect_err("a verdict must not be shopped away");
            assert!(
                error.contains("not admissible after a reviewer verdict"),
                "{error}"
            );
            assert_eq!(state["submissions"].as_array().unwrap().len(), 2);
        }
    }

    /// The primary's own operational failure is non-terminal and does not
    /// advance the stage, but a substitution counts as exactly one.
    #[test]
    fn primary_unavailability_is_non_terminal_and_one_substitution_is_bounded() {
        let mut state = reviewer_state();
        let primary = json!({"stage":2,"attempt":1,"agent":"reviewer","fallbackRuntime":binding()});
        apply_unavailable(&mut state, &unavailable("reviewer"), &primary).unwrap();
        assert_eq!(state["status"], "running");
        assert_eq!(state["currentStage"], 2);

        // The substitute's own failure is bounded: the run blocks rather than
        // reaching for a third target.
        let substitute = json!({"stage":2,"attempt":1,"agent":"reviewer","substitution":{"reason":"provider_quota"}});
        apply_unavailable(&mut state, &unavailable("reviewer"), &substitute).unwrap();
        assert_eq!(state["status"], "blocked");
    }

    /// Without any authorized target the run cannot be re-issued, so the
    /// operational failure blocks instead of advancing on no verdict.
    #[test]
    fn an_unauthorized_target_blocks_instead_of_advancing() {
        let mut state = json!({
            "status": "running",
            "currentStage": 2,
            "attempt": 1,
            "plan": {"version":1,"maxParallel":1,"stages":[
                {"stage":1,"agents":[{"role":"worker","name":"worker"}]},
                {"stage":2,"agents":[{"role":"reviewer","name":"reviewer"}]},
            ]},
            "submissions": []
        });
        let primary = json!({"stage":2,"attempt":1,"agent":"reviewer"});
        apply_unavailable(&mut state, &unavailable("reviewer"), &primary).unwrap();
        assert_eq!(state["status"], "blocked");
    }

    #[test]
    fn subject_chain_binds_prior_result_and_changes_for_each_worker_result() {
        let state = json!({"runId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","subject":{"version":1,"runId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","goalSha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","planSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","configSha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","attempt":0,"previousSubjectSha256":null,"workerArtifactSha256":null,"transitionSha256":null,"sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}});
        let assignment = json!({"stage":2,"attempt":1,"agent":"worker"});
        let first = subject_for_submission(
            &state,
            &assignment,
            &json!({"sha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"}),
        );
        let second_state = json!({"runId":state["runId"],"subject":first});
        let second = subject_for_submission(
            &second_state,
            &assignment,
            &json!({"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}),
        );
        assert_ne!(first["sha256"], second["sha256"]);
        assert_eq!(second["previousSubjectSha256"], first["sha256"]);
    }

    #[test]
    fn checked_start_subject_is_bound_to_the_event_run_id() {
        let run_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let other_run_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut subject = json!({
            "version": 1,
            "runId": run_id,
            "goalSha256": SHA,
            "planSha256": SHA,
            "configSha256": SHA,
            "attempt": 0,
            "previousSubjectSha256": null,
            "workerArtifactSha256": null,
            "transitionSha256": null
        });
        subject["sha256"] = json!(crate::evidence::hash::value(&subject));
        assert!(valid_subject(Some(&subject), Some(run_id)));
        assert!(!valid_subject(Some(&subject), Some(other_run_id)));
        assert_eq!(
            subject["sha256"],
            crate::evidence::hash::value(&without(&subject, "sha256"))
        );
    }

    fn submission_with_artifact(version: u64, artifact: serde_json::Value) -> serde_json::Value {
        json!({
            "version": version,
            "stage": 1,
            "attempt": 1,
            "agent": "worker",
            "role": "worker",
            "outcome": "completed",
            "artifact": artifact,
        })
    }

    #[test]
    fn v8_submissions_preserve_missing_bytes_and_validate_present_counts() {
        let base = json!({"root":"product", "path":"artifact.md", "sha256":SHA});
        assert!(validate_submission(&submission_with_artifact(7, base.clone()), 1).is_ok());
        assert!(validate_submission(&submission_with_artifact(8, base), 1).is_ok());
        assert!(validate_submission(
            &submission_with_artifact(
                8,
                json!({
                    "root":"product", "path":"artifact.md", "sha256":SHA, "bytes": 4
                })
            ),
            1
        )
        .is_ok());
        assert!(validate_submission(
            &submission_with_artifact(
                8,
                json!({
                    "root":"product", "path":"artifact.md", "sha256":SHA, "bytes": "4"
                })
            ),
            1
        )
        .is_err());
    }
}

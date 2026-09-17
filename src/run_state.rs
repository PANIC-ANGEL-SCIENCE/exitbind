use serde_json::{json, Map, Value};

const SHA_LEN: usize = 64;
const ROLES: &[&str] = &["lead", "adviser", "worker", "reviewer"];

pub fn make_event(mut value: Value) -> Value {
    let hash = crate::hash::value(&value);
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
        "protections": [], "events": events
    });
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
    for (index, event) in events.iter().enumerate().skip(1) {
        validate_event(event, events.get(index - 1), index + 1)?;
        if event["runId"] != first["runId"] {
            return Err(format!(
                "invalid run ledger line {}: runId changed",
                index + 1
            ));
        }
        apply_event(&mut state, event)?;
    }
    state["assignments"] = json!(crate::run_assignment::pending(&state));
    Ok(state)
}

pub fn validate_event(event: &Value, previous: Option<&Value>, line: usize) -> Result<(), String> {
    let object = event
        .as_object()
        .ok_or_else(|| format!("invalid run ledger line {line}: event must be an object"))?;
    let version = event["version"].as_u64();
    if !matches!(version, Some(1..=6)) || event["kind"] != "run" {
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
        || (matches!(version, Some(5 | 6)) && event["producer"]["name"] != "exitbind")
    {
        return Err(format!("invalid run ledger line {line}: invalid producer"));
    }
    let action = event["action"].as_str().unwrap_or_default();
    if !matches!(action, "start" | "submit" | "check" | "protect") {
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
    if !matches!(version, Some(3..=6)) && matches!(action, "check" | "protect") {
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
    if crate::hash::value(&without(event, "eventSha256")) != event["eventSha256"] {
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
    if !matches!(version, 1..=6) {
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
    if version >= 5 && !valid_subject(event.get("subject")) {
        return Err(format!(
            "invalid run ledger line {line}: checked start requires a valid subject"
        ));
    }
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
        if !crate::boundary_manifest::validate_evidence(boundary) {
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
            if let Some(references) = agent.get("memoryReferences") {
                crate::memory_selection::validate_references(references)
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
    if artifact.as_object().map_or(true, |x| {
        !matches!(x.len(), 2 | 3)
            || !x.contains_key("path")
            || !x.contains_key("sha256")
            || (x.len() == 3 && !x.contains_key("root"))
    }) || artifact
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
    if event["version"].as_u64() >= Some(6) && binds_inputs {
        if !is_sha(event["inputsSha256"].as_str()) {
            return Err(format!(
                "invalid run ledger line {line}: approval requires tested input identity"
            ));
        }
    } else if event.get("inputsSha256").is_some() {
        return Err(format!(
            "invalid run ledger line {line}: unexpected tested input identity"
        ));
    }
    Ok(())
}

fn apply_event(state: &mut Value, event: &Value) -> Result<(), String> {
    match event["action"].as_str() {
        Some("submit") => apply_submission(state, event),
        Some("check") => apply_check(state, event),
        Some("protect") => apply_protection(state, event),
        _ => Err("run event action is invalid".into()),
    }
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
    let candidate = crate::run_assignment::pending(state)
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
        "lead" => &["scoped", "blocked", "accepted", "rework", "rejected"][..],
        "adviser" | "worker" => &["completed", "blocked"][..],
        "reviewer" => &["approved", "rework", "blocked"][..],
        _ => &[],
    };
    if !all.contains(&event["outcome"].as_str().unwrap_or("")) {
        return Err(format!(
            "outcome '{}' is not allowed for role '{role}'",
            event["outcome"]
        ));
    }
    if role == "lead" && state["currentStage"] == 1 && !["scoped", "blocked"].contains(&outcome) {
        return Err(format!(
            "outcome '{}' is not allowed for this stage",
            event["outcome"]
        ));
    }
    if role == "lead" && outcome == "accepted" {
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
        .count();
    if completed == required && state["currentStage"] != stages.len() {
        let current = state["currentStage"]
            .as_u64()
            .ok_or("run current stage is invalid")?;
        state["currentStage"] = json!(current + 1);
    }
    Ok(())
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
        if matches!(version, 3..=6) {
            allowed.push("harnessReceipt");
            allowed.push("checkPolicy");
        }
        if version >= 5 {
            allowed.push("subject");
        }
        if version >= 6 {
            allowed.push("preservation");
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
        return reject_unknown_fields(object, &allowed, line);
    } else if action == "check" {
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
fn valid_subject(value: Option<&Value>) -> bool {
    let Some(value) = value else { return false };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == 10
        && value["version"] == 1
        && is_sha(value["runId"].as_str())
        && is_sha(value["goalSha256"].as_str())
        && is_sha(value["planSha256"].as_str())
        && is_sha(value["configSha256"].as_str())
        && value["attempt"].as_u64().is_some()
        && (value["previousSubjectSha256"].is_null()
            || is_sha(value["previousSubjectSha256"].as_str()))
        && (value["workerArtifactSha256"].is_null()
            || is_sha(value["workerArtifactSha256"].as_str()))
        && (value["transitionSha256"].is_null() || is_sha(value["transitionSha256"].as_str()))
        && is_sha(value["sha256"].as_str())
        && crate::hash::value(&without(value, "sha256")) == value["sha256"]
}

pub(crate) fn subject_for_submission(state: &Value, assignment: &Value, artifact: &Value) -> Value {
    let previous = state["subject"]["sha256"].clone();
    let artifact_sha = artifact["sha256"].clone();
    let attempt = assignment["attempt"].as_u64().unwrap_or_default();
    let transition = crate::hash::value(&json!({
        "runId": state["runId"], "previousSubjectSha256": previous, "stage": assignment["stage"], "attempt": attempt,
        "agent": assignment["agent"], "artifactSha256": artifact_sha,
    }));
    let mut subject = state["subject"].clone();
    subject["attempt"] = json!(attempt);
    subject["previousSubjectSha256"] = previous;
    subject["workerArtifactSha256"] = artifact_sha;
    subject["transitionSha256"] = json!(transition);
    let sha = crate::hash::value(&without(&subject, "sha256"));
    subject["sha256"] = json!(sha);
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
    use super::subject_for_submission;
    use serde_json::json;

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
}

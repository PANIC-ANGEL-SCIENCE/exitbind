fn validate_ledger(
    bytes: &[u8],
    shapes: &BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    expected_version: u64,
    expected_producer: &str,
    label: &str,
) -> Result<Vec<Value>, String> {
    let source =
        std::str::from_utf8(bytes).map_err(|error| format!("{label} is not UTF-8: {error}"))?;
    let events = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map_err(|error| format!("{label} has invalid JSON: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if events.is_empty() {
        return Err(format!("{label} is empty"));
    }
    let run_id = events[0]
        .get("runId")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} start has no runId"))?
        .to_owned();
    let mut previous: Option<String> = None;
    for (index, event) in events.iter().enumerate() {
        validate_event(
            event,
            shapes,
            expected_version,
            expected_producer,
            &format!("{label} event {}", index + 1),
        )?;
        if event["runId"] != run_id {
            return Err(format!("{label} changed runId"));
        }
        let event_hash = event["eventSha256"]
            .as_str()
            .ok_or_else(|| format!("{label} has no event hash"))?;
        if event_hash != sha256(canonical(&canonical_without_event_hash(event)).as_bytes()) {
            return Err(format!("{label} event {} hash mismatch", index + 1));
        }
        match previous.as_deref() {
            None if event["previousEventSha256"] != Value::Null => {
                return Err(format!("{label} does not begin a chain"))
            }
            Some(previous) if event["previousEventSha256"] != previous => {
                return Err(format!("{label} event {} chain mismatch", index + 1))
            }
            _ => {}
        }
        previous = Some(event_hash.to_owned());
    }
    Ok(events)
}

fn validate_event(
    event: &Value,
    shapes: &BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    expected_version: u64,
    expected_producer: &str,
    label: &str,
) -> Result<(), String> {
    let object = object(event, label)?;
    if event.get("version").and_then(Value::as_u64) != Some(expected_version)
        || event.get("kind") != Some(&Value::from("run"))
    {
        return Err(format!(
            "{label} is not a version {expected_version} run event"
        ));
    }
    let action = event
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} has no action"))?;
    let (required, properties) = shapes
        .get(action)
        .ok_or_else(|| format!("{label} has unknown action {action}"))?;
    for field in required {
        if !object.contains_key(field) {
            return Err(format!("{label} is missing {field}"));
        }
    }
    if object.keys().any(|field| !properties.contains(field)) {
        return Err(format!("{label} contains an unknown field"));
    }
    if !valid_hash(event["runId"].as_str().unwrap_or(""))
        || !valid_hash(event["eventSha256"].as_str().unwrap_or(""))
    {
        return Err(format!("{label} has an invalid identity hash"));
    }
    if event["producer"]["name"] != expected_producer
        || event["producer"]["version"]
            .as_str()
            .map_or(true, str::is_empty)
        || (expected_producer == "exitbind"
            && event["producer"]["version"] != env!("CARGO_PKG_VERSION"))
        || !(event["producer"]["commit"].is_null() || event["producer"]["commit"].is_string())
    {
        return Err(format!("{label} has invalid producer evidence"));
    }
    if event["timestamp"].as_str().map_or(true, str::is_empty) {
        return Err(format!("{label} has no timestamp"));
    }
    if expected_version == 8 {
        validate_v8_scenario_fields(event, label)?;
    }
    match action {
        "start" => validate_start(event, label),
        "submit" => validate_submit(event, label),
        "check" => validate_check(event, label),
        "protect" => validate_protect(event, label),
        _ => Err(format!("{label} has unsupported action")),
    }
}

fn validate_v8_scenario_fields(event: &Value, label: &str) -> Result<(), String> {
    if event["action"] == "start" {
        let expected = serde_json::json!({"version": 1, "budget": 3,
            "grantProtocol": 1, "noInformationLimit": 2, "postReplanLimit": 1,
            "replanBinding": "assignment_packet_v1"});
        if event["governor"] != expected {
            return Err(format!("{label} changed the scenario governor defaults"));
        }
    } else if (event["action"] != "submit"
        || matches!(event["role"].as_str(), Some("worker" | "reviewer"))
        || event["outcome"] == "accepted")
        && !valid_hash(event["inputsSha256"].as_str().unwrap_or(""))
    {
        return Err(format!("{label} has invalid current inputs identity"));
    }
    if event["role"] == "worker" {
        let mutation = &event["governorEvent"];
        let expected_keys = [
            "action",
            "attempt",
            "authorizationMode",
            "carryLineage",
            "checkpoint",
            "eventSha256",
            "grantEventSha256s",
            "inputSha256",
            "lineageSha256",
            "newEvidenceSha256",
            "operation",
            "previousSha256",
            "runId",
            "sourceInputsSha256s",
            "subjectSha256",
            "unit",
            "version",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
        let keys = object(mutation, label)?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if keys != expected_keys
            || mutation["action"] != "mutation"
            || mutation["version"] != 1
            || mutation["authorizationMode"] != "implicit"
            || mutation["carryLineage"] != true
            || mutation["unit"] != "worker"
            || mutation["operation"] != "completed_submission"
            || mutation["attempt"] != event["attempt"]
            || mutation["runId"] != event["runId"]
            || mutation["checkpoint"]
                .as_u64()
                .map_or(true, |value| value == 0)
            || mutation["grantEventSha256s"] != serde_json::json!([])
            || mutation["sourceInputsSha256s"] != serde_json::json!([])
            || mutation["eventSha256"]
                != sha256(canonical(&canonical_without_event_hash(mutation)).as_bytes())
        {
            return Err(format!("{label} has invalid scenario governor mutation"));
        }
        for field in [
            "inputSha256",
            "lineageSha256",
            "newEvidenceSha256",
            "subjectSha256",
        ] {
            if !valid_hash(mutation[field].as_str().unwrap_or("")) {
                return Err(format!("{label} has invalid governor {field}"));
            }
        }
        if !mutation["previousSha256"].is_null()
            && !valid_hash(mutation["previousSha256"].as_str().unwrap_or(""))
        {
            return Err(format!("{label} has invalid governor predecessor"));
        }
    }
    Ok(())
}

fn validate_start(event: &Value, label: &str) -> Result<(), String> {
    if event["previousEventSha256"] != Value::Null
        || event["workflow"].as_str().map_or(true, str::is_empty)
        || event["goal"].as_str().map_or(true, str::is_empty)
        || !valid_hash(event["configSha256"].as_str().unwrap_or(""))
    {
        return Err(format!("{label} has invalid start evidence"));
    }
    let policy = object(&event["checkPolicy"], &format!("{label} check policy"))?;
    if policy.get("version") != Some(&Value::from(1))
        || policy.get("origin") != Some(&Value::from("synthetic"))
        || policy
            .get("command")
            .and_then(Value::as_str)
            .map_or(true, str::is_empty)
        || !valid_hash(
            policy
                .get("commandSha256")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
    {
        return Err(format!("{label} has invalid synthetic check policy"));
    }
    Ok(())
}

fn validate_submit(event: &Value, label: &str) -> Result<(), String> {
    if event["stage"].as_u64().map_or(true, |value| value == 0)
        || event["attempt"].as_u64().map_or(true, |value| value == 0)
        || event["agent"].as_str().map_or(true, str::is_empty)
        || event["outcome"].as_str().map_or(true, str::is_empty)
        || !matches!(
            event["role"].as_str(),
            Some("lead" | "worker" | "reviewer" | "adviser")
        )
    {
        return Err(format!("{label} has invalid submission identity"));
    }
    let artifact = object(&event["artifact"], &format!("{label} artifact"))?;
    if !matches!(
        artifact.get("root").and_then(Value::as_str),
        Some("product" | "state")
    ) || artifact
        .get("path")
        .and_then(Value::as_str)
        .map_or(true, |path| {
            confined(path, &format!("{label} artifact")).is_err()
        })
        || !valid_hash(artifact.get("sha256").and_then(Value::as_str).unwrap_or(""))
    {
        return Err(format!("{label} has invalid artifact evidence"));
    }
    Ok(())
}

fn validate_check(event: &Value, label: &str) -> Result<(), String> {
    if matches!(event["version"].as_u64(), Some(4 | 5 | 8)) {
        if !valid_hash(event["targetEventSha256"].as_str().unwrap_or(""))
            || event["checkCommand"].as_str().map_or(true, str::is_empty)
            || !valid_hash(event["checkCommandSha256"].as_str().unwrap_or(""))
            || event["origin"] != "synthetic"
            || !matches!(event["acquisition"].as_str(), Some("reported" | "observed"))
            || event["result"].get("kind").is_none()
        {
            return Err(format!("{label} has invalid check evidence"));
        }
        return Ok(());
    }
    if !valid_hash(event["targetEventSha256"].as_str().unwrap_or(""))
        || event["checkCommand"].as_str().map_or(true, str::is_empty)
        || !valid_hash(event["checkCommandSha256"].as_str().unwrap_or(""))
        || event["origin"] != "synthetic"
        || event["exitCode"].as_u64().is_none()
    {
        return Err(format!("{label} has invalid check evidence"));
    }
    Ok(())
}

fn validate_protect(event: &Value, label: &str) -> Result<(), String> {
    if matches!(event["version"].as_u64(), Some(4 | 5 | 8)) {
        let evidence = event["checkEvidence"]
            .as_array()
            .filter(|evidence| !evidence.is_empty())
            .ok_or_else(|| format!("{label} has no check evidence"))?;
        if event["reason"] != "check_failed" || event["origin"] != "synthetic" {
            return Err(format!("{label} has invalid protection evidence"));
        }
        for item in evidence {
            if item["status"] != "failed"
                || !valid_hash(item["targetEventSha256"].as_str().unwrap_or(""))
                || !valid_hash(item["checkEventSha256"].as_str().unwrap_or(""))
                || item["result"].get("kind").is_none()
            {
                return Err(format!("{label} has invalid failed-check evidence"));
            }
        }
        return Ok(());
    }
    if event["stage"].as_u64().map_or(true, |value| value == 0)
        || event["attempt"].as_u64().map_or(true, |value| value == 0)
        || event["actor"].as_str().map_or(true, str::is_empty)
        || event["role"] != "lead"
        || event["attemptedOutcome"] != "accepted"
        || event["reason"] != "check_failed"
        || event["origin"] != "synthetic"
    {
        return Err(format!("{label} has invalid protection evidence"));
    }
    let evidence = event["checkEvidence"]
        .as_array()
        .filter(|evidence| !evidence.is_empty())
        .ok_or_else(|| format!("{label} has no check evidence"))?;
    for item in evidence {
        let item = object(item, &format!("{label} check evidence"))?;
        if item.get("status") != Some(&Value::from("failed"))
            || !valid_hash(
                item.get("targetEventSha256")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            || !valid_hash(
                item.get("checkEventSha256")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            || item.get("exitCode").and_then(Value::as_u64).is_none()
        {
            return Err(format!("{label} has invalid failed-check evidence"));
        }
    }
    Ok(())
}

fn reconstruct_workflow(
    blocked: &[Value],
    final_events: &[Value],
    config: &[u8],
    fixtures: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    if blocked.len() != 5
        || final_events.len() != 10
        || blocked
            .iter()
            .map(|event| event["action"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            != ["start", "submit", "submit", "check", "protect"]
        || final_events
            .iter()
            .map(|event| event["action"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            != [
                "start", "submit", "submit", "check", "protect", "submit", "submit", "check",
                "submit", "submit",
            ]
    {
        return Err("ledgers do not contain the fixed synthetic workflow sequence".into());
    }
    if blocked != &final_events[..blocked.len()] {
        return Err("final ledger does not preserve the blocked prefix".into());
    }
    let start = &blocked[0];
    if start["workflow"] != "change"
        || start["checkPolicy"]["origin"] != "synthetic"
        || start["configSha256"] != sha256(config)
    {
        return Err("start event is not bound to the exported synthetic fixture".into());
    }
    verify_plan_roles(start)?;
    let command = start["checkPolicy"]["command"]
        .as_str()
        .ok_or("start event has no check command")?;
    let worker_one = &blocked[2];
    let check_one = &blocked[3];
    let protection = &blocked[4];
    assert_submission(worker_one, 2, 1, "worker", "worker", "completed")?;
    assert_submission(&blocked[1], 1, 1, "lead", "lead", "scoped")?;
    assert_check(check_one, worker_one, command, 1)?;
    if protection["checkEvidence"][0]["targetEventSha256"] != worker_one["eventSha256"]
        || protection["checkEvidence"][0]["checkEventSha256"] != check_one["eventSha256"]
        || check_exit_code(&protection["checkEvidence"][0]) != Some(1)
        || has_acceptance(blocked)
    {
        return Err("failed-check protection did not leave a pending lead decision".into());
    }

    let worker_two = &final_events[6];
    let check_two = &final_events[7];
    assert_submission(&final_events[5], 4, 1, "lead", "lead", "rework")?;
    assert_submission(worker_two, 2, 2, "worker", "worker", "completed")?;
    assert_submission(&final_events[8], 3, 2, "reviewer", "reviewer", "approved")?;
    assert_submission(&final_events[9], 4, 2, "lead", "lead", "accepted")?;
    assert_check(check_two, worker_two, command, 0)?;
    if has_acceptance(&final_events[..9])
        || final_events[9]["agent"] != "lead"
        || final_events[9]["role"] != "lead"
    {
        return Err("canonical lead acceptance was not last after fresh review".into());
    }
    verify_artifacts(final_events, fixtures)?;
    let worker_one_artifact = artifact_key(worker_one)?;
    let worker_two_artifact = artifact_key(worker_two)?;
    if worker_one_artifact == worker_two_artifact
        || artifact_bytes(worker_one, fixtures)? == artifact_bytes(worker_two, fixtures)?
        || artifact_key_from(final_events, worker_one)? != worker_one_artifact
    {
        return Err("worker rework did not preserve a distinct prior artifact".into());
    }
    Ok(())
}

fn verify_plan_roles(start: &Value) -> Result<(), String> {
    let stages = start["plan"]["stages"]
        .as_array()
        .filter(|stages| stages.len() == 4)
        .ok_or("start plan does not expose four workflow stages")?;
    for (index, expected_role) in ["lead", "worker", "reviewer", "lead"]
        .into_iter()
        .enumerate()
    {
        if stages[index]["stage"] != index + 1
            || !stages[index]["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == expected_role))
        {
            return Err(format!("start plan stage {} has the wrong role", index + 1));
        }
    }
    Ok(())
}

fn assert_submission(
    event: &Value,
    stage: u64,
    attempt: u64,
    agent: &str,
    role: &str,
    outcome: &str,
) -> Result<(), String> {
    if event["stage"] != stage
        || event["attempt"] != attempt
        || event["agent"] != agent
        || event["role"] != role
        || event["outcome"] != outcome
    {
        return Err(format!("unexpected submission {agent}/{outcome}"));
    }
    Ok(())
}

fn assert_check(
    event: &Value,
    worker: &Value,
    command: &str,
    exit_code: u64,
) -> Result<(), String> {
    if event["targetEventSha256"] != worker["eventSha256"]
        || event["checkCommand"] != command
        || event["checkCommandSha256"] != sha256(command.as_bytes())
        || event["origin"] != "synthetic"
        || check_exit_code(event) != Some(exit_code)
    {
        return Err(format!("check does not bind to worker exit {exit_code}"));
    }
    Ok(())
}

fn check_exit_code(event: &Value) -> Option<u64> {
    event["exitCode"].as_u64().or_else(|| {
        (event["result"]["kind"] == "exit")
            .then(|| event["result"]["code"].as_u64())
            .flatten()
    })
}

fn verify_artifacts(events: &[Value], fixtures: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let mut artifacts = BTreeSet::new();
    for event in events {
        if event["action"] != "submit" {
            continue;
        }
        let artifact = object(&event["artifact"], "submission artifact")?;
        let root = artifact["root"].as_str().ok_or("artifact has no root")?;
        let path = artifact["path"].as_str().ok_or("artifact has no path")?;
        confined(path, "submission artifact")?;
        let bundle_path = format!("fixtures/{path}");
        let bytes = fixtures
            .get(&bundle_path)
            .ok_or_else(|| format!("bundle lacks artifact {root}:{path}"))?;
        let hash = artifact["sha256"].as_str().ok_or("artifact has no hash")?;
        if sha256(bytes) != hash || !artifacts.insert((root, path, hash)) {
            return Err(format!(
                "artifact bytes or identity mismatch for {root}:{path}"
            ));
        }
    }
    if artifacts.len() != 6 {
        return Err(format!(
            "expected six distinct protected submission artifacts, got {}",
            artifacts.len()
        ));
    }
    Ok(())
}

fn artifact_key(event: &Value) -> Result<(String, String, String), String> {
    let artifact = object(&event["artifact"], "worker artifact")?;
    Ok((
        artifact["root"].as_str().ok_or("artifact root")?.to_owned(),
        artifact["path"].as_str().ok_or("artifact path")?.to_owned(),
        artifact["sha256"]
            .as_str()
            .ok_or("artifact hash")?
            .to_owned(),
    ))
}

fn artifact_key_from(events: &[Value], worker: &Value) -> Result<(String, String, String), String> {
    let target = worker["eventSha256"].as_str().ok_or("worker hash")?;
    let event = events
        .iter()
        .find(|event| event["action"] == "submit" && event["eventSha256"] == target)
        .ok_or_else(|| "prior worker event missing from final ledger".to_owned())?;
    artifact_key(event)
}

fn artifact_bytes(event: &Value, fixtures: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let path = event["artifact"]["path"].as_str().ok_or("artifact path")?;
    fixtures
        .get(&format!("fixtures/{path}"))
        .cloned()
        .ok_or_else(|| format!("artifact fixture missing for {path}"))
}

fn has_acceptance(events: &[Value]) -> bool {
    events.iter().any(|event| {
        event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
    })
}

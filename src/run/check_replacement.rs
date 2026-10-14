//! Persisted evidence for an explicit replacement of a defective frozen check.

use serde_json::Value;

pub(crate) const PROTOCOL: u64 = 1;

pub(crate) fn frozen_plan(loaded: &crate::config::Loaded, start: &Value) -> Result<Value, String> {
    let plan = start
        .get("plan")
        .ok_or("check replacement predecessor has no frozen plan")?;
    crate::config::boundary::assert_current(loaded, plan)?;
    crate::project::architecture::assert_current(loaded)?;
    let current_architecture = loaded
        .architecture_contract
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(Value::Null);
    if plan
        .get("architectureContract")
        .cloned()
        .unwrap_or(Value::Null)
        != current_architecture
    {
        return Err(
            "check replacement architecture provenance differs from the frozen predecessor".into(),
        );
    }
    Ok(plan.clone())
}

pub(crate) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let Some(value) = event.get("checkReplacement") else {
        return Ok(());
    };
    validate_marker(value, line)?;
    let configured_lead = event["plan"]["stages"].as_array().is_some_and(|stages| {
        stages.iter().any(|stage| {
            stage["agents"].as_array().is_some_and(|agents| {
                agents.iter().any(|agent| {
                    agent["role"] == "lead"
                        && agent["name"] == value["leadOwner"]["agent"]
                        && agent["nativeTaskName"] == value["leadOwner"]["nativeTaskName"]
                })
            })
        })
    });
    let target = value["target"].as_str().unwrap_or("check");
    let target_valid = match target {
        "check" => {
            event["checkPolicy"]["commandSha256"] == value["newPolicy"]["commandSha256"]
                && event["checkPolicy"]["origin"] == value["newPolicy"]["origin"]
        }
        "pre_return" => event.get("preReturnPolicy").is_some_and(|policy| {
            crate::evidence::hash::value(policy) == value["newPolicy"]["policySha256"]
        }),
        _ => false,
    };
    if event["version"].as_u64() != Some(8)
        || event.get("supersedes").is_none()
        || !configured_lead
        || !target_valid
    {
        return Err(format!(
            "invalid run ledger line {line}: invalid check replacement"
        ));
    }
    Ok(())
}

/// A persisted claim has no plan; the actual start binds this marker to its Lead.
pub(crate) fn validate_marker(value: &Value, line: usize) -> Result<(), String> {
    let invalid = || format!("invalid run ledger line {line}: invalid check replacement");
    let object = value.as_object().ok_or_else(invalid)?;
    let target = value["target"].as_str().unwrap_or("check");
    if !matches!(target, "check" | "pre_return") {
        return Err(invalid());
    }
    let expected = [
        "protocol",
        "classification",
        "reason",
        "source",
        "leadOwner",
        "predecessorHeadEventSha256",
        "oldPolicy",
        "newPolicy",
    ];
    if object
        .keys()
        .any(|key| !expected.contains(&key.as_str()) && key != "target")
        || object.len() != expected.len() + usize::from(object.contains_key("target"))
        || value["protocol"].as_u64() != Some(PROTOCOL)
        || value["classification"] != "defective_contract"
        || !bounded_text(value["reason"].as_str(), 1024)
        || !sha(value["predecessorHeadEventSha256"].as_str())
    {
        return Err(invalid());
    }
    let source = value["source"].as_object().ok_or_else(invalid)?;
    let source_shape = if target == "pre_return" {
        source.len() == 3 && sha(value["source"]["attemptId"].as_str())
    } else {
        source.len() == 2
    };
    if !source_shape
        || !matches!(
            value["source"]["kind"].as_str(),
            Some("failed_check" | "blocked_worker")
        )
        || (target == "pre_return" && value["source"]["kind"] != "blocked_worker")
        || !sha(value["source"]["eventSha256"].as_str())
    {
        return Err(invalid());
    }
    let owner = value["leadOwner"].as_object().ok_or_else(invalid)?;
    if owner.len() != 2
        || !bounded_text(value["leadOwner"]["agent"].as_str(), 128)
        || !bounded_text(value["leadOwner"]["nativeTaskName"].as_str(), 128)
    {
        return Err(invalid());
    }
    let old = value["oldPolicy"].as_object().ok_or_else(invalid)?;
    let new = value["newPolicy"].as_object().ok_or_else(invalid)?;
    let policies_valid = if target == "pre_return" {
        old.len() == 1
            && new.len() == 1
            && sha(value["oldPolicy"]["policySha256"].as_str())
            && sha(value["newPolicy"]["policySha256"].as_str())
            && value["oldPolicy"]["policySha256"] != value["newPolicy"]["policySha256"]
    } else {
        old.len() == 2
            && new.len() == 2
            && sha(value["oldPolicy"]["commandSha256"].as_str())
            && sha(value["newPolicy"]["commandSha256"].as_str())
            && value["oldPolicy"]["commandSha256"] != value["newPolicy"]["commandSha256"]
            && matches!(
                value["oldPolicy"]["origin"].as_str(),
                Some("local_report" | "synthetic")
            )
            && matches!(
                value["newPolicy"]["origin"].as_str(),
                Some("local_report" | "synthetic")
            )
    };
    if !policies_valid {
        return Err(invalid());
    }
    Ok(())
}

fn bounded_text(value: Option<&str>, max: usize) -> bool {
    value.is_some_and(|value| {
        !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
    })
}

fn sha(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

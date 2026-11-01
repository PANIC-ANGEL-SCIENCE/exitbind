//! The managed result contract and Work return applied to a host message.

use crate::config::Loaded;
use serde_json::{json, Value};

/// Present the same typed result and evidence contract as managed execution.
pub(crate) fn host_result_contract(
    loaded: &Loaded,
    work: &str,
    current: &Value,
) -> Result<Value, String> {
    let role = current["role"]
        .as_str()
        .ok_or("current assignment has no role")?;
    let evidence = super::verified_worker_results(loaded, work, role, &current["packet"])?;
    let schema: Value = serde_json::from_str(super::role_schema(role)?)
        .map_err(|_| "native role schema is invalid")?;
    Ok(
        json!({"schema":schema, "textGuidance":super::result::TEXT_GUIDANCE,
        "requiredEvidenceReferences":evidence.map(|items| items.into_iter().map(|item| item.reference).collect::<Vec<_>>())}),
    )
}

/// Validate the host result and its current reviewer evidence without effects.
pub(crate) fn validate_host_result(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    message: &str,
) -> Result<(), String> {
    let role = current["role"]
        .as_str()
        .ok_or("current assignment has no role")?;
    let value: Value =
        serde_json::from_str(message).map_err(|_| "host result is not typed JSON")?;
    super::validate_final_result(role, &value)?;
    if let Some(evidence) = super::verified_worker_results(loaded, work, role, &current["packet"])?
    {
        let expected = evidence
            .iter()
            .map(|item| json!(item.reference))
            .collect::<Vec<_>>();
        if value["evidenceReferences"] != json!(expected) {
            return Err("host reviewer did not cite every verified worker result reference".into());
        }
    }
    Ok(())
}

/// Revalidate and submit an exact host final message through the Work owner.
pub(crate) fn return_host_result(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    message: &str,
) -> Result<Value, String> {
    validate_host_result(loaded, work, current, message)?;
    let assignment = current["assignment"]
        .as_str()
        .ok_or("current assignment is unavailable")?;
    let binding = current["current"]["binding"]
        .as_str()
        .ok_or("current binding is unavailable")?;
    let value: Value =
        serde_json::from_str(message).map_err(|_| "host result is not typed JSON")?;
    let outcome = value["outcome"]
        .as_str()
        .ok_or("host result outcome unavailable")?;
    crate::work::return_result_impl::return_result_with(
        loaded,
        work,
        assignment,
        outcome,
        value["reason"].as_str(),
        None,
        None,
        Some(message.as_bytes().to_vec()),
        Some(binding),
    )
}

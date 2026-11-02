//! The managed result contract and Work return applied to a host message.

use crate::config::Loaded;
use serde_json::{json, Value};

fn only_keys(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| allowed.contains(&key.as_str())))
}

/// Select only the existing required result fields for a native actor. The
/// canonical schema and the validator remain the authorities for this view.
pub(crate) fn actor_contract(role: &str, contract: &Value) -> Result<(String, Value), String> {
    let schema = &contract["schema"];
    let properties = schema["properties"]
        .as_object()
        .ok_or("native result properties are unavailable")?;
    let required = schema["required"]
        .as_array()
        .ok_or("native result required fields are unavailable")?;
    if schema["type"] != "object"
        || schema["additionalProperties"] != false
        || !only_keys(
            schema,
            &["type", "additionalProperties", "required", "properties"],
        )
    {
        return Err("native result object contract is unavailable".into());
    }
    let required_names = required
        .iter()
        .map(|value| value.as_str().ok_or("native result field is malformed"))
        .collect::<Result<Vec<_>, _>>()?;
    if !["outcome", "summary", "reason"]
        .iter()
        .all(|name| required_names.contains(name))
        || required_names.iter().any(|name| {
            !matches!(
                *name,
                "outcome" | "summary" | "reason" | "evidenceReferences"
            )
        })
    {
        return Err("native result requires an unsupported actor field".into());
    }
    let summary = properties
        .get("summary")
        .ok_or("native result summary is unavailable")?;
    if summary["type"] != "string"
        || summary["minLength"] != 1
        || summary["maxLength"].as_u64() != Some(super::result::MAX_TEXT_BYTES as u64)
        || !only_keys(summary, &["type", "minLength", "maxLength"])
    {
        return Err("native result summary constraints changed".into());
    }
    let outcome_property = properties
        .get("outcome")
        .ok_or("native result outcome is unavailable")?;
    if !only_keys(outcome_property, &["enum"]) {
        return Err("native result outcome constraints changed".into());
    }
    let outcomes = outcome_property["enum"]
        .as_array()
        .ok_or("native result outcomes are unavailable")?;
    let contract_role = crate::kernel::result_contract::Role::parse(role)
        .ok_or("native result role is unavailable")?;
    let expected = contract_role
        .outcomes()
        .iter()
        .map(|outcome| json!(outcome.as_str()))
        .collect::<Vec<_>>();
    if outcomes.is_empty() || *outcomes != expected {
        return Err("native result outcomes differ from the current role".into());
    }
    let reviewer = required_names.contains(&"evidenceReferences");
    if required_names.len() != if reviewer { 4 } else { 3 } {
        return Err("native result required fields are ambiguous".into());
    }
    let reason_property = properties
        .get("reason")
        .ok_or("native result reason is unavailable")?;
    let mut reason_map = serde_json::Map::new();
    if reviewer {
        let references = properties
            .get("evidenceReferences")
            .ok_or("native reviewer references are unavailable")?;
        if references["type"] != "array"
            || references["items"]["type"] != "string"
            || !only_keys(references, &["type", "items"])
            || !only_keys(&references["items"], &["type"])
            || !only_keys(reason_property, &["enum"])
        {
            return Err("native reviewer constraints changed".into());
        }
        for outcome in outcomes {
            let name = outcome
                .as_str()
                .ok_or("native result outcome is malformed")?;
            let reasons = super::result::reviewer_reasons(name)
                .ok_or("native reviewer reason contract is unavailable")?;
            reason_map.insert(name.to_owned(), json!(reasons));
        }
        let actual = reason_property["enum"]
            .as_array()
            .ok_or("native reviewer reasons are unavailable")?;
        let mut expected_reasons = Vec::new();
        for outcome in outcomes {
            for reason in super::result::reviewer_reasons(outcome.as_str().unwrap()).unwrap() {
                if !expected_reasons.contains(&json!(reason)) {
                    expected_reasons.push(json!(reason));
                }
            }
        }
        if *actual != expected_reasons {
            return Err("native reviewer reasons differ from the validator".into());
        }
    } else if reason_property["type"] != "string"
        || reason_property["maxLength"].as_u64() != Some(super::result::MAX_TEXT_BYTES as u64)
        || !only_keys(reason_property, &["type", "maxLength"])
    {
        return Err("native result reason constraints changed".into());
    }
    let first = outcomes[0]
        .as_str()
        .ok_or("native result outcome is malformed")?;
    let first_reason = if reviewer {
        super::result::reviewer_reasons(first)
            .and_then(|reasons| reasons.first())
            .copied()
            .ok_or("native reviewer reason is unavailable")?
    } else {
        ""
    };
    // Validate a complete exemplar against the canonical validator, then expose
    // an incomplete scaffold that cannot itself be submitted as a result.
    let mut template =
        json!({"outcome":first,"summary":"contract validation","reason":first_reason});
    if reviewer {
        template["evidenceReferences"] = contract["requiredEvidenceReferences"].clone();
    } else if !contract["requiredEvidenceReferences"].is_null() {
        return Err("native result evidence references are unexpected".into());
    }
    let guidance = contract["textGuidance"]
        .as_str()
        .filter(|guidance| *guidance == super::result::TEXT_GUIDANCE)
        .ok_or("native result text guidance is unavailable")?;
    super::result::validate_final_result(role, &template)?;
    template["outcome"] = json!("");
    template["summary"] = json!("");
    let mut text = format!(
        "Automatic Work return. ENTIRE final message: this JSON object only; no prose, fence or extra fields. Template: {template}. Fill actual outcome, summary and reason. Outcomes: {}. {guidance}",
        serde_json::to_string(outcomes).map_err(|error| error.to_string())?,
    );
    if reviewer {
        text.push_str(&format!(
            " Required evidenceReferences are exactly the template values in order. Allowed reason by outcome: {}.",
            Value::Object(reason_map)
        ));
    }
    Ok((text, template))
}

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
    let value = parse_result(role, message)?;
    if let Some(evidence) = super::verified_worker_results(loaded, work, role, &current["packet"])?
    {
        let expected = evidence
            .iter()
            .map(|item| json!(item.reference))
            .collect::<Vec<_>>();
        validate_references(&value, &expected)?;
    }
    Ok(())
}

fn parse_result(role: &str, message: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(message).map_err(|_| "host result is not typed JSON")?;
    super::validate_final_result(role, &value)?;
    Ok(value)
}

fn validate_references(value: &Value, expected: &[Value]) -> Result<(), String> {
    if value["evidenceReferences"] != json!(expected) {
        return Err("host reviewer did not cite every verified worker result reference".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(role: &str) -> Value {
        let schema: Value = serde_json::from_str(super::super::role_schema(role).unwrap()).unwrap();
        json!({"schema":schema,"textGuidance":super::super::result::TEXT_GUIDANCE,
            "requiredEvidenceReferences":if role == "reviewer" {
                json!(["ref:first", "ref:second"])
            } else { Value::Null }})
    }

    #[test]
    fn selected_templates_match_the_single_validator_for_every_role_and_outcome() {
        for role in ["worker", "reviewer", "adviser"] {
            let canonical = canonical(role);
            let (text, template) = actor_contract(role, &canonical).unwrap();
            assert!(text.contains("ENTIRE final message"));
            assert!(text.contains(&template.to_string()));
            assert!(text.contains(super::super::result::TEXT_GUIDANCE));
            assert_eq!(template["outcome"], "");
            assert_eq!(template["summary"], "");
            assert!(parse_result(role, &template.to_string()).is_err());
            let mut summary_only = template.clone();
            summary_only["summary"] = json!("Actual result");
            assert!(parse_result(role, &summary_only.to_string()).is_err());
            for outcome in canonical["schema"]["properties"]["outcome"]["enum"]
                .as_array()
                .unwrap()
            {
                let name = outcome.as_str().unwrap();
                let reasons = if role == "reviewer" {
                    super::super::result::reviewer_reasons(name).unwrap()
                } else {
                    &["", "Actual reason"][..]
                };
                assert!(text.contains(&outcome.to_string()));
                let mut outcome_only = template.clone();
                outcome_only["outcome"] = outcome.clone();
                assert!(parse_result(role, &outcome_only.to_string()).is_err());
                for reason in reasons {
                    let mut selected = template.clone();
                    selected["outcome"] = outcome.clone();
                    selected["summary"] = json!("Actual result");
                    selected["reason"] = json!(reason);
                    assert!(parse_result(role, &selected.to_string()).is_ok());
                    if role == "reviewer" {
                        let expected = vec![json!("ref:first"), json!("ref:second")];
                        assert!(validate_references(&selected, &expected).is_ok());
                    }
                }
                if role == "reviewer" {
                    for reason in canonical["schema"]["properties"]["reason"]["enum"]
                        .as_array()
                        .unwrap()
                    {
                        let mut selected = template.clone();
                        selected["outcome"] = outcome.clone();
                        selected["summary"] = json!("Actual result");
                        selected["reason"] = reason.clone();
                        assert_eq!(
                            parse_result(role, &selected.to_string()).is_ok(),
                            reasons.contains(&reason.as_str().unwrap()),
                            "outcome {name}, reason {reason}"
                        );
                    }
                }
            }
            let mut completed = template.clone();
            completed["outcome"] = canonical["schema"]["properties"]["outcome"]["enum"][0].clone();
            completed["summary"] = json!("Actual result");
            assert!(parse_result(role, &completed.to_string()).is_ok());
            let mut missing = completed.clone();
            missing.as_object_mut().unwrap().remove("reason");
            assert!(parse_result(role, &missing.to_string()).is_err());
            let mut extra = completed.clone();
            extra["extra"] = json!(true);
            assert!(parse_result(role, &extra.to_string()).is_err());
            let mut invalid = completed.clone();
            invalid["outcome"] = json!("not-an-outcome");
            assert!(parse_result(role, &invalid.to_string()).is_err());
            assert!(parse_result(role, &format!("prose\n{completed}")).is_err());
        }
        let (_, mut reviewer) = actor_contract("reviewer", &canonical("reviewer")).unwrap();
        reviewer["outcome"] = json!("approved");
        reviewer["summary"] = json!("Reviewed current evidence");
        assert!(parse_result("reviewer", &reviewer.to_string()).is_ok());
        assert!(parse_result("worker", &reviewer.to_string()).is_err());
        let expected = vec![json!("ref:first"), json!("ref:second")];
        assert!(validate_references(&reviewer, &expected).is_ok());
        let mut missing_reference = reviewer.clone();
        missing_reference["evidenceReferences"] = json!(["ref:first"]);
        assert!(parse_result("reviewer", &missing_reference.to_string()).is_ok());
        assert!(validate_references(&missing_reference, &expected).is_err());
        let mut reversed = reviewer;
        reversed["evidenceReferences"] = json!(["ref:second", "ref:first"]);
        assert!(parse_result("reviewer", &reversed.to_string()).is_ok());
        assert!(validate_references(&reversed, &expected).is_err());
    }

    #[test]
    fn selected_contract_refuses_unknown_required_constraints() {
        let mut changed = canonical("worker");
        changed["schema"]["properties"]["summary"]["pattern"] = json!("unhandled");
        assert!(actor_contract("worker", &changed).is_err());
        changed = canonical("worker");
        changed["schema"]["properties"]["summary"]["description"] =
            json!("an additional instruction");
        assert!(actor_contract("worker", &changed).is_err());
        changed = canonical("worker");
        changed["schema"]["required"]
            .as_array_mut()
            .unwrap()
            .push(json!("unhandled"));
        assert!(actor_contract("worker", &changed).is_err());
    }
}

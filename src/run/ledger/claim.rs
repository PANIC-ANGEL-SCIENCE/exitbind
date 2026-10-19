//! Exact validation for durable supersession claims.

use serde_json::Value;

pub(super) fn validate_claim(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("invalid supersession claim")?;
    let legacy_fields = [
        "version",
        "oldLedgerPath",
        "oldLedgerSha256",
        "oldRunId",
        "oldHeadEventSha256",
        "oldConfigSha256",
        "newLedgerPath",
        "workflow",
        "goalSha256",
        "configSha256",
        "newRunId",
        "timestamp",
    ];
    let carry_fields = ["carryProtocol", "governorCarrySha256"];
    let preparation_fields = [
        "verificationPreparationProtocol",
        "verificationCheckerSources",
    ];
    let has_replacement = value.get("checkReplacement").is_some();
    let has_pre_return = value.get("preReturnPolicy").is_some();
    let has_input_contract = value
        .get(crate::run::guidance_continuity::INPUT_CONTRACT_FIELD)
        .is_some();
    let has_carry =
        value.get("carryProtocol").is_some() || value.get("governorCarrySha256").is_some();
    let has_preparation = preparation_fields
        .iter()
        .any(|key| value.get(*key).is_some());
    let expected_len = legacy_fields.len()
        + usize::from(has_carry) * carry_fields.len()
        + usize::from(has_replacement)
        + usize::from(has_pre_return)
        + usize::from(has_input_contract)
        + usize::from(has_preparation) * preparation_fields.len();
    if object.len() != expected_len
        || legacy_fields.iter().any(|key| !object.contains_key(*key))
        || (has_carry
            && (carry_fields.iter().any(|key| !object.contains_key(*key))
                || (value["carryProtocol"] != crate::run::carry::PROTOCOL
                    && value["carryProtocol"] != crate::run::carry::RECOVERY_PROTOCOL)
                || !super::sha(value["governorCarrySha256"].as_str())))
        || (has_replacement
            && crate::run::check_replacement::validate_marker(&value["checkReplacement"], 1)
                .is_err())
        || (has_pre_return
            && crate::run_value::pre_return_policy_from_value(&value["preReturnPolicy"], 1)
                .is_err())
        || (has_input_contract && {
            let mut errors = Vec::new();
            crate::run::guidance_continuity::validate_input_contract(
                &value[crate::run::guidance_continuity::INPUT_CONTRACT_FIELD],
                &mut errors,
            );
            !errors.is_empty()
        })
        || (has_preparation
            && (preparation_fields
                .iter()
                .any(|key| !object.contains_key(*key))
                || value["verificationPreparationProtocol"].as_u64()
                    != Some(crate::run::verification_preparation::PROTOCOL)
                || crate::run_value::checker_sources_from_value(
                    &value["verificationCheckerSources"],
                    1,
                )
                .is_err()))
        || value["version"] != 1
        || [
            "oldLedgerSha256",
            "oldRunId",
            "oldHeadEventSha256",
            "oldConfigSha256",
            "goalSha256",
            "configSha256",
            "newRunId",
        ]
        .iter()
        .any(|key| !super::sha(value[key].as_str()))
        || chrono::DateTime::parse_from_rfc3339(value["timestamp"].as_str().unwrap_or("")).is_err()
    {
        return Err("invalid supersession claim".into());
    }
    Ok(())
}

//! Canonical event fields for the optional verification preparation protocol.

use serde_json::{json, Value};

pub(crate) const EVENT_FIELDS: &[&str] = &[
    "verificationPreparationProtocol",
    "verificationCheckerSources",
];

pub(crate) fn add_start_fields(
    event: &mut Value,
    checker_sources: Option<&[crate::run_value::CheckerSource]>,
) {
    if let Some(sources) = checker_sources {
        event["verificationPreparationProtocol"] = json!(super::PROTOCOL);
        event["verificationCheckerSources"] = crate::run_value::checker_sources_value(sources);
    }
}

pub(crate) fn append_allowed_fields(allowed: &mut Vec<&'static str>) {
    allowed.extend(EVENT_FIELDS.iter().copied());
}

pub(crate) fn validate_start_fields(
    event: &Value,
    version: u64,
    line: usize,
) -> Result<(), String> {
    if let Some(marker) = event.get("verificationPreparationProtocol") {
        if version != 8
            || marker.as_u64() != Some(super::PROTOCOL)
            || event.get("checkPolicy").is_none()
            || event.get("verificationCheckerSources").is_none()
        {
            return Err(format!("invalid run ledger line {line}: verification preparation requires a checked v8 start and supported protocol"));
        }
        crate::run_value::checker_sources_from_value(&event["verificationCheckerSources"], line)?;
    } else if event.get("verificationCheckerSources").is_some() {
        return Err(format!("invalid run ledger line {line}: verification checker sources require selected preparation"));
    }
    Ok(())
}

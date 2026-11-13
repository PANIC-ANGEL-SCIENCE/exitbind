// Retains a bounded check-log route in compact Work output.
// Requires a check-log reference with a bounded tail; see `src/work/compact.rs`.
//! Retain a bounded check-log route when large compact projections fall back.

use serde_json::{json, Value};
use std::path::Path;

pub(super) fn preferred(response: &Value, config: &Path) -> Option<Value> {
    let evidence = response
        .pointer("/residual/context/evidence")
        .or_else(|| response.pointer("/next/packet/context/evidence"))?
        .as_array()?;
    let item = evidence
        .iter()
        .find(|item| item["status"] == "failed" && item["checkLogRef"]["id"].is_string())
        .or_else(|| {
            evidence
                .iter()
                .rev()
                .find(|item| item["checkLogRef"]["id"].is_string())
        })?;
    let mut reference = item["checkLogRef"].clone();
    let work = response["work"].as_str()?;
    super::super::expansion::attach_compact_log_routes(config, work, &mut reference);
    let wanted = reference["id"].as_str()?;
    if reference["kind"] != "check_log" || !reference["boundedTail"].is_object() {
        return None;
    }
    Some(json!({"kind":"check_log","id":wanted,"boundedTail":reference["boundedTail"]}))
}

pub(super) fn retain_if_fits(
    output: &mut Value,
    reference: Option<&Value>,
    config: &Path,
) -> Result<(), String> {
    let Some(reference) = reference else {
        return Ok(());
    };
    let mut reference = reference.clone();
    for shorten in [false, true] {
        if shorten {
            if let Some(routes) = reference["boundedTail"].as_object_mut() {
                for route in routes.values_mut() {
                    super::rebudget_route(route, "command", config);
                }
            }
        }
        let mut candidate = output.clone();
        candidate["references"] = json!([reference]);
        if super::serialized_len(&candidate)? + 1 > super::MAX_RESPONSE_BYTES {
            if let Some(object) = candidate.as_object_mut() {
                object.remove("producer");
            }
        }
        if super::serialized_len(&candidate)? < super::MAX_RESPONSE_BYTES {
            *output = candidate;
            return Ok(());
        }
    }
    // Keep the existing recovery route if the bounded envelope cannot
    // also carry a log route; never overflow or hide that omission.
    let mut candidate = output.clone();
    candidate["logTailUnavailable"] = json!({"reason":"response_budget","detail":"fullCommand"});
    if super::serialized_len(&candidate)? < super::MAX_RESPONSE_BYTES {
        *output = candidate;
    }
    Ok(())
}

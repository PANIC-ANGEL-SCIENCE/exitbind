//! Typed opt-in basis, review choice, and Lead disposition records.
//!
//! These values are persisted in the governed event stream.  The reducer owns
//! lifecycle and currentness; this module owns shape, identity, and the small
//! set of categories the accepted protocol permits.

use crate::evidence::hash;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub(crate) const PROTOCOL_VERSION: u64 = 1;

const SHA_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Basis {
    pub(crate) version: u64,
    pub(crate) constraints: Vec<String>,
    pub(crate) open_zones: Vec<String>,
    pub(crate) decisive_cases: Vec<String>,
    pub(crate) boundary_sha256: Option<String>,
    pub(crate) preservation_sha256: Option<String>,
    pub(crate) sha256: String,
}

impl Basis {
    pub(crate) fn value(&self) -> Value {
        let mut value = json!({
            "version": self.version,
            "constraints": self.constraints,
            "openZones": self.open_zones,
            "decisiveCases": self.decisive_cases,
        });
        if let Some(sha256) = &self.boundary_sha256 {
            value["boundarySha256"] = json!(sha256);
        }
        if let Some(sha256) = &self.preservation_sha256 {
            value["preservationSha256"] = json!(sha256);
        }
        value["sha256"] = json!(self.sha256);
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReviewDecision {
    pub(crate) version: u64,
    pub(crate) decision: String,
    pub(crate) source: String,
    pub(crate) reason: String,
    pub(crate) previous_sha256: Option<String>,
    pub(crate) sha256: String,
}

impl ReviewDecision {
    pub(crate) fn value(&self) -> Value {
        let mut value = json!({
            "version": self.version,
            "decision": self.decision,
            "source": self.source,
            "reason": self.reason,
        });
        value["previousSha256"] = self
            .previous_sha256
            .as_ref()
            .map_or(Value::Null, |sha| json!(sha));
        value["sha256"] = json!(self.sha256);
        value
    }
}

/// A successor basis may clarify or narrow the accepted meaning, but existing
/// constraints and bound provenance must be preserved while open zones narrow.
pub(crate) fn validate_successor(current: &Basis, successor: &Basis) -> Result<(), String> {
    if !current
        .constraints
        .iter()
        .all(|item| successor.constraints.contains(item))
    {
        return Err("successor basis must preserve every current constraint".into());
    }
    if !current
        .decisive_cases
        .iter()
        .all(|item| successor.decisive_cases.contains(item))
    {
        return Err("successor basis must preserve every current decisive case".into());
    }
    if !successor
        .open_zones
        .iter()
        .all(|item| current.open_zones.contains(item))
    {
        return Err("successor basis may only narrow current open zones".into());
    }
    if current.boundary_sha256 != successor.boundary_sha256 {
        return Err("successor basis must preserve the current boundary".into());
    }
    if current.preservation_sha256 != successor.preservation_sha256 {
        return Err("successor basis must preserve the current preservation claim".into());
    }
    Ok(())
}

pub(crate) fn parse_basis(value: &Value, label: &str) -> Result<Basis, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    reject_unknown(
        object,
        &[
            "version",
            "constraints",
            "openZones",
            "decisiveCases",
            "boundarySha256",
            "preservationSha256",
            "sha256",
        ],
        label,
    )?;
    if object.get("version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(format!("{label} has an unsupported version"));
    }
    let constraints = strings(object.get("constraints"), "constraints", label, true)?;
    let open_zones = strings(object.get("openZones"), "openZones", label, false)?;
    let decisive_cases = strings(object.get("decisiveCases"), "decisiveCases", label, true)?;
    let boundary_sha256 = optional_sha(object.get("boundarySha256"), "boundarySha256", label)?;
    let preservation_sha256 = optional_sha(
        object.get("preservationSha256"),
        "preservationSha256",
        label,
    )?;
    let mut basis = Basis {
        version: PROTOCOL_VERSION,
        constraints,
        open_zones,
        decisive_cases,
        boundary_sha256,
        preservation_sha256,
        sha256: String::new(),
    };
    let expected = hash::value(&basis_without_hash(&basis));
    if let Some(actual) = object.get("sha256").and_then(Value::as_str) {
        if actual != expected {
            return Err(format!("{label} hash mismatch"));
        }
    } else if object.contains_key("sha256") {
        return Err(format!("{label} hash is invalid"));
    }
    basis.sha256 = expected;
    Ok(basis)
}

pub(crate) fn parse_basis_text(text: &str, label: &str) -> Result<Basis, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| format!("{label}: {error}"))?;
    parse_basis(&value, label)
}

pub(crate) fn parse_review(value: &Value, label: &str) -> Result<ReviewDecision, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    reject_unknown(
        object,
        &[
            "version",
            "decision",
            "source",
            "reason",
            "previousSha256",
            "sha256",
        ],
        label,
    )?;
    if object.get("version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION)
        || !matches!(
            object.get("decision").and_then(Value::as_str),
            Some("required" | "omitted")
        )
        || object.get("source").and_then(Value::as_str) != Some("owner-reported")
    {
        return Err(format!("{label} is invalid"));
    }
    let reason = text(object.get("reason"), "reason", label, 512)?;
    let previous_sha256 = optional_sha(object.get("previousSha256"), "previousSha256", label)?;
    let mut review = ReviewDecision {
        version: PROTOCOL_VERSION,
        decision: object
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        source: "owner-reported".to_owned(),
        reason,
        previous_sha256,
        sha256: String::new(),
    };
    let expected = hash::value(&review_without_hash(&review));
    if object.get("sha256").and_then(Value::as_str) != Some(expected.as_str()) {
        return Err(format!("{label} hash is missing or invalid"));
    }
    review.sha256 = expected;
    Ok(review)
}

pub(crate) fn review_value(decision: &str, reason: &str) -> Result<ReviewDecision, String> {
    review_value_with_previous(decision, reason, None)
}

pub(crate) fn review_value_with_previous(
    decision: &str,
    reason: &str,
    previous_sha256: Option<&str>,
) -> Result<ReviewDecision, String> {
    if !matches!(decision, "required" | "omitted") {
        return Err("review decision must be required or omitted".into());
    }
    if reason.trim().is_empty() || reason.len() > 512 || reason.contains('\0') {
        return Err("review decision reason must be non-empty and <=512 bytes".into());
    }
    let mut review = ReviewDecision {
        version: PROTOCOL_VERSION,
        decision: decision.to_owned(),
        source: "owner-reported".to_owned(),
        reason: reason.to_owned(),
        previous_sha256: previous_sha256.map(str::to_owned),
        sha256: String::new(),
    };
    review.sha256 = hash::value(&review_without_hash(&review));
    Ok(review)
}

fn basis_without_hash(basis: &Basis) -> Value {
    let mut value = basis.value();
    value.as_object_mut().unwrap().remove("sha256");
    value
}

fn review_without_hash(review: &ReviewDecision) -> Value {
    let mut value = review.value();
    value.as_object_mut().unwrap().remove("sha256");
    value
}

pub(super) fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    label: &str,
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{label} has unknown field '{key}'"));
    }
    Ok(())
}

pub(super) fn text(
    value: Option<&Value>,
    field: &str,
    label: &str,
    max: usize,
) -> Result<String, String> {
    let value = value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= max && !value.contains('\0'))
        .ok_or_else(|| format!("{label} {field} is invalid"))?;
    Ok(value.to_owned())
}

pub(super) fn strings(
    value: Option<&Value>,
    field: &str,
    label: &str,
    nonempty: bool,
) -> Result<Vec<String>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} {field} is invalid"))?;
    if nonempty && values.is_empty() {
        return Err(format!("{label} {field} must not be empty"));
    }
    let mut result = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for value in values {
        let value = value
            .as_str()
            .filter(|value| {
                !value.trim().is_empty() && value.len() <= 1024 && !value.contains('\0')
            })
            .ok_or_else(|| format!("{label} {field} contains an invalid item"))?;
        if !seen.insert(value) {
            return Err(format!("{label} {field} contains a duplicate item"));
        }
        result.push(value.to_owned());
    }
    Ok(result)
}

pub(super) fn sha_array(
    value: Option<&Value>,
    field: &str,
    label: &str,
    nonempty: bool,
) -> Result<Vec<String>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} {field} is invalid"))?;
    if nonempty && values.is_empty() {
        return Err(format!("{label} {field} must not be empty"));
    }
    let mut result = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for value in values {
        let value = required_sha(Some(value), field, label)?;
        if !seen.insert(value.clone()) {
            return Err(format!("{label} {field} contains a duplicate item"));
        }
        result.push(value);
    }
    Ok(result)
}

fn required_sha(value: Option<&Value>, field: &str, label: &str) -> Result<String, String> {
    let value = value
        .and_then(Value::as_str)
        .filter(|value| is_sha(value))
        .ok_or_else(|| format!("{label} {field} is invalid"))?;
    Ok(value.to_owned())
}

pub(super) fn optional_sha(
    value: Option<&Value>,
    field: &str,
    label: &str,
) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => required_sha(Some(value), field, label).map(Some),
    }
}

fn is_sha(value: &str) -> bool {
    value.len() == SHA_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn basis_and_review_records_are_canonical_and_strict() {
        let basis = parse_basis(
            &json!({
                "version": 1,
                "constraints": ["preserve history"],
                "openZones": ["worker implementation"],
                "decisiveCases": ["contradiction routes to Lead"]
            }),
            "basis",
        )
        .unwrap();
        assert_eq!(basis.sha256.len(), SHA_LEN);
        assert!(parse_basis(
            &json!({"version":1,"constraints":[],"openZones":[],"decisiveCases":[],"extra":true}),
            "basis"
        )
        .is_err());
        assert!(parse_basis(&json!({"version": 1}), "basis").is_err());
        assert!(parse_basis(
            &json!({"version":null,"constraints":[],"openZones":[],"decisiveCases":[]}),
            "basis"
        )
        .is_err());
        assert!(parse_basis(
            &json!({"version":"1","constraints":[],"openZones":[],"decisiveCases":[]}),
            "basis"
        )
        .is_err());
        assert!(parse_review(&json!({"version": 1}), "review").is_err());
        assert!(parse_review(
            &json!({"version":1,"decision":null,"source":"owner-reported","reason":"x"}),
            "review"
        )
        .is_err());
        assert!(parse_review(
            &json!({"version":1,"decision":"required","source":null,"reason":"x"}),
            "review"
        )
        .is_err());

        let required = review_value("required", "owner choice").unwrap();
        let mut value = required.value();
        value["decision"] = json!("omitted");
        assert!(parse_review(&value, "review").is_err());
        let revised =
            review_value_with_previous("omitted", "owner choice", Some(&required.sha256)).unwrap();
        assert_eq!(
            revised.previous_sha256.as_deref(),
            Some(required.sha256.as_str())
        );
    }
}

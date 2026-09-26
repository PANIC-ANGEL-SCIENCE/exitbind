//! Lead disposition records: the Lead's decision on a pending finding cycle.
//!
//! A protocol-v1 record carries no `decision` and always starts another
//! attempt; its shape and hash are unchanged so historical ledgers replay
//! exactly.  A decision record names what the Lead authorized: `repair` and
//! `supersede` start a bounded attempt, while `defer` and `reject` keep the
//! finding as evidence and start nothing.

use super::basis::{optional_sha, parse_basis, reject_unknown, sha_array, strings, text, Basis};
use crate::evidence::hash;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Repair,
    Defer,
    Reject,
    Supersede,
}

impl Decision {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "repair" => Some(Self::Repair),
            "defer" => Some(Self::Defer),
            "reject" => Some(Self::Reject),
            "supersede" => Some(Self::Supersede),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Repair => "repair",
            Self::Defer => "defer",
            Self::Reject => "reject",
            Self::Supersede => "supersede",
        }
    }

    /// `defer` and `reject` close the review cycle without another attempt.
    pub(crate) fn resolves_review(self) -> bool {
        matches!(self, Self::Defer | Self::Reject)
    }
}

/// The Lead-authored repair terms of a record that starts another attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Repair {
    pub(crate) category: String,
    pub(crate) causal_assumption: String,
    pub(crate) affected_paths: Vec<String>,
    pub(crate) repair_boundary: String,
    pub(crate) decisive_regression: String,
    pub(crate) invalidated_evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Disposition {
    /// `None` for a protocol-v1 record.
    pub(crate) decision: Option<Decision>,
    pub(crate) reason: Option<String>,
    pub(crate) finding_sha256s: Vec<String>,
    pub(crate) basis_sha256: Option<String>,
    /// Absent exactly for `defer` and `reject`.
    pub(crate) repair: Option<Repair>,
    pub(crate) successor_basis: Option<Basis>,
    pub(crate) sha256: String,
}

impl Disposition {
    pub(crate) fn resolves_review(&self) -> bool {
        self.decision.is_some_and(Decision::resolves_review)
    }

    pub(crate) fn value(&self) -> Value {
        let mut value = match &self.repair {
            Some(repair) => json!({
                "category": repair.category,
                "findingSha256s": self.finding_sha256s,
                "basisSha256": self.basis_sha256,
                "causalAssumption": repair.causal_assumption,
                "affectedPaths": repair.affected_paths,
                "repairBoundary": repair.repair_boundary,
                "decisiveRegression": repair.decisive_regression,
                "invalidatedEvidence": repair.invalidated_evidence,
                "successorBasis": self.successor_basis.as_ref().map_or(Value::Null, Basis::value),
            }),
            None => json!({
                "findingSha256s": self.finding_sha256s,
                "basisSha256": self.basis_sha256,
            }),
        };
        if let Some(decision) = self.decision {
            value["decision"] = json!(decision.as_str());
            value["reason"] = json!(self.reason);
        }
        value["sha256"] = json!(self.sha256);
        value
    }
}

/// Lead-supplied terms for a decision record.  Exitbind fills the finding
/// cycle and basis identity; the Lead never composes hashes.
pub(crate) struct Terms<'a> {
    pub(crate) decision: Decision,
    pub(crate) reason: &'a str,
    pub(crate) repair_boundary: Option<&'a str>,
    pub(crate) decisive_regression: Option<&'a str>,
    pub(crate) category: Option<&'a str>,
    pub(crate) successor_basis: Option<&'a str>,
}

pub(crate) fn decide(
    terms: &Terms<'_>,
    finding_sha256s: &[String],
    basis_sha256: Option<&str>,
) -> Result<Disposition, String> {
    let resolving = terms.decision.resolves_review();
    let supplied_repair_terms = terms.repair_boundary.is_some()
        || terms.decisive_regression.is_some()
        || terms.category.is_some();
    if resolving && (supplied_repair_terms || terms.successor_basis.is_some()) {
        return Err(format!(
            "--decision {} records no repair terms or successor basis",
            terms.decision.as_str()
        ));
    }
    if terms.decision != Decision::Supersede && terms.successor_basis.is_some() {
        return Err("--successor-basis requires --decision supersede".into());
    }
    let mut value = json!({
        "decision": terms.decision.as_str(),
        "reason": terms.reason,
        "findingSha256s": finding_sha256s,
        "basisSha256": basis_sha256,
    });
    if !resolving {
        let boundary = terms
            .repair_boundary
            .ok_or("--decision repair and supersede require --repair-boundary TEXT")?;
        let regression = terms
            .decisive_regression
            .ok_or("--decision repair and supersede require --regression TEXT")?;
        let category = match terms.decision {
            Decision::Supersede => "contract_or_design_defect",
            _ => terms.category.unwrap_or("implementation_defect"),
        };
        if terms.decision == Decision::Supersede && terms.category.is_some() {
            return Err("--category is fixed by --decision supersede".into());
        }
        value["category"] = json!(category);
        value["causalAssumption"] = json!(terms.reason);
        value["affectedPaths"] = json!([]);
        value["repairBoundary"] = json!(boundary);
        value["decisiveRegression"] = json!(regression);
        value["invalidatedEvidence"] = json!(finding_sha256s);
        value["successorBasis"] = match terms.successor_basis {
            Some(text) => {
                let raw: Value = serde_json::from_str(text)
                    .map_err(|error| format!("--successor-basis is not valid JSON: {error}"))?;
                parse_basis(&raw, "successorBasis")?.value()
            }
            None => Value::Null,
        };
    }
    value["sha256"] = json!(hash::value(&value));
    parse_disposition(&value, "disposition")
}

pub(crate) fn parse_disposition(value: &Value, label: &str) -> Result<Disposition, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    let decision = match object.get("decision") {
        None => None,
        Some(raw) => Some(
            raw.as_str()
                .and_then(Decision::parse)
                .ok_or_else(|| format!("{label} decision is invalid"))?,
        ),
    };
    let resolving = decision.is_some_and(Decision::resolves_review);
    let mut allowed = vec!["findingSha256s", "basisSha256", "sha256"];
    if decision.is_some() {
        allowed.extend(["decision", "reason"]);
    }
    if !resolving {
        allowed.extend([
            "category",
            "causalAssumption",
            "affectedPaths",
            "repairBoundary",
            "decisiveRegression",
            "invalidatedEvidence",
            "successorBasis",
        ]);
    }
    reject_unknown(object, &allowed, label)?;
    let reason = decision
        .map(|_| text(object.get("reason"), "reason", label, 1024))
        .transpose()?;
    let finding_sha256s = sha_array(object.get("findingSha256s"), "findingSha256s", label, true)?;
    let basis_sha256 = optional_sha(object.get("basisSha256"), "basisSha256", label)?;
    let (repair, successor_basis) = if resolving {
        (None, None)
    } else {
        let (repair, successor) = parse_repair(object, label, decision.is_some())?;
        match decision {
            Some(Decision::Repair) if repair.category == "contract_or_design_defect" => {
                return Err(format!("{label} repair cannot change the basis"));
            }
            Some(Decision::Supersede) if successor.is_none() => {
                return Err(format!("{label} supersede requires successorBasis"));
            }
            _ => {}
        }
        (Some(repair), successor)
    };
    let mut disposition = Disposition {
        decision,
        reason,
        finding_sha256s,
        basis_sha256,
        repair,
        successor_basis,
        sha256: String::new(),
    };
    let mut unhashed = disposition.value();
    unhashed.as_object_mut().unwrap().remove("sha256");
    let expected = hash::value(&unhashed);
    if object.get("sha256").and_then(Value::as_str) != Some(expected.as_str()) {
        return Err(format!("{label} hash is missing or invalid"));
    }
    disposition.sha256 = expected;
    Ok(disposition)
}

fn parse_repair(
    object: &serde_json::Map<String, Value>,
    label: &str,
    decision_record: bool,
) -> Result<(Repair, Option<Basis>), String> {
    let category = object
        .get("category")
        .and_then(Value::as_str)
        .filter(|value| {
            matches!(
                *value,
                "implementation_defect" | "contract_or_design_defect" | "evidence_gap"
            )
        })
        .ok_or_else(|| format!("{label} category is invalid"))?
        .to_owned();
    let causal_assumption = text(
        object.get("causalAssumption"),
        "causalAssumption",
        label,
        1024,
    )?;
    // A protocol-v1 record always named affected paths; a decision record's
    // scope is its repair boundary.
    let affected_paths = strings(
        object.get("affectedPaths"),
        "affectedPaths",
        label,
        !decision_record,
    )?;
    let repair_boundary = text(object.get("repairBoundary"), "repairBoundary", label, 1024)?;
    let decisive_regression = text(
        object.get("decisiveRegression"),
        "decisiveRegression",
        label,
        1024,
    )?;
    let invalidated_evidence = sha_array(
        object.get("invalidatedEvidence"),
        "invalidatedEvidence",
        label,
        false,
    )?;
    let successor_basis = object
        .get("successorBasis")
        .filter(|value| !value.is_null())
        .map(|value| parse_basis(value, "successorBasis"))
        .transpose()?;
    if category == "contract_or_design_defect" && successor_basis.is_none() {
        return Err(format!("{label} design defects require successorBasis"));
    }
    if category != "contract_or_design_defect" && successor_basis.is_some() {
        return Err(format!("{label} successorBasis requires a design defect"));
    }
    Ok((
        Repair {
            category,
            causal_assumption,
            affected_paths,
            repair_boundary,
            decisive_regression,
            invalidated_evidence,
        },
        successor_basis,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINDING: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn terms(decision: Decision) -> Terms<'static> {
        Terms {
            decision,
            reason: "Lead rationale",
            repair_boundary: None,
            decisive_regression: None,
            category: None,
            successor_basis: None,
        }
    }

    #[test]
    fn protocol_v1_records_keep_their_shape() {
        assert!(parse_disposition(&json!({"category":"evidence_gap"}), "disposition").is_err());
        let mut legacy = json!({
            "category": "implementation_defect",
            "findingSha256s": [FINDING],
            "basisSha256": null,
            "causalAssumption": "x",
            "affectedPaths": ["a"],
            "repairBoundary": "b",
            "decisiveRegression": "c",
            "invalidatedEvidence": [],
            "successorBasis": null,
        });
        legacy["sha256"] = json!(hash::value(&legacy));
        let parsed = parse_disposition(&legacy, "disposition").unwrap();
        assert_eq!(parsed.decision, None);
        assert_eq!(parsed.value(), legacy);
    }

    #[test]
    fn defer_and_reject_carry_no_repair_terms() {
        for decision in [Decision::Defer, Decision::Reject] {
            let record = decide(&terms(decision), &[FINDING.into()], None).unwrap();
            assert!(record.resolves_review());
            assert!(record.repair.is_none());
            let value = record.value();
            assert!(value.get("repairBoundary").is_none());
            assert_eq!(parse_disposition(&value, "d").unwrap(), record);
            let mut with_boundary = terms(decision);
            with_boundary.repair_boundary = Some("x");
            assert!(decide(&with_boundary, &[FINDING.into()], None).is_err());
        }
    }

    #[test]
    fn repair_requires_a_lead_boundary_and_supersede_a_successor() {
        assert!(decide(&terms(Decision::Repair), &[FINDING.into()], None).is_err());
        let mut repair = terms(Decision::Repair);
        repair.repair_boundary = Some("only the parser");
        repair.decisive_regression = Some("the parser test fails before the fix");
        let record = decide(&repair, &[FINDING.into()], None).unwrap();
        assert!(!record.resolves_review());
        assert_eq!(
            record.repair.as_ref().unwrap().repair_boundary,
            "only the parser"
        );
        let mut supersede = repair;
        supersede.decision = Decision::Supersede;
        assert!(decide(&supersede, &[FINDING.into()], None).is_err());
        let mut bogus = record.value();
        bogus["decision"] = json!("approve");
        assert!(parse_disposition(&bogus, "d").is_err());
    }
}

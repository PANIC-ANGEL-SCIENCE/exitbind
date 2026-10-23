//! The Lead's reason on its `submit` event.
//!
//! A Lead act records the text the Lead gave with `--reason`, followed by the
//! saved Lead decisions of the Work it acts on, as the optional `reason`
//! field of the one `submit` event the act writes. The reducer copies it into
//! the state's submission item, where the next role's packet reads it. A
//! `disposition` keeps its reason inside its own record instead.

use crate::kernel::result_contract::{Outcome, Role};
use serde_json::Value;

/// The recorded reason has the disposition reason's bound.
pub(crate) const MAX_BYTES: usize = 1024;

/// The first ledger version whose `submit` event may carry `reason`.
pub(crate) const FIRST_VERSION: u64 = 8;

/// Whether a submission of `role` with `outcome` records the Lead's reason:
/// every Lead outcome of the result contract except `disposition`.
pub(crate) fn recorded(role: &str, outcome: &str) -> bool {
    Role::parse(role) == Some(Role::Lead)
        && Outcome::parse(outcome)
            .is_some_and(|outcome| Role::Lead.permits(outcome) && outcome != Outcome::Disposition)
}

/// The text to record: the Lead's own text, then one line per saved decision
/// of this Work. Nothing to record is `None`; above the bound the act is
/// refused before any write.
pub(crate) fn compose(text: Option<&str>, decisions: &[String]) -> Result<Option<String>, String> {
    let own = text.filter(|text| !text.trim().is_empty());
    let mut lines: Vec<String> = own.map(str::to_owned).into_iter().collect();
    lines.extend(
        decisions
            .iter()
            .map(|decision| crate::host::session_recovery::decision_line(decision)),
    );
    if lines.is_empty() {
        return Ok(None);
    }
    let reason = lines.join("\n");
    if reason.len() > MAX_BYTES {
        return Err(format!(
            "the Lead reason to record has {} bytes ({} from --reason and {} saved Lead decisions of this Work); at most {MAX_BYTES} bytes are recorded; no mutation was made",
            reason.len(),
            own.map_or(0, str::len),
            decisions.len()
        ));
    }
    Ok(Some(reason))
}

/// Replay check of a recorded reason: a Lead text of 1 to `MAX_BYTES` bytes
/// on an outcome that records one.
pub(crate) fn validate_event(event: &Value) -> Result<(), String> {
    let Some(reason) = event.get("reason") else {
        return Ok(());
    };
    let text_ok = reason
        .as_str()
        .is_some_and(|text| !text.trim().is_empty() && text.len() <= MAX_BYTES);
    let outcome_ok = recorded(
        event["role"].as_str().unwrap_or_default(),
        event["outcome"].as_str().unwrap_or_default(),
    );
    if text_ok && outcome_ok {
        Ok(())
    } else {
        Err(format!(
            "submit reason must be a Lead text of 1 to {MAX_BYTES} bytes on a Lead outcome other than disposition"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_lead_outcome_but_disposition_records_the_reason() {
        let recording: Vec<&str> = Role::Lead
            .outcomes()
            .iter()
            .map(|outcome| outcome.as_str())
            .filter(|outcome| recorded("lead", outcome))
            .collect();
        assert_eq!(
            recording,
            ["scoped", "blocked", "accepted", "rework", "rejected"]
        );
        for role in [Role::Worker, Role::Reviewer, Role::Adviser] {
            for outcome in role.outcomes() {
                assert!(!recorded(role.as_str(), outcome.as_str()));
            }
        }
    }

    #[test]
    fn the_lead_text_comes_first_then_each_decision_and_the_bound_holds() {
        let decisions = ["first".to_owned(), "second".to_owned()];
        assert_eq!(compose(None, &[]).unwrap(), None);
        assert_eq!(compose(Some(""), &[]).unwrap(), None);
        assert_eq!(compose(Some(" \n"), &[]).unwrap(), None);
        assert_eq!(
            compose(Some("own"), &decisions).unwrap().unwrap(),
            "own\nSaved Lead decision: first\nSaved Lead decision: second"
        );
        assert_eq!(
            compose(None, &decisions[..1]).unwrap().unwrap(),
            "Saved Lead decision: first"
        );
        let exact = "x".repeat(MAX_BYTES);
        assert_eq!(compose(Some(&exact), &[]).unwrap().unwrap(), exact);
        let refusal = compose(Some(&exact), &decisions[..1]).unwrap_err();
        assert!(
            refusal.contains("1051 bytes (1024 from --reason and 1 saved Lead decisions"),
            "{refusal}"
        );
        assert!(refusal.ends_with("no mutation was made"), "{refusal}");
    }

    #[test]
    fn replay_admits_a_bounded_lead_reason_only() {
        let event = |role: &str, outcome: &str, reason: Value| json!({"role": role, "outcome": outcome, "reason": reason});
        assert!(validate_event(&json!({"role": "lead", "outcome": "accepted"})).is_ok());
        assert!(validate_event(&event("lead", "accepted", json!("why"))).is_ok());
        for bad in [
            event("lead", "disposition", json!("why")),
            event("reviewer", "unavailable", json!("provider_quota")),
            event("lead", "accepted", json!("")),
            event("lead", "accepted", json!("  ")),
            event("lead", "accepted", json!(7)),
            event("lead", "accepted", json!("x".repeat(MAX_BYTES + 1))),
        ] {
            assert!(validate_event(&bad).is_err(), "{bad}");
        }
    }
}

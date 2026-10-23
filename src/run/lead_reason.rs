//! The Lead's reason on its `submit` event.
//!
//! A Lead act records the text the Lead gave with `--reason`, followed by the
//! saved Lead decisions of the Work it acts on, as the optional `reason`
//! field of the one `submit` event the act writes. The reducer copies it into
//! the state's submission item, where the next role's packet reads it. A
//! `disposition` keeps its own reason inside its record; given beside an
//! explicit `--disposition`, `--reason` is recorded as Lead text, while alone
//! it still carries the disposition JSON.

use crate::kernel::result_contract::{Outcome, Role};
use serde_json::Value;

/// The Lead's own text has the disposition reason's bound.
pub(crate) const MAX_BYTES: usize = 1024;

/// The largest recorded reason: the Lead's text and every saved decision.
pub(crate) const REPLAY_MAX_BYTES: usize =
    MAX_BYTES + crate::host::session_recovery::SAVED_DECISIONS_MAX_BYTES;

/// The first ledger version whose `submit` event may carry `reason`.
pub(crate) const FIRST_VERSION: u64 = 8;

/// Whether a submission of `role` with `outcome` records the Lead's reason
/// and spends its Work's saved decisions: every Lead outcome of the result
/// contract except `disposition`.
pub(crate) fn recorded(role: &str, outcome: &str) -> bool {
    Role::parse(role) == Some(Role::Lead)
        && Outcome::parse(outcome)
            .is_some_and(|outcome| Role::Lead.permits(outcome) && outcome != Outcome::Disposition)
}

/// The reason a submission records, refused before any write when the
/// Lead's own text is above its bound. A Lead disposition records only the
/// text given beside an explicit `--disposition`, and spends no decision.
pub(crate) fn for_submission(
    role: &str,
    outcome: &str,
    text: Option<&str>,
    explicit_disposition: bool,
    decisions: &[String],
) -> Result<Option<String>, String> {
    if recorded(role, outcome) {
        compose(text, decisions)
    } else if role == "lead" && outcome == "disposition" && explicit_disposition {
        compose(text, &[])
    } else {
        Ok(None)
    }
}

/// The text to record: the Lead's own text, then one line per saved decision
/// of this Work. Nothing to record is `None`.
pub(crate) fn compose(text: Option<&str>, decisions: &[String]) -> Result<Option<String>, String> {
    let own = text.filter(|text| !text.trim().is_empty());
    if let Some(own) = own.filter(|own| own.len() > MAX_BYTES) {
        return Err(format!(
            "--reason has {} bytes; at most {MAX_BYTES} bytes are recorded; no mutation was made",
            own.len()
        ));
    }
    let mut lines: Vec<String> = own.map(str::to_owned).into_iter().collect();
    lines.extend(
        decisions
            .iter()
            .map(|decision| crate::host::session_recovery::decision_line(decision)),
    );
    Ok((!lines.is_empty()).then(|| lines.join("\n")))
}

/// Replay check of a recorded reason: a Lead text of 1 to
/// `REPLAY_MAX_BYTES` bytes on a Lead outcome.
pub(crate) fn validate_event(event: &Value) -> Result<(), String> {
    let Some(reason) = event.get("reason") else {
        return Ok(());
    };
    let text_ok = reason
        .as_str()
        .is_some_and(|text| !text.trim().is_empty() && text.len() <= REPLAY_MAX_BYTES);
    let outcome_ok = event["role"] == "lead"
        && event["outcome"]
            .as_str()
            .and_then(Outcome::parse)
            .is_some_and(|outcome| Role::Lead.permits(outcome));
    if text_ok && outcome_ok {
        Ok(())
    } else {
        Err(format!(
            "submit reason must be a Lead text of 1 to {REPLAY_MAX_BYTES} bytes on a Lead outcome"
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
        let decisions = ["d".to_owned()];
        let pair = for_submission("lead", "disposition", Some("why"), true, &decisions);
        assert_eq!(pair.unwrap().as_deref(), Some("why"));
        let carrier = for_submission("lead", "disposition", Some("{}"), false, &decisions);
        assert_eq!(carrier.unwrap(), None);
        let reviewer = for_submission("reviewer", "unavailable", Some("rate_limit"), false, &[]);
        assert_eq!(reviewer.unwrap(), None);
    }

    #[test]
    fn the_lead_text_comes_first_then_each_decision_and_only_the_text_is_bounded() {
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
        let refusal = compose(Some(&"x".repeat(MAX_BYTES + 1)), &[]).unwrap_err();
        assert!(refusal.starts_with("--reason has 1025 bytes"), "{refusal}");
        assert!(refusal.ends_with("no mutation was made"), "{refusal}");
        // The largest record: a text at its bound and every decision at its
        // bound compose without refusal and replay.
        let full = vec!["d".repeat(512); 8];
        let largest = compose(Some(&"x".repeat(MAX_BYTES)), &full)
            .unwrap()
            .unwrap();
        assert_eq!(largest.len(), REPLAY_MAX_BYTES);
        let event = json!({"role": "lead", "outcome": "accepted", "reason": largest});
        validate_event(&event).unwrap();
    }

    #[test]
    fn replay_admits_a_bounded_lead_reason_only() {
        let event = |role: &str, outcome: &str, reason: Value| json!({"role": role, "outcome": outcome, "reason": reason});
        assert!(validate_event(&json!({"role": "lead", "outcome": "accepted"})).is_ok());
        assert!(validate_event(&event("lead", "accepted", json!("why"))).is_ok());
        assert!(validate_event(&event("lead", "disposition", json!("why"))).is_ok());
        for bad in [
            event("reviewer", "unavailable", json!("provider_quota")),
            event("worker", "completed", json!("why")),
            event("lead", "accepted", json!("")),
            event("lead", "accepted", json!("  ")),
            event("lead", "accepted", json!(7)),
            event("lead", "accepted", json!("x".repeat(REPLAY_MAX_BYTES + 1))),
        ] {
            assert!(validate_event(&bad).is_err(), "{bad}");
        }
    }
}

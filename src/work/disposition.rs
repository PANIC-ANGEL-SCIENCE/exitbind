//! `work disposition`: the Lead's decision on a pending finding cycle.
//!
//! The Lead supplies a decision and its own words; Exitbind binds them to the
//! pending finding cycle and current basis and records the disposition
//! through the ordinary Lead return, so no hash or protocol JSON is composed
//! by hand.

use super::*;
use crate::kernel::disposition::{decide, Decision, Terms};

pub(crate) struct DispositionOptions<'a> {
    pub(crate) decision: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) repair_boundary: Option<&'a str>,
    pub(crate) regression: Option<&'a str>,
    pub(crate) category: Option<&'a str>,
    pub(crate) successor_basis: Option<&'a str>,
}

pub(crate) fn dispose(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    options: DispositionOptions<'_>,
) -> Result<Value, String> {
    let decision = Decision::parse(options.decision)
        .ok_or("--decision must be repair, defer, reject, or supersede")?;
    let ledger = resolve(loaded, work)?;
    let action = next_for(loaded, work, &ledger)?;
    if action["action"] != "lead_decision" || action["assignment"] != assignment {
        return Err("assignment is not the current pending Lead decision".into());
    }
    let pending = action["packet"]
        .get("pendingDisposition")
        .filter(|value| value.is_object())
        .ok_or("no finding is pending a Lead disposition")?;
    if !pending["decisions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == decision.as_str()))
    {
        return Err(format!(
            "--decision {} is not available for this finding cycle",
            decision.as_str()
        ));
    }
    let findings = pending["findingSha256s"]
        .as_array()
        .ok_or("pending finding cycle is invalid")?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or("pending finding cycle is invalid")?;
    let record = decide(
        &Terms {
            decision,
            reason: options.reason,
            repair_boundary: options.repair_boundary,
            decisive_regression: options.regression,
            category: options.category,
            successor_basis: options.successor_basis,
        },
        &findings,
        action["packet"]["basisSha256"].as_str(),
    )?;
    let value = record.value();
    let text = serde_json::to_string(&value).map_err(|error| error.to_string())?;
    return_result_impl::return_result_with(
        loaded,
        work,
        assignment,
        "disposition",
        None,
        Some(&text),
        None,
        Some(render(&value).into_bytes()),
    )
}

pub(super) fn pending(next: &Value) -> bool {
    next["packet"].get("pendingDisposition").is_some()
}

/// The review fact while a finding pends or after the Lead resolved it.
pub(super) fn review_fact(next: &Value) -> Option<&'static str> {
    if pending(next) {
        Some("finding_pending")
    } else if next["packet"].get("reviewResolution").is_some() {
        Some("resolved_by_lead_disposition")
    } else {
        None
    }
}

/// What happened and what the Lead does next, for the same two states.
pub(super) fn lead_help(next: &Value) -> Option<(&'static str, &'static str)> {
    match review_fact(next)? {
        "finding_pending" => Some((
            "An adverse finding needs Lead disposition. For a clear defect inside the accepted task, record repair and continue without asking the owner again; ask the owner only when scope, authority, review choice, or an irreversible decision changes.",
            "record the Lead decision: work disposition WORK ASSIGNMENT --decision repair|defer|reject|supersede --reason TEXT",
        )),
        _ if next["action"] == "lead_decision" => Some((
            "Required checks are current. The review finding was resolved by Lead disposition, not approved.",
            "decide acceptance (lead outcome: accepted or blocked)",
        )),
        _ => None,
    }
}

/// The human-readable disposition artifact recorded with the decision.
fn render(value: &Value) -> String {
    let mut text = format!(
        "# Lead disposition: {}\n\nReason: {}\n",
        value["decision"].as_str().unwrap_or_default(),
        value["reason"].as_str().unwrap_or_default()
    );
    for (label, field) in [
        ("Repair boundary", "repairBoundary"),
        ("Decisive regression", "decisiveRegression"),
    ] {
        if let Some(item) = value[field].as_str() {
            text.push_str(&format!("{label}: {item}\n"));
        }
    }
    text
}

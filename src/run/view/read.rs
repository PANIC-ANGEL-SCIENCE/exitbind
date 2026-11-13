// Projects read-only Run status, explanation and report views.
// Governor recovery is attached only for the marked protocol; see `src/run/view.rs`.
//! Read-only Run status, explanation and report projections.

use super::*;

pub fn inspect(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let mut view = RunSnapshot::capture(loaded, ledger)?.inspect_view();
    attach_governor_recovery(&mut view);
    Ok(view)
}

/// Read-only current-state view with artifact/config revalidation.
pub fn status(loaded: &Loaded, ledger: &str) -> Result<Value, String> {
    let snapshot = RunSnapshot::capture(loaded, ledger)?;
    let mut status = snapshot.status_view()?;
    let mut inspect = snapshot.inspect_view();
    attach_governor_recovery(&mut inspect);
    if let Some(recovery) = inspect.get("governorRecovery") {
        status["governorRecovery"] = recovery.clone();
    }
    Ok(status)
}

fn attach_governor_recovery(view: &mut Value) {
    let start = view
        .get("events")
        .and_then(Value::as_array)
        .and_then(|events| events.first());
    let Some(start) =
        start.filter(|event| event["carryProtocol"] == crate::run::carry::RECOVERY_PROTOCOL)
    else {
        return;
    };
    view["governorRecovery"] = json!({
        "protocol": crate::run::carry::RECOVERY_PROTOCOL,
        "state": view["governor"]["state"],
        "evidence": start["governorCarry"]["recovery"],
    });
}

/// Read-only explanation for a run or one of its factual protection events.
pub fn explain(loaded: &Loaded, ledger: &str, event_id: Option<&str>) -> Result<Value, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    let drift = drift_warning(loaded, &state)?;
    warn_drift(drift.as_ref());
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_value::explain_with_artifact(&state, event_id, artifact_current)
}

/// The terminal block this run offers a reader, if any. Read-only, and
/// separate from the status projection so the display surfaces can share one
/// rule with the work facade.
pub(crate) fn terminal_display(loaded: &Loaded, ledger: &str) -> Option<&'static str> {
    let snapshot = RunSnapshot::capture(loaded, ledger).ok()?;
    let terminal = crate::work::packet::terminal_display(&snapshot.inspect_view(), || {
        crate::run::inputs::fingerprint_for_state(loaded, snapshot.state()).ok()
    });
    match crate::work::locator_for_ledger(ledger) {
        Some(work) => crate::session_goal::terminal_for_work(loaded, &work, terminal)
            .ok()
            .flatten(),
        None => terminal,
    }
}

/// Build the typed read-only projection used by the default human status view.
pub(crate) fn human_status(
    loaded: &Loaded,
    ledger: &str,
) -> Result<crate::run_human::HumanStatus, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    let drift = drift_warning(loaded, &state)?;
    warn_drift(drift.as_ref());
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_human::human_status(&state, artifact_current)
}

/// Build the typed read-only projection used by the default human explanation view.
pub(crate) fn human_explain(
    loaded: &Loaded,
    ledger: &str,
    event_id: Option<&str>,
) -> Result<crate::run_human::HumanExplanation, String> {
    let (_, events, _) = load(loaded, ledger)?;
    let state = reduce_live(loaded, &events)?;
    let drift = drift_warning(loaded, &state)?;
    warn_drift(drift.as_ref());
    let artifact_current = artifact_current(loaded, &state)?;
    predecessor(loaded, &events[0])?;
    crate::run_human::human_explain(&state, event_id, artifact_current)
}

/// Generate a local redacted report from explicitly selected ledgers.  The
/// report only contains hashes, counts, bounded statuses, and observed command
/// durations; it never returns goals, commands, prompts, profiles, paths, or
/// artifact bytes.
pub fn report(loaded: &Loaded, ledgers: &[&str]) -> Result<Value, String> {
    let mut states = Vec::with_capacity(ledgers.len());
    let mut warnings = Vec::new();
    for ledger in ledgers {
        let (_, events, _) = load(loaded, ledger)?;
        let state = reduce_live(loaded, &events)?;
        if let Some(warning) = drift_warning(loaded, &state)? {
            warn_drift(Some(&warning));
            warnings.push(json!({"ledger": ledger, "warning": warning}));
        }
        predecessor(loaded, &events[0])?;
        states.push(state);
    }
    let mut report = crate::run_value::aggregate(&states)?;
    report["warnings"] = json!(warnings);
    Ok(report)
}

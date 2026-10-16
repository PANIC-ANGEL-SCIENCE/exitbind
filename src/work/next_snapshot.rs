use super::*;

/// Derive the next action, residual packet and presentation from one captured
/// revision so the paths cannot report conflicting state.
pub(super) fn next_and_residual(
    loaded: &Loaded,
    work: &str,
    ledger: &str,
    resumed: bool,
) -> Result<(Value, Value, Value), String> {
    let snapshot = run::RunSnapshot::capture(loaded, ledger)?;
    next_and_residual_from_snapshot(loaded, work, &snapshot, resumed, true)
}

pub(super) fn next_and_residual_from_snapshot(
    loaded: &Loaded,
    work: &str,
    snapshot: &run::RunSnapshot,
    resumed: bool,
    remember_presentation: bool,
) -> Result<(Value, Value, Value), String> {
    let mut next = super::next_from(loaded, work, snapshot)?;
    if let Some(assignment) = next["assignment"].as_str().map(str::to_owned) {
        super::attach_held(loaded, work, &assignment, &mut next)?;
    }
    let residual = crate::work::packet::project(work, snapshot, &next)?;
    next["resolvedActor"] = residual["humanHelp"]["nextAction"]["actor"].clone();
    let facts = crate::work::packet::facts(snapshot, &next, || {
        crate::run::inputs::fingerprint(loaded).ok()
    })?;
    let mut presentation = if remember_presentation {
        crate::presentation_events::project(
            &loaded.state_root,
            work,
            &residual,
            &next["progress"],
            &facts,
            resumed,
        )
    } else {
        crate::presentation_events::project_without_memory(
            &loaded.state_root,
            work,
            &residual,
            &next["progress"],
            &facts,
            resumed,
        )
    };
    presentation["goalProgress"] =
        crate::session_goal::progress_for_work(loaded, work, &next["progress"])?;
    presentation["terminal"] = json!(crate::session_goal::terminal_for_work(
        loaded,
        work,
        presentation["terminal"].is_string().then_some("EXIT READY"),
    )?);
    super::details::automatic_progress(&mut presentation["goalProgress"], &next["current"]);
    Ok((next, residual, presentation))
}

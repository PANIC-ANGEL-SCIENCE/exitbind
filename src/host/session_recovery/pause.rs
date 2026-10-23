//! The Lead's pause record: the saved step and the Lead decisions taken on an
//! in-flight Work before its Lead act, written through the existing recovery
//! record, and the two emitted routes that write it. `goal status`,
//! `work resume` and `session recovery show` print the routes; the next
//! session reads the record from `session recovery show`.

use super::{
    hint::{read_hint, FILE, MAX_ITEMS},
    record::{owned, store_locked, verify_draft, Draft},
};
use crate::{config, session_goal};
use serde_json::{json, Value};
use std::path::Path;

/// One record route: `session recovery record`, the option and its
/// registered placeholder, then the configuration.
fn record_route(option: &str, slot: &str, config: &Path) -> crate::emitted::Route {
    crate::emitted::Route::new(["session", "recovery", "record", option, slot]).config(config)
}

/// The two record routes for the Goal owner configuration `config`.
pub(crate) fn routes(config: &Path) -> Result<Value, String> {
    let route = |option: &str, slot: String, meaning: &str| -> Result<Value, String> {
        let argv = record_route(option, &slot, config).argv()?;
        Ok(json!({"command": argv, "fill": crate::emitted::fill_flags(&argv), "meaning": meaning}))
    };
    Ok(json!({
        "actor": "lead",
        "saveStep": route("--step", crate::emitted::slot("SAVED_STEP"),
            "replace the saved step; saved Lead decisions are kept")?,
        "saveDecision": route("--decision", crate::emitted::slot("LEAD_DECISION"),
            "add one Lead decision to the saved step (at most eight; the oldest is dropped)")?,
        "text": lines(config),
    }))
}

/// The same two routes as printed lines.
pub(crate) fn lines(config: &Path) -> Vec<String> {
    let line = |option: &str, slot: String| record_route(option, &slot, config).text();
    vec![
        format!(
            "Lead pause, save the step: {}",
            line("--step", crate::emitted::slot("SAVED_STEP"))
        ),
        format!(
            "Lead pause, save a decision taken before the Lead act: {}",
            line("--decision", crate::emitted::slot("LEAD_DECISION"))
        ),
    ]
}

/// The routes for a view of `loaded`, when this native session owns (or
/// adopted) the open Goal of this project; otherwise nothing.
pub(crate) fn for_view(loaded: &config::Loaded) -> Option<Value> {
    let scope = session_goal::native::scope(loaded, false).ok()?;
    if !scope.is_session() || !scope.owner_matches {
        return None;
    }
    let record = session_goal::read(&scope.root).ok()??;
    if session_goal::goal_summary::closed_revision(&record).is_some() {
        return None;
    }
    routes(&loaded.path).ok()
}

/// Write the step and/or one Lead decision into the session's record under
/// its lock. The rest of a current record of this Goal is kept; references
/// that no longer verify are dropped, since the step is what is saved.
pub(crate) fn record(
    owner_loaded: &config::Loaded,
    step: Option<&str>,
    decision: Option<&str>,
) -> Result<Value, String> {
    if step.is_none() && decision.is_none() {
        return Err(
            "session recovery record takes --step TEXT, --decision TEXT, or JSON on standard input"
                .into(),
        );
    }
    let session = session_goal::native::current()?
        .ok_or("session recovery record requires a host-reported native session")?;
    let ledger = crate::run::ledger::ledger_path(&session.root, FILE, true)?;
    crate::run::ledger::with_lock(&ledger, || {
        let owned = owned(owner_loaded)?;
        let saved = read_hint(&owned.session.root)
            .ok()
            .flatten()
            .filter(|hint| hint.belongs_to(&owned.session, &owned.owner, &owned.record["goalId"]));
        let mut draft = match (saved, step) {
            (Some(hint), _) => {
                let mut draft = Draft::of(&hint);
                if verify_draft(owner_loaded, &owned, &draft).is_err() {
                    draft = fresh(draft.current_step, draft.latest_corrections);
                }
                draft
            }
            (None, Some(step)) => fresh(step.to_owned(), Vec::new()),
            (None, None) => {
                let route = &lines(&owner_loaded.path)[0];
                return Err(format!(
                    "no step is saved for this Goal yet; nothing was written. {route}"
                ));
            }
        };
        if let Some(step) = step {
            draft.current_step = step.to_owned();
        }
        if let Some(decision) = decision {
            draft.latest_corrections.push(decision.to_owned());
            let excess = draft.latest_corrections.len().saturating_sub(MAX_ITEMS);
            draft.latest_corrections.drain(..excess);
        }
        store_locked(owner_loaded, &owned, draft)
    })
}

fn fresh(current_step: String, latest_corrections: Vec<String>) -> Draft {
    Draft {
        current_step,
        current_work: super::hint::WorkSelection {
            state: "unresolved".into(),
            work: None,
        },
        latest_corrections,
        evidence_refs: Vec::new(),
        work_refs: Vec::new(),
        pending_operation: None,
        guidance_refs: Vec::new(),
    }
}

/// The saved step and decisions `session recovery show` reports, with the
/// revision they were recorded at and the session that wrote them.
pub(super) fn active_step(hint: &super::hint::Hint, goal_current: bool) -> Value {
    json!({
        "step": hint.current_step,
        "leadDecisions": hint.latest_corrections,
        "recordedAtGoalRevision": hint.goal_revision,
        "goalCurrent": goal_current,
        "recordedBySessionKey": hint.recorded_by(),
    })
}

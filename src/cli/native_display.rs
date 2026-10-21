//! Wiring only: after a successful Goal or Work ledger command, refresh the
//! human display state of the current native session. A display refresh never
//! changes the command's result, output or exit status, and prints nothing.

use super::{config, Arguments};
use crate::session_goal::display::{self, Journal};

/// Work actions that can change what the Goal display shows.
const WORK_ACTIONS: [&str; 8] = [
    "begin",
    "return",
    "act",
    "disposition",
    "closeout",
    "check",
    "resume",
    "child",
];

pub(super) fn after_goal(
    loaded: &config::Loaded,
    a: &Arguments,
    result: Result<(), String>,
) -> Result<(), String> {
    if result.is_ok() && a.positional.first().map(String::as_str) != Some("status") {
        refresh(
            loaded,
            Some(if a.flags.contains_key("project") {
                Journal::Project
            } else {
                Journal::Session
            }),
        );
    }
    result
}

pub(super) fn after_work(
    loaded: &config::Loaded,
    a: &Arguments,
    result: Result<(), String>,
) -> Result<(), String> {
    let action = a.positional.first().map(String::as_str).unwrap_or_default();
    if result.is_ok() && WORK_ACTIONS.contains(&action) {
        let journal =
            (action == "begin" && a.flags.contains_key("project-goal")).then_some(Journal::Project);
        refresh(loaded, journal);
    }
    result
}

/// Refresh the display file of the current native session, if there is one.
/// Without an explicit journal the one the file was last projected from is
/// kept, else the session journal. Every failure is silent: display only.
pub(crate) fn refresh(loaded: &config::Loaded, journal: Option<Journal>) {
    let _ = try_refresh(loaded, journal);
}

fn try_refresh(loaded: &config::Loaded, journal: Option<Journal>) -> Result<(), String> {
    let Some(session) = crate::session_goal::native::current()? else {
        return Ok(());
    };
    let path = display::path_for_key(&session.key)?;
    let journal = journal.unwrap_or_else(|| match display::read(&path) {
        Some(previous) if previous["journal"] == "project" => Journal::Project,
        _ => Journal::Session,
    });
    let root = match journal {
        Journal::Project => loaded.state_root.clone(),
        Journal::Session => {
            let scope = crate::session_goal::native::scope(loaded, false)?;
            if !scope.owner_matches {
                return Ok(());
            }
            scope.root
        }
    };
    let state = display::build(
        loaded,
        &root,
        journal,
        session.host,
        &crate::memory::state::now(),
    )?;
    display::write(&path, &state)
}

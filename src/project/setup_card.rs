//! Read a round's task card for setup and register its step table.
//!
//! `setup --task-card PATH` turns the card's step table into the Goal's named
//! requirements (`S<step>` with the exact `Done when` cell) and prints the
//! start path that follows: `goal cover`, `goal prepare`, `work begin`, for the
//! rows `--first-work` names (default: every row). Where the Goal is recorded
//! is decided before setup writes anything: a native session Goal another
//! project owns selects this project's journal, and the report says so. The
//! Goal store and requirement rules stay with `session_goal`; this module only
//! parses the card and calls those owners.

use super::SetupFacts;
use crate::config::Loaded;
use crate::project::path::inputs::{self, Placed};
use serde_json::{json, Value};
use std::path::Path;

/// The private writer's label for a task-card snapshot.
const SNAPSHOT: &str = "task card snapshot";

/// The step table of one task card, ready to register.
pub(super) struct Card {
    /// Project-relative card path, the approved requirement source: the card
    /// itself, or its snapshot when the card lies outside the project.
    pub(super) path: String,
    /// The card as given (absolute when snapshotted) and its sha256.
    source: String,
    source_sha256: String,
    /// The card bytes to snapshot under `path` when the card lies outside.
    snapshot: Option<Vec<u8>>,
    pub(super) goal_id: String,
    pub(super) rows: Vec<(String, String)>,
    /// The rows the first Work covers, in card order.
    pub(super) first: Vec<String>,
}

impl Card {
    /// The first Work's requirement list for `goal prepare` and `work begin`.
    pub(super) fn ids(&self) -> String {
        self.first.join(",")
    }

    pub(super) fn view(&self) -> Value {
        let remaining: Vec<&str> = self
            .rows
            .iter()
            .map(|(id, _)| id.as_str())
            .filter(|id| !self.first.iter().any(|first| first == id))
            .collect();
        json!({"path": self.path, "source": self.source, "sourceSha256": self.source_sha256,
            "snapshotted": self.snapshot.is_some(), "goalId": self.goal_id,
            "requirements": self.rows.iter().map(|(id, text)| json!({"id": id, "obligation": text})).collect::<Vec<_>>(),
            "firstWork": self.first, "remaining": remaining})
    }
}

/// Where the card's Goal and rows are recorded.
pub(super) struct CardScope {
    pub(super) project: bool,
    /// Why the project journal was selected although a session is present.
    pub(super) reason: Option<&'static str>,
}

const FOREIGN_OWNER: &str =
    "the native session Goal is owned by another configured project; this project's journal records the card";
const UNOWNED_JOURNAL: &str =
    "the native session Goal journal has no owner record; this project's journal records the card";

/// Decide, without writing, where the card is recorded: the native session
/// scope when this project owns it or no one does yet, otherwise the project
/// journal. `config` is the project configuration, which may not exist yet.
pub(super) fn card_scope(facts: &SetupFacts, config: &Path) -> Result<CardScope, String> {
    let project = |reason| {
        Ok(CardScope {
            project: true,
            reason,
        })
    };
    let Some(session) = crate::session_goal::native::current()? else {
        return project(None);
    };
    let Some(owner) = session.owner()? else {
        if crate::session_goal::read(&session.root)?.is_some() {
            return project(Some(UNOWNED_JOURNAL));
        }
        return Ok(CardScope {
            project: false,
            reason: None,
        });
    };
    let local = if config.is_file() {
        let path = config
            .to_str()
            .ok_or("configuration path is not valid UTF-8")?;
        crate::session_goal::native::owner_for(&crate::config::load(Some(path))?)?
    } else {
        let root = Path::new(&facts.root);
        let state = facts.state_root.as_deref().map_or(root, Path::new);
        crate::session_goal::native::owner_for_roots(root, state, facts.project_id.clone())?
    };
    if owner.identity_sha256 == local.identity_sha256 && owner.project_id == local.project_id {
        Ok(CardScope {
            project: false,
            reason: None,
        })
    } else {
        project(Some(FOREIGN_OWNER))
    }
}

/// Parse the card named by `--task-card`, or `None` when none was given.
pub(super) fn plan(facts: &SetupFacts) -> Result<Option<Card>, String> {
    let Some(card) = facts.task_card.as_deref() else {
        if facts.goal_id.is_some() || facts.first_work.is_some() {
            return Err("--goal-id and --first-work require --task-card PATH".into());
        }
        return Ok(None);
    };
    if facts.goal.is_none() || facts.check_command.is_none() || facts.review_policy.is_none() {
        return Err("--task-card requires --goal, --check-command and --review-policy".into());
    }
    let root = Path::new(&facts.root);
    let placed = inputs::place(root, card).map_err(|error| format!("task card {card}: {error}"))?;
    // The Goal ID derives from the directory of the card as given, so a
    // snapshot's suffixed directory never renames the Goal.
    let (origin, bytes) = match &placed {
        Placed::Inside(path) => (
            std::path::PathBuf::from(path),
            read_card(&root.join(path), card)?,
        ),
        Placed::Outside(path) => {
            crate::project::private_write::writable(root, SNAPSHOT)?;
            (path.clone(), read_card(path, card)?)
        }
    };
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| format!("task card {card} is not UTF-8 text"))?;
    let goal_id = match facts.goal_id.as_deref() {
        Some(id) if valid_id(id) => id.to_owned(),
        Some(_) => return Err("--goal-id must be 1-64 letters, digits, '.', '_' or '-'".into()),
        None => origin
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .filter(|name| valid_id(name))
            .ok_or("the task card's directory name is not a valid Goal ID; pass --goal-id ID")?
            .to_owned(),
    };
    let rows = step_rows(&text)?;
    let first = match facts.first_work.as_deref() {
        None => rows.iter().map(|(id, _)| id.clone()).collect(),
        Some(ids) => first_rows(&rows, ids)?,
    };
    let source_sha256 = crate::evidence::hash::bytes(&bytes);
    let (path, source, snapshot) = match placed {
        Placed::Inside(path) => (path.clone(), path, None),
        Placed::Outside(outside) => {
            let (path, _) = crate::project::private_write::place_suffixed(
                root,
                crate::host::plan_card::CARDS,
                &goal_id,
                "TASK.md",
                &bytes,
                SNAPSHOT,
            )?;
            let source = outside
                .to_str()
                .ok_or("task card path is not UTF-8")?
                .to_owned();
            (path, source, Some(bytes))
        }
    };
    Ok(Some(Card {
        rows,
        path,
        source,
        source_sha256,
        snapshot,
        goal_id,
        first,
    }))
}

/// The card's bytes: a regular file of at most 64 KiB.
fn read_card(path: &Path, given: &str) -> Result<Vec<u8>, String> {
    let info = std::fs::metadata(path)
        .map_err(|error| format!("task card {given} cannot be read: {error}"))?;
    if !info.is_file() || info.len() > 64 * 1024 {
        return Err(format!(
            "task card {given} must be a regular file of at most 65536 bytes"
        ));
    }
    std::fs::read(path).map_err(|error| format!("task card {given} cannot be read: {error}"))
}

/// The `--first-work` IDS in card order; an ID the card has no row for is
/// refused here, before setup writes anything.
fn first_rows(rows: &[(String, String)], ids: &str) -> Result<Vec<String>, String> {
    let named: Vec<&str> = ids.split(',').map(str::trim).collect();
    if named.iter().any(|id| id.is_empty()) {
        return Err("--first-work must name card rows as S<step>[,S<step>...]".into());
    }
    if let Some(unknown) = named
        .iter()
        .find(|id| !rows.iter().any(|(row, _)| row == *id))
    {
        let known: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
        return Err(format!(
            "--first-work {unknown} names no task card row; the card has {}",
            known.join(", ")
        ));
    }
    Ok(rows
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| named.contains(&id.as_str()))
        .collect())
}

/// Record the card's Goal and requirements in the native session scope when
/// a native session is present, otherwise in the project journal.
pub(super) fn register(
    loaded: &Loaded,
    facts: &SetupFacts,
    next: &mut Value,
) -> Result<(), String> {
    let Some(card) = plan(facts)? else {
        return Ok(());
    };
    let failed = |error: String| {
        format!("setup applied the configuration, but the task card was not registered: {error}")
    };
    let selected = card_scope(facts, &loaded.path).map_err(failed)?;
    let scope = if selected.project {
        crate::session_goal::native::project_scope(loaded)
    } else {
        crate::session_goal::native::scope(loaded, true).map_err(failed)?
    };
    scope.require_owner().map_err(failed)?;
    let mut card = card;
    if let Some(bytes) = &card.snapshot {
        // The outside card is snapshotted through the plan-card route; a
        // byte-identical earlier snapshot is reused.
        let (path, _) = crate::project::private_write::write_suffixed(
            &loaded.product_root,
            crate::host::plan_card::CARDS,
            &card.goal_id,
            "TASK.md",
            bytes,
            SNAPSHOT,
        )
        .map_err(failed)?;
        if path != card.path {
            next["taskCard"]["path"] = json!(path);
            card.path = path;
        }
    }
    let open = crate::session_goal::read(&scope.root)
        .map_err(failed)?
        .is_some_and(|record| {
            record["goalId"] == card.goal_id && record["closure"]["closed"] != true
        });
    if !open {
        crate::session_goal::incorporate(
            &scope.root,
            loaded,
            &card.goal_id,
            facts.goal.as_deref().unwrap_or_default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            scope.is_session(),
        )
        .map_err(failed)?;
    }
    for (id, text) in &card.rows {
        crate::session_goal::requirements::require(
            &scope.root,
            loaded,
            &card.goal_id,
            id,
            text,
            &card.path,
        )
        .map_err(failed)?;
    }
    next["taskCard"]["registration"] = json!({"registered": true,
        "scope": if scope.is_session() { "session" } else { "project" },
        "scopeReason": selected.reason,
        "incorporated": !open, "requirements": card.rows.len()});
    Ok(())
}

/// Rows of the first markdown table whose header has a `Done when` column:
/// `S<first cell>` with the exact `Done when` cell text.
pub(crate) fn step_rows(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut lines = text.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        let Some(header) = cells(line) else {
            continue;
        };
        let Some(column) = header.iter().position(|cell| *cell == "Done when") else {
            continue;
        };
        let mut rows = Vec::new();
        while let Some(row) = lines.peek().and_then(|line| cells(line)) {
            lines.next();
            if row
                .iter()
                .all(|cell| !cell.is_empty() && cell.chars().all(|c| matches!(c, '-' | ':')))
            {
                continue;
            }
            let step = row.first().copied().unwrap_or_default();
            let done = row.get(column).copied().unwrap_or_default();
            if !valid_id(step) || done.is_empty() {
                return Err(format!(
                    "task card step row needs a step number and a Done when cell: {}",
                    row.join(" | ")
                ));
            }
            let id = format!("S{step}");
            if rows.iter().any(|(existing, _)| *existing == id) {
                return Err(format!("task card repeats step {step}"));
            }
            rows.push((id, done.to_owned()));
        }
        if rows.is_empty() {
            return Err("task card step table has no rows".into());
        }
        return Ok(rows);
    }
    Err("task card has no step table with a `Done when` column".into())
}

/// Cells of a markdown table line, split on unescaped pipes and trimmed.
fn cells(line: &str) -> Option<Vec<&str>> {
    let inner = line.strip_prefix('|')?;
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let (mut start, mut escaped) = (0, false);
    for (index, c) in inner.char_indices() {
        match c {
            '\\' => escaped = !escaped,
            '|' if !escaped => {
                cells.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => escaped = false,
        }
    }
    cells.push(inner[start..].trim());
    Some(cells)
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::step_rows;

    #[test]
    fn step_rows_take_the_done_when_column_verbatim() {
        let card = "# Card\n\n| A | B |\n| - | - |\n| x | y |\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 1 | Make `a \\| b` | Tests pass. |\n| 2 | More | Docs agree. |\n\nAfter.\n";
        let rows = step_rows(card).unwrap();
        assert_eq!(
            rows,
            [
                ("S1".to_owned(), "Tests pass.".to_owned()),
                ("S2".to_owned(), "Docs agree.".to_owned())
            ]
        );
        assert!(rows.iter().all(|(_, text)| card.contains(text.as_str())));
        assert!(step_rows("| Step | Done |\n| - | - |\n| 1 | x |\n").is_err());
    }
}

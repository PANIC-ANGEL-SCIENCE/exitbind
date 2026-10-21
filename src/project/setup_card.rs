//! Read a round's task card for setup and register its step table.
//!
//! `setup --task-card PATH` turns the card's step table into the Goal's named
//! requirements (`S<step>` with the exact `Done when` cell) and prints the
//! start path that follows: `goal cover`, `goal prepare`, `work begin`. The
//! Goal store and requirement rules stay with `session_goal`; this module only
//! parses the card and calls those owners.

use super::SetupFacts;
use crate::config::Loaded;
use serde_json::{json, Value};
use std::path::{Component, Path};

/// The step table of one task card, ready to register.
pub(super) struct Card {
    /// Project-relative card path, the approved requirement source.
    pub(super) path: String,
    pub(super) goal_id: String,
    pub(super) rows: Vec<(String, String)>,
}

impl Card {
    pub(super) fn ids(&self) -> String {
        self.rows
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub(super) fn view(&self) -> Value {
        json!({"path": self.path, "goalId": self.goal_id,
            "requirements": self.rows.iter().map(|(id, text)| json!({"id": id, "obligation": text})).collect::<Vec<_>>()})
    }
}

/// Parse the card named by `--task-card`, or `None` when none was given.
pub(super) fn plan(facts: &SetupFacts) -> Result<Option<Card>, String> {
    let Some(card) = facts.task_card.as_deref() else {
        if facts.goal_id.is_some() {
            return Err("--goal-id requires --task-card PATH".into());
        }
        return Ok(None);
    };
    if facts.goal.is_none() || facts.check_command.is_none() || facts.review_policy.is_none() {
        return Err("--task-card requires --goal, --check-command and --review-policy".into());
    }
    let path = relative_card(Path::new(&facts.root), card)?;
    let text = std::fs::read_to_string(Path::new(&facts.root).join(&path))
        .map_err(|error| format!("task card {path} cannot be read as UTF-8 text: {error}"))?;
    let goal_id = match facts.goal_id.as_deref() {
        Some(id) if valid_id(id) => id.to_owned(),
        Some(_) => return Err("--goal-id must be 1-64 letters, digits, '.', '_' or '-'".into()),
        None => Path::new(&path)
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .filter(|name| valid_id(name))
            .ok_or("the task card's directory name is not a valid Goal ID; pass --goal-id ID")?
            .to_owned(),
    };
    Ok(Some(Card {
        rows: step_rows(&text)?,
        path,
        goal_id,
    }))
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
    let scope = crate::session_goal::native::scope(loaded, true).map_err(failed)?;
    scope.require_owner().map_err(failed)?;
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

/// The card path relative to the project root; absolute paths must lie inside.
fn relative_card(root: &Path, card: &str) -> Result<String, String> {
    let path = Path::new(card);
    let relative = if path.is_absolute() {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let card = path
            .canonicalize()
            .map_err(|error| format!("task card {card} cannot be read: {error}"))?;
        card.strip_prefix(&root)
            .map_err(|_| "task card must be a file inside the project root".to_owned())?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().ok_or("task card path is not UTF-8")?)
            }
            Component::CurDir => {}
            _ => return Err("task card path must stay inside the project root".into()),
        }
    }
    if parts.is_empty() {
        return Err("task card path names no file".into());
    }
    Ok(parts.join("/"))
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

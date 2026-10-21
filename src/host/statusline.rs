//! Claude Code `statusLine` command: reads the host's stdin JSON, opens the
//! display file for that `session_id` and prints one line for the human. It
//! never reads the Goal ledger or configuration, never fails visibly, and
//! prints nothing when there is no display state.

use serde_json::Value;
use std::io::Read;

const STDIN_MAX_BYTES: u64 = 256 * 1024;
const GOAL_CHARS: usize = 80;
const NEXT_CHARS: usize = 60;

pub(crate) fn run() -> Result<(), String> {
    let mut bytes = Vec::new();
    let _ = std::io::stdin()
        .take(STDIN_MAX_BYTES)
        .read_to_end(&mut bytes);
    if let Some(line) = line_for_input(&bytes) {
        println!("{line}");
    }
    Ok(())
}

fn line_for_input(bytes: &[u8]) -> Option<String> {
    let input: Value = serde_json::from_slice(bytes).ok()?;
    let session = input["session_id"].as_str()?;
    if session.is_empty() || session.len() > 512 || session.chars().any(char::is_control) {
        return None;
    }
    let path = crate::session_goal::display::path_for("claude", session).ok()?;
    line(&crate::session_goal::display::read(&path)?)
}

/// Terminal-safe text cut to `limit` characters with an ellipsis.
fn cut(text: &str, limit: usize) -> String {
    let clean = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= limit {
        return clean;
    }
    let mut out = clean.chars().take(limit - 1).collect::<String>();
    out.push('…');
    out
}

/// The one status line for a display state; `None` when there is no Goal.
pub(crate) fn line(state: &Value) -> Option<String> {
    let goal = state["goal"]
        .as_str()
        .filter(|goal| !goal.trim().is_empty())?;
    let mut parts = vec![format!("◎ {}", cut(goal, GOAL_CHARS))];
    let total = state["total"].as_u64().unwrap_or(0);
    match state["state"].as_str()? {
        "closed" => parts.push("closed".into()),
        "open" => {
            if total > 0 {
                parts.push(format!(
                    "S{}/{total} met",
                    state["met"].as_u64().unwrap_or(0)
                ));
            }
            if let Some(next) = state["nextAction"].as_str() {
                parts.push(format!("next {}", cut(next, NEXT_CHARS)));
            }
        }
        _ => return None,
    }
    Some(parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_goal_line_cuts_the_goal_and_names_progress_and_next_action() {
        let goal = "g".repeat(100);
        let rendered = line(&json!({"state":"open","goal":goal,"met":3,"total":10,
            "nextAction":"S3: Probe result recorded"}))
        .unwrap();
        assert_eq!(
            rendered,
            format!(
                "◎ {}… · S3/10 met · next S3: Probe result recorded",
                "g".repeat(79)
            )
        );
    }

    #[test]
    fn closed_goal_reads_closed_and_absent_goal_prints_nothing() {
        let rendered =
            line(&json!({"state":"closed","goal":"Ship it","met":0,"total":10})).unwrap();
        assert_eq!(rendered, "◎ Ship it · closed");
        assert!(line(&json!({"state":"none"})).is_none());
        assert!(line_for_input(b"not json").is_none());
        assert!(line_for_input(b"{\"session_id\":\"\"}").is_none());
    }

    #[test]
    fn control_characters_never_reach_the_terminal() {
        let rendered = line(&json!({"state":"open","goal":"a\u{1b}[31mb\nc","total":0})).unwrap();
        assert_eq!(rendered, "◎ a [31mb c");
    }
}

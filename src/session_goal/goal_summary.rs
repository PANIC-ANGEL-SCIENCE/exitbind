//! Lead-facing Goal facts: how the native journey closes, read from the
//! journey itself, and the project-lesson state. The same facts are appended
//! to the shared Goal system text (`goal status`, Work responses), the
//! `setup --apply` response and the SessionStart hook text. They are plain
//! statements of fact, bounded so the compact Goal text keeps its limit.

use crate::config::Loaded;
use serde_json::Value;

/// The compact Work projection keeps a system text up to this many characters.
const SYSTEM_TEXT_MAX_CHARS: usize = 512;
const READ_ROUTE: &str = "exitbind goal status --json (goalProgress.journeyCompletion.unresolved)";
const CLOSE_ROUTES: &str =
    "closes when each member is accepted and current, or by one final --journey-integration Work";

fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

/// The journey fact, or `None` for a Goal without native journey membership.
/// `completion` is the already computed `journeyCompletion` when available;
/// without it only the recorded membership is stated.
pub(crate) fn journey(record: &Value, completion: Option<&Value>, short: bool) -> Option<String> {
    let state = super::journey::state(record).ok().flatten()?;
    Some(journey_text(
        state.members.len(),
        state.admissions.len(),
        completion,
        short,
    ))
}

fn journey_text(
    members: usize,
    admissions: usize,
    completion: Option<&Value>,
    short: bool,
) -> String {
    let mut text = format!("Journey: {}", plural(members, "member"));
    if admissions > 0 {
        text.push_str(&format!(", {}", plural(admissions, "pending admission")));
    }
    if short {
        text.push_str(&format!("; read: {READ_ROUTE}"));
        return text;
    }
    if members == 0 && admissions == 0 {
        text.push_str(&format!(
            "; each Work begun in this conversation joins it; {CLOSE_ROUTES}"
        ));
        return text;
    }
    let Some(completion) = completion else {
        text.push_str(&format!("; {CLOSE_ROUTES}; not yet current: {READ_ROUTE}"));
        return text;
    };
    let current = completion["members"]["current"].as_u64().unwrap_or(0)
        + completion["members"]["integrationSupported"]
            .as_u64()
            .unwrap_or(0);
    text.push_str(&format!(", {current} current"));
    let stale = completion["unresolved"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item["state"] == "stale");
    if completion["state"] == "current" {
        text.push_str("; closing route met: every member is current");
    } else if let Some(work) = completion["integrationWork"].as_str() {
        text.push_str(&format!(
            "; closes by integration Work {work} once every member is covered; not yet current: {READ_ROUTE}"
        ));
    } else if stale {
        text.push_str(&format!(
            "; a member's accepted result is stale, so it closes by one final --journey-integration Work; not yet current: {READ_ROUTE}"
        ));
    } else {
        text.push_str(&format!("; {CLOSE_ROUTES}; not yet current: {READ_ROUTE}"));
    }
    text
}

/// The project-lesson fact for the Goal's task text.
pub(crate) fn lessons(loaded: &Loaded, task: Option<&str>) -> String {
    match crate::memory::lessons::deliverable(loaded, task) {
        None => "Lessons: none configured".into(),
        Some(Ok(count)) => format!("Lessons: {count} deliverable"),
        Some(Err(_)) => format!(
            "Lessons: configured; read: exitbind memory resolve {} --json",
            loaded.lead().unwrap_or("lead")
        ),
    }
}

fn joined(parts: &[Option<&str>]) -> String {
    parts
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Append both facts to the view's system text, within the compact bound.
pub(super) fn append(loaded: &Loaded, record: &Value, view: &mut Value) {
    let Some(text) = view["systemText"].as_str().map(str::to_owned) else {
        return;
    };
    let task = record["goal"]
        .as_str()
        .filter(|goal| !goal.trim().is_empty());
    if task.is_none() {
        return;
    }
    let lessons = lessons(loaded, task);
    let completion = view.get("journeyCompletion").cloned();
    let full = journey(record, completion.as_ref(), false);
    let mut out = joined(&[Some(&text), full.as_deref(), Some(&lessons)]);
    if out.chars().count() > SYSTEM_TEXT_MAX_CHARS {
        let short = journey(record, None, true);
        out = joined(&[Some(&text), short.as_deref(), Some(&lessons)]);
    }
    if out.chars().count() > SYSTEM_TEXT_MAX_CHARS {
        out = joined(&[Some(&text), Some(&lessons)]);
    }
    view["systemText"] = Value::String(out);
}

/// The SessionStart line for the native Goal: journey from its record, and
/// lessons from the verified owner configuration when one is available.
pub(crate) fn hook_line(record: &Value, owner: Option<&Loaded>) -> Option<String> {
    let journey = journey(record, None, false);
    let lessons = owner.map(|loaded| lessons(loaded, record["goal"].as_str()));
    if journey.is_none() && lessons.is_none() {
        return None;
    }
    Some(format!(
        "Goal {}: {}",
        record["goalId"].as_str().unwrap_or("unknown"),
        joined(&[journey.as_deref(), lessons.as_deref()])
    ))
}

/// The `setup --apply` response line: the current Goal's system text in the
/// scope `goal status` reads, or the lesson fact when no Goal exists yet.
pub(crate) fn setup_text(loaded: &Loaded) -> Result<String, String> {
    let scope = super::native::scope(loaded, false)?;
    if !scope.is_session() || scope.owner_matches {
        if let Some(record) = super::read(&scope.root)? {
            let presentation =
                super::presentation_for_selected(loaded, Some(&record), &scope.root, false)?;
            if let Some(text) = presentation["goalProgress"]["systemText"].as_str() {
                return Ok(text.to_owned());
            }
        }
    }
    Ok(format!(
        "No Goal is incorporated yet | {}",
        lessons(loaded, None)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_goal_without_journey_membership_states_no_journey() {
        assert!(journey(&json!({"goal":"g"}), None, false).is_none());
    }

    #[test]
    fn the_closing_route_follows_the_journey_completion() {
        let stale = json!({"state":"blocked","members":{"current":1,"integrationSupported":0},
            "unresolved":[{"work":"smw_1","state":"stale"}],"integrationWork":null});
        let current = json!({"state":"current","members":{"current":2,"integrationSupported":0},
            "unresolved":[],"integrationWork":null});
        let open = json!({"state":"blocked","members":{"current":0,"integrationSupported":0},
            "unresolved":[{"work":"smw_0","state":"unresolved"}],"integrationWork":null});
        let integrated = json!({"state":"blocked","members":{"current":0,"integrationSupported":1},
            "unresolved":[{"work":"smw_0","state":"unresolved"}],"integrationWork":"smw_i"});
        let text = journey_text(2, 0, Some(&stale), false);
        assert!(text.starts_with("Journey: 2 members, 1 current;"), "{text}");
        assert!(
            text.contains("closes by one final --journey-integration Work"),
            "{text}"
        );
        let text = journey_text(2, 0, Some(&current), false);
        assert!(text.contains("every member is current"), "{text}");
        assert!(!text.contains(READ_ROUTE), "{text}");
        let text = journey_text(1, 1, Some(&open), false);
        assert!(
            text.starts_with("Journey: 1 member, 1 pending admission, 0 current;"),
            "{text}"
        );
        assert!(
            text.contains(CLOSE_ROUTES) && text.contains(READ_ROUTE),
            "{text}"
        );
        let text = journey_text(1, 0, Some(&integrated), false);
        assert!(text.contains("closes by integration Work smw_i"), "{text}");
        let text = journey_text(0, 0, None, false);
        assert!(
            text.contains("each Work begun in this conversation joins it"),
            "{text}"
        );
        let text = journey_text(3, 0, None, false);
        assert!(
            text.contains(CLOSE_ROUTES) && text.contains(READ_ROUTE),
            "{text}"
        );
        assert!(journey_text(32, 0, Some(&open), false).chars().count() < 260);
    }
}

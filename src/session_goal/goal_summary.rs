// Projects Lead-facing Goal closure and journey facts.
// Open records have no closed revision; see `src/session_goal.rs`.
//! Lead-facing Goal facts: how the native journey closes, read from the
//! journey itself, and the project-lesson state. The same facts are appended
//! to the shared Goal system text (`goal status`, Work responses), the
//! `setup --apply` response and the SessionStart hook text. They are plain
//! statements of fact, bounded so the compact Goal text keeps its limit.

use crate::config::Loaded;
use serde_json::Value;

/// The compact Work projection keeps a system text up to this many characters.
const SYSTEM_TEXT_MAX_CHARS: usize = 512;
/// The read route for the journey's unresolved members, printed with the
/// executable this process runs as.
fn read_route() -> String {
    format!(
        "{} (goalProgress.journeyCompletion.unresolved)",
        crate::emitted::command_line(&["goal", "status", "--json"], None)
    )
}
const CLOSE_ROUTES: &str =
    "closes when each member is accepted and current, or by one final --journey-integration Work";

fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

/// The revision a closed Goal record closed at, or `None` while it is open.
/// A reopening admission resets `closure.closed`, so this reads open again.
pub(crate) fn closed_revision(record: &Value) -> Option<u64> {
    if record["closure"]["closed"] != true {
        return None;
    }
    Some(
        record["closure"]["revision"]
            .as_u64()
            .or_else(|| record["revision"].as_u64())
            .unwrap_or(0),
    )
}

/// A closed Goal whose only open journey items are members accepted at
/// binding stays complete: a later change to a member's bound inputs is
/// reported on the member, not by reopening the Goal.
pub(crate) fn closed_at_binding(record: &Value, journey: Option<&Value>) -> bool {
    closed_revision(record).is_some()
        && journey.is_some_and(|value| {
            value["unresolved"].as_array().is_some_and(|items| {
                !items.is_empty()
                    && items
                        .iter()
                        .all(|item| item["state"] == "accepted_at_binding")
            })
        })
}

/// A complete, closed Goal's system text head reads closed with its revision.
pub(crate) fn closed_head(record: &Value, view: &mut Value) {
    let Some(revision) = closed_revision(record) else {
        return;
    };
    if view["overall"] != "complete" {
        return;
    }
    if let Some(text) = view["systemText"].as_str() {
        let head = format!("Goal: CLOSED (revision {revision})");
        view["systemText"] = Value::String(text.replacen("Goal: COMPLETE", &head, 1));
    }
}

/// The journey fact, or `None` for a Goal without native journey membership.
/// `completion` is the already computed `journeyCompletion` when available;
/// without it only the recorded membership is stated. A closed Goal states
/// only its member count: there is no route left to close or read.
/// `tasks` is the registered decomposition (`complete`, `total`) when known:
/// the closing route is never stated as met while a registered task is not.
pub(crate) fn journey(
    record: &Value,
    completion: Option<&Value>,
    short: bool,
    tasks: Option<(u64, u64)>,
) -> Option<String> {
    let state = super::journey::state(record).ok().flatten()?;
    if let Some(revision) = closed_revision(record) {
        return Some(format!(
            "Journey: {}; closed with the Goal (revision {revision})",
            plural(state.members.len(), "member")
        ));
    }
    Some(journey_text(
        state.members.len(),
        state.admissions.len(),
        completion,
        short,
        tasks,
    ))
}

/// The registered decomposition of a Goal view, as (`complete`, `total`).
pub(crate) fn task_counts(view: &Value) -> Option<(u64, u64)> {
    let decomposition = &view["decomposition"];
    (decomposition["available"] == true).then(|| {
        (
            decomposition["complete"].as_u64().unwrap_or(0),
            decomposition["total"].as_u64().unwrap_or(0),
        )
    })
}

fn journey_text(
    members: usize,
    admissions: usize,
    completion: Option<&Value>,
    short: bool,
    tasks: Option<(u64, u64)>,
) -> String {
    let read_route = read_route();
    let mut text = format!("Journey: {}", plural(members, "member"));
    if admissions > 0 {
        text.push_str(&format!(", {}", plural(admissions, "pending admission")));
    }
    if short {
        text.push_str(&format!("; read: {read_route}"));
        return text;
    }
    if members == 0 && admissions == 0 {
        text.push_str(&format!(
            "; each Work begun in this conversation joins it; {CLOSE_ROUTES}"
        ));
        return text;
    }
    let Some(completion) = completion else {
        text.push_str(&format!("; {CLOSE_ROUTES}; not yet current: {read_route}"));
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
    let unmet = tasks.filter(|(complete, total)| complete < total);
    if let (Some((complete, total)), "current") =
        (unmet, completion["state"].as_str().unwrap_or(""))
    {
        text.push_str(&format!(
            "; every member is current; closing route not met: {} of {total} registered tasks are not complete",
            total - complete
        ));
    } else if completion["state"] == "current" {
        text.push_str("; closing route met: every member is current");
    } else if let Some(work) = completion["integrationWork"].as_str() {
        text.push_str(&format!(
            "; closes by integration Work {work} once every member is covered; not yet current: {read_route}"
        ));
    } else if stale {
        text.push_str(&format!(
            "; a member's accepted result is stale, so it closes by one final --journey-integration Work; not yet current: {read_route}"
        ));
    } else {
        text.push_str(&format!("; {CLOSE_ROUTES}; not yet current: {read_route}"));
    }
    text
}

/// The project-lesson fact for the Goal's task text.
pub(crate) fn lessons(loaded: &Loaded, task: Option<&str>) -> String {
    match crate::memory::lessons::deliverable(loaded, task) {
        None => "Lessons: none configured".into(),
        Some(Ok(count)) => format!("Lessons: {count} deliverable"),
        Some(Err(_)) => format!(
            "Lessons: configured; read: {}",
            crate::emitted::command_line(
                &[
                    "memory",
                    "resolve",
                    loaded.lead().unwrap_or("lead"),
                    "--json"
                ],
                Some(&loaded.path)
            )
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
    let tasks = task_counts(view);
    let full = journey(record, completion.as_ref(), false, tasks);
    let mut out = joined(&[Some(&text), full.as_deref(), Some(&lessons)]);
    if out.chars().count() > SYSTEM_TEXT_MAX_CHARS {
        let short = journey(record, None, true, tasks);
        out = joined(&[Some(&text), short.as_deref(), Some(&lessons)]);
    }
    if out.chars().count() > SYSTEM_TEXT_MAX_CHARS {
        out = joined(&[Some(&text), Some(&lessons)]);
    }
    view["systemText"] = Value::String(out);
}

/// The SessionStart line for the native Goal, with complete requirements
/// preferred over optional journey and lesson detail within the byte budget.
pub(crate) fn hook_line(
    record: &Value,
    owner: Option<&Loaded>,
    max_bytes: usize,
) -> Option<String> {
    let full = journey(record, None, false, None);
    let lessons = owner.map(|loaded| lessons(loaded, record["goal"].as_str()));
    let unmapped = unmapped_text(record);
    if full.is_none() && lessons.is_none() && unmapped.is_none() {
        return None;
    }
    let head = format!("Goal {}: ", record["goalId"].as_str().unwrap_or("unknown"));
    let line = |journey: Option<&str>, lessons: Option<&str>| {
        let facts = joined(&[journey, unmapped.as_deref(), lessons]);
        (!facts.is_empty()).then(|| format!("{head}{facts}"))
    };
    let complete = line(full.as_deref(), lessons.as_deref());
    if complete
        .as_ref()
        .is_some_and(|value| value.len() <= max_bytes)
    {
        return complete;
    }
    let short = journey(record, None, true, None);
    for (journey, lessons) in [
        (short.as_deref(), lessons.as_deref()),
        (None, lessons.as_deref()),
        (None, None),
    ] {
        let candidate = line(journey, lessons);
        if candidate
            .as_ref()
            .is_some_and(|value| value.len() <= max_bytes)
        {
            return candidate;
        }
    }
    hook_omission(
        &head,
        unmapped.is_some(),
        owner.map(|loaded| loaded.path.as_path()),
        max_bytes,
    )
}

fn hook_omission(
    head: &str,
    unmapped: bool,
    config: Option<&std::path::Path>,
    max_bytes: usize,
) -> Option<String> {
    let subject = if unmapped {
        "Requirements without a Work omitted"
    } else {
        "Goal facts omitted"
    };
    if let Some(config) = config {
        let route = crate::emitted::Route::new(["goal", "status", "--json"]).config(config);
        let bounded = route.within(max_bytes);
        if !bounded.needs_config() && !bounded.needs_executable() {
            for prefix in [head, ""] {
                let line = format!("{prefix}{subject}; read `{}`.", route.shell_text());
                if line.len() <= max_bytes {
                    return Some(line);
                }
            }
        }
    }
    let reason = if config.is_some() {
        "exact retrieval route omitted; retain the current executable and owner configuration"
    } else {
        "exact retrieval route unavailable without the owner configuration"
    };
    for prefix in [head, ""] {
        let line = format!("{prefix}{subject}; {reason}.");
        if line.len() <= max_bytes {
            return Some(line);
        }
    }
    None
}

/// `Requirements without a Work: R2 (source S), R3 (source S)`, or nothing.
pub(crate) fn unmapped_text(record: &Value) -> Option<String> {
    let items = super::requirements::unmapped(record);
    (!items.is_empty()).then(|| {
        let items = items
            .iter()
            .map(|(id, source)| format!("{id} (source {source})"))
            .collect::<Vec<_>>();
        format!("Requirements without a Work: {}", items.join(", "))
    })
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

    fn member(n: char) -> Value {
        let sha = |c: char| c.to_string().repeat(64);
        json!({"work":format!("smw_{}", sha(n)),"startEventSha256":sha('a'),
            "parentGoalId":"goal","parentObservedGoalEventSha256":sha('b'),
            "workGoalSha256":sha('c'),"host":"claude","sessionKey":sha('d'),
            "ownerIdentitySha256":sha('e'),"ownerProjectId":null,
            "projectIdentitySha256":sha('f'),"localIdentitySha256":sha('a'),
            "configPath":"/tmp/p/exitbind.json","configSha256":sha('b'),
            "productRoot":"/tmp/p","stateRoot":"/tmp/p/.exitbind"})
    }

    #[test]
    fn a_closed_goal_states_its_journey_closed_without_any_route() {
        let mut record = json!({"goal":"g","goalId":"goal","revision":30,
            "journeyMembershipProtocol":2,"journeyMembers":[member('1'), member('2')],
            "journeyIntegrations":[],"journeyAdmissions":[],
            "closure":{"closed":true,"revision":30,"resultRefs":[]}});
        let open = json!({"state":"blocked","members":{"current":0,"integrationSupported":0},
            "unresolved":[{"work":"smw_0","state":"unavailable"}],"integrationWork":null});
        let expected = "Journey: 2 members; closed with the Goal (revision 30)";
        for (completion, short) in [(None, false), (Some(&open), false), (None, true)] {
            let text = journey(&record, completion, short, None).unwrap();
            assert_eq!(text, expected);
            assert!(!text.contains("not yet current") && !text.contains("closes"));
            assert!(!text.contains(&read_route()) && !text.contains("read:"));
        }
        let line = hook_line(&record, None, 384).unwrap();
        assert_eq!(line, format!("Goal goal: {expected}"));
        // A reopening admission resets closure, and the open text returns.
        record["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
        assert!(closed_revision(&record).is_none());
        let text = journey(&record, None, false, None).unwrap();
        assert!(text.contains("not yet current"), "{text}");
    }

    #[test]
    fn a_goal_without_journey_membership_states_no_journey() {
        assert!(journey(&json!({"goal":"g"}), None, false, None).is_none());
    }

    #[test]
    fn hook_omission_names_exact_owner_route_or_truthfully_reports_it_unavailable() {
        let config = std::path::Path::new("/tmp/project/exitbind.json");
        let route = crate::emitted::command_line(&["goal", "status", "--json"], Some(config));
        let full = hook_omission("Goal goal: ", true, Some(config), 384).unwrap();
        assert!(full.contains(&route), "bytes={}: {full}", full.len());
        assert!(full.contains("Requirements without a Work omitted"));
        let long = format!("/tmp/{}/exitbind.json", "a".repeat(500));
        let omitted =
            hook_omission("Goal goal: ", true, Some(std::path::Path::new(&long)), 384).unwrap();
        assert!(
            omitted.contains("exact retrieval route omitted"),
            "{omitted}"
        );
        assert!(
            omitted.contains("current executable and owner configuration"),
            "{omitted}"
        );
        assert!(!omitted.contains(&long), "{omitted}");
        let unavailable = hook_omission("Goal goal: ", false, None, 384).unwrap();
        assert!(
            unavailable
                .contains("exact retrieval route unavailable without the owner configuration"),
            "{unavailable}"
        );
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
        let text = journey_text(2, 0, Some(&stale), false, None);
        assert!(text.starts_with("Journey: 2 members, 1 current;"), "{text}");
        assert!(
            text.contains("closes by one final --journey-integration Work"),
            "{text}"
        );
        let text = journey_text(2, 0, Some(&current), false, None);
        assert!(text.contains("every member is current"), "{text}");
        assert!(!text.contains(&read_route()), "{text}");
        let text = journey_text(1, 1, Some(&open), false, None);
        assert!(
            text.starts_with("Journey: 1 member, 1 pending admission, 0 current;"),
            "{text}"
        );
        assert!(
            text.contains(CLOSE_ROUTES) && text.contains(&read_route()),
            "{text}"
        );
        let text = journey_text(1, 0, Some(&integrated), false, None);
        assert!(text.contains("closes by integration Work smw_i"), "{text}");
        let text = journey_text(0, 0, None, false, None);
        assert!(
            text.contains("each Work begun in this conversation joins it"),
            "{text}"
        );
        let text = journey_text(3, 0, None, false, None);
        assert!(
            text.contains(CLOSE_ROUTES) && text.contains(&read_route()),
            "{text}"
        );
        // The bound assumed an eight-byte product name; the route now names
        // the executable this process runs as.
        let executable = crate::emitted::executable().unwrap().len();
        assert!(
            journey_text(32, 0, Some(&open), false, None)
                .chars()
                .count()
                < 252 + executable
        );
        // Every member current never reads as met while a registered task is open.
        let text = journey_text(1, 0, Some(&current), false, Some((0, 11)));
        assert!(
            text.contains("closing route not met: 11 of 11 registered tasks are not complete")
                && !text.contains("closing route met"),
            "{text}"
        );
        let text = journey_text(1, 0, Some(&current), false, Some((11, 11)));
        assert!(text.contains("closing route met"), "{text}");
    }
}

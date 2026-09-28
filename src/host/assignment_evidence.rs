//! Evidence read routes presented to a native child with its assignment.
//!
//! The child receives references it can resolve itself through work expand,
//! never evidence content or a Lead's summary of it. References come from the
//! same validated assignment context the packet digest covers, so a stale or
//! foreign reference is never offered as current evidence.

use crate::config::Loaded;
use serde_json::{json, Value};
use std::path::Path;

pub(crate) const MAX_CONTEXT_OUTPUT: usize = 16 * 1024;
const MAX_ENTRIES: usize = 8;

pub(crate) fn lines(loaded: &Loaded, work: &str, context: &Value) -> Vec<String> {
    let entries = context["evidence"].as_array().cloned().unwrap_or_default();
    let indices = preview_indices(&entries);
    let mut lines = vec![format!(
        "Evidence for this assignment (current subject SHA-256 {}). Resolve each reference yourself; a summary from the Lead is not evidence. A passing check is evidence to consider, not an approval:",
        text(&context["subject"]["sha256"])
    )];
    let has_checks = entries.iter().any(is_check);

    for index in &indices {
        let entry = &entries[*index];
        let kind = entry["kind"].as_str().unwrap_or("event");
        if is_check(entry) {
            let label = if kind == "preservation" {
                "preservation check"
            } else {
                "check"
            };
            let requirement = entry["requirementId"]
                .as_str()
                .map(|id| format!(", requirement {id}"))
                .unwrap_or_default();
            lines.push(format!(
                "- {label} {} ({}{}), checker {} (SHA-256 {}), acquisition {}, origin {}, event {}",
                text(&entry["status"]),
                text(&entry["freshness"]),
                requirement,
                text(&entry["checker"]),
                text(&entry["checkerSha256"]),
                text(&entry["acquisition"]),
                text(&entry["origin"]),
                text(&entry["checkEventSha256"]),
            ));
            lines.push(read_route_line(
                loaded,
                work,
                &entry["rawEventRef"],
                &format!("{label} target event"),
            ));
            lines.push(read_route_line(
                loaded,
                work,
                &entry["checkEventRef"],
                &format!("{label} record"),
            ));
            if entry["checkLogRef"].is_object() {
                lines.push(read_route_line(
                    loaded,
                    work,
                    &entry["checkLogRef"],
                    &format!("{label} log"),
                ));
            } else {
                lines.push(format!(
                    "  {label} log route unavailable (log status {}).",
                    text(&entry["logStatus"])
                ));
            }
        } else {
            lines.push(read_route_line(
                loaded,
                work,
                &entry["rawEventRef"],
                &format!("{kind} event"),
            ));
        }
    }

    if entries.len() > indices.len() {
        lines.push(format!(
            "- {} evidence entries are not shown in this preview; use the full assignment packet route.",
            entries.len() - indices.len()
        ));
    }
    if !has_checks {
        lines.push("- No check record belongs to this assignment yet.".into());
    }
    let packet_route = exact_route(
        loaded,
        vec!["work".into(), "next".into(), work.into(), "--full".into()],
    );
    lines.push(match packet_route {
        Some(route) => format!("Full assignment packet route: {route}"),
        None => "Full assignment packet route unavailable: executable or configuration path cannot be represented exactly.".into(),
    });
    lines
}

fn preview_indices(entries: &[Value]) -> Vec<usize> {
    let mut prioritized = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry["kind"] == "preservation")
        .map(|(index, _)| index)
        .chain(
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry["kind"] == "check")
                .map(|(index, _)| index),
        )
        .chain(
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| !is_check(entry))
                .map(|(index, _)| index),
        )
        .take(MAX_ENTRIES)
        .collect::<Vec<_>>();
    prioritized.sort_unstable();
    prioritized
}

fn is_check(entry: &Value) -> bool {
    matches!(entry["kind"].as_str(), Some("check" | "preservation"))
}

fn read_route_line(loaded: &Loaded, work: &str, reference: &Value, label: &str) -> String {
    let Some(id) = reference["id"].as_str().filter(|id| {
        id.starts_with("ref:") && id.len() <= 80 && id.bytes().all(|byte| byte.is_ascii_graphic())
    }) else {
        return format!("  {label} route unavailable.");
    };
    let route = exact_route(
        loaded,
        vec!["work".into(), "expand".into(), work.into(), id.to_owned()],
    );
    match route {
        Some(route) => format!("  {label} route: {route}"),
        None => format!("  {label} route unavailable: executable or configuration path cannot be represented exactly."),
    }
}

fn exact_route(loaded: &Loaded, suffix: Vec<String>) -> Option<String> {
    let route = crate::work::compact::continuation_route(&loaded.path, suffix);
    if route["sameConfigRequired"] != false || route["sameExecutableRequired"] != false {
        return None;
    }
    let argv = route["command"].as_array()?;
    let executable = argv.first()?.as_str()?;
    if !Path::new(executable).is_absolute() {
        return None;
    }
    let config = loaded.path.to_str()?;
    if !argv
        .windows(2)
        .any(|pair| pair[0].as_str() == Some("--config") && pair[1].as_str() == Some(config))
    {
        return None;
    }
    let cwd = loaded.product_root.to_str()?;
    let mut invocation = json!({"argv": route["command"], "cwd": cwd});
    let mut environment = serde_json::Map::new();
    if loaded.mode == crate::project::layout_types::Mode::Local {
        for name in [
            "EXITBIND_BINDINGS_DIR",
            "SOULMATE_BINDINGS_DIR",
            "XDG_STATE_HOME",
            "HOME",
        ] {
            if let Some(value) = std::env::var_os(name) {
                environment.insert(name.into(), json!(value.to_str()?));
            }
        }
    }
    if !environment.is_empty() {
        invocation["env"] = Value::Object(environment);
    }
    serde_json::to_string(&invocation).ok()
}

fn text(value: &Value) -> String {
    let raw = value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_null() {
            "unavailable".to_owned()
        } else {
            serde_json::to_string(value).unwrap_or_else(|_| "unavailable".to_owned())
        }
    });
    let mut clean = raw
        .chars()
        .map(|character| {
            if character.is_control() || character as u32 == 96 {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if clean.len() > 160 {
        let mut end = 157;
        while !clean.is_char_boundary(end) {
            end -= 1;
        }
        clean.truncate(end);
        clean.push_str("...");
    }
    clean
}

const PREVIEW_SHORTENED: &str =
    "Evidence preview shortened to fit; use the full assignment packet route.";

pub(crate) fn append_within_budget(base: &str, event: &str, lines: &[String]) -> Option<String> {
    if lines.len() < 2 {
        return Some(base.to_owned());
    }
    let header = &lines[0];
    let route = lines.last().expect("at least two evidence lines");
    let mut selected = vec![header.clone(), route.clone()];
    let mut shortened = false;
    if !fits(base, event, &selected) {
        selected = vec![route.clone()];
        shortened = true;
        if !fits(base, event, &selected) {
            return None;
        }
    }

    for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
        let mut candidate = selected.clone();
        candidate.insert(candidate.len() - 1, line.clone());
        if fits(base, event, &candidate) {
            selected = candidate;
        } else {
            shortened = true;
        }
    }
    if shortened {
        let mut candidate = selected.clone();
        candidate.insert(candidate.len() - 1, PREVIEW_SHORTENED.into());
        if fits(base, event, &candidate) {
            selected = candidate;
        } else {
            return None;
        }
    }
    Some(append(base, &selected))
}

fn fits(base: &str, event: &str, suffix: &[String]) -> bool {
    let candidate = append(base, suffix);
    serde_json::to_vec(&json!({"hookSpecificOutput":{
        "hookEventName":event,"additionalContext":candidate
    }}))
    .is_ok_and(|bytes| bytes.len() <= MAX_CONTEXT_OUTPUT)
}

fn append(base: &str, suffix: &[String]) -> String {
    if suffix.is_empty() {
        return base.to_owned();
    }
    format!("{base}\n{}", suffix.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialized_len(event: &str, context: &str) -> usize {
        serde_json::to_vec(&json!({"hookSpecificOutput":{
            "hookEventName":event,"additionalContext":context
        }}))
        .unwrap()
        .len()
    }

    #[test]
    fn near_limit_profile_and_perspective_keep_the_full_packet_route() {
        let profile = "p".repeat(11_500);
        let perspective = "x".repeat(3_400);
        let base = format!(
            "Profile bytes:\n{profile}\nSelected task perspectives:\n{perspective}\nPROFILE_END\nPERSPECTIVE_END"
        );
        assert!(serialized_len("SubagentStart", &base) < MAX_CONTEXT_OUTPUT);
        let route = r#"Full assignment packet route: {"argv":["/tmp/exitbind","work","next","smw_test","--full","--config","/tmp/config.json"],"cwd":"/tmp/project"}"#;
        let lines = vec![
            "Evidence for this assignment.".to_owned(),
            format!("{} preview detail one", "e".repeat(700)),
            format!("{} preview detail two", "e".repeat(700)),
            format!("{} preview detail three", "e".repeat(700)),
            route.to_owned(),
        ];

        let context = append_within_budget(&base, "SubagentStart", &lines).unwrap();

        assert!(context.contains("PROFILE_END"));
        assert!(context.contains("PERSPECTIVE_END"));
        assert!(context.contains(route));
        assert!(context.contains(PREVIEW_SHORTENED));
        assert!(serialized_len("SubagentStart", &context) <= MAX_CONTEXT_OUTPUT);
    }

    #[test]
    fn full_packet_route_that_cannot_fit_returns_unavailable() {
        let event = "SubagentStart";
        let route = "Full assignment packet route: exact-route".to_owned();
        let mut base = String::new();
        while serialized_len(event, &base) < MAX_CONTEXT_OUTPUT - 1 {
            base.push('p');
        }
        assert_eq!(serialized_len(event, &base), MAX_CONTEXT_OUTPUT - 1);
        let lines = vec!["Evidence for this assignment.".to_owned(), route];

        assert!(append_within_budget(&base, event, &lines).is_none());
    }

    #[test]
    fn omitted_preview_details_require_a_fitting_shortened_notice() {
        let event = "SubagentStart";
        let route = "Full assignment packet route: exact-route".to_owned();
        let route_only = vec![route.clone()];
        let notice_and_route = vec![PREVIEW_SHORTENED.to_owned(), route.clone()];
        let mut base = String::new();
        while fits(&base, event, &notice_and_route) {
            base.push('p');
        }
        assert!(fits(&base, event, &route_only));

        let lines = vec![
            "Evidence for this assignment.".to_owned(),
            "optional evidence detail".to_owned(),
            route,
        ];
        assert!(append_within_budget(&base, event, &lines).is_none());
    }

    #[test]
    fn preservation_checks_keep_preview_slots_after_ordinary_checks() {
        let mut entries = (0..10).map(|_| json!({"kind":"check"})).collect::<Vec<_>>();
        entries.push(json!({"kind":"preservation"}));

        let shown = preview_indices(&entries);

        assert_eq!(shown.len(), MAX_ENTRIES);
        assert!(shown.contains(&10));
        assert_eq!(shown, [0, 1, 2, 3, 4, 5, 6, 10]);
        assert!(entries.iter().any(is_check));
    }
}

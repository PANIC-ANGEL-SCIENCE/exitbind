//! Plain-text rendering of `work detail WORK --view action`.
//!
//! The JSON view stays the machine contract (`--json`). This rendering shows
//! the same current action, bound commands and section routes as readable
//! lines so a reader needs no JSON or hex tooling to act on it.
use serde_json::{json, Value};

pub(crate) fn render(view: &Value) -> String {
    let action = &view["effectiveAction"];
    let decision = &view["decision"];
    let mut lines = Vec::new();
    lines.push(format!(
        "Work {} ({})",
        text(&view["work"]),
        text(&decision["readiness"])
    ));
    if let Some(phase) = decision["phase"].as_object() {
        lines.push(format!(
            "Stage {}, attempt {}, {}",
            text(&phase.get("stage").cloned().unwrap_or(Value::Null)),
            text(&phase.get("attempt").cloned().unwrap_or(Value::Null)),
            text(&phase.get("status").cloned().unwrap_or(Value::Null))
        ));
    }
    lines.push(match action["delegation"]["text"].as_str() {
        Some(delegation) => delegation.to_owned(),
        None => match action["state"].as_str() {
            Some("action") => format!("next action: {}", text(&action["kind"])),
            Some("blocked") => format!("next action: blocked ({})", compact(&action["blocker"])),
            Some(state) => format!("next action: {state}"),
            None => "next action: unknown".to_owned(),
        },
    });
    if let Some(actor) = action["actor"].as_str() {
        lines.push(format!("Actor: {actor}"));
    }
    if let Some(grant) = action["currentGrant"].as_object() {
        if let Some(operation) = grant.get("operation").and_then(Value::as_str) {
            lines.push(format!("Current edit grant: {operation}"));
        }
    }
    push_form(&mut lines, "Before editing", &action["beforeEditing"]);
    push_form(&mut lines, "Next request", &action["nextRequest"]);
    push_form(&mut lines, "Command", &action["command"]);
    if let Some(choices) = action["choices"].as_array() {
        for choice in choices {
            let label = format!("Choice {}", text(&choice["label"]));
            push_form(&mut lines, &label, choice);
        }
    }
    push_guidance(&mut lines, &view["guidance"]);
    if let Some(routes) = view["details"].as_object() {
        lines.push("Complete sections (read-only):".to_owned());
        for (name, route) in routes {
            lines.push(format!("  {name}: {}", argv(&route["command"])));
        }
    }
    if let Some(warnings) = action["warnings"]
        .as_array()
        .filter(|items| !items.is_empty())
    {
        for warning in warnings {
            lines.push(format!("Warning: {}", compact(warning)));
        }
    }
    lines.push("Machine form: add --json".to_owned());
    let mut rendered = lines.join("\n");
    rendered.push('\n');
    rendered
}

/// Every delivered item whole with its read route, then every omitted item
/// with its reason and route; the same items as `guidance` in the JSON form.
fn push_guidance(lines: &mut Vec<String>, guidance: &Value) {
    if !guidance.is_object() {
        return;
    }
    if guidance["available"] == false {
        lines.push(format!(
            "Delivered guidance unavailable ({}); read route: {}",
            text(&guidance["reason"]),
            route(&guidance["readRoute"])
        ));
        return;
    }
    let phases = guidance["phases"]
        .as_array()
        .map(|phases| phases.iter().map(text).collect::<Vec<_>>().join(", "));
    lines.push(format!(
        "Delivered guidance for {}{}: {} item(s) whole, {} omitted; read before dependent work",
        text(&guidance["agent"]),
        phases.map_or_else(String::new, |phases| format!(" (phases: {phases})")),
        text(&guidance["itemCount"]),
        text(&guidance["omittedCount"])
    ));
    for item in guidance["items"].as_array().into_iter().flatten() {
        let id = item_name(item);
        match item["kind"].as_str() {
            Some("project_lesson") => {
                lines.push(format!("Lesson {id}: {}", text(&item["content"])));
                lines.push(format!("  read route: {}", route(&item["readRoute"])));
            }
            kind => {
                let phase = item["phase"]
                    .as_str()
                    .map_or_else(String::new, |phase| format!(", phase {phase}"));
                lines.push(format!(
                    "Guidance {} {id}{phase} ({} bytes, sha256 {}):",
                    kind.unwrap_or("memory"),
                    text(&item["byteLength"]),
                    text(&item["sourceSha256"])
                ));
                lines.push(format!("  read route: {}", route(&item["readRoute"])));
                lines.push(format!("----- begin {id} -----"));
                let body = text(&item["content"]);
                lines.push(body.strip_suffix('\n').unwrap_or(&body).to_owned());
                lines.push(format!("----- end {id} -----"));
            }
        }
    }
    for item in guidance["omitted"].as_array().into_iter().flatten() {
        lines.push(format!(
            "Omitted {} ({}): read route: {}",
            item_name(item),
            text(&item["reason"]),
            route(&item["readRoute"])
        ));
    }
}

fn item_name(item: &Value) -> String {
    text(if item["id"].is_string() {
        &item["id"]
    } else {
        &item["itemId"]
    })
}

/// A read route as one command line; a config path the bounded form left out
/// is marked rather than invented.
fn route(route: &Value) -> String {
    let mut rendered = argv(&json!({"argv": route["command"]}));
    if route["sameConfigRequired"] == true
        && route["command"]
            .as_array()
            .and_then(|argv| argv.last())
            .is_some_and(|last| last == "--config")
    {
        rendered.push_str(" <same config>");
    }
    rendered
}

fn push_form(lines: &mut Vec<String>, label: &str, form: &Value) {
    let command = if form["command"].is_object() {
        &form["command"]
    } else {
        form
    };
    if !command["argv"].is_array() {
        return;
    }
    lines.push(format!("{label}: {}", argv(command)));
    if let Some(placeholders) = form["placeholders"]
        .as_array()
        .filter(|items| !items.is_empty())
    {
        let names: Vec<_> = placeholders.iter().map(text).collect();
        lines.push(format!("  fill: {}", names.join(", ")));
    }
    if let Some(stdin) = form["stdin"]["description"].as_str() {
        lines.push(format!("  stdin: {stdin}"));
    }
    if let Some(meaning) = form["meaning"].as_str() {
        lines.push(format!("  meaning: {meaning}"));
    }
}

fn argv(command: &Value) -> String {
    let parts = command["argv"]
        .as_array()
        .or_else(|| command.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| quote(&text(item)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut rendered = parts.join(" ");
    if command["sameConfigRequired"] == true {
        rendered.push_str(" <same config>");
    }
    rendered
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=<>@%+,".contains(&byte));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "unknown".to_owned(),
        other => other.to_string(),
    }
}

fn compact(value: &Value) -> String {
    value["message"]
        .as_str()
        .or(value["code"].as_str())
        .or(value.as_str())
        .map_or_else(|| value.to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_view_renders_delegation_forms_and_routes_as_text() {
        let view = json!({"work":"smw_1","decision":{"readiness":"IN_PROGRESS",
            "phase":{"stage":2,"attempt":1,"status":"running"}},
            "effectiveAction":{"state":"action","kind":"spawn","actor":"worker",
                "delegation":{"text":"next action: delegate to worker agent 'worker' (assignment sma_1)"},
                "beforeEditing":{"command":{"argv":["/bin/exitbind","work","permit","smw_1","sma_1",
                    "--operation","<OPERATION>"],"sameConfigRequired":false},"placeholders":["OPERATION"]},
                "choices":[{"label":"completed","command":{"argv":["/bin/exitbind","work","return",
                    "--reason","<REASON>"]},"placeholders":["REASON"],
                    "stdin":{"description":"Pass the complete result artifact on stdin"}}]},
            "details":{"assignment":{"command":["/bin/exitbind","work","expand","smw_1","ref:a"]}}});
        let text = render(&view);
        assert!(text.contains("next action: delegate to worker agent 'worker' (assignment sma_1)"));
        assert!(text.contains(
            "Before editing: /bin/exitbind work permit smw_1 sma_1 --operation <OPERATION>"
        ));
        assert!(text.contains("Choice completed: /bin/exitbind work return --reason <REASON>"));
        assert!(text.contains("stdin: Pass the complete result artifact on stdin"));
        assert!(text.contains("  assignment: /bin/exitbind work expand smw_1 ref:a"));
        assert!(!text.trim_start().starts_with('{'));
        assert_eq!(quote("two words"), "'two words'");
    }
}

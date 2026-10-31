//! Plain-text rendering of `work detail WORK --view action`.
//!
//! The JSON view stays the machine contract (`--json`). This rendering shows
//! the same current action, bound commands and section routes as readable
//! lines so a reader needs no JSON or hex tooling to act on it. Every printed
//! command line names the full executable and configuration, also where the
//! JSON form left them out to stay within its byte bound.
use serde_json::{json, Value};
use std::path::Path;

/// The executable and configuration a bounded command left out.
struct Context {
    executable: String,
    config: String,
}

pub(crate) fn render(view: &Value, config: &Path) -> Result<String, String> {
    let context = Context {
        executable: crate::emitted::executable()?,
        config: config
            .to_str()
            .ok_or("configuration path is not valid UTF-8")?
            .to_owned(),
    };
    Ok(render_with(view, &context))
}

fn render_with(view: &Value, context: &Context) -> String {
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
    if matches!(
        action["formState"].as_str(),
        Some(
            "interrupted_pre_return"
                | "pre_return_retry_interrupted"
                | "pre_return_uncertain_result"
                | "pre_return_ambiguous"
                | "pre_return_evidence_unavailable"
        )
    ) {
        if let Some(meaning) = action["meaning"].as_str() {
            lines.push(format!("Status: {meaning}"));
        }
        if action["diagnostic"]["command"]["argv"].is_array() {
            lines.push(format!(
                "Inspect: {}",
                argv(context, &action["diagnostic"]["command"])
            ));
        }
    }
    if let Some(actor) = action["actor"].as_str() {
        lines.push(format!("Actor: {actor}"));
    }
    if action["leadChoiceRequired"] == true {
        lines.push(format!(
            "Lead choice: decided by lead agent '{}', who supplies every fill value",
            text(&action["choiceActor"]["agent"])
        ));
    }
    if let Some(grant) = action["currentGrant"].as_object() {
        if let Some(operation) = grant.get("operation").and_then(Value::as_str) {
            lines.push(format!("Current edit grant: {operation}"));
        }
    }
    let fill = &action["fillRoute"];
    push_form(
        &mut lines,
        context,
        fill,
        "Before editing",
        &action["beforeEditing"],
    );
    push_form(
        &mut lines,
        context,
        fill,
        "Next request",
        &action["nextRequest"],
    );
    push_form(&mut lines, context, fill, "Command", &action["command"]);
    if let Some(choices) = action["choices"].as_array() {
        for choice in choices {
            let label = format!("Choice {}", text(&choice["label"]));
            push_form(&mut lines, context, fill, &label, choice);
        }
    }
    push_guidance(&mut lines, context, &view["guidance"]);
    if let Some(routes) = view["details"].as_object() {
        lines.push("Complete sections (read-only):".to_owned());
        for (name, route) in routes {
            lines.push(format!("  {name}: {}", route_line(context, route)));
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
fn push_guidance(lines: &mut Vec<String>, context: &Context, guidance: &Value) {
    if !guidance.is_object() {
        return;
    }
    if guidance["available"] == false {
        lines.push(format!(
            "Delivered guidance unavailable ({}); read route: {}",
            text(&guidance["reason"]),
            route_line(context, &guidance["readRoute"])
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
                lines.push(format!(
                    "  read route: {}",
                    route_line(context, &item["readRoute"])
                ));
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
                lines.push(format!(
                    "  read route: {}",
                    route_line(context, &item["readRoute"])
                ));
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
            route_line(context, &item["readRoute"])
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

/// A route object (`command` argv plus its same-executable and same-config
/// markers) as one complete command line.
fn route_line(context: &Context, route: &Value) -> String {
    argv(
        context,
        &json!({"argv": route["command"],
            "sameExecutableRequired": route["sameExecutableRequired"],
            "sameConfigRequired": route["sameConfigRequired"]}),
    )
}

fn push_form(lines: &mut Vec<String>, context: &Context, fill: &Value, label: &str, form: &Value) {
    let command = if form["command"].is_object() {
        &form["command"]
    } else {
        form
    };
    if !command["argv"].is_array() {
        return;
    }
    lines.push(format!("{label}: {}", argv(context, command)));
    if let Some(placeholders) = form["placeholders"]
        .as_array()
        .filter(|items| !items.is_empty())
    {
        let names: Vec<_> = placeholders
            .iter()
            .map(|name| match form["fill"]["options"][text(name)].as_str() {
                Some(option) => format!("{} ({option})", text(name)),
                None => text(name),
            })
            .collect();
        lines.push(format!("  fill: {}", names.join(", ")));
    }
    if form["fill"]["choice"].is_string() && fill["argv"].is_array() {
        let mut route = argv(context, fill);
        route.push_str(&format!(
            " --choice {}",
            quote(&text(&form["fill"]["choice"]))
        ));
        let options = form["fill"]["options"].as_object().into_iter().flatten();
        for option in options.filter_map(|(_, option)| option.as_str()) {
            route.push_str(&format!(" {option} VALUE"));
        }
        if let Some(artifact) = form["fill"]["artifactOption"].as_str() {
            route.push_str(&format!(" {artifact} PATH"));
        }
        lines.push(format!("  fill route: {route}"));
    }
    if let Some(stdin) = form["stdin"]["description"].as_str() {
        lines.push(format!("  stdin: {stdin}"));
    }
    if let Some(meaning) = form["meaning"].as_str() {
        lines.push(format!("  meaning: {meaning}"));
    }
}

/// One command line with the executable and configuration the bounded JSON
/// form left out restored, so the printed line runs as shown.
fn argv(context: &Context, command: &Value) -> String {
    let mut parts: Vec<String> = command["argv"]
        .as_array()
        .or_else(|| command.as_array())
        .map(|items| items.iter().map(text).collect())
        .unwrap_or_default();
    if parts.is_empty() {
        return String::new();
    }
    if command["sameExecutableRequired"] == true && parts[0] != context.executable {
        parts.insert(0, context.executable.clone());
    }
    if command["sameConfigRequired"] == true {
        if parts.last().map(String::as_str) != Some("--config") {
            parts.push("--config".into());
        }
        parts.push(context.config.clone());
    }
    crate::emitted::Route::printed(&parts)
        .map(|route| route.shell_text())
        .unwrap_or_default()
}

fn quote(word: &str) -> String {
    crate::emitted::word(word)
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
                "delegation":{"text":"next action: worker agent 'worker' performs assignment sma_1"},
                "beforeEditing":{"command":{"argv":["/bin/exitbind","work","permit","smw_1","sma_1",
                    "--operation",crate::emitted::slot("OPERATION")],"sameConfigRequired":false},"placeholders":["OPERATION"]},
                "choices":[{"label":"completed","command":{"argv":["/bin/exitbind","work","return",
                    "--reason",crate::emitted::slot("REASON")]},"placeholders":["REASON"],
                    "stdin":{"description":"Pass the complete result artifact on stdin"}}]},
            "details":{"assignment":{"command":["/bin/exitbind","work","expand","smw_1","ref:a"]}}});
        let context = Context {
            executable: "/bin/exitbind".into(),
            config: "/p/exitbind.json".into(),
        };
        let text = render_with(&view, &context);
        assert!(text.contains("next action: worker agent 'worker' performs assignment sma_1"));
        let operation = crate::emitted::slot("OPERATION");
        assert!(text.contains(&format!(
            "Before editing: /bin/exitbind work permit smw_1 sma_1 --operation {operation}"
        )));
        let reason = crate::emitted::slot("REASON");
        assert!(text.contains(&format!(
            "Choice completed: /bin/exitbind work return --reason {reason}"
        )));
        assert!(text.contains("stdin: Pass the complete result artifact on stdin"));
        assert!(text.contains("  assignment: /bin/exitbind work expand smw_1 ref:a"));
        assert!(!text.trim_start().starts_with('{'));
        assert_eq!(quote("two words"), "'two words'");
    }

    /// W1 review F4: a command the JSON form bounded (executable and config
    /// left out) still prints as a complete line, and a Lead choice names
    /// its actor and fill route.
    #[test]
    fn bounded_choice_lines_print_full_executable_config_actor_and_fill_route() {
        let reason = crate::emitted::slot("REASON");
        let view = json!({"work":"smw_1","decision":{"readiness":"IN_PROGRESS"},
            "effectiveAction":{"state":"action","kind":"lead_decision","actor":"lead",
                "leadChoiceRequired":true,"choiceActor":{"role":"lead","agent":"lead"},
                "fillRoute":{"argv":["work","fill","smw_1","--current-binding","b1","--config"],
                    "sameExecutableRequired":true,"sameConfigRequired":true},
                "choices":[{"label":"accepted","command":{"argv":["work","return","smw_1","sma_1",
                    "--outcome","accepted","--reason",reason,"--config"],
                    "sameExecutableRequired":true,"sameConfigRequired":true},
                    "placeholders":["REASON"],"stdin":{"required":true,"description":"artifact"},
                    "fill":{"choice":"accepted","options":{"REASON":"--reason"},
                        "artifactOption":"--artifact-file"}}]}});
        let context = Context {
            executable: "/opt/exitbind".into(),
            config: "/p/exitbind.json".into(),
        };
        let text = render_with(&view, &context);
        assert!(text.contains(&format!(
            "Choice accepted: /opt/exitbind work return smw_1 sma_1 --outcome accepted --reason {} --config /p/exitbind.json\n",
            crate::emitted::slot("REASON")
        )), "{text}");
        assert!(text.contains("  fill: REASON (--reason)\n"), "{text}");
        assert!(text.contains("  fill route: /opt/exitbind work fill smw_1 --current-binding b1 --config /p/exitbind.json --choice accepted --reason VALUE --artifact-file PATH\n"), "{text}");
        assert!(text.contains(
            "Lead choice: decided by lead agent 'lead', who supplies every fill value\n"
        ));
        assert!(!text.contains("<same config>"));
    }
}

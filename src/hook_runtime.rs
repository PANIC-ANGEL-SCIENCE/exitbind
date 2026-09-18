use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAX_INPUT: usize = 64 * 1024;
const MAX_PROFILE: u64 = 12 * 1024;
const MAX_OUTPUT: usize = 16 * 1024;
const EVENTS: [&str; 2] = ["SessionStart", "SubagentStart"];

/// Hook execution is deliberately fail-open: malformed, ambiguous, or unsafe
/// host input produces no output and never turns a host session into an error.
pub fn run() -> Result<(), String> {
    let mut bytes = Vec::new();
    let mut input = io::stdin().take((MAX_INPUT + 1) as u64);
    input.read_to_end(&mut bytes).map_err(|_| String::new())?;
    if bytes.len() > MAX_INPUT {
        return Ok(());
    }
    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    let Some(object) = payload.as_object() else {
        return Ok(());
    };
    let event = object
        .get("hook_event_name")
        .or_else(|| object.get("hookEventName"))
        .or_else(|| object.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !EVENTS.contains(&event) {
        return Ok(());
    }
    let update_context = if event == "SessionStart" {
        crate::update::session_context()
    } else {
        None
    };
    let cwd = object
        .get("cwd")
        .or_else(|| object.get("current_directory"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(project) = absolute_directory(cwd) else {
        if let Some(text) = unconfigured_text(event, update_context.as_deref()) {
            emit(event, &text)?;
        }
        return Ok(());
    };
    let portable = if crate::producer::exitbind_surface() {
        project.join("exitbind.json")
    } else {
        project.join("soulmate.json")
    };
    let config_path = if contained(&project, &portable)
        && portable.is_file()
        && contained_existing(&project, &portable)
    {
        portable
    } else {
        match crate::project_layout::config_for_product(&project) {
            Ok(Some(path)) => path,
            _ => {
                if let Some(text) = unconfigured_text(event, update_context.as_deref()) {
                    emit(event, &text)?;
                }
                return Ok(());
            }
        }
    };
    let loaded = match crate::config::load(config_path.to_str()) {
        Ok(value) => value,
        Err(_) => {
            if let Some(text) = unconfigured_text(event, update_context.as_deref()) {
                emit(event, &text)?;
            }
            return Ok(());
        }
    };
    if fs::canonicalize(&loaded.product_root).ok().as_deref() != Some(project.as_path())
        || (loaded.mode == crate::project_layout::Mode::Portable
            && !contained_existing(&project, &loaded.control_root))
    {
        if let Some(text) = unconfigured_text(event, update_context.as_deref()) {
            emit(event, &text)?;
        }
        return Ok(());
    }
    let text = if event == "SessionStart" {
        let mut text = session_summary(&loaded.config);
        if let Some(update) = update_context {
            text.push('\n');
            text.push_str(&update);
        }
        text
    } else {
        let Some(agent) = exact_agent(object, &loaded.config) else {
            return Ok(());
        };
        let configured = &loaded.config["agents"][&agent];
        let profile_requested = configured
            .get("profile")
            .and_then(Value::as_str)
            .unwrap_or("");
        let profile = match crate::config::file(&loaded.control_root, profile_requested) {
            Ok(path) => path,
            Err(_) => return Ok(()),
        };
        let metadata = match fs::metadata(&profile) {
            Ok(info) => info,
            Err(_) => return Ok(()),
        };
        if metadata.len() > MAX_PROFILE {
            return Ok(());
        }
        let source = match fs::read_to_string(&profile) {
            Ok(value) => value,
            Err(_) => return Ok(()),
        };
        if !contained_existing(&loaded.control_root, &profile) {
            return Ok(());
        }
        let Some(context) =
            format_agent_context(&loaded.control_root, &agent, configured, &profile, &source)
        else {
            return Ok(());
        };
        bounded(context)
    };
    emit(event, &text)
}

/// A session in a repository Exitbind does not govern yet still needs to know
/// Exitbind exists and when to reach for it. This stays small: the detailed
/// protocol arrives only after the project is configured.
const UNCONFIGURED: &str = "Exitbind is installed on this machine, and this project is not configured for it yet.\nFor material repository work - multi-file changes, refactors, migrations, compatibility-sensitive edits, resumed work, or changes that need a check or review - select Exitbind before consequential edits.\nStart or continue with `exitbind work resume` or `exitbind work begin`. If the project must be configured first, explain that single project write and ask the owner before running `exitbind init --mode portable --root .`.\nKeep read-only questions and tiny obvious reversible edits direct.\nDo not report Exitbind as active until a work handle, recorded check, review, or acceptance exists.";

fn unconfigured_text(event: &str, update: Option<&str>) -> Option<String> {
    // The legacy Soulmate surface keeps its original silent contract; only the
    // Exitbind surface offers the bootstrap.
    if event != "SessionStart" || !crate::producer::exitbind_surface() {
        return update.map(str::to_owned);
    }
    let mut text = String::from(UNCONFIGURED);
    if let Some(update) = update {
        text.push('\n');
        text.push_str(update);
    }
    Some(bounded(text))
}

fn emit(event: &str, text: &str) -> Result<(), String> {
    let output = serde_json::to_string(
        &json!({"hookSpecificOutput":{"hookEventName":event,"additionalContext":text}}),
    )
    .map_err(|_| String::new())?;
    if output.len() <= MAX_OUTPUT {
        println!("{output}");
    }
    Ok(())
}

fn exact_agent(payload: &serde_json::Map<String, Value>, config: &Value) -> Option<String> {
    let mut found = Vec::new();
    let agents = config["agents"].as_object()?;
    for key in ["agent_name", "agent_type", "subagent_type"] {
        let Some(candidate) = payload.get(key).and_then(Value::as_str) else {
            continue;
        };
        for (name, agent) in agents {
            if (candidate == name || candidate == crate::config::native_name(name, agent))
                && !found.contains(name)
            {
                found.push(name.clone());
            }
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

fn format_agent_context(
    project: &Path,
    agent: &str,
    config: &Value,
    path: &Path,
    source: &str,
) -> Option<String> {
    let list = |name: &str| {
        config[name]
            .as_array()
            .map(|items| {
                if items.is_empty() {
                    "none".into()
                } else {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(safe_inline)
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            })
            .unwrap_or_else(|| "none".into())
    };
    let relative = path
        .strip_prefix(project)
        .ok()?
        .to_str()?
        .replace('\\', "/");
    let native = config["nativeName"]
        .as_str()
        .unwrap_or(agent)
        .to_ascii_lowercase()
        .replace('-', "_");
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    let mut lines = vec![
        format!("{product} plan-only context for {agent} (role selected by native host event)."),
        format!("Agent ID: {}", safe_inline(agent)),
        format!("Native task name: {}", safe_inline(&native)),
        format!("Profile selected/presented: {}", safe_inline(&relative)),
        format!("Profile SHA-256: {}", crate::hash::text(source)),
        "Evidence is selected/presented bytes; it does not prove a model read or followed the profile.".into(),
        "Declared boundary:".into(),
    ];
    for key in [
        "observe",
        "write",
        "commands",
        "skills",
        "memoryRead",
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryReject",
        "memoryRevoke",
        "memoryExpire",
        "memoryForget",
    ] {
        lines.push(format!("  {key}: {}", list(key)));
    }
    lines.push(format!(
        "  retention: {}",
        safe_inline(config["retention"].as_str().unwrap_or(""))
    ));
    lines.push(format!(
        "  crossContext: {}",
        safe_inline(config["crossContext"].as_str().unwrap_or(""))
    ));
    lines.push("Profile bytes:".into());
    lines.push(redact(source));
    Some(lines.join("\n"))
}

fn session_summary(config: &Value) -> String {
    let agents = config["agents"]
        .as_object()
        .map(|map| {
            let mut names = map.keys().cloned().collect::<Vec<_>>();
            names.sort();
            names.join(", ")
        })
        .unwrap_or_default();
    let workflows = config["workflows"]
        .as_object()
        .map(|map| {
            let mut names = map.keys().cloned().collect::<Vec<_>>();
            names.sort();
            names.join(", ")
        })
        .unwrap_or_default();
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    bounded(format!("{product} plan-only project context.\nLead: {}\nNamed agents: {}\nWorkflows: {}\nPreserve the existing root host conversation; {product} context only augments it. Do not replace, reset, fork, or request compaction of it for role loading or handoff.\nKeep recent user corrections and rejected approaches with their rationale; refer frozen-run conflicts to the existing lead for explicit supersession. Native conversational recall is distinct from durable role memory.\nNo model was selected or launched; declarations are not an OS sandbox.", safe_inline(config["orchestration"]["lead"].as_str().unwrap_or("")), safe_inline(if agents.is_empty() { "none" } else { &agents }), safe_inline(if workflows.is_empty() { "none" } else { &workflows })))
}

fn bounded(value: String) -> String {
    if value.len() <= MAX_OUTPUT {
        return safe_multiline(&value);
    }
    let limit = MAX_OUTPUT - 32;
    let end = value
        .char_indices()
        .take_while(|(index, _)| *index < limit)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0);
    format!("{}\n[context truncated]", safe_multiline(&value[..end]))
}
fn redact(value: &str) -> String {
    let source = safe_multiline(value);
    let home_prefix = concat!("/", "home", "/");
    let user_prefix = concat!("/", "Users", "/");
    redact_prefix(
        &redact_prefix(&source, home_prefix, "[home path redacted]"),
        user_prefix,
        "[user path redacted]",
    )
}
fn redact_prefix(source: &str, prefix: &str, replacement: &str) -> String {
    let mut output = String::new();
    let mut rest = source;
    while let Some(index) = rest.find(prefix) {
        output.push_str(&rest[..index]);
        let end = rest[index..]
            .find(char::is_whitespace)
            .map(|offset| index + offset)
            .unwrap_or(rest.len());
        output.push_str(replacement);
        rest = &rest[end..];
    }
    output.push_str(rest);
    output
}
fn safe_inline(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn safe_multiline(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect()
}
fn absolute_directory(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.contains('\0') || !Path::new(value).is_absolute() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_dir().then(|| fs::canonicalize(path).ok()).flatten()
}
fn contained(root: &Path, target: &Path) -> bool {
    target.starts_with(root)
}
fn contained_existing(root: &Path, target: &Path) -> bool {
    fs::canonicalize(target)
        .ok()
        .is_some_and(|path| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redact_replaces_machine_home_paths_and_preserves_non_paths() {
        let home_path = ["/", "home", "/", "account", "/project"].concat();
        let user_path = ["/", "Users", "/", "account", "/project"].concat();
        let input = format!("home={home_path}\nuser={user_path}\nordinary text stays");

        assert_eq!(
            redact(&input),
            "home=[home path redacted]\nuser=[user path redacted]\nordinary text stays"
        );
    }
}

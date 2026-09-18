use crate::hook_settings;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const PROTOCOL: &str = "soulmate-hook-v1";
pub const HOOK_COMMAND: &str = "command -v soulmate >/dev/null 2>&1 && soulmate hook-run || true";
const MARKER: &str = "soulmate hook-run";
const EVENTS: [&str; 2] = ["SessionStart", "SubagentStart"];
const CODEX_CONTEXT_LIMIT: i64 = 4096;
const HOOK_TIMEOUT_SECONDS: i64 = 5;

fn hook_command() -> &'static str {
    if crate::producer::exitbind_surface() {
        "command -v exitbind >/dev/null 2>&1 && exitbind hook-run || true"
    } else {
        HOOK_COMMAND
    }
}

fn hook_marker() -> &'static str {
    if crate::producer::exitbind_surface() {
        "exitbind hook-run"
    } else {
        MARKER
    }
}

pub fn manage(action: &str, hosts: &str, root: &str) -> Result<Vec<Value>, String> {
    if !matches!(action, "plan" | "status" | "apply" | "remove") {
        return Err(format!("unsupported hooks action '{action}'"));
    }
    let selected = parse_hosts(hosts)?;
    if cfg!(windows) {
        return selected
            .iter()
            .map(|host| unsupported(action, host, root))
            .collect();
    }
    if action == "apply" {
        hook_settings::require_compatible_command(PROTOCOL)?;
    }
    let mut states = Vec::new();
    for host in &selected {
        let mut state = preflight(host, root)?;
        // A record that still invokes the previous product generation is
        // Exitbind's own superseded wiring, not a third party's hook. Retire it
        // so a current binary cannot keep an old host bridge; everything else
        // stays a conflict and is never rewritten.
        if action == "apply" && crate::producer::exitbind_surface() && !state.conflicts.is_empty() {
            let retired = retire_superseded(&mut state.document);
            if retired > 0 {
                let (exact, conflicts) = inspect(&state.document, host, &state.target)?;
                state.exact = exact;
                state.conflicts = conflicts;
            }
        }
        states.push(state);
    }
    if matches!(action, "plan" | "status") {
        return Ok(states.iter().map(|state| result(action, state)).collect());
    }
    if states.iter().any(|state| !state.conflicts.is_empty()) {
        return Ok(states
            .into_iter()
            .map(|state| {
                let mut value = result(action, &state);
                value["blocked"] = Value::Bool(true);
                value["reason"] = Value::String(format!(
                    "{} ownership conflict; no file was changed",
                    if crate::producer::exitbind_surface() {
                        "Exitbind"
                    } else {
                        "Soulmate"
                    }
                ));
                value
            })
            .collect());
    }
    let staged = states
        .iter()
        .map(|state| stage(action, state))
        .collect::<Result<Vec<_>, _>>()?;
    let mut output = Vec::new();
    for item in staged {
        if let Some(serialized) = item.serialized.as_deref() {
            hook_settings::atomic_write(
                &item.state.target,
                serialized,
                item.state.mode,
                item.state.source.as_deref(),
                &item.state.real_root,
            )?;
        }
        let mut value = result(action, &item.result_state);
        value["changed"] = Value::Bool(item.changed);
        value["actions"] = Value::Array(item.actions.into_iter().map(Value::String).collect());
        if !item.changed {
            value["reason"] = Value::String("already in the requested state".into());
        }
        output.push(value);
    }
    Ok(output)
}

#[derive(Clone)]
struct State {
    host: String,
    target: PathBuf,
    real_root: PathBuf,
    source: Option<String>,
    mode: Option<u32>,
    document: Value,
    exact: [usize; 2],
    conflicts: Vec<String>,
}

fn parse_hosts(value: &str) -> Result<Vec<String>, String> {
    if value.trim().is_empty() {
        return Err("hooks requires an explicit --hosts codex,claude selection".into());
    }
    let mut result = Vec::new();
    for raw in value.split(',') {
        let host = raw.trim();
        if host != "codex" && host != "claude" {
            return Err(format!(
                "unsupported host '{host}'; expected codex or claude"
            ));
        }
        if !result.iter().any(|item| item == host) {
            result.push(host.to_owned());
        }
    }
    if result.is_empty() {
        return Err("hooks requires at least one host".into());
    }
    Ok(result)
}

fn preflight(host: &str, root: &str) -> Result<State, String> {
    let loaded = hook_settings::load(host, root)?;
    let (exact, conflicts) = inspect(&loaded.document, host, &loaded.target)?;
    Ok(State {
        host: host.into(),
        target: loaded.target,
        real_root: loaded.real_root,
        source: loaded.source,
        mode: loaded.mode,
        document: loaded.document,
        exact,
        conflicts,
    })
}

fn inspect(
    document: &Value,
    host: &str,
    target: &Path,
) -> Result<([usize; 2], Vec<String>), String> {
    let hooks = match document.get("hooks") {
        None => return Ok(([0, 0], Vec::new())),
        Some(value) => value.as_object().ok_or_else(|| {
            format!(
                "unexpected hooks shape in {}: hooks must be an object",
                target.display()
            )
        })?,
    };
    let expected = expected(host);
    let expected_object = expected
        .as_object()
        .ok_or("expected hook handler must be an object")?;
    let mut exact = [0usize; 2];
    let mut conflicts = Vec::new();
    for (event_index, event) in EVENTS.iter().enumerate() {
        let Some(groups) = hooks.get(*event) else {
            continue;
        };
        let groups = groups.as_array().ok_or_else(|| {
            format!(
                "unexpected hooks shape in {}: {event} must be an array",
                target.display()
            )
        })?;
        for (group_index, group) in groups.iter().enumerate() {
            let object = group.as_object().ok_or_else(|| {
                format!(
                    "unexpected hooks shape in {}: {event}[{group_index}] must be an object",
                    target.display()
                )
            })?;
            let handlers = object.get("hooks").and_then(Value::as_array).ok_or_else(|| format!("unexpected hooks shape in {}: {event}[{group_index}].hooks must be an array", target.display()))?;
            for (handler_index, handler) in handlers.iter().enumerate() {
                let handler_object = handler.as_object().ok_or_else(|| format!("unexpected hooks shape in {}: {event}[{group_index}].hooks[{handler_index}] must be an object", target.display()))?;
                if same_record(handler_object, expected_object) {
                    exact[event_index] += 1;
                } else if handler_object
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| {
                        command.contains(MARKER) || command.contains(hook_marker())
                    })
                {
                    conflicts.push(format!("{event}[{group_index}].hooks[{handler_index}]"));
                }
            }
        }
    }
    Ok((exact, conflicts))
}

/// Remove handler records that invoke the superseded caller, keeping every
/// other hook and preserving surrounding structure.
fn retire_superseded(document: &mut Value) -> usize {
    let current = hook_marker();
    let Some(hooks) = document.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut retired = 0;
    for event in EVENTS {
        let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
            continue;
        };
        for group in groups.iter_mut() {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            handlers.retain(|handler| {
                let superseded = handler
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(MARKER) && !command.contains(current));
                if superseded {
                    retired += 1;
                }
                !superseded
            });
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .map_or(true, |handlers| !handlers.is_empty())
        });
        if groups.is_empty() {
            hooks.remove(event);
        }
    }
    retired
}

fn stage(action: &str, state: &State) -> Result<Staged, String> {
    let staged = hook_settings::stage(
        action,
        &state.document,
        state.source.is_some(),
        state.exact,
        &expected(&state.host),
    )?;
    let result_state = if staged.changed {
        let (exact, conflicts) = inspect(&staged.document, &state.host, &state.target)?;
        State {
            document: staged.document.clone(),
            source: Some(String::new()),
            exact,
            conflicts,
            ..state.clone()
        }
    } else {
        state.clone()
    };
    Ok(Staged {
        state: state.clone(),
        result_state,
        serialized: staged.serialized,
        changed: staged.changed,
        actions: staged.actions,
    })
}

struct Staged {
    state: State,
    result_state: State,
    serialized: Option<String>,
    changed: bool,
    actions: Vec<String>,
}
fn result(action: &str, state: &State) -> Value {
    let installed = state.exact.iter().all(|count| *count > 0);
    let partial = state.exact.iter().any(|count| *count > 0);
    let state_name = if !state.conflicts.is_empty() {
        "conflict"
    } else if installed {
        "installed"
    } else if partial {
        "partial"
    } else {
        "absent"
    };
    let mut actions = Vec::new();
    if action == "plan" {
        if !state.conflicts.is_empty() {
            actions.push(format!(
                "stop: {} ownership conflict; no write planned",
                if crate::producer::exitbind_surface() {
                    "Exitbind"
                } else {
                    "Soulmate"
                }
            ));
        } else {
            for (i, event) in EVENTS.iter().enumerate() {
                actions.push(if state.exact[i] > 0 {
                    format!("keep exact {event} handler")
                } else {
                    format!(
                        "{} {event} handler",
                        if state.source.is_some() {
                            "add"
                        } else {
                            "create"
                        }
                    )
                });
            }
        }
    }
    json!({"action":action,"host":state.host,"supported":true,"targetPath":state.target.display().to_string(),"settingsFileExists":state.source.is_some(),"state":state_name,"exactHandlers":{"SessionStart":state.exact[0],"SubagentStart":state.exact[1]},"conflicts":state.conflicts,"changed":false,"actions":actions})
}

fn unsupported(action: &str, host: &str, root: &str) -> Result<Value, String> {
    let target = hook_settings::target_path(host, root)?;
    Ok(
        json!({"action":action,"host":host,"supported":false,"targetPath":target.display().to_string(),"settingsFileExists":false,"state":"unsupported","exactHandlers":{"SessionStart":0,"SubagentStart":0},"conflicts":[],"changed":false,"actions":["unsupported on windows; no file changed"],"reason":"project-local hook mutation is unsupported on win32"}),
    )
}

fn expected(host: &str) -> Value {
    if host == "codex" {
        json!({"type":"command","command":hook_command(),"timeout":HOOK_TIMEOUT_SECONDS,"additionalContextLimit":CODEX_CONTEXT_LIMIT})
    } else {
        json!({"type":"command","command":hook_command(),"timeout":HOOK_TIMEOUT_SECONDS})
    }
}
fn same_record(left: &Map<String, Value>, right: &Map<String, Value>) -> bool {
    left == right
}

// Configures managed hooks for explicitly selected hosts.
// Requires an explicit host selection; see `src/host/mod.rs`.
use super::settings;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The hook handshake token written by every published release to date.
///
/// It is a wire identifier, not user-facing text: a binary that manages hooks
/// asks the CLI on PATH for its token and refuses to write records it could not
/// have produced. Renaming it now would make every already-published binary
/// reject a newer CLI on PATH, so the current surface keeps emitting the
/// historical spelling and only widens what it accepts. See
/// `compatibility::ACCEPTED_HOOK_PROTOCOLS` for the retirement criterion.
pub const PROTOCOL: &str = "soulmate-hook-v1";
pub const HOOK_COMMAND: &str = "command -v soulmate >/dev/null 2>&1 && soulmate hook-run || true";
const MARKER: &str = "soulmate hook-run";
const EVENTS: [&str; 6] = [
    "SessionStart",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "Stop",
];

/// Claude Code's plan approval: `PostToolUse` limited by a matcher, so the
/// hook runs only after the human approves a plan.
const CLAUDE_EVENTS: [&str; 5] = [
    "SessionStart",
    "SubagentStart",
    "SubagentStop",
    "PostToolUse",
    "Stop",
];
pub(crate) const PLAN_APPROVAL: (&str, &str) = ("PostToolUse", "ExitPlanMode");

/// Claude Code and Codex deliver SubagentStop, which Exitbind uses to capture
/// a prepared native child's final message; other hosts keep the first two.
fn host_events(host: &str) -> &'static [&'static str] {
    if host == "codex" {
        if crate::producer::exitbind_surface() {
            &EVENTS
        } else {
            &EVENTS[..3]
        }
    } else if host == "claude" {
        if crate::producer::exitbind_surface() {
            &CLAUDE_EVENTS
        } else {
            &EVENTS[..3]
        }
    } else {
        &EVENTS[..2]
    }
}
const CODEX_CONTEXT_LIMIT: i64 = 4096;
const HOOK_TIMEOUT_SECONDS: i64 = 5;

fn hook_command(host: &str, event: &str) -> &'static str {
    if crate::producer::exitbind_surface() && event == "Stop" && host == "codex" {
        "command -v exitbind >/dev/null 2>&1 && exitbind hook-run --host codex || true"
    } else if crate::producer::exitbind_surface() && event == "Stop" && host == "claude" {
        "command -v exitbind >/dev/null 2>&1 && exitbind hook-run --host claude || true"
    } else if crate::producer::exitbind_surface() {
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
        settings::require_compatible_command(PROTOCOL)?;
        if crate::producer::exitbind_surface() && selected.iter().any(|host| host == "claude") {
            require_multipart_command()?;
        }
    }
    let mut states = Vec::new();
    for host in &selected {
        let mut state = preflight(host, root)?;
        // A record that still invokes the previous product generation is
        // Exitbind's own superseded wiring, not a third party's hook. Retire it
        // so a current binary cannot keep an old host bridge; everything else
        // stays a conflict and is never rewritten.
        if matches!(action, "apply" | "remove") && crate::producer::exitbind_surface() {
            let retired = retire_superseded(&mut state.document, host);
            if retired > 0 {
                state.retired = retired;
                let (exact, conflicts, legacy_start) =
                    inspect(&state.document, host, &state.target)?;
                state.exact = exact;
                state.conflicts = conflicts;
                state.legacy_start = legacy_start;
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
            settings::atomic_write(
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
    exact: Vec<usize>,
    conflicts: Vec<String>,
    retired: usize,
    legacy_start: bool,
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
    let loaded = settings::load(host, root)?;
    let (exact, conflicts, legacy_start) = inspect(&loaded.document, host, &loaded.target)?;
    Ok(State {
        host: host.into(),
        target: loaded.target,
        real_root: loaded.real_root,
        source: loaded.source,
        mode: loaded.mode,
        document: loaded.document,
        exact,
        conflicts,
        retired: 0,
        legacy_start,
    })
}

fn inspect(
    document: &Value,
    host: &str,
    target: &Path,
) -> Result<(Vec<usize>, Vec<String>, bool), String> {
    let specifications = specifications(host);
    let hooks = match document.get("hooks") {
        None => return Ok((vec![0; specifications.len()], Vec::new(), false)),
        Some(value) => value.as_object().ok_or_else(|| {
            format!(
                "unexpected hooks shape in {}: hooks must be an object",
                target.display()
            )
        })?,
    };
    let mut exact = vec![0usize; specifications.len()];
    let mut conflicts = Vec::new();
    let mut legacy_start = false;
    for event in host_events(host) {
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
            // The plan-approval record counts only with its matcher; without
            // it the hook would run on every tool call.
            let matched = if *event == PLAN_APPROVAL.0 {
                object.get("matcher").and_then(Value::as_str) == Some(PLAN_APPROVAL.1)
            } else if crate::producer::exitbind_surface()
                && host == "claude"
                && *event == "SubagentStart"
            {
                unfiltered(object)
            } else {
                true
            };
            let handlers = object.get("hooks").and_then(Value::as_array).ok_or_else(|| format!("unexpected hooks shape in {}: {event}[{group_index}].hooks must be an array", target.display()))?;
            for (handler_index, handler) in handlers.iter().enumerate() {
                let handler_object = handler.as_object().ok_or_else(|| format!("unexpected hooks shape in {}: {event}[{group_index}].hooks[{handler_index}] must be an object", target.display()))?;
                let matching = specifications
                    .iter()
                    .enumerate()
                    .find(|(_, (name, expected))| {
                        name == event
                            && expected
                                .as_object()
                                .is_some_and(|expected| same_record(handler_object, expected))
                    });
                if let Some((index, _)) = matching.filter(|_| matched) {
                    exact[index] += 1;
                } else if obsolete_claude_start(handler, host, event) && unfiltered(object) {
                    // A published single-part start is an upgradeable partial
                    // install, not an unknown ownership conflict.
                    legacy_start = true;
                } else if handler_object
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(hook_marker()))
                    || is_superseded_record(handler, host)
                {
                    // Ours, but not the record we would write: an ownership
                    // conflict. A superseded published record is counted here
                    // too so a plan reports it before retirement rewrites it.
                    conflicts.push(format!("{event}[{group_index}].hooks[{handler_index}]"));
                }
            }
        }
    }
    Ok((exact, conflicts, legacy_start))
}

/// Remove handler records that invoke the superseded caller, keeping every
/// other hook and preserving surrounding structure.
fn retire_superseded(document: &mut Value, host: &str) -> usize {
    let Some(hooks) = document.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut retired = 0;
    for event in host_events(host).iter().copied() {
        let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
            continue;
        };
        for group in groups.iter_mut() {
            let can_retire_start = group.as_object().is_some_and(unfiltered);
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            handlers.retain(|handler| {
                let superseded = is_superseded_record(handler, host)
                    || (can_retire_start && obsolete_claude_start(handler, host, event));
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
    let specifications = specifications(&state.host);
    let events = specifications
        .iter()
        .map(|(event, _)| *event)
        .collect::<Vec<_>>();
    let expected = specifications
        .into_iter()
        .map(|(_, handler)| handler)
        .collect::<Vec<_>>();
    let staged = settings::stage(
        action,
        &state.document,
        state.source.is_some(),
        &state.exact,
        &events,
        &expected,
    )?;
    let changed = staged.changed || state.retired > 0;
    let serialized = match staged.serialized {
        Some(serialized) => Some(serialized),
        None if state.retired > 0 => Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&staged.document).map_err(|error| error.to_string())?
        )),
        None => None,
    };
    let result_state = if changed {
        let (exact, conflicts, legacy_start) =
            inspect(&staged.document, &state.host, &state.target)?;
        State {
            document: staged.document.clone(),
            source: Some(String::new()),
            exact,
            conflicts,
            legacy_start,
            ..state.clone()
        }
    } else {
        state.clone()
    };
    let mut actions = staged.actions;
    if state.retired > 0 {
        actions.push(format!("retire {} superseded handlers", state.retired));
    }
    Ok(Staged {
        state: state.clone(),
        result_state,
        serialized,
        changed,
        actions,
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
    let specifications = specifications(&state.host);
    let applicable = &state.exact;
    let installed = applicable.iter().all(|count| *count > 0);
    let partial = state.legacy_start || applicable.iter().any(|count| *count > 0);
    let state_name = if !state.conflicts.is_empty() {
        "conflict"
    } else if installed && !state.legacy_start {
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
            for (i, (event, _)) in specifications.iter().enumerate() {
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
    let mut exact_handlers = Map::new();
    for (i, (event, _)) in specifications.iter().enumerate() {
        let count = exact_handlers
            .get(*event)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        exact_handlers.insert((*event).to_owned(), json!(count + state.exact[i] as u64));
    }
    json!({"action":action,"host":state.host,"supported":true,"targetPath":state.target.display().to_string(),"settingsFileExists":state.source.is_some(),"state":state_name,"exactHandlers":exact_handlers,"conflicts":state.conflicts,"changed":false,"actions":actions})
}

fn unsupported(action: &str, host: &str, root: &str) -> Result<Value, String> {
    let target = settings::target_path(host, root)?;
    let exact_handlers = host_events(host)
        .iter()
        .map(|event| ((*event).to_owned(), Value::from(0)))
        .collect::<Map<_, _>>();
    Ok(
        json!({"action":action,"host":host,"supported":false,"targetPath":target.display().to_string(),"settingsFileExists":false,"state":"unsupported","exactHandlers":exact_handlers,"conflicts":[],"changed":false,"actions":["unsupported on windows; no file changed"],"reason":"project-local hook mutation is unsupported on win32"}),
    )
}

/// The exact record shapes Exitbind published for the superseded caller. Only
/// these are retired on upgrade; a wrapper, a variant, or any other hook that
/// merely mentions the old command stays untouched and is reported instead.
fn is_superseded_record(handler: &Value, host: &str) -> bool {
    let known = [
        json!({"type":"command","command":HOOK_COMMAND,"timeout":HOOK_TIMEOUT_SECONDS}),
        json!({"type":"command","command":HOOK_COMMAND,"timeout":HOOK_TIMEOUT_SECONDS,"additionalContextLimit":CODEX_CONTEXT_LIMIT}),
    ];
    let _ = host;
    known.iter().any(|record| record == handler)
}

fn obsolete_claude_start(handler: &Value, host: &str, event: &str) -> bool {
    crate::producer::exitbind_surface()
        && host == "claude"
        && event == "SubagentStart"
        && handler == &expected(host, event)
}

fn unfiltered(group: &Map<String, Value>) -> bool {
    group
        .get("matcher")
        .map_or(true, |matcher| matcher.as_str() == Some(""))
}

fn specifications(host: &str) -> Vec<(&'static str, Value)> {
    let mut result = Vec::new();
    for &event in host_events(host) {
        if crate::producer::exitbind_surface() && host == "claude" && event == "SubagentStart" {
            for slot in 1..=super::runtime::multipart::SLOTS {
                result.push((event, json!({"type":"command", "command":format!(
                    "command -v exitbind >/dev/null 2>&1 && exitbind hook-run --host claude --index {slot} || true"
                ), "timeout":HOOK_TIMEOUT_SECONDS})));
            }
        } else {
            result.push((event, expected(host, event)));
        }
    }
    result
}

/// The legacy protocol token alone does not prove that the PATH binary accepts
/// multipart flags. Probe its parser before installing those handlers.
fn require_multipart_command() -> Result<(), String> {
    let message = "PATH-installed exitbind does not support Claude multipart hooks; update the CLI before applying hooks";
    let mut child = Command::new("exitbind")
        .args(["hook-run", "--host", "claude", "--index", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| message.to_owned())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().map_err(|_| message.to_owned())? {
            return if status.success() {
                Ok(())
            } else {
                Err(message.into())
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(message.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn expected(host: &str, event: &str) -> Value {
    if host == "codex" {
        json!({"type":"command","command":hook_command(host, event),"timeout":HOOK_TIMEOUT_SECONDS,"additionalContextLimit":CODEX_CONTEXT_LIMIT})
    } else {
        json!({"type":"command","command":hook_command(host, event),"timeout":HOOK_TIMEOUT_SECONDS})
    }
}
fn same_record(left: &Map<String, Value>, right: &Map<String, Value>) -> bool {
    left == right
}

#[cfg(test)]
mod recovery_event_tests {
    use super::host_events;

    #[test]
    fn codex_recovery_and_claude_plan_approval_events_are_additive() {
        let codex = host_events("codex");
        if crate::producer::exitbind_surface() {
            assert_eq!(
                codex,
                &[
                    "SessionStart",
                    "SubagentStart",
                    "SubagentStop",
                    "PreCompact",
                    "PostCompact",
                    "Stop"
                ]
            );
        } else {
            assert_eq!(codex, &["SessionStart", "SubagentStart", "SubagentStop"]);
        }
        let claude: &[&str] = if crate::producer::exitbind_surface() {
            &[
                "SessionStart",
                "SubagentStart",
                "SubagentStop",
                "PostToolUse",
                "Stop",
            ]
        } else {
            &["SessionStart", "SubagentStart", "SubagentStop"]
        };
        assert_eq!(host_events("claude"), claude);
        assert_eq!(host_events("other"), &["SessionStart", "SubagentStart"]);
    }
}

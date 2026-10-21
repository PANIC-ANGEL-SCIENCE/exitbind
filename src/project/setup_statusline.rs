//! `setup --apply` installs the Exitbind Goal status line in the project's
//! Claude Code settings. It is a display for the human only. An existing
//! status line of the human's own, in project, local or user settings, is kept
//! and reported instead of being replaced.

use crate::host::settings;
use serde_json::{json, Value};
use std::path::Path;

pub(crate) const COMMAND: &str =
    "command -v exitbind >/dev/null 2>&1 && exitbind statusline || true";

fn ours(value: &Value) -> bool {
    value["type"] == "command" && value["command"] == COMMAND
}

fn foreign_status_line(path: &Path) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|document| document.get("statusLine").cloned())
        .is_some_and(|value| !value.is_null() && !ours(&value))
}

/// Install the status line in `<root>/.claude/settings.json` and report what
/// happened. Never touches user or local settings.
pub(crate) fn apply(root: &Path) -> Result<Value, String> {
    let root_text = root.to_str().ok_or("project root is not valid UTF-8")?;
    let local = root.join(".claude/settings.local.json");
    let user = std::env::var_os("HOME").map(|home| Path::new(&home).join(".claude/settings.json"));
    if foreign_status_line(&local) {
        return Ok(json!({"status":"kept","reason":"project local settings define a status line"}));
    }
    if user.as_deref().is_some_and(foreign_status_line) {
        return Ok(json!({"status":"kept","reason":"user settings define a status line"}));
    }
    let loaded = settings::load("claude", root_text)?;
    match loaded.document.get("statusLine") {
        Some(value) if ours(value) => {
            return Ok(json!({"status":"unchanged","path":loaded.target}));
        }
        Some(value) if !value.is_null() => {
            return Ok(json!({"status":"kept","reason":"project settings define a status line"}));
        }
        _ => {}
    }
    let mut document = loaded.document.clone();
    document
        .as_object_mut()
        .ok_or("Claude settings root must be an object")?
        .insert(
            "statusLine".into(),
            json!({"type":"command","command":COMMAND,"padding":0}),
        );
    let mut serialized = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())?;
    serialized.push('\n');
    settings::atomic_write(
        &loaded.target,
        &serialized,
        loaded.mode,
        loaded.source.as_deref(),
        &loaded.real_root,
    )?;
    Ok(json!({"status":"installed","path":loaded.target}))
}

/// Record the status-line outcome in the setup report when Claude Code is a
/// selected host; returns whether a file was written. Failure is reported,
/// never fatal: the status line is a display.
pub(crate) fn report(root: &Path, hosts: &[String], report: &mut Value) -> bool {
    if !hosts.iter().any(|host| host == "claude") {
        return false;
    }
    let value =
        apply(root).unwrap_or_else(|error| json!({"status":"not_installed","reason":error}));
    let installed = value["status"] == "installed";
    report["statusLine"] = value;
    installed
}

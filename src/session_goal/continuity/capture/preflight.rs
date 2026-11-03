//! Read-only delivery preflight for a candidate native child intent.
use super::context;
use crate::config::Loaded;
use serde_json::Value;

pub(super) fn check(loaded: &Loaded, work: &str, intent: &Value) -> Result<Option<Value>, String> {
    if intent["directAssignment"] != true {
        return Ok(None);
    }
    let read = || {
        context::candidate_host_context(loaded, work, intent)
            .map_err(|error| format!("child transport preflight: {error}; no child was prepared"))
    };
    let child = read()?;
    let transport = crate::host::runtime::preflight_child_transport(loaded, work, intent, &child)?;
    if read()? != child {
        return Err(
            "child delivery changed during transport preflight; no child was prepared".into(),
        );
    }
    Ok(Some(transport))
}

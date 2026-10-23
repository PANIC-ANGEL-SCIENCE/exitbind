//! The read-only view of a Work's prepared children: the summary the
//! continuation view and the child-blocked refusals read, and the
//! `work child recover` line for a retained result.

use crate::config::Loaded;
use serde_json::{json, Value};

/// The `work child recover` line for a retained result: without `--context`
/// for a child of the native session that holds the Work.
pub(super) fn recover_route(loaded: &Loaded, work: &str, intent: &Value) -> String {
    let id = intent["id"].as_str().unwrap_or_default();
    let route = crate::emitted::Route::new(["work", "child", "recover", work, id]);
    let route = if intent["receiving"] == super::same_host::RECEIVING {
        route
    } else {
        route.words(["--context".to_owned(), crate::emitted::slot("FRESH_TOKEN")])
    };
    route.config(&loaded.path).shell_text()
}

/// Read-only summary of this work's prepared children, without result text.
pub(crate) fn summary(loaded: &Loaded, work: &str) -> Value {
    let Ok(intents) = super::load(loaded) else {
        return json!({"error": "child intent state is unreadable"});
    };
    Value::Array(
        intents
            .iter()
            .filter(|intent| intent["work"] == work)
            .map(|intent| {
                let pending = intent["pendingResult"].as_str().map(str::len);
                json!({
                    "intent": intent["id"],
                    "assignment": intent["assignment"],
                    "state": intent["state"],
                    "bindingRevision": intent["bindingRevision"],
                    "session": intent["session"],
                    "host": intent["host"],
                    "nativeChild": intent["nativeChild"],
                    "agentType": intent["agentType"],
                    "failure": intent["failure"],
                    "conflict": intent["conflict"],
                    "resultSha256": intent["resultSha256"],
                    "retainedResultBytes": pending,
                    "recover": pending.map(|_| recover_route(loaded, work, intent)),
                })
            })
            .collect(),
    )
}

/// The first child of `work` that blocks an act which must not leave one
/// behind (still live or prepared, or with a retained result), named with
/// its state and, for a retained result, the recover line that clears it.
/// `Err` when the child state is unreadable.
pub(crate) fn blocking(loaded: &Loaded, work: &str) -> Result<Option<String>, String> {
    let intents = super::load(loaded)?;
    let retained = |intent: &Value| {
        intent["pendingResult"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    };
    let Some(intent) = intents.iter().find(|intent| {
        intent["work"] == work
            && (!intent["state"]
                .as_str()
                .is_some_and(|state| super::TERMINAL.contains(&state))
                || retained(intent))
    }) else {
        return Ok(None);
    };
    let named = format!(
        "child {} ({}, {})",
        intent["id"].as_str().unwrap_or_default(),
        intent["assignment"].as_str().unwrap_or_default(),
        intent["state"].as_str().unwrap_or_default(),
    );
    Ok(Some(if retained(intent) {
        format!(
            "{named}; record its retained result with: {}",
            recover_route(loaded, work, intent)
        )
    } else {
        named
    }))
}

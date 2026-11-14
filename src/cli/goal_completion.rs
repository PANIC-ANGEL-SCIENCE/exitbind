// Builds committed-Goal completion and read-only recovery responses.
// The mutation response emits a terminal event only for a new close; see `src/cli/mod.rs`.
//! Compact committed-goal response and one exact read-only recovery route.
use crate::{config::Loaded, session_goal};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn response(
    loaded: &Loaded,
    record: Option<&Value>,
    newly_closed: bool,
    project: bool,
) -> Result<Value, String> {
    response_for_selected(
        loaded,
        record,
        newly_closed,
        project,
        &loaded.state_root,
        false,
    )
}

fn response_for_selected(
    loaded: &Loaded,
    record: Option<&Value>,
    newly_closed: bool,
    project: bool,
    root: &Path,
    recover_terminal: bool,
) -> Result<Value, String> {
    let rendered = session_goal::presentation_for_selected(loaded, record, root, project);
    let event = rendered.as_ref().ok().and_then(|rendered| {
        record.and_then(|record| crate::presentation_events::goal_completion(record, rendered))
    });
    let mut route = crate::emitted::Route::new(["goal", "status", "--view", "event", "--json"]);
    if project {
        route = route.words(["--project"]);
    }
    let recovery = crate::work::compact::continuation_route(&loaded.path, route);
    let mut metadata = event.clone().unwrap_or(Value::Null);
    if let Some(object) = metadata.as_object_mut() {
        object.remove("terminal");
    }
    Ok(json!({
        "kind":"goal_completion_response", "effect":if newly_closed { "committed" } else { "no-change" },
        "goalRevision":record.map(|record| record["revision"].clone()),
        "completionEvent":metadata,
        "presentation":{"terminal":if newly_closed || recover_terminal { event.as_ref().map(|event| event["terminal"].clone()) } else { None }},
        "displayReadFailure":rendered.err(),
        "recovery":{"readOnly":true,"route":recovery,
            "meaning":"If the completion reply was lost, read this event once. A read never closes or accepts work; the Lead deduplicates the event ID."},
    }))
}

pub(super) fn read_response(
    loaded: &Loaded,
    record: Option<&Value>,
    project: bool,
    root: &Path,
) -> Result<Value, String> {
    response_for_selected(loaded, record, false, project, root, true)
}

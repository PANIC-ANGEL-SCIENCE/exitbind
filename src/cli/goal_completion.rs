//! Compact committed-goal response and one exact read-only recovery route.
use crate::{config::Loaded, session_goal};
use serde_json::{json, Value};

pub(super) fn response(
    loaded: &Loaded,
    record: Option<&Value>,
    newly_closed: bool,
    project: bool,
) -> Result<Value, String> {
    let rendered = session_goal::presentation_for_loaded(loaded, record);
    let event = rendered.as_ref().ok().and_then(|rendered| {
        record.and_then(|record| crate::presentation_events::goal_completion(record, rendered))
    });
    let mut argv = vec![
        "goal".to_owned(),
        "status".to_owned(),
        "--view".to_owned(),
        "event".to_owned(),
        "--json".to_owned(),
    ];
    if project {
        argv.push("--project".to_owned());
    }
    let recovery = crate::work::compact::continuation_route(&loaded.path, argv);
    let mut metadata = event.clone().unwrap_or(Value::Null);
    if let Some(object) = metadata.as_object_mut() {
        object.remove("terminal");
    }
    Ok(json!({
        "kind":"goal_completion_response", "effect":if newly_closed { "committed" } else { "no-change" },
        "goalRevision":record.map(|record| record["revision"].clone()),
        "completionEvent":metadata,
        "presentation":{"terminal":if newly_closed { event.as_ref().map(|event| event["terminal"].clone()) } else { None }},
        "displayReadFailure":rendered.err(),
        "recovery":{"readOnly":true,"route":recovery,
            "meaning":"If the completion reply was lost, read this event once. A read never closes or accepts work; the Lead deduplicates the event ID."},
    }))
}

pub(super) fn read_response(
    loaded: &Loaded,
    record: Option<&Value>,
    project: bool,
) -> Result<Value, String> {
    let mut response = response(loaded, record, false, project)?;
    let rendered = session_goal::presentation_for_loaded(loaded, record)?;
    response["presentation"]["terminal"] = record
        .and_then(|record| crate::presentation_events::goal_completion(record, &rendered))
        .map_or(Value::Null, |event| event["terminal"].clone());
    Ok(response)
}

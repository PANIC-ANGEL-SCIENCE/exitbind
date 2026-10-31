use super::*;

pub(super) fn failure_response(loaded: &Loaded, attempt_id: &str, result: &Value) -> String {
    let attempt = read_record(&record_path(loaded, attempt_id, "attempt").unwrap_or_default())
        .unwrap_or(Value::Null);
    let work = attempt["binding"]["selector"].as_str().unwrap_or_default();
    let read = json!({
        "metadata": read_route(loaded, work, attempt_id, None),
        "stdoutBytes": read_route(loaded, work, attempt_id, Some("stdout")),
        "stderrBytes": read_route(loaded, work, attempt_id, Some("stderr")),
    });
    let next_actions = failure_actions(&attempt, result);
    let mut response = json!({"effect":"no_change","preReturn":"refused","attemptId":attempt_id,
        "effectMeaning":"No worker result, final check, or acceptance was recorded. Admission and diagnostic audit records can still advance the Work head.",
        "failureKind":result["failureKind"],"exitCode":result["exitCode"],"stdout":result["stdout"],"stderr":result["stderr"],
        "readRoutes":read,"nextActions":next_actions,
        "finalCheckCredited":false,"automaticRework":false,"replayed":result["replayed"]==true});
    for field in ["execution", "diagnostic"] {
        if let Some(value) = result.get(field) {
            response[field] = value.clone();
        }
    }
    format!("EXITBIND_JSON:{}", response)
}

fn failure_actions(attempt: &Value, result: &Value) -> Value {
    if attempt["binding"]["preReturnReconciliationSha256"].is_string() {
        json!([
            "inspect the one linked retry; a revised result cannot inherit its exact-held reconciliation",
            "report blocked to the Lead; another retry is not admitted"
        ])
    } else if result["failureKind"] == "preparation_refused" {
        json!([
            "inspect the retained preparation diagnostic; establish termination and effects before any further execution",
            "report the preparation blocker to the Lead; changed result bytes do not repair a frozen preparation policy"
        ])
    } else if super::execution::known_ended(result) {
        json!([
            "repair within the current assignment and valid grant",
            "report blocked to the Lead if repair is unavailable"
        ])
    } else {
        json!([
            "inspect the retained attempt; do not replay while its outcome is unknown",
            "report blocked to the Lead"
        ])
    }
}

pub(super) fn read_route(
    loaded: &Loaded,
    work: &str,
    attempt_id: &str,
    stream: Option<&str>,
) -> Value {
    let mut route = crate::emitted::Route::new(["run", "pre-return", "read", work, attempt_id]);
    if let Some(stream) = stream {
        route = route.words(["--stream", stream]);
    }
    route.config(&loaded.path).within(2048).argv_json()
}

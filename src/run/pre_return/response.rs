use super::*;

pub(super) fn failure_response(
    loaded: &Loaded,
    attempt_id: &str,
    result: &Value,
    attempt: &Value,
) -> String {
    let work = attempt["binding"]["selector"].as_str().unwrap_or_default();
    let read = json!({
        "metadata": read_route(loaded, work, attempt_id, None),
        "stdoutBytes": read_route(loaded, work, attempt_id, Some("stdout")),
        "stderrBytes": read_route(loaded, work, attempt_id, Some("stderr")),
    });
    let next_actions = failure_actions(attempt, result);
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

/// A newly observed result is rendered only after the saved owner and result
/// have been re-read under the current ledger lock.
pub(super) fn current_failure_response(
    loaded: &Loaded,
    attempt_id: &str,
    expected_attempt: &Value,
) -> Result<String, String> {
    let ledger = expected_attempt["binding"]["ledger"]
        .as_str()
        .ok_or("pre-return attempt has no owning ledger")?;
    let path = ledger_path(&loaded.state_root, ledger, false)?;
    with_lock(&path, || {
        let (_, events, _) = load_at(loaded, &path)?;
        let state = reduce_live(loaded, &events)?;
        let attempt = predecessor::read_attempt(loaded, &state, attempt_id)?;
        if attempt != *expected_attempt {
            return Err("pre-return admission changed after execution; do not replay".into());
        }
        let result = read_record(&record_path(loaded, attempt_id, "result")?)?;
        predecessor::validate_saved(loaded, &attempt, &result)?;
        Ok(failure_response(loaded, attempt_id, &result, &attempt))
    })
}

/// Name a supported blocked report without treating missing ownership as
/// evidence that a checker did or did not start.
pub(super) fn unavailable_refusal(
    loaded: &Loaded,
    state: &Value,
    identity: &Value,
    id: &str,
    problem: &str,
) -> String {
    let ledger = identity["ledger"].as_str().unwrap_or_default();
    let route = crate::work::locator_for_ledger(ledger).and_then(|work| {
        let current = crate::run::assignment::pending(state)
            .into_iter()
            .find(|candidate| {
                ["stage", "attempt", "agent", "role"]
                    .iter()
                    .all(|field| candidate[*field] == identity[*field])
            })?;
        let assignment = crate::run::assignment::handle(&work, &current).ok()?;
        Some(crate::emitted::command_line(
            &["work", "return", &work, &assignment, "--outcome", "blocked"],
            Some(&loaded.path),
        ))
    });
    let action = match route {
        Some(route) => format!("report blocked with {route}, providing the report on stdin"),
        None => {
            let inspect = crate::emitted::command_line(
                &["run", "next", ledger, "--json"],
                Some(&loaded.path),
            );
            let agent = identity["agent"].as_str().unwrap_or_default();
            let report = crate::emitted::slot("ARTIFACT_PATH");
            let blocked = crate::emitted::command_line(
                &[
                    "run",
                    "submit",
                    agent,
                    ledger,
                    "--outcome",
                    "blocked",
                    "--artifact",
                    &report,
                    "--artifact-root",
                    "state",
                ],
                Some(&loaded.path),
            );
            format!("inspect with {inspect}; write a blocked report in the state root, replace {report} with its path, and run {blocked}")
        }
    };
    format!("pre-return attempt {id}: {problem}; execution is unknown and this admission provides no check credit; preserve the record, do not replay, and {action}")
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

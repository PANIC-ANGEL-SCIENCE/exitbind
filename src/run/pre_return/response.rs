use super::*;

pub(super) fn failure_response(loaded: &Loaded, attempt_id: &str, result: &Value) -> String {
    let work = read_record(&record_path(loaded, attempt_id, "attempt").unwrap_or_default())
        .ok()
        .and_then(|attempt| attempt["binding"]["selector"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let read = json!({
        "metadata": read_route(loaded, &work, attempt_id, None),
        "stdoutBytes": read_route(loaded, &work, attempt_id, Some("stdout")),
        "stderrBytes": read_route(loaded, &work, attempt_id, Some("stderr")),
    });
    let mut response = json!({"effect":"no_change","preReturn":"refused","attemptId":attempt_id,
        "failureKind":result["failureKind"],"exitCode":result["exitCode"],"stdout":result["stdout"],"stderr":result["stderr"],
        "readRoutes":read,"nextActions":["repair within the current assignment and valid grant","report blocked to the Lead if repair is unavailable"],
        "finalCheckCredited":false,"automaticRework":false,"replayed":result["replayed"]==true});
    for field in ["execution", "diagnostic"] {
        if let Some(value) = result.get(field) {
            response[field] = value.clone();
        }
    }
    format!("EXITBIND_JSON:{}", response)
}

pub(super) fn read_route(
    loaded: &Loaded,
    work: &str,
    attempt_id: &str,
    stream: Option<&str>,
) -> Value {
    let mut argv = vec![
        "run".into(),
        "pre-return".into(),
        "read".into(),
        work.into(),
        attempt_id.into(),
    ];
    if let Some(stream) = stream {
        argv.extend(["--stream".into(), stream.into()]);
    }
    argv.push("--config".into());
    let route = crate::work::response_recovery::bounded_argv(argv, loaded.path.to_str(), 2048);
    json!({"argv":route.argv,"sameConfigRequired":route.same_config,"sameExecutableRequired":route.same_executable})
}

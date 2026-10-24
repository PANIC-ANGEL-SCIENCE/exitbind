//! G7: classify a failed native start and name exactly one route.
//!
//! A provider turn that ends without a typed result is not one state. When no
//! thread identity and no effect exist the assignment never started, so the
//! same assignment may be launched again; once either exists today's resume or
//! inspection route stays authoritative. Classification reads only bounded
//! journal facts: the provider thread identity, observed command outcomes, the
//! count of host items without an outcome, the recorded process identity and
//! the allowlisted diagnostic codes.

use super::*;

/// Why the provider did not start. Only a host text denial names a cause a
/// relaunch can change, and only when nothing started to change.
pub(super) fn permission_denied(journal: &Value) -> bool {
    journal["observation"]["diagnostic"]["codes"]
        .as_array()
        .is_some_and(|codes| codes.iter().any(|code| code == "host_permission_denied"))
}

/// Whether the provider produced a thread identity. A thread is the only
/// evidence that a provider session exists to resume.
pub(super) fn started(journal: &Value) -> bool {
    journal["threadId"]
        .as_str()
        .is_some_and(|thread| !thread.trim().is_empty())
}

/// Observed or possible effects: completed command items plus host items whose
/// outcome was never observed. A managed-edit receipt is not in this bound
/// journal, so a controlled-effects assignment is never classified as having
/// no effect.
pub(super) fn effects(journal: &Value) -> u64 {
    let observation = &journal["observation"];
    let commands = observation["commands"].as_array().map_or(0, Vec::len) as u64;
    commands + observation["unobservedItemCount"].as_u64().unwrap_or(0)
}

fn process_ended(journal: &Value) -> bool {
    if journal.get("processIdentity").is_none() {
        // No child claim was recorded: the spawn callback never ran, so there
        // is no owned process to wait for. This is the pre-spawn failure the
        // scope classifies as not started too.
        return true;
    }
    recovery::liveness(journal) == ProcessLiveness::Ended
}

/// A pre-thread, no-effect failure. The provider process must have ended (or
/// never have been recorded), and a controlled-effects assignment is excluded
/// because its file receipts are not in this journal.
pub(super) fn relaunchable(journal: &Value) -> bool {
    !started(journal)
        && effects(journal) == 0
        && journal["request"]["controlledEffects"] != true
        && process_ended(journal)
}

/// Record `not_started` in the journal when this failure is relaunchable.
/// Returns the journal after the decision (unchanged when it is not), or
/// `None` when no journal exists for this assignment.
pub(super) fn settle(path: &Path) -> Result<Option<Value>, String> {
    let Some(mut journal) = read_journal(path)? else {
        return Ok(None);
    };
    if !matches!(journal["status"].as_str(), Some("started" | "running")) {
        return Ok(Some(journal));
    }
    if relaunchable(&journal) {
        journal["status"] = json!("not_started");
        write_journal(path, &journal, false)?;
    }
    Ok(Some(journal))
}

/// The one route this classified state names. A relaunch reuses the same
/// assignment; only a host permission denial with no thread and no effect
/// changes the sandbox, because no session exists whose sandbox a resume
/// would keep.
pub(super) fn route(work: &str, journal: &Value) -> crate::emitted::Route {
    if started(journal) {
        return crate::emitted::Route::new(["work", "act", work, "--resume"]);
    }
    if !relaunchable(journal) {
        return crate::emitted::Route::new(["work", "act", work, "--inspect"]);
    }
    if permission_denied(journal) {
        return crate::emitted::Route::new([
            "work",
            "act",
            work,
            "--sandbox-mode",
            "danger-full-access",
        ]);
    }
    crate::emitted::Route::new(["work", "act", work])
}

/// A failure message that keeps the typed error and names the one route.
pub(super) fn message(loaded: &Loaded, work: &str, error: &str, journal: Option<&Value>) -> String {
    let Some(journal) = journal else {
        return error.to_owned();
    };
    let route = route(work, journal).config(&loaded.path).shell_text();
    if started(journal) {
        return format!(
            "{error}; native assignment started a provider thread; continue it with {route}"
        );
    }
    if relaunchable(journal) {
        let cause = if permission_denied(journal) {
            "; the host denied the provider's write, and no thread or effect exists"
        } else {
            "; no provider thread and no observed effect exists"
        };
        return format!("{error}; native assignment did not start (not_started){cause}; launch the same assignment with {route}");
    }
    format!("{error}; native assignment may have changed files; inspect it with {route}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(codes: Value, commands: Value, thread: Value) -> Value {
        json!({
            "status": "started", "threadId": thread, "request": {"controlledEffects": false},
            "processIdentity": {"pid": 123, "processGroup": 123, "startTimeTicks": 45},
            "observation": {
                "commands": commands,
                "unobservedItemCount": 0,
                "diagnostic": {"status": "observed", "codes": codes},
            }
        })
    }

    #[test]
    fn thread_identity_keeps_the_resume_route() {
        let journal = failed(json!(["turn_failed"]), json!([]), json!("thread-1"));
        assert!(!relaunchable(&journal));
        assert!(route("smw_1", &journal)
            .shell_text()
            .ends_with("work act smw_1 --resume"));
    }

    #[test]
    fn any_effect_refuses_relaunch_even_without_a_thread() {
        let journal = failed(
            json!([]),
            json!([{"status": "completed", "exitCode": 0}]),
            Value::Null,
        );
        assert!(!relaunchable(&journal));
        assert!(route("smw_1", &journal).shell_text().ends_with("--inspect"));
        let mut unobserved = failed(json!([]), json!([]), Value::Null);
        unobserved["observation"]["unobservedItemCount"] = json!(1);
        assert!(!relaunchable(&unobserved));
    }

    #[test]
    fn a_controlled_effects_assignment_is_never_not_started() {
        let mut journal = failed(json!([]), json!([]), Value::Null);
        journal["request"]["controlledEffects"] = json!(true);
        assert!(!relaunchable(&journal));
    }

    #[test]
    fn host_permission_denial_names_the_sandbox_relaunch() {
        let journal = failed(json!(["host_permission_denied"]), json!([]), Value::Null);
        assert!(relaunchable(&journal));
        assert!(route("smw_1", &journal)
            .shell_text()
            .ends_with("work act smw_1 --sandbox-mode danger-full-access"));
    }

    #[test]
    fn a_missing_child_claim_is_a_pre_spawn_failure() {
        let mut journal = failed(json!([]), json!([]), Value::Null);
        journal.as_object_mut().unwrap().remove("processIdentity");
        assert!(relaunchable(&journal));
    }
}

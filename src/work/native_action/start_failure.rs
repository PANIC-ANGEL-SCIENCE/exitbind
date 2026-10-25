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

/// The sandbox values the host accepts, in widening order.
const SANDBOX_MODES: [&str; 3] = ["read-only", "workspace-write", "danger-full-access"];

/// The sandbox recorded for the failed start, when the journal holds one.
pub(super) fn recorded_sandbox(journal: &Value) -> Option<&str> {
    journal["request"]["sandbox"].as_str()
}

/// The sandbox values a relaunch may use: every accepted value except the
/// recorded one, because "another sandbox" is the reason the route exists.
pub(super) fn sandbox_choices(journal: &Value) -> Vec<&'static str> {
    let recorded = recorded_sandbox(journal);
    SANDBOX_MODES
        .iter()
        .copied()
        .filter(|mode| Some(*mode) != recorded)
        .collect()
}

/// The provider executable recorded for the failed start, when one exists.
fn recorded_binary(journal: &Value) -> Option<&str> {
    journal["request"]["executable"]
        .as_str()
        .filter(|path| !path.is_empty())
}

/// A recorded launch value that a relaunch must carry unchanged.
fn recorded_text<'a>(journal: &'a Value, field: &str) -> Option<&'a str> {
    journal["request"][field]
        .as_str()
        .filter(|value| !value.is_empty())
}

/// The provider timeout recorded for the failed start.
fn recorded_timeout(journal: &Value) -> Option<u64> {
    journal["request"]["timeoutMs"].as_u64()
}

/// A relaunch of the same assignment with every recorded launch value: the
/// executable, model, effort, timeout and (unless a host permission denial
/// changed it) the sandbox. Only a denial's sandbox stays a placeholder: the
/// human chooses another one, the product never picks a wider sandbox.
fn relaunch_route(work: &str, journal: &Value, sandbox: bool) -> crate::emitted::Route {
    let mut route = crate::emitted::Route::new(["work", "act", work]);
    if let Some(executable) = recorded_binary(journal) {
        route = route.words(["--codex-bin", executable]);
    }
    if let Some(model) = recorded_text(journal, "model") {
        route = route.words(["--model", model]);
    }
    if let Some(effort) = recorded_text(journal, "reasoningEffort") {
        route = route.words(["--reasoning-effort", effort]);
    }
    if sandbox {
        let slot = crate::emitted::slot("SANDBOX_MODE");
        route = route.words(["--sandbox-mode", slot.as_str()]);
    } else if let Some(recorded) = recorded_sandbox(journal) {
        route = route.words(["--sandbox-mode", recorded]);
    }
    if let Some(timeout) = recorded_timeout(journal).map(|millis| millis.to_string()) {
        route = route.words(["--timeout-ms", timeout.as_str()]);
    }
    route
}

/// The note a repeat relaunch carries: the first relaunch of an assignment is
/// a bounded, product-named route; a second one of the same assignment is a
/// human decision, not an automatic repetition.
pub(super) const REPEAT_NOTE: &str = "; this assignment already used its one automatic relaunch and failed the same way, so do not repeat this route automatically";

/// Whether this assignment already used its one automatic relaunch. The
/// archived not-started journal beside the live one is the durable attempt
/// record; no separate counter is kept.
pub(super) fn prior_relaunch(journal: &Path) -> bool {
    let Some(name) = journal.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(directory) = journal.parent() else {
        return false;
    };
    let prefix = format!(".{name}.not-started.");
    fs::read_dir(directory)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(&prefix))
            })
        })
        .unwrap_or(false)
}

/// The one route this classified state names. A relaunch reuses the same
/// assignment; only a host permission denial with no thread and no effect
/// offers a sandbox choice, because no session exists whose sandbox a resume
/// would keep.
pub(super) fn route(work: &str, journal: &Value, config: &Path) -> crate::emitted::Route {
    if started(journal) {
        return crate::emitted::Route::new(["work", "act", work, "--resume"]).config(config);
    }
    if !relaunchable(journal) {
        return crate::emitted::Route::new(["work", "act", work, "--inspect"]).config(config);
    }
    relaunch_route(work, journal, permission_denied(journal)).config(config)
}

/// The sandbox choice written into the route, for the human who fills it.
pub(super) fn sandbox_note(journal: &Value) -> String {
    let choices = sandbox_choices(journal).join(", ");
    match recorded_sandbox(journal) {
        Some(recorded) => format!(
            " (fill SANDBOX_MODE with one of {choices}; {recorded} is the recorded sandbox that failed)"
        ),
        None => format!(" (fill SANDBOX_MODE with one of {choices})"),
    }
}

/// A failure message that keeps the typed error and names the one route.
pub(super) fn message(
    loaded: &Loaded,
    work: &str,
    error: &str,
    journal: Option<&Value>,
    relaunched: bool,
) -> String {
    let Some(journal) = journal else {
        return error.to_owned();
    };
    let route = route(work, journal, &loaded.path).shell_text();
    if started(journal) {
        return format!(
            "{error}; native assignment started a provider thread; continue it with {route}"
        );
    }
    if relaunchable(journal) {
        if permission_denied(journal) {
            let note = sandbox_note(journal);
            return format!("{error}; native assignment did not start (not_started); the host printed a sandbox write denial, and a provider config, auth or session read refusal prints the same errno text, so the two cannot be told apart here; no thread or effect exists; choose a sandbox and launch the same assignment with {route}{note}");
        }
        let mut text = format!("{error}; native assignment did not start (not_started); no provider thread and no observed effect exists; launch the same assignment with {route}");
        if relaunched {
            text.push_str(REPEAT_NOTE);
        }
        return text;
    }
    format!("{error}; native assignment may have changed files; inspect it with {route}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> &'static Path {
        Path::new("/fixture/exitbind.json")
    }

    /// A failed start recorded before any child claim (no `processIdentity`),
    /// so classification never reads the host process table.
    fn failed(codes: Value, commands: Value, thread: Value) -> Value {
        json!({
            "status": "started", "threadId": thread,
            "request": {"controlledEffects": false, "sandbox": "workspace-write",
                "executable": "/fixture/bin/codex"},
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
        assert!(route("smw_1", &journal, config())
            .shell_text()
            .ends_with("work act smw_1 --resume --config /fixture/exitbind.json"));
    }

    #[test]
    fn any_effect_refuses_relaunch_even_without_a_thread() {
        let journal = failed(
            json!([]),
            json!([{"status": "completed", "exitCode": 0}]),
            Value::Null,
        );
        assert!(!relaunchable(&journal));
        assert!(route("smw_1", &journal, config())
            .shell_text()
            .ends_with("--inspect --config /fixture/exitbind.json"));
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
    fn host_permission_denial_offers_another_sandbox_and_keeps_the_binary() {
        let journal = failed(json!(["host_permission_denied"]), json!([]), Value::Null);
        assert!(relaunchable(&journal));
        let route = route("smw_1", &journal, config());
        assert!(route.shell_text().ends_with(
            "work act smw_1 --codex-bin /fixture/bin/codex --sandbox-mode <SANDBOX_MODE> \
             --config /fixture/exitbind.json"
        ));
        assert_eq!(
            sandbox_choices(&journal),
            ["read-only", "danger-full-access"]
        );
        assert!(sandbox_note(&journal).contains("workspace-write is the recorded sandbox"));
    }

    #[test]
    fn a_relaunch_without_a_denial_keeps_every_recorded_launch_value() {
        let mut journal = failed(json!([]), json!([]), Value::Null);
        journal["request"]["model"] = json!("gpt-recorded");
        journal["request"]["reasoningEffort"] = json!("high");
        journal["request"]["timeoutMs"] = json!(123456_u64);
        let route = route("smw_1", &journal, config()).shell_text();
        assert!(route.ends_with(
            "work act smw_1 --codex-bin /fixture/bin/codex --model gpt-recorded \
             --reasoning-effort high --sandbox-mode workspace-write --timeout-ms 123456 \
             --config /fixture/exitbind.json"
        ));
    }

    #[test]
    fn a_denial_relaunch_keeps_recorded_values_but_leaves_the_sandbox_to_the_human() {
        let mut journal = failed(json!(["host_permission_denied"]), json!([]), Value::Null);
        journal["request"]["model"] = json!("gpt-recorded");
        journal["request"]["timeoutMs"] = json!(123456_u64);
        let route = route("smw_1", &journal, config()).shell_text();
        assert!(route.ends_with(
            "work act smw_1 --codex-bin /fixture/bin/codex --model gpt-recorded \
             --sandbox-mode <SANDBOX_MODE> --timeout-ms 123456 \
             --config /fixture/exitbind.json"
        ));
    }

    #[test]
    fn a_missing_child_claim_is_a_pre_spawn_failure() {
        let journal = failed(json!([]), json!([]), Value::Null);
        assert!(journal.get("processIdentity").is_none());
        assert!(relaunchable(&journal));
    }

    #[test]
    fn a_recorded_child_is_relaunchable_only_once_the_host_proves_it_ended() {
        let mut journal = failed(json!([]), json!([]), Value::Null);
        journal["processIdentity"] = json!({"pid": 123, "processGroup": 123, "startTimeTicks": 45});
        // Host-independent: a recorded child never relaunches unless the host
        // reads it as ended; a host that cannot prove it stays conservative.
        assert_eq!(
            relaunchable(&journal),
            recovery::liveness(&journal) == ProcessLiveness::Ended
        );
        // Linux reads the process table, where this start time cannot match.
        #[cfg(target_os = "linux")]
        assert!(relaunchable(&journal));
    }
}

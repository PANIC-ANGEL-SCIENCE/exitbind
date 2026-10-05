//! One current Work action at a time, with internal assignment resolution.

use super::*;

#[path = "native_action.rs"]
mod native_action;

pub(crate) fn delivery_packet(assignment: &Value) -> Value {
    native_action::delivery_packet(assignment)
}

pub(crate) struct ActOptions<'a> {
    pub(crate) current_binding: Option<&'a str>,
    pub(crate) outcome: Option<&'a str>,
    pub(crate) reason: Option<&'a str>,
    pub(crate) codex_bin: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) sandbox_mode: Option<&'a str>,
    pub(crate) timeout_ms: Option<&'a str>,
    pub(crate) resume: bool,
    pub(crate) operation: Option<&'a str>,
    pub(crate) inspect: bool,
    pub(crate) controlled_effects: bool,
}

pub(crate) fn act(loaded: &Loaded, work: &str, options: ActOptions<'_>) -> Result<Value, String> {
    details::ensure_action_binding(loaded, work, options.current_binding)?;
    let ledger = resolve(loaded, work)?;
    let current = next_for(loaded, work, &ledger)?;
    if options.inspect {
        if options.resume
            || options.operation.is_some()
            || options.outcome.is_some()
            || options.reason.is_some()
            || options.codex_bin.is_some()
            || options.model.is_some()
            || options.reasoning_effort.is_some()
            || options.sandbox_mode.is_some()
            || options.timeout_ms.is_some()
            || options.controlled_effects
        {
            return Err("--inspect is read-only and takes no execution or decision options".into());
        }
        if current["action"] != "spawn" {
            return Err("--inspect requires a current native assignment".into());
        }
        return native_action::inspect(loaded, work, &current);
    }
    if options.controlled_effects
        && (options.resume || current["action"] != "spawn" || current["role"] != "worker")
    {
        return Err(
            "--controlled-effects requires a fresh current native worker assignment".into(),
        );
    }
    if options.operation.is_some() && !options.resume {
        return Err("--operation requires --resume".into());
    }
    if options.resume {
        let overrides = options.codex_bin.is_some()
            || options.model.is_some()
            || options.reasoning_effort.is_some()
            || options.sandbox_mode.is_some()
            || options.timeout_ms.is_some();
        if let Some(recovered) =
            native_action::recover(loaded, work, &current, options.operation, overrides)?
        {
            return Ok(recovered);
        }
    }
    match current["action"].as_str() {
        Some("check") => {
            if options.resume {
                return Err("--resume requires a native assignment".into());
            }
            if options.outcome.is_some() || options.reason.is_some() {
                return Err("a check takes no Lead decision".into());
            }
            check_with_timeout(loaded, work, options.timeout_ms)
        }
        Some("lead_decision") => {
            if options.resume {
                return Err("--resume requires a native assignment".into());
            }
            lead_decision(loaded, work, &current, &options)
        }
        Some("spawn") => native_assignment(loaded, work, &current, &options),
        Some("done") => Err("this Work has no pending action".into()),
        _ => Err("current Work action is unavailable".into()),
    }
}

fn lead_decision(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    options: &ActOptions<'_>,
) -> Result<Value, String> {
    let outcome = options
        .outcome
        .filter(|value| !value.trim().is_empty())
        .ok_or("Lead action requires an explicit --outcome")?;
    let reason = options
        .reason
        .filter(|value| !value.trim().is_empty())
        .ok_or("Lead action requires an explicit --reason")?;
    let assignment = current["assignment"]
        .as_str()
        .ok_or("current Lead assignment is unavailable")?;
    let result = json!({"role": "lead", "outcome": outcome, "reason": reason});
    return_result_impl::return_result_with(
        loaded,
        work,
        assignment,
        outcome,
        Some(reason),
        None,
        None,
        Some(result.to_string().into_bytes()),
        None,
    )
}

fn native_assignment(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    options: &ActOptions<'_>,
) -> Result<Value, String> {
    if options.outcome.is_some() || options.reason.is_some() {
        return Err("native worker and reviewer verdicts must come from their own return".into());
    }
    native_action::execute(
        loaded,
        work,
        current,
        native_action::Options {
            codex_bin: options.codex_bin,
            model: options.model,
            reasoning_effort: options.reasoning_effort,
            sandbox_mode: options.sandbox_mode,
            timeout_ms: options.timeout_ms,
            resume: options.resume,
            controlled_effects: options.controlled_effects,
        },
    )
}

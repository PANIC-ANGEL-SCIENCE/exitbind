//! One current Work action at a time, with internal assignment resolution.

use super::*;

#[path = "native_action.rs"]
mod native_action;

pub(crate) struct ActOptions<'a> {
    pub(crate) outcome: Option<&'a str>,
    pub(crate) reason: Option<&'a str>,
    pub(crate) codex_bin: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) sandbox_mode: Option<&'a str>,
    pub(crate) timeout_ms: Option<&'a str>,
    pub(crate) resume: bool,
}

pub(crate) fn act(loaded: &Loaded, work: &str, options: ActOptions<'_>) -> Result<Value, String> {
    let ledger = resolve(loaded, work)?;
    let current = next_for(loaded, work, &ledger)?;
    match current["action"].as_str() {
        Some("check") => {
            if options.resume {
                return Err("--resume requires a native assignment".into());
            }
            if options.outcome.is_some() || options.reason.is_some() {
                return Err("a check takes no Lead decision".into());
            }
            check(loaded, work)
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
        },
    )
}

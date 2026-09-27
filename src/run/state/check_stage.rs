//! Check events change review routing only after the recorded result is known.

use serde_json::{json, Value};

use super::{lead_stage, reviewer_stage};

pub(super) fn apply_check(state: &mut Value, event: &Value) -> Result<(), String> {
    if state["status"] != "running" {
        return Err("run has already reached a terminal state".into());
    }
    crate::run_value::validate_check_against_state(state, event, 0)
        .map_err(|error| error.replacen("line 0", "state", 1))?;
    if !crate::run_exit::reduce(state)?.subject_is_current(&event["subjectSha256"]) {
        return Err("check is bound to a stale subject".into());
    }
    state["checks"]
        .as_array_mut()
        .ok_or("run state checks are invalid")?
        .push(event.clone());
    if check_failed(event) {
        state["currentStage"] = json!(lead_stage(state)?);
    } else if state["reviewPolicy"]["decision"] != "omitted"
        && state["currentStage"] == json!(lead_stage(state)?)
        && !crate::run_exit::assess(state)?.is_blocked()
        && !has_current_attempt_review(state)
    {
        if let Ok(stage) = reviewer_stage(state) {
            state["currentStage"] = json!(stage);
        }
    }
    Ok(())
}

fn has_current_attempt_review(state: &Value) -> bool {
    state["submissions"].as_array().is_some_and(|submissions| {
        submissions.iter().any(|submission| {
            submission["role"] == "reviewer" && submission["attempt"] == state["attempt"]
        })
    })
}

fn check_failed(event: &Value) -> bool {
    event["result"]["signal"].is_number()
        || event["result"]["code"]
            .as_i64()
            .is_some_and(|code| code != 0)
        || event["exitCode"].as_i64().is_some_and(|code| code != 0)
}

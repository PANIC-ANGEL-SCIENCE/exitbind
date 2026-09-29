//! Reviewer findings move to the Lead under the current disposition protocol.
//! Historical producers that predate it keep their recorded Lead transition.

use serde_json::{json, Value};

use super::{historical_review, lead_stage};

/// Returns whether this reviewer rework used the Lead transition. Other
/// rework roles and old runs without a basis use the worker-attempt transition.
pub(super) fn apply(state: &mut Value, event: &Value) -> Result<bool, String> {
    if event["role"] != "reviewer" || state.get("basisProtocol").is_none() {
        return Ok(false);
    }
    if !historical_review::old_rework_to_lead(state, event) {
        state["pendingDisposition"] = json!({
            "owner": "lead",
            "triggerEventSha256": event["eventSha256"],
            "basisSha256": state["basis"]["sha256"],
            "kind": "review_rework",
            "findingSha256s": [event["eventSha256"]]
        });
    }
    state["currentStage"] = json!(lead_stage(state)?);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::super::apply_submission;
    use serde_json::{json, Value};

    const SHA: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    fn state() -> Value {
        json!({
            "version": 8, "status": "running", "currentStage": 4, "attempt": 1,
            "basisProtocol": 1, "basis": {"sha256": SHA},
            "subject": {"sha256": SHA}, "pendingDisposition": null,
            "reviewPolicy": {"decision": "required", "sha256": SHA},
            "plan": {"stages": [
                {"stage": 2, "agents": [{"role": "worker", "name": "worker"}]},
                {"stage": 3, "agents": [{"role": "reviewer", "name": "reviewer"}]},
                {"stage": 4, "agents": [{"role": "lead", "name": "lead"}]}
            ]},
            "submissions": [{"stage": 2, "attempt": 1, "agent": "worker",
                "role": "worker", "outcome": "completed", "eventSha256": "target"}],
            "checks": [{"targetEventSha256": "target", "result": {"code": 101}}]
        })
    }

    fn event(outcome: &str, producer: &str) -> Value {
        json!({"version": 8, "stage": 3, "attempt": 1, "agent": "reviewer",
            "role": "reviewer", "outcome": outcome, "artifact": null,
            "producer": {"version": producer}, "subjectSha256": SHA,
            "basisSha256": SHA, "eventSha256": "review"})
    }

    #[test]
    fn historical_review_reducer_preserves_block_and_old_lead_rework() {
        let mut blocked = state();
        apply_submission(&mut blocked, &event("blocked", "0.24.0-rc.1")).unwrap();
        assert_eq!(blocked["status"], "blocked");
        assert_eq!(blocked["submissions"].as_array().unwrap().len(), 2);

        let mut rework = state();
        apply_submission(&mut rework, &event("rework", "0.24.0-rc.1")).unwrap();
        assert_eq!(rework["status"], "running");
        assert_eq!(rework["currentStage"], 4);
        assert_eq!(rework["attempt"], 1);
        assert!(rework["pendingDisposition"].is_null());
        assert_eq!(rework["submissions"][1]["outcome"], "rework");

        let mut after_passing_check = state();
        after_passing_check["currentStage"] = json!(3);
        after_passing_check["checks"][0]["result"]["code"] = json!(0);
        apply_submission(&mut after_passing_check, &event("rework", "0.24.0-rc.1")).unwrap();
        assert_eq!(after_passing_check["currentStage"], 4);
        assert!(after_passing_check["pendingDisposition"].is_null());
    }

    #[test]
    fn current_review_rework_keeps_the_lead_disposition_gate() {
        let mut state = state();
        state["currentStage"] = json!(3);
        state["checks"][0]["result"]["code"] = json!(0);
        apply_submission(&mut state, &event("rework", "0.25.1")).unwrap();
        assert_eq!(state["currentStage"], 4);
        assert_eq!(state["pendingDisposition"]["kind"], "review_rework");
        assert_eq!(state["pendingDisposition"]["triggerEventSha256"], "review");
    }
}

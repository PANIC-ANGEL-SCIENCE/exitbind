//! Progress arithmetic and the stable JSON projection.
//!
//! Classification is delegated to `run_exit`; this module only computes the
//! weighted components from kernel-owned facts.

use crate::run_exit::{self, ExitDecision, ExitState};
use serde_json::{json, Value};

const SCOPE_WEIGHT: u64 = 15;
const WORKER_WEIGHT: u64 = 25;
const CHECK_WEIGHT: u64 = 15;
const REVIEW_WEIGHT: u64 = 20;
const LEAD_WEIGHT: u64 = 25;

pub(crate) fn project(state: &Value) -> Value {
    let Ok(kernel) = run_exit::reduce(state) else {
        return json!({
            "valid": false,
            "error": {"code": "invalid_state", "detail": "validated run state is required"},
            "runStatus": state["status"]
        });
    };
    project_kernel(&kernel)
}

pub(crate) fn project_kernel(kernel: &ExitState) -> Value {
    let decision = kernel.decision();
    let (state, reason, detail) = decision.wire();
    if matches!(decision, ExitDecision::NotApplicable(_)) {
        return json!({
            "applicable": false,
            "percent": Value::Null,
            "state": state,
            "reason": {"code": reason, "detail": detail},
            "runStatus": kernel.status
        });
    }
    let check_completed = kernel
        .assessment
        .targets
        .iter()
        .filter(|target| target.status == run_exit::CheckTargetStatus::Passed)
        .count();
    let scope_earned = u64::from(kernel.scope_completed) * SCOPE_WEIGHT;
    let worker_earned = proportional(WORKER_WEIGHT, kernel.worker_completed, kernel.worker_total);
    let check_earned = proportional(CHECK_WEIGHT, check_completed, kernel.worker_total);
    let review_earned = proportional(
        REVIEW_WEIGHT,
        kernel.reviewer_completed,
        kernel.reviewer_total,
    );
    let lead_completed = u64::from(kernel.lead_accepted());
    let lead_earned = lead_completed * LEAD_WEIGHT;
    let percent = scope_earned + worker_earned + check_earned + review_earned + lead_earned;
    json!({
        "applicable": true,
        "percent": percent,
        "state": state,
        "reason": {"code": reason, "detail": detail},
        "weights": {
            "scope": SCOPE_WEIGHT,
            "worker": WORKER_WEIGHT,
            "check": CHECK_WEIGHT,
            "review": REVIEW_WEIGHT,
            "lead": LEAD_WEIGHT
        },
        "components": {
            "scope": {"completed": u64::from(kernel.scope_completed), "total": 1, "earned": scope_earned},
            "worker": {"completed": kernel.worker_completed, "total": kernel.worker_total, "earned": worker_earned},
            "check": {"completed": check_completed, "total": kernel.worker_total, "earned": check_earned},
            "review": {"completed": kernel.reviewer_completed, "total": kernel.reviewer_total, "earned": review_earned},
            "lead": {"completed": lead_completed, "total": 1, "earned": lead_earned}
        }
    })
}

fn proportional(weight: u64, completed: usize, total: usize) -> u64 {
    if total == 0 {
        0
    } else if completed >= total {
        weight
    } else {
        weight * completed as u64 / total as u64
    }
}

#[cfg(test)]
mod tests {
    use super::project;
    use serde_json::json;

    #[test]
    fn reducer_failure_stays_outside_lifecycle_projection() {
        let progress = project(&json!({"status": "running"}));
        assert_eq!(progress["valid"], false);
        assert_eq!(progress["error"]["code"], "invalid_state");
        assert!(progress["state"].is_null());
        assert!(progress["reason"].is_null());
    }
}

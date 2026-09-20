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
    // Every completed worker owes one functional check plus one per accepted
    // preservation requirement; crediting per worker would over-report while a
    // required check is still missing.
    let check_total = kernel.worker_total * kernel.assessment.targets_per_worker;
    let check_earned = proportional(CHECK_WEIGHT, check_completed, check_total);
    let review_earned = if kernel.review_required {
        proportional(
            REVIEW_WEIGHT,
            kernel.reviewer_completed,
            kernel.reviewer_total,
        )
    } else {
        REVIEW_WEIGHT
    };
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
            "check": {"completed": check_completed, "total": check_total, "earned": check_earned},
            "review": {
                "status": if kernel.review_required { "required" } else { "omitted" },
                "completed": kernel.reviewer_completed,
                "total": kernel.reviewer_total,
                "earned": review_earned
            },
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

    fn checked(events: serde_json::Value) -> serde_json::Value {
        events
    }

    /// One worker can owe several checks. Crediting the whole check weight
    /// after one of them passes would over-report progress while a required
    /// preservation check is still missing.
    #[test]
    fn several_required_checks_for_one_worker_share_the_check_weight() {
        let subject = "b".repeat(64);
        let state = checked(json!({
            "version": 6,
            "status": "running",
            "attempt": 1,
            "currentStage": 2,
            "subject": {"sha256": subject},
            "inputsSha256": "c".repeat(64),
            "runId": "a".repeat(64),
            "checkPolicy": {"version":1,"command":"true","commandSha256":crate::evidence::hash::text("true"),"origin":"local_report"},
            "preservation": {"version":1,"requirements":[{"id":"precedence","text":"env wins","command":"true","commandSha256":crate::evidence::hash::text("true"),"origin":"local_report"}]},
            "plan": {"version":1,"maxParallel":1,"stages":[{"agents":[{"role":"worker","name":"worker"}]},{"agents":[{"role":"reviewer","name":"reviewer"}]}]},
            "submissions": [
                {"stage":1,"attempt":1,"agent":"lead","role":"lead","outcome":"scoped","eventSha256":"d".repeat(64)},
                {"stage":1,"attempt":1,"agent":"worker","role":"worker","outcome":"completed","eventSha256":"e".repeat(64)}
            ],
            "checks": [
                {"targetEventSha256":"e".repeat(64),"subjectSha256":subject,"inputsSha256":"c".repeat(64),"result":{"kind":"exit","code":0},"eventSha256":"f".repeat(64)}
            ]
        }));
        let progress = project(&state);
        assert_eq!(progress["applicable"], true);
        assert_eq!(progress["components"]["check"]["completed"], 1);
        assert_eq!(
            progress["components"]["check"]["total"], 2,
            "a preservation requirement adds a required check target"
        );
        assert_eq!(progress["components"]["check"]["earned"], 7);
    }

    /// A review that belongs to earlier tested inputs is not progress.
    #[test]
    fn a_stale_review_earns_no_review_progress() {
        let subject = "b".repeat(64);
        let current_inputs = "c".repeat(64);
        let state = |review_inputs: &str| {
            json!({
                "version": 6,
                "status": "running",
                "attempt": 1,
                "currentStage": 3,
                "subject": {"sha256": subject},
                "inputsSha256": current_inputs,
                "runId": "a".repeat(64),
                "checkPolicy": {"version":1,"command":"true","commandSha256":crate::evidence::hash::text("true"),"origin":"local_report"},
                "plan": {"version":1,"maxParallel":1,"stages":[{"agents":[{"role":"worker","name":"worker"}]},{"agents":[{"role":"reviewer","name":"reviewer"}]}]},
                "submissions": [
                    {"stage":1,"attempt":1,"agent":"lead","role":"lead","outcome":"scoped","eventSha256":"d".repeat(64)},
                    {"stage":1,"attempt":1,"agent":"worker","role":"worker","outcome":"completed","eventSha256":"e".repeat(64)},
                    {"stage":2,"attempt":1,"agent":"reviewer","role":"reviewer","outcome":"approved","inputsSha256":review_inputs,"eventSha256":"1".repeat(64)}
                ],
                "checks": [
                    {"targetEventSha256":"e".repeat(64),"subjectSha256":subject,"inputsSha256":current_inputs,"result":{"kind":"exit","code":0},"eventSha256":"f".repeat(64)}
                ]
            })
        };
        let current = project(&state(&current_inputs));
        assert_eq!(current["components"]["review"]["completed"], 1);
        assert_eq!(current["components"]["review"]["earned"], 20);

        let stale = project(&state(&"9".repeat(64)));
        assert_eq!(stale["components"]["review"]["completed"], 0);
        assert_eq!(stale["components"]["review"]["earned"], 0);
        assert!(stale["percent"].as_u64() < current["percent"].as_u64());
    }

    #[test]
    fn reducer_failure_stays_outside_lifecycle_projection() {
        let progress = project(&json!({"status": "running"}));
        assert_eq!(progress["valid"], false);
        assert_eq!(progress["error"]["code"], "invalid_state");
        assert!(progress["state"].is_null());
        assert!(progress["reason"].is_null());
    }
}

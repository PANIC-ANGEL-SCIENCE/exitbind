use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Releases before the check-routing correction could record a reviewer
/// verdict after a failed main check. Accept only those producers while
/// replaying that historical event; current routing still applies afterward.
const PRODUCERS_WITH_HISTORICAL_REVIEW_ORDER: &[&str] = &[
    "0.20.0",
    "0.21.0",
    "0.23.1-rc.2",
    "0.24.0-rc.1",
    "0.24.0-rc.7",
    "0.25.0-rc.1",
];

pub(super) fn reviewer_assignment(state: &Value, event: &Value) -> Option<Value> {
    let lead = super::lead_stage(state).ok()?;
    if event["role"] != "reviewer"
        || !["approved", "rework", "blocked"].contains(&event["outcome"].as_str()?)
        || !historical_producer(event)
        || state["currentStage"] != lead
        || !has_failed_main_check(state)
    {
        return None;
    }
    let attempt = state["attempt"].as_u64()?;
    if event["attempt"] != attempt {
        return None;
    }
    state["plan"]["stages"]
        .as_array()?
        .iter()
        .find_map(|stage| {
            let has_reviewer = stage["agents"].as_array().is_some_and(|agents| {
                agents
                    .iter()
                    .any(|agent| agent["role"] == "reviewer" && agent["name"] == event["agent"])
            });
            if has_reviewer && stage["stage"] == event["stage"] {
                Some(json!({
                    "stage": stage["stage"],
                    "attempt": attempt,
                    "agent": event["agent"],
                    "role": "reviewer"
                }))
            } else {
                None
            }
        })
}

/// These v8 producers sent a reviewer finding to the Lead without the later
/// pending-disposition protocol. Replay their transition without creating a
/// new disposition obligation that the original ledger could not contain.
pub(super) fn old_rework_to_lead(state: &Value, event: &Value) -> bool {
    state.get("basisProtocol").is_some()
        && event["version"] == 8
        && event["role"] == "reviewer"
        && event["outcome"] == "rework"
        && matches!(
            event["producer"]["version"].as_str(),
            Some("0.23.1-rc.2" | "0.24.0-rc.1" | "0.24.0-rc.7")
        )
}

/// The old failed-check reviewer event is already at the Lead stage. A blocked
/// verdict still terminates the run; approval and rework do not imply success.
pub(super) fn complete(state: &mut Value, outcome: &str) {
    if outcome == "blocked" {
        state["status"] = json!("blocked");
    }
}

fn historical_producer(event: &Value) -> bool {
    event["producer"]["version"]
        .as_str()
        .is_some_and(|version| PRODUCERS_WITH_HISTORICAL_REVIEW_ORDER.contains(&version))
}

fn has_failed_main_check(state: &Value) -> bool {
    let workers = state["submissions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|submission| {
            submission["attempt"] == state["attempt"]
                && submission["role"] == "worker"
                && submission["outcome"] == "completed"
        })
        .filter_map(|submission| submission["eventSha256"].as_str().map(str::to_owned));
    let targets = workers.collect::<BTreeSet<_>>();
    state["checks"].as_array().is_some_and(|checks| {
        checks.iter().any(|check| {
            check["targetEventSha256"]
                .as_str()
                .is_some_and(|target| targets.contains(target))
                && check.get("requirementId").is_none()
                && super::check_stage::check_failed(check)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{old_rework_to_lead, reviewer_assignment};
    use serde_json::{json, Value};

    fn state() -> Value {
        json!({
            "currentStage": 4, "attempt": 1, "basisProtocol": 1,
            "plan": {"stages": [
                {"stage": 2, "agents": [{"role": "worker", "name": "worker"}]},
                {"stage": 3, "agents": [{"role": "reviewer", "name": "reviewer"}]},
                {"stage": 4, "agents": [{"role": "lead", "name": "lead"}]}
            ]},
            "submissions": [{"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": "target"}],
            "checks": [{"targetEventSha256": "target", "result": {"code": 101}}]
        })
    }

    fn event(outcome: &str) -> Value {
        json!({"version": 8, "role": "reviewer", "outcome": outcome,
            "producer": {"version": "0.24.0-rc.1"}, "attempt": 1,
            "stage": 3, "agent": "reviewer"})
    }

    #[test]
    fn historical_failed_check_preserves_only_planned_reviewer_verdicts() {
        let state = state();
        for outcome in ["approved", "rework", "blocked"] {
            assert!(reviewer_assignment(&state, &event(outcome)).is_some());
        }
        for field in ["agent", "stage", "attempt", "producer"] {
            let mut wrong = event("rework");
            match field {
                "agent" => wrong["agent"] = json!("other"),
                "stage" => wrong["stage"] = json!(2),
                "attempt" => wrong["attempt"] = json!(2),
                _ => wrong["producer"]["version"] = json!("0.25.1"),
            }
            assert!(reviewer_assignment(&state, &wrong).is_none(), "{field}");
        }
        let mut passing = state;
        passing["checks"][0]["result"]["code"] = json!(0);
        assert!(reviewer_assignment(&passing, &event("rework")).is_none());
    }

    #[test]
    fn old_v8_rework_uses_pre_disposition_lead_transition() {
        assert!(old_rework_to_lead(&state(), &event("rework")));
        let mut current = event("rework");
        current["producer"]["version"] = json!("0.25.1");
        assert!(!old_rework_to_lead(&state(), &current));
        let mut no_basis = state();
        no_basis.as_object_mut().unwrap().remove("basisProtocol");
        assert!(!old_rework_to_lead(&no_basis, &event("rework")));
    }
}

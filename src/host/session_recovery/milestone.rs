//! Read-only, bounded recovery milestone derived from the native Goal and a
//! freshly rendered current Work detail. The saved hint never selects action.

use super::{
    hint::{Hint, WorkRef},
    references::validated_work_configs,
};
use crate::{evidence::hash, session_goal};
use serde_json::{json, Value};

const MAX_PRETTY_BYTES: usize = 4 * 1024;
const MAX_LINES: usize = 50;

pub(super) fn project(
    record: &Value,
    session: &session_goal::native::Session,
    owner: &session_goal::native::Owner,
    hint: Option<&Hint>,
) -> Value {
    let goal = json!({
        "id": record["goalId"],
        "revision": record["revision"],
        "eventSha256": record["eventSha256"],
    });
    let Some(hint) = hint else {
        return bounded(json!({
            "protocol": 1,
            "state": "unknown",
            "goal": goal["id"],
            "workState": "unknown",
            "assignment": {"state":"unknown"},
            "phase": {"state":"unknown"},
            "checkReferences": {"current": null, "historical": null},
            "routes": [],
        }));
    };

    let goal_current = hint.goal_revision == record["revision"].as_u64().unwrap_or(0)
        && hint.goal_event_sha256 == record["eventSha256"].as_str().unwrap_or_default();
    let selected = (hint.current_work.state == "known")
        .then_some(hint.current_work.work.as_deref())
        .flatten()
        .and_then(|work| {
            hint.work_refs
                .iter()
                .find(|reference| reference.work == work)
        });
    let historical = json!({
        "savedStepRouteRef":"savedHint.currentStep",
        "current":false,
    });
    let mut routes = Vec::new();

    let Some(reference) = selected else {
        let result = json!({
            "protocol":1,
            "state":"pending_or_unknown",
            "goal":goal["id"],
            "goalCurrent":goal_current,
            "workState":"unresolved",
            "assignment":{"state":"unknown"},
            "phase":{"state":"unknown"},
            "checkReferences":{"current":null,"historical":historical},
            "routes":routes,
        });
        return bounded(result);
    };

    let Ok(mut configs) =
        validated_work_configs(record, session, owner, std::slice::from_ref(reference))
    else {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_member_unavailable",
        );
    };
    let Some((work, loaded)) = configs.pop() else {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_member_unavailable",
        );
    };
    let Ok((_, events, _)) = crate::run::ledger::load(
        &loaded,
        &match crate::work::resolve(&loaded, &work) {
            Ok(ledger) => ledger,
            Err(_) => {
                return unavailable(
                    goal,
                    goal_current,
                    reference,
                    historical,
                    routes,
                    "work_detail_unavailable",
                )
            }
        },
    ) else {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_detail_unavailable",
        );
    };
    let Some(start) = events.first() else {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_start_unavailable",
        );
    };
    if start["eventSha256"] != reference.start_event_sha256 {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_start_changed",
        );
    }
    let Ok(state) = crate::run::state::reduce(&events) else {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_state_unavailable",
        );
    };
    if !crate::run::guidance_continuity::enabled(&state) {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "legacy_work_currentness",
        );
    }

    let Ok(detail) = crate::work::readable::current(&loaded, &work) else {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_detail_unavailable",
        );
    };
    if detail["kind"] != "work_detail" || detail["valid"] != true || detail["work"] != work {
        return unavailable(
            goal,
            goal_current,
            reference,
            historical,
            routes,
            "work_detail_unverified",
        );
    }

    let member_index = hint
        .work_refs
        .iter()
        .position(|item| item.work == work)
        .unwrap_or(0);
    let work_detail_route = format!("readRoutes.workDetailArgv[{member_index}]");
    routes.push(json!({"kind":"work_detail","routeRef":work_detail_route.clone()}));
    routes.push(json!({"kind":"action_refresh","routeRef":work_detail_route}));
    let action = detail["effectiveAction"]["kind"].as_str();
    let assignment_id = detail["recipient"]["assignment"].as_str();
    let assignment = match (detail["recipient"]["role"].as_str(), assignment_id) {
        (Some(role), Some(id)) if !role.is_empty() && !id.is_empty() => {
            json!({"state":"known","role":role,"id":id})
        }
        _ => json!({"state":"pending_or_unknown"}),
    };
    let binding = detail["binding"].as_str().unwrap_or_default();
    let current = json!({
        "bindingSha256":hash::text(binding),
        "action":action,
        "readiness":detail["effectiveAction"]["readiness"],
    });
    let check_references = check_references(&state, &detail, &historical);
    bounded(json!({
        "protocol":1,
        "state":"current_detail_observed",
        "goal":goal["id"],
        "goalCurrent":goal_current,
        "workRef":format!("readRoutes.workDetailArgv[{member_index}]"),
        "work":work,
        "workStartEventSha256":reference.start_event_sha256,
        "assignment":assignment,
        "phase":{"stage":state["currentStage"],"attempt":state["attempt"],"status":state["status"]},
        "currentWorkDetail":current,
        "checkReferences":check_references,
        "routes":routes,
    }))
}

/// Compact check anchors encode `eventSha256|targetEventSha256|attempt`.
/// The reducer has already validated the referenced events and their chain.
fn check_references(state: &Value, detail: &Value, saved_step: &Value) -> Value {
    let checks = state["checks"].as_array().cloned().unwrap_or_default();
    let has_current_check_status = detail["sections"]["evidence"]["checkEvidence"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["status"] == "passed"));
    let attested = detail["sections"]["evidence"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            has_current_check_status
                && item["historical"] == false
                && matches!(item["kind"].as_str(), Some("check" | "preservation"))
        })
        .filter_map(|item| item["eventSha256"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let current_event = checks.last().filter(|event| {
        has_current_check_status
            && attested.contains(event["eventSha256"].as_str().unwrap_or_default())
            && crate::run_value::check_passed(event)
    });
    let current = current_event.map(compact_check_reference);
    let prior = checks
        .iter()
        .rev()
        .find(|event| Some(*event) != current_event)
        .map(compact_check_reference);
    let mut historical = vec![saved_step.clone()];
    if let Some(prior) = prior {
        historical.push(json!(prior));
    }
    json!({"current":current,"historical":historical})
}

fn compact_check_reference(event: &Value) -> String {
    format!(
        "{}|{}|{}",
        event["eventSha256"].as_str().unwrap_or("unknown"),
        event["targetEventSha256"].as_str().unwrap_or("unknown"),
        event["attempt"].as_u64().unwrap_or_default(),
    )
}

fn unavailable(
    goal: Value,
    goal_current: bool,
    reference: &WorkRef,
    historical: Value,
    routes: Vec<Value>,
    reason: &str,
) -> Value {
    bounded(json!({
        "protocol":1,
        "state":"pending_or_unknown",
        "reason":reason,
        "goal":goal["id"],
        "goalCurrent":goal_current,
        "workRef":reference.work,
        "assignment":{"state":"unknown"},
        "phase":{"state":"unknown"},
        "currentWorkDetail":null,
        "checkReferences":{"current":null,"historical":historical},
        "routes":routes,
    }))
}

fn bounded(value: Value) -> Value {
    let Ok(bytes) = serde_json::to_vec_pretty(&value) else {
        return json!({"protocol":1,"state":"pending_or_unknown","reason":"milestone_serialization_failed","work":{"state":"unknown"},"assignment":{"state":"unknown"},"evidence":{"currentWorkDetail":null,"historicalSavedStep":null},"routes":[]});
    };
    let lines = bytes.iter().filter(|byte| **byte == b'\n').count() + 1;
    if bytes.len() <= MAX_PRETTY_BYTES && lines <= MAX_LINES {
        return value;
    }
    json!({"protocol":1,"state":"pending_or_unknown","reason":if bytes.len() > MAX_PRETTY_BYTES {"milestone_bytes_exceeded"} else {"milestone_lines_exceeded"},"measuredBytes":bytes.len(),"measuredLines":lines,"work":{"state":"unknown"},"assignment":{"state":"unknown"},"evidence":{"currentWorkDetail":null,"historicalSavedStep":null},"routes":[]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_references_keep_current_and_historical_exact_anchors_separate() {
        let prior_event = "a".repeat(64);
        let prior_target = "b".repeat(64);
        let current_event = "c".repeat(64);
        let current_target = "d".repeat(64);
        let state = json!({
            "attempt":2,
            "checks":[
                {"eventSha256":prior_event,"targetEventSha256":prior_target,"attempt":1,"result":{"code":1}},
                {"eventSha256":current_event,"targetEventSha256":current_target,"attempt":2,"result":{"kind":"exit","code":0}}
            ]
        });
        let detail = json!({"sections":{"evidence":{
            "checkEvidence":[{"status":"passed"}],
            "items":[{"kind":"check","eventSha256":current_event,"historical":false}]
        }}});
        let saved_step = json!({"savedStepRouteRef":"savedHint.currentStep","current":false});
        let references = check_references(&state, &detail, &saved_step);

        assert_eq!(
            references["current"],
            format!("{current_event}|{current_target}|2")
        );
        assert_eq!(
            references["historical"][0]["savedStepRouteRef"],
            "savedHint.currentStep"
        );
        assert_eq!(references["historical"][0]["current"], false);
        assert_eq!(
            references["historical"][1],
            format!("{prior_event}|{prior_target}|1")
        );
    }

    #[test]
    fn same_attempt_check_without_fresh_detail_attestation_stays_historical() {
        let event_sha = "c".repeat(64);
        let target_sha = "d".repeat(64);
        let state = json!({"attempt":2,"checks":[{
            "eventSha256":event_sha,"targetEventSha256":target_sha,"attempt":2
        }]});
        let stale_detail = json!({"sections":{"evidence":{
            "checkEvidence":[{"status":"missing"}],
            "items":[{"kind":"check","eventSha256":event_sha,"historical":false}]
        }}});
        let saved_step = json!({"savedStepRouteRef":"savedHint.currentStep","current":false});

        let references = check_references(&state, &stale_detail, &saved_step);

        assert_eq!(references["current"], Value::Null);
        assert_eq!(
            references["historical"][1],
            format!("{event_sha}|{target_sha}|2")
        );
    }

    #[test]
    fn failed_latest_preservation_is_not_masked_by_an_earlier_pass() {
        let check_sha = "a".repeat(64);
        let check_target = "b".repeat(64);
        let preservation_sha = "c".repeat(64);
        let preservation_target = "d".repeat(64);
        let state = json!({
            "checks":[
                {"eventSha256":check_sha,"targetEventSha256":check_target,"attempt":1,"result":{"kind":"exit","code":0}},
                {"eventSha256":preservation_sha,"targetEventSha256":preservation_target,"attempt":1,"requirementId":"preserve","result":{"kind":"exit","code":1}}
            ]
        });
        let detail = json!({"sections":{"evidence":{
            "checkEvidence":[{"status":"passed"}],
            "items":[
                {"kind":"check","eventSha256":check_sha,"historical":false},
                {"kind":"preservation","eventSha256":preservation_sha,"historical":false}
            ]
        }}});
        let saved_step = json!({"savedStepRouteRef":"savedHint.currentStep","current":false});

        let references = check_references(&state, &detail, &saved_step);

        assert_eq!(references["current"], Value::Null);
        assert_eq!(
            references["historical"][1],
            format!("{preservation_sha}|{preservation_target}|1")
        );
    }
}

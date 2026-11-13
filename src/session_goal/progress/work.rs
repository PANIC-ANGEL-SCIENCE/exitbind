// Projects Goal progress against a verified Work association.
// Reports an unbound Goal as unavailable; see `src/session_goal/progress.rs`.
//! Work-bound Goal projection and verified association.
use super::{
    project_with_limit, unavailable, GoalProjectionContext, JourneyObservation, MAX_TASKS,
};
use crate::config::Loaded;
use serde_json::{json, Value};

pub(in crate::session_goal) fn project_for_work(
    loaded: &Loaded,
    work: &str,
    goal_root: &std::path::Path,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
    journey: &mut JourneyObservation,
) -> Value {
    let Some(record) = record else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "no canonical Lead goal",
            None,
            "unavailable",
        );
    };
    match record_bound_to_work_with_lineage(loaded, record, work) {
        Ok(true) => {}
        Ok(false) => {
            return unavailable(
                result,
                "overall_goal_unavailable",
                "canonical Lead goal is not bound to this Work",
                None,
                "unavailable",
            )
        }
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    }
    let mut projection = project_with_limit(
        GoalProjectionContext {
            loaded: Some(loaded),
            root: Some(goal_root),
            project_selected: false,
        },
        Some(record),
        rendered,
        result,
        MAX_TASKS,
        false,
        Some(journey),
    );
    let association = match work_association_with_lineage(loaded, record, work) {
        Ok(value) => value,
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    };
    if let Some(association) = association {
        projection["workAssociation"] = association_view(record, association, work);
    }
    projection
}

pub(in crate::session_goal) fn project_detail_for_work(
    loaded: &Loaded,
    work: &str,
    goal_root: &std::path::Path,
    record: Option<&Value>,
    rendered: &Value,
    result: Option<&Value>,
    journey: &mut JourneyObservation,
) -> Value {
    let Some(record) = record else {
        return unavailable(
            result,
            "overall_goal_unavailable",
            "no canonical Lead goal",
            None,
            "unavailable",
        );
    };
    match record_bound_to_work_with_lineage(loaded, record, work) {
        Ok(true) => {}
        Ok(false) => {
            return unavailable(
                result,
                "overall_goal_unavailable",
                "canonical Lead goal is not bound to this Work",
                None,
                "unavailable",
            )
        }
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    }
    let mut projection = project_with_limit(
        GoalProjectionContext {
            loaded: Some(loaded),
            root: Some(goal_root),
            project_selected: false,
        },
        Some(record),
        rendered,
        result,
        usize::MAX,
        true,
        Some(journey),
    );
    let association = match work_association_with_lineage(loaded, record, work) {
        Ok(value) => value,
        Err(error) => {
            return unavailable(
                result,
                "successor_lineage_unavailable",
                &error,
                None,
                "unavailable",
            )
        }
    };
    if let Some(association) = association {
        projection["workAssociation"] = association_view(record, association, work);
    }
    projection
}

pub(in crate::session_goal) fn record_bound_to_work(record: &Value, work: &str) -> bool {
    record["goalId"].as_str() == Some(work)
        || record["continuation"]["work"].as_str() == Some(work)
        || super::super::association::for_work(record, work).is_some()
        || super::super::requirements::bound(record, work)
}

pub(in crate::session_goal) fn record_bound_to_work_with_lineage(
    loaded: &Loaded,
    record: &Value,
    work: &str,
) -> Result<bool, String> {
    if record_bound_to_work(record, work) {
        return Ok(true);
    }
    Ok(super::super::journey::member_for_work_with_lineage(loaded, record, work)?.is_some())
}

/// These projections serve the owner project (and legacy project Goals). A
/// Work whose exact start is recorded in this Goal's journey is shown as a
/// member; a link without membership keeps its context-only label.
fn association_view(record: &Value, association: Value, work: &str) -> Value {
    if association["kind"] == "verified_successor_lineage" {
        return association;
    }
    let member = super::super::journey::member_for_work(record, work)
        .ok()
        .flatten();
    if let Some(member) =
        member.filter(|member| member["startEventSha256"] == association["startEventSha256"])
    {
        return json!({
            "kind": "journey_member",
            "membershipProtocol": record["journeyMembershipProtocol"],
            "goalId": record["goalId"],
            "work": member["work"],
            "startEventSha256": member["startEventSha256"],
        });
    }
    json!({
        "kind": "context_only",
        "protocol": super::super::association::PROTOCOL,
        "work": association["work"],
        "startEventSha256": association["startEventSha256"],
    })
}

fn work_association_with_lineage(
    loaded: &Loaded,
    record: &Value,
    work: &str,
) -> Result<Option<Value>, String> {
    if let Some(association) = super::super::association::for_work(record, work) {
        return Ok(Some(association));
    }
    Ok(
        super::super::journey::member_for_work_with_lineage(loaded, record, work)?.map(|member| {
            json!({
                "kind":"verified_successor_lineage",
                "work":work,
                "originalMemberWork":member["work"],
                "originalStartEventSha256":member["startEventSha256"],
            })
        }),
    )
}

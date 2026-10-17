//! Shape validation only. A persisted marker does not authenticate its claim;
//! the continuation runtime verifies the actual association before effects.

use crate::project::continuation::control::Association;
use serde_json::Value;

pub(super) fn validate_start(event: &Value, line: usize) -> Result<(), String> {
    let Some(marker) = event.get("projectContinuation") else {
        return Ok(());
    };
    let invalid = || format!("invalid run ledger line {line}: invalid project continuation");
    if event["version"].as_u64() != Some(8) {
        return Err(invalid());
    }
    let association: Association = serde_json::from_value(marker.clone()).map_err(|_| invalid())?;
    association.validate().map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn marker() -> Value {
        json!({
            "version":1,"project":"example-project","mandateId":"bounded-improvement",
            "mandateRevision":1,"specificationSha256":"a".repeat(64),
            "taskId":"first-slice","claimRoot":".exitbind/artifacts/continuation/example"
        })
    }

    fn start() -> Value {
        let plan = json!({"version":1,"maxParallel":1,"stages":[
            {"stage":1,"agents":[{
                "name":"worker","role":"worker","displayName":"worker",
                "nativeTaskName":"worker","profile":"agents/worker.md",
                "profileSha256":"b".repeat(64),"runtime":{},"declaredBoundary":{}
            }]}
        ]});
        let run_id = "c".repeat(64);
        let configuration = "d".repeat(64);
        let subject = super::super::subject(
            "bounded useful outcome",
            &plan,
            &configuration,
            &run_id,
            None,
        );
        super::super::state::make_event(json!({
            "version":8,"kind":"run","producer":{"name":"exitbind","version":"test","commit":null},
            "action":"start","runId":run_id,"workflow":"default","goal":"bounded useful outcome",
            "configSha256":configuration,"plan":plan,"subject":subject,
            "previousEventSha256":null,"timestamp":"2026-01-01T00:00:00Z"
        }))
    }

    #[test]
    fn associated_v8_replays_exact_marker_and_ordinary_start_has_none() {
        let ordinary = start();
        let ordinary_state = crate::run::state::reduce(&[ordinary.clone()]).unwrap();
        assert!(ordinary_state.get("projectContinuation").is_none());
        let mut associated = ordinary;
        associated["projectContinuation"] = marker();
        associated.as_object_mut().unwrap().remove("eventSha256");
        let associated = crate::run::state::make_event(associated);
        crate::run::state::validate_event(&associated, None, 1).unwrap();
        let state = crate::run::state::reduce(&[associated.clone()]).unwrap();
        assert_eq!(
            state["projectContinuation"],
            associated["projectContinuation"]
        );
    }

    #[test]
    fn malformed_or_additive_association_cannot_replay() {
        for case in ["null", "extra", "revision", "digest", "task", "version"] {
            let mut event = start();
            let mut association = marker();
            match case {
                "null" => association = Value::Null,
                "extra" => association["ownerApproved"] = json!(true),
                "revision" => association["mandateRevision"] = json!(0),
                "digest" => association["specificationSha256"] = json!("unknown"),
                "task" => association["taskId"] = json!("../different"),
                "version" => association["version"] = json!(2),
                _ => unreachable!(),
            }
            event["projectContinuation"] = association;
            assert!(
                crate::run::state::validate_start(&event, 7).is_err(),
                "{case}"
            );
        }
    }

    #[test]
    fn association_requires_v8_and_unknown_neighbors_remain_strict() {
        let mut event = start();
        event["projectContinuation"] = marker();
        for version in 1..8 {
            event["version"] = json!(version);
            let error = crate::run::state::validate_start(&event, 1).unwrap_err();
            assert!(
                error.contains("unknown field 'projectContinuation'"),
                "{version}: {error}"
            );
        }
        event["version"] = json!(8);
        event["projectContinuations"] = marker();
        let error = crate::run::state::validate_start(&event, 1).unwrap_err();
        assert!(error.contains("unknown field 'projectContinuations'"));
    }

    #[test]
    fn exact_association_is_part_of_stable_start_intent() {
        let mut event = start();
        let ordinary = crate::run::start_event_intent_sha256(&event).unwrap();
        event["projectContinuation"] = marker();
        let associated = crate::run::start_event_intent_sha256(&event).unwrap();
        assert_ne!(ordinary, associated);
        event["timestamp"] = json!("2026-01-02T00:00:00Z");
        event["runId"] = json!("e".repeat(64));
        assert_eq!(
            associated,
            crate::run::start_event_intent_sha256(&event).unwrap()
        );
        for field in ["taskId", "claimRoot", "specificationSha256"] {
            let mut changed = event.clone();
            changed["projectContinuation"][field] = json!("changed");
            assert_ne!(
                associated,
                crate::run::start_event_intent_sha256(&changed).unwrap(),
                "{field}"
            );
        }
    }
}

//! Immutable verification acknowledgement and replay guards. These commitments
//! do not attest that the planned execution environment was actually exercised.
use super::*;

pub(crate) fn acknowledgement(marker: &Value, event: &Value) -> Result<Value, String> {
    validate_marker(marker, 0)?;
    let frozen = &marker["verification"];
    if event["action"] != "submit"
        || event["role"] != "worker"
        || event["outcome"] != "completed"
        || event.get("governorEvent").is_some()
        || event["artifact"]["sha256"] != frozen["artifactSha256"]
        || event["inputsSha256"] != frozen["inputsSha256"]
        || event["stage"].as_u64().map_or(true, |stage| stage == 0)
        || event["attempt"].as_u64() != Some(1)
        || !text(&event["agent"], 128)
    {
        return Err("verification completion must acknowledge the exact frozen artifact and inputs without mutation".into());
    }
    Ok(json!({
        "protocol":PROTOCOL,"identity":one_shot_identity(marker)?,
        "decisionSha256":marker["decisionSha256"],
        "artifactSha256":frozen["artifactSha256"],"inputsSha256":frozen["inputsSha256"],
        "commandSha256":frozen["commandSha256"],"configSha256":frozen["configSha256"],
        "environmentSha256":frozen["environmentSha256"],
        "stage":event["stage"],"attempt":event["attempt"],"agent":event["agent"]
    }))
}

pub(crate) fn validate_event(state: &Value, event: &Value) -> Result<(), String> {
    let Some(marker) = state.get("quarantineVerification") else {
        return if event.get("quarantineVerificationAck").is_some() {
            Err("verification acknowledgement requires a marked verification run".into())
        } else {
            Ok(())
        };
    };
    validate_marker(marker, 0)?;
    if event.get("governorEvent").is_some()
        || matches!(
            event["action"].as_str(),
            Some("govern" | "protect" | "review_policy")
        )
        || (event["role"] == "lead"
            && matches!(event["outcome"].as_str(), Some("rework" | "disposition")))
    {
        return Err(
            "verification-only authority cannot mutate accounting, policy or repair scope".into(),
        );
    }
    if event.get("inputsSha256").is_some()
        && event["inputsSha256"] != marker["verification"]["inputsSha256"]
    {
        return Err("verification inputs differ from the frozen authorization".into());
    }
    if event["action"] == "submit" && event["role"] == "worker" && event["outcome"] == "completed" {
        if event["quarantineVerificationAck"] != acknowledgement(marker, event)?
            || state["submissions"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["role"] == "worker" && item["outcome"] == "completed")
            })
        {
            return Err(
                "verification completion is missing, changed or already acknowledged".into(),
            );
        }
    } else if event.get("quarantineVerificationAck").is_some() {
        return Err("verification acknowledgement is only valid on its worker completion".into());
    }
    if event["action"] == "check_observation" {
        let events = state["events"]
            .as_array()
            .ok_or("verification ledger events are missing")?;
        if events
            .iter()
            .take_while(|prior| prior["eventSha256"] != event["eventSha256"])
            .any(|prior| prior["action"] == "check_observation")
        {
            return Err(
                "verification check allowance is already admitted; no retry is available".into(),
            );
        }
        if event["observation"]["binding"]["inputsSha256"] != marker["verification"]["inputsSha256"]
            || event["observation"]["binding"]["checkCommandSha256"]
                != marker["verification"]["commandSha256"]
        {
            return Err("verification admission differs from its frozen inputs or command".into());
        }
    }
    if event["action"] == "check"
        && (event["acquisition"] != "observed"
            || event["origin"] != "local_report"
            || event["checkCommandSha256"] != marker["verification"]["commandSha256"]
            || state["checks"]
                .as_array()
                .map_or(true, |items| !items.is_empty()))
    {
        return Err("verification requires its single actually observed frozen check".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({"quarantineVerification":crate::run::quarantine::tests::marker(),
            "submissions":[],"checks":[],"events":[]})
    }
    fn completion() -> Value {
        json!({"action":"submit","role":"worker","outcome":"completed",
            "artifact":{"sha256":"a".repeat(64)},"inputsSha256":"a".repeat(64),
            "stage":2,"attempt":1,"agent":"worker"})
    }
    #[test]
    fn quarantine_completion_preserves_original_accounting_without_grant_or_charge() {
        let state = state();
        let before = state.to_string();
        let mut event = completion();
        event["quarantineVerificationAck"] =
            acknowledgement(&state["quarantineVerification"], &event).unwrap();
        validate_event(&state, &event).unwrap();
        assert_eq!(state.to_string(), before);
        assert!(event.get("governorEvent").is_none());
        assert_eq!(
            state["quarantineVerification"]["predecessor"]["accounting"]["governor"]["spent"],
            9
        );
        assert!(
            state["quarantineVerification"]["predecessor"]["accounting"]["usageAccount"]["totals"]
                .is_null()
        );
    }
    #[test]
    fn quarantine_completion_refuses_unmarked_mixed_changed_extra_and_duplicate_ack() {
        let state = state();
        let mut event = completion();
        event["quarantineVerificationAck"] =
            acknowledgement(&state["quarantineVerification"], &event).unwrap();
        assert!(validate_event(&json!({}), &event).is_err());
        for path in [
            "/artifact/sha256",
            "/inputsSha256",
            "/quarantineVerificationAck/environmentSha256",
        ] {
            let mut changed = event.clone();
            *changed.pointer_mut(path).unwrap() = json!("b".repeat(64));
            assert!(validate_event(&state, &changed).is_err(), "{path}");
        }
        let mut mixed = event.clone();
        mixed["governorEvent"] = json!({"action":"mutation"});
        assert!(validate_event(&state, &mixed).is_err());
        let mut extra = event.clone();
        extra["quarantineVerificationAck"]["budget"] = json!(3);
        assert!(validate_event(&state, &extra).is_err());
        let mut duplicate = state.clone();
        duplicate["submissions"] = json!([{"role":"worker","outcome":"completed"}]);
        assert!(validate_event(&duplicate, &event).is_err());
        let mut missing = event;
        missing
            .as_object_mut()
            .unwrap()
            .remove("quarantineVerificationAck");
        assert!(validate_event(&state, &missing).is_err());
    }
    #[test]
    fn quarantine_transition_refuses_mutation_policy_repair_reported_and_second_checks() {
        let state = state();
        for action in ["govern", "protect", "review_policy"] {
            assert!(validate_event(&state, &json!({"action":action})).is_err());
        }
        for outcome in ["rework", "disposition"] {
            assert!(validate_event(
                &state,
                &json!({"action":"submit","role":"lead","outcome":outcome})
            )
            .is_err());
        }
        let observed = json!({"action":"check","acquisition":"observed","origin":"local_report",
            "checkCommandSha256":"a".repeat(64),"inputsSha256":"a".repeat(64)});
        validate_event(&state, &observed).unwrap();
        let mut reported = observed.clone();
        reported["acquisition"] = json!("reported");
        assert!(validate_event(&state, &reported).is_err());
        let mut used = state;
        used["checks"] = json!([observed.clone()]);
        assert!(validate_event(&used, &observed).is_err());
    }
    #[test]
    fn quarantine_check_admission_is_once_and_future_events_do_not_consume_it() {
        let mut state = state();
        let admission = json!({"action":"check_observation","eventSha256":"1".repeat(64),
            "observation":{"binding":{"inputsSha256":"a".repeat(64),"checkCommandSha256":"a".repeat(64)}}});
        state["events"] =
            json!([admission.clone(),{"action":"check_observation","eventSha256":"2".repeat(64)}]);
        validate_event(&state, &admission).unwrap();
        let mut second = admission;
        second["eventSha256"] = json!("2".repeat(64));
        assert!(validate_event(&state, &second).is_err());
        validate_event(&json!({}), &json!({"action":"govern"})).unwrap();
    }
}

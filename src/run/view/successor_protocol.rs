//! Exact successor replay comparison and canonical Work response.
use super::*;

pub(super) fn attach_successor_work(loaded: &Loaded, new_ledger: &str, mut value: Value) -> Value {
    if let Some(work) = crate::work::locator_for_ledger(new_ledger) {
        value["work"] = json!(work);
        value["currentDetail"] = json!({
            "route": "work detail",
            "readOnly": true,
            "requiresFreshContext": true,
            "command": crate::work::action_forms::current_detail_command(loaded, &work),
        });
    }
    value
}

pub(super) fn attach_goal_linkage(loaded: &Loaded, ledger: &str, mut value: Value) -> Value {
    if let Err(error) = crate::project::continuation::runtime::link_successor_goal(loaded, ledger) {
        value["linkageRecovery"] = json!(error);
    }
    value
}

pub(super) fn assert_same_request(existing: &[Value], event: &Value) -> Result<(), String> {
    let request_fields = [
        "runId",
        "workflow",
        "goal",
        "configSha256",
        "plan",
        "checkPolicy",
        "preservation",
        "basisProtocol",
        "basis",
        "reviewPolicy",
        "recoveryProtocol",
        "observationNoveltyProtocol",
        "governor",
        "supersedes",
        "carryProtocol",
        "governorCarrySha256",
        "checkReplacement",
        "preReturnPolicy",
        "verificationPreparationProtocol",
        "verificationCheckerSources",
        "verificationInputContract",
        "projectContinuation",
    ];
    let mismatched = existing.first().map(|current| {
        request_fields
            .into_iter()
            .filter(|field| current.get(*field) != event.get(*field))
            .collect::<Vec<_>>()
    });
    let same_subject = existing.first().is_some_and(|current| {
        match (current.get("subject"), event.get("subject")) {
            (Some(Value::Object(current)), Some(Value::Object(requested))) => current
                .iter()
                .filter(|(key, _)| key.as_str() != "sha256")
                .all(|(key, value)| requested.get(key) == Some(value)),
            (None, None) => true,
            _ => false,
        }
    });
    if mismatched.as_ref().is_some_and(Vec::is_empty) && same_subject {
        return Ok(());
    }
    let mut fields = mismatched.unwrap_or_default();
    if !same_subject {
        fields.push("subject");
    }
    Err(format!(
        "successor ledger already exists with different provenance (fields: {})",
        fields.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successor_replay_requires_the_exact_frozen_association() {
        let event = json!({"runId":"same-run","goal":"same-outcome",
            "projectContinuation":{"version":1,"taskId":"same-task","claimRoot":"same-root"}});
        assert_same_request(&[event.clone()], &event).unwrap();
        for change in ["removed", "task", "root", "extra"] {
            let mut changed = event.clone();
            match change {
                "removed" => {
                    changed
                        .as_object_mut()
                        .unwrap()
                        .remove("projectContinuation");
                }
                "task" => changed["projectContinuation"]["taskId"] = json!("different"),
                "root" => changed["projectContinuation"]["claimRoot"] = json!("different"),
                "extra" => changed["projectContinuation"]["approved"] = json!(true),
                _ => unreachable!(),
            }
            let error = assert_same_request(&[event.clone()], &changed).unwrap_err();
            assert!(error.contains("projectContinuation"), "{change}: {error}");
        }
    }

    #[test]
    fn ordinary_successor_replay_keeps_its_existing_comparison() {
        let event =
            json!({"runId":"same-run","goal":"same-outcome","checkPolicy":{"command":"check"}});
        assert_same_request(&[event.clone()], &event).unwrap();
        let mut changed = event.clone();
        changed["goal"] = json!("different-outcome");
        assert!(assert_same_request(&[event], &changed)
            .unwrap_err()
            .contains("goal"));
        assert!(assert_same_request(&[], &changed).is_err());
    }
}

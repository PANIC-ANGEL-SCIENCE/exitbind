//! Recover an existing failed native debit from its owning journal. These
//! observations do not authorize another provider attempt or relax retry rules.
use super::*;

fn identity(journal: &Value, work: &str) -> Result<Option<(String, Value)>, String> {
    let Some(key) = journal.get("continuationOperation") else {
        return Ok(None);
    };
    let key = bounded_id(key, "operation")?;
    if journal["version"] != 1
        || journal["work"] != work
        || !matches!(journal["status"].as_str(), Some("started" | "running"))
    {
        return Err("failed native recovery journal does not match its Work or status".into());
    }
    let assignment = bounded_id(&journal["assignment"], "assignment")?;
    if assignment.len() > 128
        || !assignment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        || !journal["assignmentSha256"].as_str().is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || !matches!(
            journal["role"].as_str(),
            Some("worker" | "reviewer" | "adviser")
        )
        || journal["stage"]
            .as_u64()
            .filter(|stage| *stage > 0)
            .is_none()
        || journal["attempt"]
            .as_u64()
            .filter(|attempt| *attempt > 0)
            .is_none()
    {
        return Err("failed native recovery assignment identity is invalid".into());
    }
    bounded_id(&journal["agent"], "agent")?;
    let operation = bounded_id(&journal["operationId"], "operation identity")?;
    if !journal["request"].is_object() {
        return Err("failed native recovery request is invalid".into());
    }
    let input = journal["request"].clone();
    let expected = hash::value(&json!({"work":work,"assignment":assignment,
        "operation":operation,"request":input}));
    if key != expected {
        return Err("failed native recovery operation does not bind its original request".into());
    }
    Ok(Some((key, input)))
}

fn failed_proof(
    journal: &Value,
    work: &str,
    liveness: ProcessLiveness,
) -> Result<Option<(String, Value)>, String> {
    let Some(bound) = identity(journal, work)? else {
        return Ok(None);
    };
    if liveness != ProcessLiveness::Ended {
        return Ok(None);
    }
    let pid = journal["processIdentity"]["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0);
    let group = journal["processIdentity"]["processGroup"].as_i64();
    if pid.is_none() || group != pid.map(i64::from) {
        return Err("failed native recovery has no original owned process identity".into());
    }
    if let Some(result) = journal.get("provisionalFinalResult") {
        let bytes = serde_json::to_vec(result).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_RESULT_BYTES
            || journal["provisionalFinalResultBytes"] != hex_encode(&bytes)
        {
            return Err("failed native recovery retained result bytes do not match".into());
        }
    } else if journal.get("provisionalFinalResultBytes").is_some() {
        return Err("failed native recovery retained result is missing".into());
    }
    let process = &journal["observation"]["process"];
    let failed_process = process["success"] == false
        && (process["code"].as_i64().is_some_and(|code| code != 0)
            || process["signal"].as_i64().is_some_and(|signal| signal > 0)
            || process["timedOut"] == true);
    let interruption = &journal["controlledInterruption"];
    let controlled = interruption["version"] == 1
        && interruption["spawned"] == true
        && interruption["bounded"] == true
        && interruption["rootReaped"] == true
        && interruption["processIdentity"] == journal["processIdentity"]
        && interruption["actualProcessOutcome"].is_object();
    let error = journal["error"]
        .as_str()
        .is_some_and(|error| !error.trim().is_empty() && error.chars().count() <= 1024);
    if !(failed_process || controlled || error) {
        // Accounting flags, root exit alone, and an unknown/live group never
        // establish that the original provider operation has safely ended.
        return Ok(None);
    }
    Ok(Some(bound))
}

pub(in crate::work::action::native_action) fn reconcile_failed(
    loaded: &Loaded,
    work: &str,
    journal: &Value,
) -> Result<(), String> {
    if journal.get("continuationOperation").is_none() || journal["status"] == "completed" {
        return Ok(());
    }
    let state = state(loaded, work)?;
    if state.get("projectContinuation").is_none() {
        return Err("failed native continuation result has no Work association".into());
    }
    if let Some((key, input)) = failed_proof(journal, work, recovery::liveness(journal))? {
        runtime::recover_observed(loaded, &state, &key, Kind::Native, &input, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed() -> Value {
        let mut journal = json!({"version":1,"work":"fixture-work","status":"started",
            "assignment":"fixture-assignment","assignmentSha256":hash::text("packet"),
            "role":"worker","agent":"worker-a","stage":2,"attempt":1,
            "operationId":"fixture-operation","request":{"executable":"/fixture/native"},
            "processIdentity":{"pid":123,"processGroup":123,"startTimeTicks":45},
            "observation":{"process":{"code":1,"signal":null,"success":false,"timedOut":false}},
            "accountingRecovery":{"status":"pending","success":false,"knownEnded":true}});
        journal["continuationOperation"] = json!(hash::value(&json!({"work":"fixture-work",
            "assignment":journal["assignment"],"operation":journal["operationId"],
            "request":journal["request"]})));
        journal
    }

    #[test]
    fn known_failed_native_recovery_requires_actual_original_group_end() {
        let journal = failed();
        let proof = failed_proof(&journal, "fixture-work", ProcessLiveness::Ended)
            .unwrap()
            .unwrap();
        assert_eq!(proof.0, journal["continuationOperation"].as_str().unwrap());
        assert_eq!(proof.1, journal["request"]);
        for live in [ProcessLiveness::Alive, ProcessLiveness::Uncertain] {
            assert!(failed_proof(&journal, "fixture-work", live)
                .unwrap()
                .is_none());
        }
        let mut no_failure = journal.clone();
        no_failure.as_object_mut().unwrap().remove("observation");
        assert!(
            failed_proof(&no_failure, "fixture-work", ProcessLiveness::Ended)
                .unwrap()
                .is_none(),
            "accountingRecovery alone cannot prove failure"
        );
        no_failure["error"] = json!("native provider stream is malformed");
        assert!(
            failed_proof(&no_failure, "fixture-work", ProcessLiveness::Ended)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn failed_native_recovery_binds_request_operation_assignment_and_retained_bytes() {
        let journal = failed();
        for (field, value) in [
            ("work", json!("other-work")),
            ("assignment", json!("other-assignment")),
            ("assignmentSha256", Value::Null),
            ("operationId", json!("other-operation")),
            ("continuationOperation", json!("another-receipt")),
            ("request", json!({"executable":"/other/native"})),
            ("role", json!("lead")),
            ("stage", json!(0)),
            ("processIdentity", json!({"pid":123,"processGroup":124})),
        ] {
            let mut invalid = journal.clone();
            invalid[field] = value;
            assert!(
                failed_proof(&invalid, "fixture-work", ProcessLiveness::Ended).is_err(),
                "{field}"
            );
        }
        let mut retained = journal.clone();
        let result = json!({"outcome":"completed","summary":"provisional","reason":""});
        retained["provisionalFinalResult"] = result.clone();
        retained["provisionalFinalResultBytes"] =
            json!(hex_encode(&serde_json::to_vec(&result).unwrap()));
        assert!(
            failed_proof(&retained, "fixture-work", ProcessLiveness::Ended)
                .unwrap()
                .is_some()
        );
        retained["provisionalFinalResultBytes"] = json!("00");
        assert!(failed_proof(&retained, "fixture-work", ProcessLiveness::Ended).is_err());
        let mut ordinary = journal;
        ordinary
            .as_object_mut()
            .unwrap()
            .remove("continuationOperation");
        assert!(
            failed_proof(&ordinary, "fixture-work", ProcessLiveness::Uncertain)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn controlled_native_recovery_requires_owned_facts_and_dynamic_group_end() {
        let mut journal = failed();
        journal.as_object_mut().unwrap().remove("observation");
        journal["controlledInterruption"] = json!({"version":1,"spawned":true,"bounded":true,
            "rootReaped":true,"processIdentity":journal["processIdentity"],
            "actualProcessOutcome":{"code":null,"signal":15,"success":false},
            "wholeGroupEnded":true});
        assert!(
            failed_proof(&journal, "fixture-work", ProcessLiveness::Ended)
                .unwrap()
                .is_some()
        );
        assert!(
            failed_proof(&journal, "fixture-work", ProcessLiveness::Uncertain)
                .unwrap()
                .is_none()
        );
        journal["controlledInterruption"]["processIdentity"]["pid"] = json!(999);
        assert!(
            failed_proof(&journal, "fixture-work", ProcessLiveness::Ended)
                .unwrap()
                .is_none()
        );
    }
}

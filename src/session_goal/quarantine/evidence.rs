//! Acquisition and validation of the immutable old and verification Work evidence.

use super::{decision_digest, Endpoint, Residual};
use crate::{config::Loaded, evidence::hash};
use serde_json::{json, Value};

fn events_for(loaded: &Loaded, work_id: &str) -> Result<(Vec<Value>, String, String), String> {
    let ledger = crate::work::resolve(loaded, work_id)?;
    let (_, events, bytes) = crate::run::ledger::load(loaded, &ledger)?;
    Ok((events, ledger, bytes))
}

struct EndpointEvidence<'a> {
    member: &'a super::super::journey::Member,
    work: &'a str,
    events: &'a [Value],
    bytes: &'a str,
    state: &'a Value,
    check: Option<&'a Value>,
    review: Option<&'a Value>,
    acceptance: Option<&'a Value>,
}

fn endpoint(loaded: &Loaded, evidence: EndpointEvidence<'_>) -> Result<Endpoint, String> {
    let start = evidence
        .events
        .first()
        .ok_or("quarantine Work ledger is empty")?;
    let head = evidence
        .events
        .last()
        .ok_or("quarantine Work ledger is empty")?;
    let artifact = evidence.state["submissions"]
        .as_array()
        .and_then(|items| items.iter().rev().find(|item| item["role"] == "worker"))
        .map(|item| item["artifact"]["sha256"].clone())
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or("quarantine Work has no worker artifact")?;
    Ok(Endpoint {
        work: evidence.work.to_owned(),
        member_sha256: super::super::journey::member_identity(evidence.member),
        start_event_sha256: start["eventSha256"]
            .as_str()
            .ok_or("Work start hash missing")?
            .into(),
        head_event_sha256: head["eventSha256"]
            .as_str()
            .ok_or("Work head hash missing")?
            .into(),
        ledger_sha256: hash::text(evidence.bytes),
        config_sha256: hash::text(&loaded.source),
        run_id: start["runId"]
            .as_str()
            .ok_or("Work run identity missing")?
            .into(),
        subject_sha256: evidence.state["subject"]["sha256"]
            .as_str()
            .ok_or("Work subject hash missing")?
            .into(),
        artifact_sha256: artifact,
        inputs_sha256: evidence.state["inputsSha256"]
            .as_str()
            .ok_or("Work inputs hash missing")?
            .into(),
        command_sha256: start["checkPolicy"]["commandSha256"]
            .as_str()
            .ok_or("Work check command hash missing")?
            .into(),
        check_event_sha256: evidence
            .check
            .map(|event| event["eventSha256"].as_str().unwrap_or_default().to_owned()),
        review_event_sha256: evidence
            .review
            .map(|event| event["eventSha256"].as_str().unwrap_or_default().to_owned()),
        acceptance_event_sha256: evidence
            .acceptance
            .map(|event| event["eventSha256"].as_str().unwrap_or_default().to_owned()),
    })
}

/// The verification Work records tested inputs with the state-aware digest.
/// The quarantine marker records the full fingerprint separately.
fn verification_inputs_current(
    source: &Loaded,
    state: &Value,
    marker: &Value,
) -> Result<bool, String> {
    let state_inputs = crate::run::inputs::fingerprint_for_state(source, state)?;
    let marker_inputs = crate::run::inputs::fingerprint(source)?;
    Ok(
        state["inputsSha256"].as_str() == Some(state_inputs.as_str())
            && marker["verification"]["inputsSha256"] == marker_inputs,
    )
}

pub(super) fn exact_quarantine_evidence(
    source: &Loaded,
    old_member: &super::super::journey::Member,
    old_work: &str,
    verification_member: &super::super::journey::Member,
) -> Result<(Residual, Value), String> {
    let (old_events, old_ledger, old_bytes) = events_for(source, old_work)?;
    let old_start = old_events.first().ok_or("predecessor ledger is empty")?;
    let old_head = old_events.last().ok_or("predecessor ledger is empty")?;
    let old_state = crate::run::state::reduce(&old_events)?;
    if old_state.get("quarantineVerification").is_some()
        || !matches!(old_state["status"].as_str(), Some("running" | "blocked"))
        || !crate::run::state::check_observation::unresolved(&old_state)
        || old_state["checkObservation"]["state"] != "failed"
        || old_state["checkObservation"]["facts"]["termination"] != "unknown"
    {
        return Err("predecessor is no longer the exact unresolved check observation".into());
    }
    let observation_admission = old_state["checkObservation"]["eventSha256"]
        .as_str()
        .ok_or("predecessor failed observation has no admission identity")?;
    let admission = old_events
        .iter()
        .find(|event| {
            event["action"] == "check_observation" && event["eventSha256"] == observation_admission
        })
        .ok_or("predecessor failed observation admission event is missing")?;
    if old_events.iter().any(|event| {
        (event["action"] == "check" && event["observationEventSha256"] == admission["eventSha256"])
            || (event["action"] == "check_observation_recovered"
                && event["observation"]["admissionEventSha256"] == admission["eventSha256"])
    }) {
        return Err(
            "current unknown observation admission already has a committed check or recovery"
                .into(),
        );
    }
    let old_canonical_ledger =
        crate::run::ledger::ledger_path(&source.state_root, &old_ledger, false)?;
    if crate::run::ledger::valid_claim(&crate::run::ledger::claim_path(&old_canonical_ledger))?
        .is_some()
    {
        return Err("unknown predecessor has already been superseded".into());
    }
    let failure = old_events.iter().rev().find(|event| {
        event["action"] == "check_observation_failed"
            && event["observation"]["admissionEventSha256"] == admission["eventSha256"]
            && event["observation"]["facts"]["termination"] == "unknown"
    });
    let failure = failure.ok_or("predecessor failed unknown observation event is missing")?;
    let observation = &failure["eventSha256"];
    let (verification_events, _verification_ledger, verification_bytes) =
        events_for(source, &verification_member.work)?;
    let start = verification_events
        .first()
        .ok_or("verification ledger is empty")?;
    let marker = start
        .get("quarantineVerification")
        .ok_or("verification Work has no quarantine marker")?;
    crate::run::quarantine::validate_marker(marker, 1)?;
    if marker["oldCredited"] != false
        || marker["oldReset"] != false
        || marker["predecessor"]["work"] != old_work
        || marker["predecessor"]["startEventSha256"] != old_start["eventSha256"]
        || marker["predecessor"]["headEventSha256"] != old_head["eventSha256"]
        || marker["predecessor"]["ledger"] != old_ledger
        || marker["predecessor"]["ledgerSha256"] != hash::text(&old_bytes)
        || marker["predecessor"]["admissionEventSha256"] != admission["eventSha256"]
        || &marker["predecessor"]["observationEventSha256"] != observation
        || marker["predecessor"]["runId"] != old_start["runId"]
        || marker["predecessor"]["configSha256"] != old_start["configSha256"]
        || marker["predecessor"]["configSha256"] != hash::text(&source.source)
    {
        return Err("verification marker does not bind the exact unresolved predecessor".into());
    }
    let old_accounting = json!({
        "governor": crate::run::carry::accounting(&old_state, &old_start["governor"])? ,
        "usageAccount": crate::session_goal::usage::details_for_work(source, old_work),
    });
    if marker["predecessor"]["accounting"] != old_accounting {
        return Err("predecessor accounting changed since quarantine verification".into());
    }

    let new_state = crate::run::state::reduce(&verification_events)?;
    crate::run::artifact::assert_current(source, &new_state)?;
    let result = crate::run::result_ref_evidence(source, &verification_member.work)?
        .ok_or("verification result evidence is unavailable")?;
    if new_state["status"] != "accepted"
        || !verification_inputs_current(source, &new_state, marker)?
        || !result.accepted
        || !result.artifact_current
        || result.drift.is_some()
        || result.inputs_current != Some(true)
        || start["configSha256"] != marker["verification"]["configSha256"]
        || start["configSha256"] != hash::text(&source.source)
    {
        return Err("verification Work is not currently accepted on current artifact, inputs and configuration".into());
    }
    let exit = crate::run::gate::exit::ExitState::reduce_with_artifact(&new_state, Some(true))?;
    if exit.status != "accepted"
        || !exit.lead_accepted()
        || !exit.review_required
        || !exit.reviewer_approved()
    {
        return Err(
            "verification Work lacks its current independent review and Lead acceptance".into(),
        );
    }
    let check = verification_events
        .iter()
        .rev()
        .find(|event| event["action"] == "check")
        .filter(|event| {
            event["acquisition"] == "observed"
                && (event["exitCode"].as_i64() == Some(0)
                    || event["result"]["code"].as_i64() == Some(0))
        })
        .ok_or("verification Work has no actual successful check")?;
    let review = verification_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit"
                && event["role"] == "reviewer"
                && event["outcome"] == "approved"
        })
        .ok_or("verification Work has no independent reviewer approval")?;
    let acceptance = verification_events
        .iter()
        .rev()
        .find(|event| {
            event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
        })
        .ok_or("verification Work has no Lead acceptance")?;
    let subject = new_state["subject"]["sha256"]
        .as_str()
        .ok_or("verification subject hash is missing")?;
    if check["subjectSha256"] != subject
        || review["subjectSha256"] != subject
        || acceptance["subjectSha256"] != subject
    {
        return Err("verification check, reviewer approval and Lead acceptance do not bind the current subject".into());
    }
    let old_endpoint = endpoint(
        source,
        EndpointEvidence {
            member: old_member,
            work: old_work,
            events: &old_events,
            bytes: &old_bytes,
            state: &old_state,
            check: Some(failure),
            review: None,
            acceptance: None,
        },
    )?;
    let verification_endpoint = endpoint(
        source,
        EndpointEvidence {
            member: verification_member,
            work: &verification_member.work,
            events: &verification_events,
            bytes: &verification_bytes,
            state: &new_state,
            check: Some(check),
            review: Some(review),
            acceptance: Some(acceptance),
        },
    )?;
    if marker["verification"]["inputsSha256"] != verification_endpoint.inputs_sha256
        || marker["verification"]["commandSha256"] != verification_endpoint.command_sha256
    {
        return Err(
            "verification result differs from its immutable quarantine artifact, inputs or command"
                .into(),
        );
    }
    let source_config = super::super::recovery::config_binding_for_current(source)?;
    let mut residual = Residual {
        request_id: marker["requestId"]
            .as_str()
            .ok_or("quarantine request id missing")?
            .to_owned(),
        request_sha256: String::new(),
        reason: marker["reason"]
            .as_str()
            .ok_or("quarantine reason missing")?
            .to_owned(),
        source_config,
        old_member: old_member.clone(),
        old: old_endpoint,
        verification: verification_endpoint,
        verified_artifact_sha256: marker["verification"]["artifactSha256"]
            .as_str()
            .ok_or("quarantine verified artifact hash missing")?
            .to_owned(),
        verification_environment_sha256: marker["verification"]["environmentSha256"]
            .as_str()
            .ok_or("quarantine verification environment hash missing")?
            .to_owned(),
        verification_decision_sha256: marker["decisionSha256"]
            .as_str()
            .ok_or("quarantine decision hash missing")?
            .to_owned(),
        old_accounting,
        decision_sha256: String::new(),
    };
    residual.decision_sha256 = decision_digest(&residual)?;
    Ok((residual, marker.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn marked_verification_inputs_compare_with_the_state_aware_digest() {
        let root = std::env::temp_dir().join(format!(
            "exitbind-quarantine-inputs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".exitbind")).unwrap();
        std::fs::write(root.join("exitbind.json"), "{}\n").unwrap();
        std::fs::write(root.join("src.rs"), "fn main() {}\n").unwrap();
        let loaded = Loaded {
            config: json!({}),
            agents: BTreeMap::new(),
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: BTreeMap::new(),
            verification_preparation: None,
            path: root.join("exitbind.json"),
            control_root: root.join(".exitbind"),
            product_root: root.clone(),
            state_root: root.join(".exitbind"),
            mode: crate::project::layout_types::Mode::Portable,
            project_id: None,
            source: "{}\n".to_owned(),
        };
        let state = json!({
            "guidanceContinuityProtocol": crate::run::guidance_continuity::PROTOCOL,
            "plan": {"stages": []},
            "checkPolicy": {"command": "true"},
            "verificationCheckerSources": [{
                "path": "checks/check.sh",
                "argumentPath": "checks/check.sh",
                "sha256": "a".repeat(64)
            }]
        });
        let state_inputs = crate::run::inputs::fingerprint_for_state(&loaded, &state).unwrap();
        let full_inputs = crate::run::inputs::fingerprint(&loaded).unwrap();
        assert_ne!(
            state_inputs, full_inputs,
            "the marked fixture must exclude a guidance path from the state digest"
        );
        let mut recorded = state;
        recorded["inputsSha256"] = json!(state_inputs);
        let marker = json!({"verification": {"inputsSha256": full_inputs}});
        assert!(verification_inputs_current(&loaded, &recorded, &marker).unwrap());

        let mut full_recorded = recorded;
        full_recorded["inputsSha256"] = json!(full_inputs);
        assert!(!verification_inputs_current(&loaded, &full_recorded, &marker).unwrap());

        std::fs::remove_dir_all(root).unwrap();
    }
}

use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

mod apply;
pub(crate) mod check_observation;
mod check_stage;
mod governor_validation;
mod historical_review;
mod recovery;
mod reviewer_transition;
mod validation;

use apply::apply_event;
#[cfg(test)]
use apply::{apply_govern, apply_submission, apply_unavailable};
use governor_validation::validate_grant_acknowledgement;
pub(crate) use recovery::forward_repair_candidate;
#[cfg(test)]
use validation::validate_submission;
pub use validation::{validate_event, validate_start};

const SHA_LEN: usize = 64;
const ROLES: &[&str] = &["lead", "adviser", "worker", "reviewer"];
pub(crate) const RECOVERY_PROTOCOL_VERSION: u64 = 1;
/// Bounded operational reasons a reviewer target may fail to execute. These are
/// the only admissible causes for `unavailable`; vendor prose is never parsed.
pub(crate) const FALLBACK_REASONS: &[&str] =
    &["provider_quota", "rate_limit", "provider_unavailable"];

pub fn make_event(mut value: Value) -> Value {
    let hash = crate::evidence::hash::value(&value);
    value["eventSha256"] = json!(hash);
    value
}

pub fn reduce(events: &[Value]) -> Result<Value, String> {
    if events.is_empty() {
        return Err("run ledger has no start event".into());
    }
    validate_start(&events[0], 1)?;
    let first = &events[0];
    let mut state = json!({
        "version": first["version"],
        "runId": first["runId"], "workflow": first["workflow"], "goal": first["goal"],
        "configSha256": first["configSha256"], "plan": first["plan"], "status": "running",
        "currentStage": 1, "attempt": 1, "submissions": [], "checks": [],
        "protections": [], "events": events,
        "basisHistory": [], "dispositions": [], "pendingDisposition": Value::Null
    });
    if let Some(marker) = first.get("basisProtocol") {
        if marker != crate::kernel::basis::PROTOCOL_VERSION {
            return Err("invalid run start: unsupported basis protocol".into());
        }
        state["basisProtocol"] = marker.clone();
        if let Some(basis) = first.get("basis") {
            state["basis"] = basis.clone();
        }
        if let Some(review) = first.get("reviewPolicy") {
            state["reviewPolicy"] = review.clone();
            state["reviewDecisions"] = json!([review]);
        }
    }
    if let Some(marker) = first.get("recoveryProtocol") {
        state["recoveryProtocol"] = marker.clone();
    }
    if let Some(marker) = first.get("checkObservationProtocol") {
        state["checkObservationProtocol"] = marker.clone();
    }
    let governor_enabled = super::carry::initialize_governor(first, &mut state)?;
    if let Some(receipt) = first.get("harnessReceipt") {
        state["harnessReceipt"] = receipt.clone();
    }
    if let Some(policy) = first.get("checkPolicy") {
        state["checkPolicy"] = policy.clone();
    }
    if let Some(preservation) = first.get("preservation") {
        state["preservation"] = preservation.clone();
    }
    if let Some(subject) = first.get("subject") {
        state["subject"] = subject.clone();
    }
    let mut request_scopes = BTreeSet::new();
    for (index, event) in events.iter().enumerate().skip(1) {
        validate_event(event, events.get(index - 1), index + 1)?;
        if event["runId"] != first["runId"] {
            return Err(format!(
                "invalid run ledger line {}: runId changed",
                index + 1
            ));
        }
        if event["action"] == "govern" && !governor_enabled {
            return Err(format!(
                "invalid run ledger line {}: governor action requires a v0.22 marker",
                index + 1
            ));
        }
        if let Some(request_id) = event.get("requestId").and_then(Value::as_str) {
            let scope = crate::evidence::hash::value(&json!({
                "runId": event["runId"],
                "stage": event["stage"],
                "attempt": event["attempt"],
                "agent": event["agent"],
                "role": event["role"],
                "assignmentSha256": event["assignmentSha256"],
                "requestId": request_id,
            }));
            if !request_scopes.insert(scope) {
                return Err(format!(
                    "invalid run ledger line {}: duplicate governor request identity",
                    index + 1
                ));
            }
        }
        if state["governor"]["grantProtocol"] == crate::context::GRANT_PROTOCOL_VERSION
            && event["action"] == "submit"
            && event["role"] == "worker"
            && event["outcome"] == "completed"
        {
            let governor_event = event.get("governorEvent").ok_or_else(|| {
                format!(
                    "invalid run ledger line {}: completion authorization is missing",
                    index + 1
                )
            })?;
            validate_grant_acknowledgement(&state, &events[..index], event, governor_event)?;
        }
        if let Some(governor_event) = event.get("governorEvent") {
            if !governor_enabled {
                return Err(format!(
                    "invalid run ledger line {}: governor event requires a v0.22 marker",
                    index + 1
                ));
            }
            if event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
                && (governor_event.get("authorizationMode").is_some()
                    || governor_event.get("grantEventSha256s").is_some())
            {
                validate_grant_acknowledgement(&state, &events[..index], event, governor_event)?;
            }
            recovery::validate_forward_repair(&state, &events[..index], event)?;
            recovery::append_governor(&mut state, governor_event.clone())?;
        }
        apply_event(&mut state, event)?;
        recovery::after_event(&mut state, event)?;
    }
    state["assignments"] = json!(crate::run::assignment::pending(&state));
    Ok(state)
}

fn reject_unknown(
    object: &Map<String, Value>,
    action: &str,
    version: u64,
    line: usize,
) -> Result<(), String> {
    let allowed: &[&str] = if action == "start" {
        let mut allowed = vec![
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "workflow",
            "goal",
            "configSha256",
            "plan",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
            "supersedes",
        ];
        if version == 2 {
            allowed.push("harnessReceipt");
        }
        if matches!(version, 3..=8) {
            allowed.push("harnessReceipt");
            allowed.push("checkPolicy");
        }
        if version >= 5 {
            allowed.push("subject");
        }
        if version >= 6 {
            allowed.push("preservation");
        }
        if version >= 6 {
            allowed.push("governor");
        }
        if version >= 8 {
            allowed.push("basisProtocol");
            allowed.push("basis");
            allowed.push("reviewPolicy");
            allowed.push("recoveryProtocol");
            allowed.push("checkObservationProtocol");
            allowed.push("carryProtocol");
            allowed.push("governorCarry");
            allowed.push("governorCarrySha256");
        }
        return object
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
            .map_or(Ok(()), |key| {
                Err(format!(
                    "invalid run ledger line {line}: unknown field '{key}'"
                ))
            });
    } else if matches!(
        action,
        "check_observation" | "check_observation_failed" | "check_observation_recovered"
    ) {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "observation",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "submit" {
        let mut allowed = vec![
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "stage",
            "attempt",
            "agent",
            "role",
            "outcome",
            "artifact",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ];
        if version >= 5 {
            allowed.push("subjectSha256");
        }
        if version >= 6 {
            allowed.push("inputsSha256");
        }
        if version >= 6 {
            allowed.push("assignmentSha256");
        }
        if version >= 7 {
            allowed.push("fallback");
        }
        if version >= 8 {
            allowed.push("basisSha256");
            allowed.push("reviewDecisionSha256");
            allowed.push("disposition");
        }
        if version >= 6 {
            allowed.push("governorEvent");
        }
        return reject_unknown_fields(object, &allowed, line);
    } else if action == "review_policy" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "basisProtocol",
                "basisSha256",
                "previousDecisionSha256",
                "reviewPolicy",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "govern" {
        return reject_unknown_fields(
            object,
            &[
                "version",
                "kind",
                "producer",
                "action",
                "runId",
                "stage",
                "attempt",
                "agent",
                "role",
                "subjectSha256",
                "inputsSha256",
                "assignmentSha256",
                "assignmentPacketSha256",
                "operation",
                "requestId",
                "requestDigest",
                "governorEvent",
                "previousEventSha256",
                "timestamp",
                "eventSha256",
            ],
            line,
        );
    } else if action == "check" {
        if version == 8 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "configSha256",
                    "targetEventSha256",
                    "requirementId",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "observationEventSha256",
                    "stdout",
                    "stderr",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version >= 6 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "targetEventSha256",
                    "requirementId",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 5 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "targetEventSha256",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 4 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "targetEventSha256",
                    "checkCommand",
                    "checkCommandSha256",
                    "origin",
                    "acquisition",
                    "result",
                    "durationMs",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        &[
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "targetEventSha256",
            "checkCommand",
            "checkCommandSha256",
            "origin",
            "exitCode",
            "durationMs",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ]
    } else {
        if version >= 5 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "subjectSha256",
                    "inputsSha256",
                    "stage",
                    "attempt",
                    "actor",
                    "role",
                    "attemptedOutcome",
                    "reason",
                    "checkEvidence",
                    "origin",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        if version == 4 {
            return reject_unknown_fields(
                object,
                &[
                    "version",
                    "kind",
                    "producer",
                    "action",
                    "runId",
                    "stage",
                    "attempt",
                    "actor",
                    "role",
                    "attemptedOutcome",
                    "reason",
                    "checkEvidence",
                    "origin",
                    "previousEventSha256",
                    "timestamp",
                    "eventSha256",
                ],
                line,
            );
        }
        &[
            "version",
            "kind",
            "producer",
            "action",
            "runId",
            "stage",
            "attempt",
            "actor",
            "role",
            "attemptedOutcome",
            "reason",
            "checkEvidence",
            "origin",
            "previousEventSha256",
            "timestamp",
            "eventSha256",
        ]
    };
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown field '{key}'"
            ))
        })
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    line: usize,
) -> Result<(), String> {
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map_or(Ok(()), |key| {
            Err(format!(
                "invalid run ledger line {line}: unknown field '{key}'"
            ))
        })
}
fn without(value: &Value, key: &str) -> Value {
    let mut copy = value.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove(key);
    }
    copy
}
fn relative(value: Option<&str>) -> bool {
    let Some(v) = value else { return false };
    if v.contains('\\') {
        return false;
    }
    !v.trim().is_empty()
        && !v.contains('\0')
        && !v.starts_with('/')
        && !v.contains(":/")
        && !v.split('/').any(|x| x.is_empty() || x == "." || x == "..")
}
fn is_sha(value: Option<&str>) -> bool {
    value.is_some_and(|x| {
        x.len() == SHA_LEN
            && x.bytes().all(|b| b.is_ascii_hexdigit())
            && x.bytes().all(|b| !b.is_ascii_uppercase())
    })
}
fn display_name(value: Option<&str>) -> bool {
    value.is_some_and(|x| {
        !x.is_empty() && x.len() <= 80 && x.trim() == x && !x.bytes().any(|b| b < 0x20 || b == 0x7f)
    })
}
fn native_name(value: Option<&str>) -> bool {
    value.is_some_and(|x| {
        !x.is_empty()
            && x.len() <= 64
            && x.bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_lowercase() || b.is_ascii_digit() || (b == b'_' && i > 0))
    })
}
fn is_timestamp(value: Option<&str>) -> bool {
    value.is_some_and(|x| x.contains('T') && timestamp_ms(Some(x)) != i64::MIN)
}
fn valid_subject(value: Option<&Value>, event_run_id: Option<&str>) -> bool {
    let Some(value) = value else { return false };
    let Some(object) = value.as_object() else {
        return false;
    };
    (object.len() == 10 || object.len() == 11)
        && value["version"] == 1
        && is_sha(value["runId"].as_str())
        && value["runId"].as_str() == event_run_id
        && is_sha(value["goalSha256"].as_str())
        && is_sha(value["planSha256"].as_str())
        && is_sha(value["configSha256"].as_str())
        && (value.get("basisSha256").is_none() || is_sha(value["basisSha256"].as_str()))
        && value["attempt"].as_u64().is_some()
        && (value["previousSubjectSha256"].is_null()
            || is_sha(value["previousSubjectSha256"].as_str()))
        && (value["workerArtifactSha256"].is_null()
            || is_sha(value["workerArtifactSha256"].as_str()))
        && (value["transitionSha256"].is_null() || is_sha(value["transitionSha256"].as_str()))
        && is_sha(value["sha256"].as_str())
        && crate::evidence::hash::value(&without(value, "sha256")) == value["sha256"]
}

pub(crate) fn subject_for_submission(state: &Value, assignment: &Value, artifact: &Value) -> Value {
    let previous = state["subject"]["sha256"].clone();
    let artifact_sha = artifact["sha256"].clone();
    let attempt = assignment["attempt"].as_u64().unwrap_or_default();
    let transition = crate::evidence::hash::value(&json!({
        "runId": state["runId"], "previousSubjectSha256": previous, "stage": assignment["stage"], "attempt": attempt,
        "agent": assignment["agent"], "artifactSha256": artifact_sha,
    }));
    let mut subject = state["subject"].clone();
    subject["attempt"] = json!(attempt);
    subject["previousSubjectSha256"] = previous;
    subject["workerArtifactSha256"] = artifact_sha;
    subject["transitionSha256"] = json!(transition);
    let sha = crate::evidence::hash::value(&without(&subject, "sha256"));
    subject["sha256"] = json!(sha);
    subject
}

fn stage_for_role(state: &Value, role: &str, last: bool) -> Result<u64, String> {
    let mut stages = state["plan"]["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|stage| {
            stage["agents"]
                .as_array()
                .is_some_and(|agents| agents.iter().any(|agent| agent["role"] == role))
        })
        .filter_map(|stage| stage["stage"].as_u64());
    let stage = if last {
        stages.next_back()
    } else {
        stages.next()
    };
    stage.ok_or_else(|| format!("run has no {role} stage"))
}

pub(super) fn lead_stage(state: &Value) -> Result<u64, String> {
    stage_for_role(state, "lead", true)
}
fn reviewer_stage(state: &Value) -> Result<u64, String> {
    stage_for_role(state, "reviewer", false)
}
pub(super) fn worker_stage(state: &Value) -> Result<u64, String> {
    stage_for_role(state, "worker", false)
}

pub(super) fn subject_for_basis_revision(
    state: &Value,
    basis_sha256: &Value,
    disposition_sha256: &str,
) -> Value {
    let mut subject = state["subject"].clone();
    let previous = subject["sha256"].clone();
    subject["basisSha256"] = basis_sha256.clone();
    subject["attempt"] = json!(state["attempt"].as_u64().unwrap_or_default() + 1);
    subject["previousSubjectSha256"] = previous.clone();
    subject["workerArtifactSha256"] = Value::Null;
    subject["transitionSha256"] = json!(crate::evidence::hash::value(&json!({
        "runId": state["runId"],
        "previousSubjectSha256": previous,
        "dispositionSha256": disposition_sha256,
        "basisSha256": basis_sha256,
    })));
    subject["sha256"] = json!(crate::evidence::hash::value(&without(&subject, "sha256")));
    subject
}
fn timestamp_ms(value: Option<&str>) -> i64 {
    value
        .and_then(|x| chrono::DateTime::parse_from_rfc3339(x).ok())
        .map(|x| x.timestamp_millis())
        .unwrap_or(i64::MIN)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_govern, apply_unavailable, subject_for_submission, valid_subject,
        validate_submission, without,
    };
    use serde_json::json;

    const SHA: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    /// The alternate execution binding: where a review runs, and nothing that
    /// could carry a second reviewer contract.
    fn binding() -> serde_json::Value {
        json!({"host": "claude", "model": "alternate-review", "reasoningEffort": "high"})
    }

    fn reviewer_state() -> serde_json::Value {
        json!({
            "status": "running",
            "currentStage": 2,
            "attempt": 1,
            "plan": {"version":1,"maxParallel":1,"stages":[
                {"stage":1,"agents":[{"role":"worker","name":"worker"}]},
                {"stage":2,"agents":[{"role":"reviewer","name":"reviewer","fallbackRuntime":binding()}]},
            ]},
            "submissions": []
        })
    }

    fn strict_replan_fixture() -> (serde_json::Value, serde_json::Value) {
        let state = json!({
            "status": "running",
            "currentStage": 1,
            "attempt": 1,
            "runId": "run",
            "subject": {"sha256": SHA},
            "inputsSha256": SHA,
            "events": [],
            "plan": {"version":1,"maxParallel":1,"stages":[
                {"stage":1,"agents":[{"role":"worker","name":"worker","displayName":"worker","nativeTaskName":"worker","purpose":"test","profile":"worker.md","profileSha256":SHA,"runtime":{"host":null,"model":null,"reasoningEffort":null},"declaredBoundary":{}}]}
            ]},
            "submissions": [],
            "governor": {"defaults": {"replanBinding":"assignment_packet_v1"}}
        });
        let assignment = crate::run::assignment::pending(&state)
            .into_iter()
            .next()
            .unwrap();
        let event = json!({
            "action":"govern", "governorEvent": {
                "action":"replan", "identityTransition":"carried_mutation_v1",
                "assignmentPacketSha256": crate::evidence::hash::value(&assignment)
            },
            "agent":"worker", "role":"worker", "stage":1, "attempt":1,
            "subjectSha256":SHA, "inputsSha256":SHA, "eventSha256":"event"
        });
        (state, event)
    }

    #[test]
    fn strict_replan_rejects_forged_input_packet_and_actor_bindings() {
        for label in ["input", "packet", "actor", "markers"] {
            let (mut state, mut event) = strict_replan_fixture();
            match label {
                "input" => event["inputsSha256"] = json!("0".repeat(64)),
                "packet" => {
                    event["governorEvent"]["assignmentPacketSha256"] = json!("0".repeat(64))
                }
                "actor" => {
                    event["agent"] = json!("reviewer");
                    event["role"] = json!("reviewer");
                }
                "markers" => {
                    event["governorEvent"]
                        .as_object_mut()
                        .unwrap()
                        .remove("identityTransition");
                    event["governorEvent"]
                        .as_object_mut()
                        .unwrap()
                        .remove("assignmentPacketSha256");
                }
                _ => unreachable!(),
            }
            assert!(
                apply_govern(&mut state, &event).is_err(),
                "{label} bypassed"
            );
        }
    }

    fn unavailable(agent: &str) -> serde_json::Value {
        json!({
            "stage":2,"attempt":1,"agent":agent,"role":"reviewer","outcome":"unavailable",
            "artifact":{"path":"a.md","sha256":SHA},"eventSha256":SHA,
            "fallback":{"reason":"provider_quota","runtime":binding(),"identitySource":"host-reported"},
        })
    }

    /// D and E, enforced in the reducer rather than the façade: once any
    /// reviewer verdict exists, an operational failure cannot re-open the
    /// review to seek a different answer.
    #[test]
    fn a_reviewer_verdict_refuses_a_later_unavailability() {
        for verdict in ["rework", "blocked"] {
            let mut state = reviewer_state();
            state["submissions"] = json!([{
                "stage":2,"attempt":1,"agent":"worker","role":"worker","outcome":"completed",
            }, {
                "stage":2,"attempt":1,"agent":"reviewer","role":"reviewer","outcome":verdict,
            }]);
            let primary = json!({"stage":2,"attempt":1,"agent":"reviewer","role":"reviewer","fallbackRuntime":binding()});
            let error = apply_unavailable(&mut state, &unavailable("reviewer"), &primary)
                .expect_err("a verdict must not be shopped away");
            assert!(
                error.contains("not admissible after a reviewer verdict"),
                "{error}"
            );
            assert_eq!(state["submissions"].as_array().unwrap().len(), 2);
        }
    }

    /// The primary's own operational failure is non-terminal and does not
    /// advance the stage, but a substitution counts as exactly one.
    #[test]
    fn primary_unavailability_is_non_terminal_and_one_substitution_is_bounded() {
        let mut state = reviewer_state();
        let primary = json!({"stage":2,"attempt":1,"agent":"reviewer","fallbackRuntime":binding()});
        apply_unavailable(&mut state, &unavailable("reviewer"), &primary).unwrap();
        assert_eq!(state["status"], "running");
        assert_eq!(state["currentStage"], 2);

        // The substitute's own failure is bounded: the run blocks rather than
        // reaching for a third target.
        let substitute = json!({"stage":2,"attempt":1,"agent":"reviewer","substitution":{"reason":"provider_quota"}});
        apply_unavailable(&mut state, &unavailable("reviewer"), &substitute).unwrap();
        assert_eq!(state["status"], "blocked");
    }

    /// Without any authorized target the run cannot be re-issued, so the
    /// operational failure blocks instead of advancing on no verdict.
    #[test]
    fn an_unauthorized_target_blocks_instead_of_advancing() {
        let mut state = json!({
            "status": "running",
            "currentStage": 2,
            "attempt": 1,
            "plan": {"version":1,"maxParallel":1,"stages":[
                {"stage":1,"agents":[{"role":"worker","name":"worker"}]},
                {"stage":2,"agents":[{"role":"reviewer","name":"reviewer"}]},
            ]},
            "submissions": []
        });
        let primary = json!({"stage":2,"attempt":1,"agent":"reviewer"});
        apply_unavailable(&mut state, &unavailable("reviewer"), &primary).unwrap();
        assert_eq!(state["status"], "blocked");
    }

    #[test]
    fn subject_chain_binds_prior_result_and_changes_for_each_worker_result() {
        let state = json!({"runId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","subject":{"version":1,"runId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","goalSha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","planSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","configSha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","attempt":0,"previousSubjectSha256":null,"workerArtifactSha256":null,"transitionSha256":null,"sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}});
        let assignment = json!({"stage":2,"attempt":1,"agent":"worker"});
        let first = subject_for_submission(
            &state,
            &assignment,
            &json!({"sha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"}),
        );
        let second_state = json!({"runId":state["runId"],"subject":first});
        let second = subject_for_submission(
            &second_state,
            &assignment,
            &json!({"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}),
        );
        assert_ne!(first["sha256"], second["sha256"]);
        assert_eq!(second["previousSubjectSha256"], first["sha256"]);
    }

    #[test]
    fn checked_start_subject_is_bound_to_the_event_run_id() {
        let run_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let other_run_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut subject = json!({
            "version": 1,
            "runId": run_id,
            "goalSha256": SHA,
            "planSha256": SHA,
            "configSha256": SHA,
            "attempt": 0,
            "previousSubjectSha256": null,
            "workerArtifactSha256": null,
            "transitionSha256": null
        });
        subject["sha256"] = json!(crate::evidence::hash::value(&subject));
        assert!(valid_subject(Some(&subject), Some(run_id)));
        assert!(!valid_subject(Some(&subject), Some(other_run_id)));
        assert_eq!(
            subject["sha256"],
            crate::evidence::hash::value(&without(&subject, "sha256"))
        );
    }

    fn submission_with_artifact(version: u64, artifact: serde_json::Value) -> serde_json::Value {
        json!({
            "version": version,
            "stage": 1,
            "attempt": 1,
            "agent": "worker",
            "role": "worker",
            "outcome": "completed",
            "artifact": artifact,
        })
    }

    #[test]
    fn v8_submissions_preserve_missing_bytes_and_validate_present_counts() {
        let base = json!({"root":"product", "path":"artifact.md", "sha256":SHA});
        assert!(validate_submission(&submission_with_artifact(7, base.clone()), 1).is_ok());
        assert!(validate_submission(&submission_with_artifact(8, base), 1).is_ok());
        assert!(validate_submission(
            &submission_with_artifact(
                8,
                json!({
                    "root":"product", "path":"artifact.md", "sha256":SHA, "bytes": 4
                })
            ),
            1
        )
        .is_ok());
        assert!(validate_submission(
            &submission_with_artifact(
                8,
                json!({
                    "root":"product", "path":"artifact.md", "sha256":SHA, "bytes": "4"
                })
            ),
            1
        )
        .is_err());
    }
}

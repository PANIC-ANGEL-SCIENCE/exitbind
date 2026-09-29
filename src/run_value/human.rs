//! Human-readable status and explanation derived from checked state.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanIdentity {
    pub(crate) stage: u64,
    pub(crate) name: String,
    pub(crate) display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanRecord {
    pub(crate) identity: HumanIdentity,
    pub(crate) role: String,
    pub(crate) attempt: u64,
    pub(crate) outcome: String,
    pub(crate) event_sha256: String,
    pub(crate) artifact_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanWorker {
    pub(crate) identity: HumanIdentity,
    pub(crate) attempt: u64,
    pub(crate) current: Option<HumanRecord>,
    pub(crate) history: Vec<HumanRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanReviewer {
    pub(crate) identity: HumanIdentity,
    pub(crate) current: Option<HumanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HumanLeadState {
    Pending,
    Accepted,
    Rejected,
    Blocked,
    TerminalWithoutLeadDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanLeadDecision {
    pub(crate) state: HumanLeadState,
    pub(crate) current: Vec<HumanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HumanCheckState {
    Unconfigured,
    NotObserved,
    Blocked,
    Passed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanCheckTarget {
    pub(crate) worker: Option<HumanIdentity>,
    pub(crate) target_event_sha256: Option<String>,
    pub(crate) status: String,
    pub(crate) check_event_sha256: Option<String>,
    pub(crate) exit_code: Option<u64>,
    pub(crate) acquisition: Option<String>,
    pub(crate) result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanChecks {
    pub(crate) state: HumanCheckState,
    pub(crate) observed_capable: bool,
    pub(crate) command: Option<String>,
    pub(crate) command_sha256: Option<String>,
    pub(crate) origin: Option<String>,
    pub(crate) targets: Vec<HumanCheckTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanProtection {
    pub(crate) attempt: u64,
    pub(crate) actor: String,
    pub(crate) attempted_outcome: String,
    pub(crate) reason: String,
    pub(crate) origin: String,
    pub(crate) event_sha256: String,
    pub(crate) current: bool,
    pub(crate) evidence: Vec<HumanCheckTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanStatus {
    pub(crate) run_id: String,
    pub(crate) status: String,
    pub(crate) stage: u64,
    pub(crate) attempt: u64,
    pub(crate) artifact_status: String,
    pub(crate) workers: Vec<HumanWorker>,
    pub(crate) checks: HumanChecks,
    pub(crate) reviewers: Vec<HumanReviewer>,
    pub(crate) lead: HumanLeadDecision,
    pub(crate) history: Vec<HumanRecord>,
    pub(crate) protections: Vec<HumanProtection>,
    pub(crate) guidance: Option<&'static str>,
    pub(crate) session_goal: Value,
    pub(crate) progress: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HumanExplanation {
    pub(crate) status: HumanStatus,
    pub(crate) protection: Option<HumanProtection>,
    pub(crate) guidance: &'static str,
}

fn required_str(value: &Value, key: &str, context: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("validated {context} is missing {key}"))
}

fn required_u64(value: &Value, key: &str, context: &str) -> Result<u64, String> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("validated {context} is missing {key}"))
}

fn planned_identities(state: &Value, role: &str) -> Result<Vec<HumanIdentity>, String> {
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("validated run state plan stages are invalid")?;
    stages
        .iter()
        .flat_map(|stage| {
            let stage_number = stage["stage"].as_u64();
            stage["agents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(move |agent| agent["role"] == role)
                .map(move |agent| (stage_number, agent))
        })
        .map(|(stage, agent)| {
            Ok(HumanIdentity {
                stage: stage.ok_or("validated run stage is missing stage")?,
                name: required_str(agent, "name", "planned agent")?,
                display_name: required_str(agent, "displayName", "planned agent")?,
            })
        })
        .collect()
}

fn identity_for_submission(state: &Value, submission: &Value) -> Result<HumanIdentity, String> {
    let stage = required_u64(submission, "stage", "submission")?;
    let agent = required_str(submission, "agent", "submission")?;
    let role = required_str(submission, "role", "submission")?;
    planned_identities(state, &role)?
        .into_iter()
        .find(|identity| identity.stage == stage && identity.name == agent)
        .ok_or_else(|| format!("validated submission has no planned identity for {role} '{agent}'"))
}

fn record_from_submission(state: &Value, submission: &Value) -> Result<HumanRecord, String> {
    Ok(HumanRecord {
        identity: identity_for_submission(state, submission)?,
        role: required_str(submission, "role", "submission")?,
        attempt: required_u64(submission, "attempt", "submission")?,
        outcome: required_str(submission, "outcome", "submission")?,
        event_sha256: required_str(submission, "eventSha256", "submission")?,
        artifact_sha256: required_str(&submission["artifact"], "sha256", "submission artifact")?,
    })
}

fn current_submission<'a>(
    submissions: &'a [Value],
    identity: &HumanIdentity,
    role: &str,
    attempt: u64,
) -> Option<&'a Value> {
    submissions.iter().rev().find(|submission| {
        submission["stage"] == identity.stage
            && submission["attempt"] == attempt
            && submission["agent"] == identity.name
            && submission["role"] == role
    })
}

fn history_for_identity(
    state: &Value,
    submissions: &[Value],
    identity: &HumanIdentity,
    role: &str,
    attempt: u64,
) -> Result<Vec<HumanRecord>, String> {
    submissions
        .iter()
        .filter(|submission| {
            submission["stage"] == identity.stage
                && submission["agent"] == identity.name
                && submission["role"] == role
                && submission["attempt"]
                    .as_u64()
                    .is_some_and(|value| value < attempt)
        })
        .map(|submission| record_from_submission(state, submission))
        .collect()
}

fn target_for_worker<'a>(
    worker: &HumanWorker,
    assessment: &'a CheckAssessment,
) -> Option<&'a CheckTarget> {
    let event = worker.current.as_ref()?.event_sha256.as_str();
    assessment
        .targets
        .iter()
        .find(|target| target.target_event_sha256 == event)
}

fn human_check_target(
    worker: Option<HumanIdentity>,
    target_event_sha256: Option<String>,
    status: &str,
    check_event_sha256: Option<String>,
    exit_code: Option<u64>,
    acquisition: Option<String>,
    result: Option<Value>,
) -> HumanCheckTarget {
    HumanCheckTarget {
        worker,
        target_event_sha256,
        status: status.to_owned(),
        check_event_sha256,
        exit_code,
        acquisition,
        result: result.map(|value| value.to_string()),
    }
}

fn human_checks(
    workers: &[HumanWorker],
    assessment: &CheckAssessment,
    version: u64,
) -> HumanChecks {
    let Some(policy) = assessment.policy.as_ref() else {
        return HumanChecks {
            state: HumanCheckState::Unconfigured,
            observed_capable: false,
            command: None,
            command_sha256: None,
            origin: None,
            targets: Vec::new(),
        };
    };

    let mut targets = Vec::new();
    for worker in workers {
        let current_event = worker
            .current
            .as_ref()
            .map(|record| record.event_sha256.clone());
        if let Some(target) = target_for_worker(worker, assessment) {
            targets.push(human_check_target(
                Some(worker.identity.clone()),
                Some(target.target_event_sha256.clone()),
                target.status.as_str(),
                target.check_event_sha256.clone(),
                target.exit_code,
                target.acquisition.clone(),
                target.result.clone(),
            ));
        } else {
            targets.push(human_check_target(
                Some(worker.identity.clone()),
                current_event,
                "missing",
                None,
                None,
                None,
                None,
            ));
        }
    }
    for target in &assessment.targets {
        if !targets.iter().any(|item| {
            item.target_event_sha256.as_deref() == Some(target.target_event_sha256.as_str())
        }) {
            targets.push(human_check_target(
                None,
                Some(target.target_event_sha256.clone()),
                target.status.as_str(),
                target.check_event_sha256.clone(),
                target.exit_code,
                target.acquisition.clone(),
                target.result.clone(),
            ));
        }
    }

    let state = if targets.iter().any(|target| target.status == "failed") {
        HumanCheckState::Blocked
    } else if assessment.incomplete
        || targets.is_empty()
        || targets.iter().any(|target| target.status == "missing")
    {
        HumanCheckState::NotObserved
    } else {
        HumanCheckState::Passed
    };
    if !matches!(version, 4..=8) {
        for target in &mut targets {
            target.acquisition = Some("reported".to_owned());
        }
    }
    HumanChecks {
        state,
        observed_capable: matches!(version, 4..=8),
        command: Some(policy.command.clone()),
        command_sha256: Some(policy.command_sha256.clone()),
        origin: Some(policy.origin.as_str().to_owned()),
        targets,
    }
}

fn check_guidance(state: &Value) -> &'static str {
    if matches!(state["version"].as_u64(), Some(4..=8)) {
        "observe the frozen check locally with run observe-check, or report the actual result from the host with run record-check, for every current worker target"
    } else {
        "run the configured check in its host and report the actual result for every current worker target with run record-check"
    }
}

fn protection_from_value(
    event: &Value,
    state: &Value,
    workers: &[HumanWorker],
    current_attempt: u64,
) -> Result<HumanProtection, String> {
    let evidence = event["checkEvidence"]
        .as_array()
        .ok_or("validated protection evidence is invalid")?
        .iter()
        .map(|item| {
            let target = required_str(item, "targetEventSha256", "protection evidence")?;
            let worker = workers
                .iter()
                .find(|candidate| {
                    candidate
                        .current
                        .as_ref()
                        .is_some_and(|record| record.event_sha256 == target)
                        || candidate
                            .history
                            .iter()
                            .any(|record| record.event_sha256 == target)
                })
                .map(|candidate| candidate.identity.clone());
            let check_event_sha256 = item["checkEventSha256"].as_str().map(str::to_owned);
            let acquisition = check_event_sha256
                .as_deref()
                .and_then(|wanted| {
                    state["checks"].as_array()?.iter().find_map(|check| {
                        (check["eventSha256"].as_str() == Some(wanted)).then(|| {
                            check["acquisition"]
                                .as_str()
                                .unwrap_or("reported")
                                .to_owned()
                        })
                    })
                })
                .or_else(|| (state["version"] == 3).then(|| "reported".to_owned()));
            Ok(human_check_target(
                worker,
                Some(target),
                &required_str(item, "status", "protection evidence")?,
                check_event_sha256,
                item["exitCode"].as_u64(),
                acquisition,
                item["result"].as_object().map(|_| item["result"].clone()),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(HumanProtection {
        attempt: required_u64(event, "attempt", "protection")?,
        actor: required_str(event, "actor", "protection")?,
        attempted_outcome: required_str(event, "attemptedOutcome", "protection")?,
        reason: required_str(event, "reason", "protection")?,
        origin: required_str(event, "origin", "protection")?,
        event_sha256: required_str(event, "eventSha256", "protection")?,
        current: event["attempt"].as_u64() == Some(current_attempt),
        evidence,
    })
}

pub(crate) fn human_status_from_kernel(
    state: &Value,
    artifact_current: bool,
    kernel: &crate::run_exit::ExitState,
) -> Result<HumanStatus, String> {
    let current_attempt = kernel.attempt;
    let submissions = state["submissions"]
        .as_array()
        .ok_or("validated run state submissions are invalid")?;
    let worker_identities = planned_identities(state, "worker")?;
    let workers = worker_identities
        .into_iter()
        .map(|identity| {
            let current = current_submission(submissions, &identity, "worker", current_attempt)
                .map(|submission| record_from_submission(state, submission))
                .transpose()?;
            Ok(HumanWorker {
                history: history_for_identity(
                    state,
                    submissions,
                    &identity,
                    "worker",
                    current_attempt,
                )?,
                identity,
                attempt: current_attempt,
                current,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let reviewer_identities = planned_identities(state, "reviewer")?;
    let reviewers = reviewer_identities
        .into_iter()
        .map(|identity| {
            let current = current_submission(submissions, &identity, "reviewer", current_attempt)
                .map(|submission| record_from_submission(state, submission))
                .transpose()?
                .filter(|record| {
                    record.outcome != "approved"
                        || (artifact_current
                            && kernel.current_reviews.contains(&record.event_sha256))
                });
            Ok(HumanReviewer { identity, current })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let current_lead = submissions
        .iter()
        .filter(|submission| {
            submission["attempt"] == current_attempt && submission["role"] == "lead"
        })
        .map(|submission| record_from_submission(state, submission))
        .collect::<Result<Vec<_>, String>>()?;
    let lead_state = current_lead
        .iter()
        .rev()
        .find_map(|record| match record.outcome.as_str() {
            "accepted" => Some(HumanLeadState::Accepted),
            "rejected" => Some(HumanLeadState::Rejected),
            "blocked" => Some(HumanLeadState::Blocked),
            _ => None,
        })
        .unwrap_or_else(|| {
            if matches!(
                state["status"].as_str(),
                Some("accepted" | "rejected" | "blocked")
            ) {
                HumanLeadState::TerminalWithoutLeadDecision
            } else {
                HumanLeadState::Pending
            }
        });
    let history = submissions
        .iter()
        .filter(|submission| {
            submission["attempt"]
                .as_u64()
                .is_some_and(|value| value < current_attempt)
        })
        .map(|submission| record_from_submission(state, submission))
        .collect::<Result<Vec<_>, String>>()?;
    let assessment = kernel.assessment.clone();
    let checks = human_checks(
        &workers,
        &assessment,
        state["version"].as_u64().unwrap_or(3),
    );
    let empty_protections = Vec::new();
    let protections = state["protections"]
        .as_array()
        .unwrap_or(&empty_protections)
        .iter()
        .map(|event| protection_from_value(event, state, &workers, current_attempt))
        .collect::<Result<Vec<_>, String>>()?;
    let guidance = if state["status"] != "running" {
        Some("this run is terminal; inspect its recorded outcome before choosing an explicit successor where supported")
    } else if !artifact_current {
        Some(
            "artifact drift prevents progression; restore the exact recorded bytes before retrying",
        )
    } else {
        match checks.state {
            HumanCheckState::NotObserved | HumanCheckState::Blocked => Some(check_guidance(state)),
            HumanCheckState::Passed => Some(
                "checks passed after recording the actual result; reviewer approval and lead acceptance remain separate authority steps",
            ),
            HumanCheckState::Unconfigured => None,
        }
    };
    let progress = crate::run_progress::project(state);
    let run_ready = matches!(lead_state, HumanLeadState::Accepted)
        && crate::run_exit::is_ready(progress["state"].as_str().unwrap_or(""))
        && artifact_current;
    let session_goal = json!({
        "requestId": state["runId"],
        // Run acceptance is a separate fact. The human renderer may set the
        // explicit Lead session closure only when its caller supplies that
        // declaration; it is never inferred from READY here.
        "explicitLeadClosure": false,
        "leadAccepted": run_ready,
        "subgoals": if run_ready { json!([]) } else { json!([state["goal"].clone()]) },
        "findings": [],
        "blockers": if run_ready { json!([]) } else { json!([progress["reason"]["code"].clone()]) },
        "decisions": if run_ready { json!([]) } else { json!(["lead_decision"]) },
        "externalActions": [],
    });
    Ok(HumanStatus {
        run_id: required_str(state, "runId", "run state")?,
        status: required_str(state, "status", "run state")?,
        stage: required_u64(state, "currentStage", "run state")?,
        attempt: current_attempt,
        artifact_status: if artifact_current {
            "current"
        } else {
            "drifted"
        }
        .to_owned(),
        workers,
        checks,
        reviewers,
        lead: HumanLeadDecision {
            state: lead_state,
            current: current_lead,
        },
        history,
        protections,
        guidance,
        session_goal,
        progress,
    })
}

/// Project status from ledger facts and optional artifact revalidation.
pub(crate) fn status(state: &Value, artifact_current: Option<bool>) -> Result<Value, String> {
    let assessment = crate::run_exit::reduce(state)?.assessment;
    let submissions = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?;
    let current_attempt = state["attempt"].as_u64().unwrap_or_default();
    let claim = submissions.iter().rev().find(|submission| {
        submission["attempt"] == current_attempt && submission["role"] == "worker"
    });
    let review = submissions.iter().rev().find(|submission| {
        submission["attempt"] == current_attempt && submission["role"] == "reviewer"
    });
    let acceptance = submissions.iter().rev().find(|submission| {
        submission["attempt"] == current_attempt && submission["outcome"] == "accepted"
    });
    let mut checks = json!({
        "configured": assessment.policy.is_some(),
        "origin": assessment
            .policy
            .as_ref()
            .map_or("none", |policy| policy.origin.as_str()),
        "status": if assessment.policy.is_none() {
            "not_configured"
        } else if assessment.incomplete
            || assessment
                .targets
                .iter()
            .any(CheckTarget::is_missing)
            || assessment.targets.is_empty()
        {
            "not_observed"
        } else if assessment.is_blocked() {
            "blocked"
        } else {
            "passed"
        },
        "targets": assessment.targets.iter().map(CheckTarget::value).collect::<Vec<_>>(),
    });
    if let Some(object) = checks.as_object_mut() {
        object.insert("targetCount".into(), json!(assessment.targets.len()));
        object.insert(
            "observedCount".into(),
            json!(assessment
                .targets
                .iter()
                .filter(|target| target.is_observed())
                .count()),
        );
        object.insert(
            "failedCount".into(),
            json!(assessment
                .targets
                .iter()
                .filter(|target| target.is_failed())
                .count()),
        );
        object.insert(
            "missingCount".into(),
            json!(assessment
                .targets
                .iter()
                .filter(|target| target.is_missing())
                .count()),
        );
    }
    let review_value = crate::evidence::review_report::status(state, review, artifact_current);
    Ok(json!({
        "version": VALUE_REPORT_VERSION,
        "runId": state["runId"],
        "workflow": state["workflow"],
        "status": state["status"],
        "stage": state["currentStage"],
        "attempt": state["attempt"],
        "claim": claim.map_or_else(|| json!({"status":"absent"}), |submission| json!({
            "status": submission["outcome"],
            "agent": submission["agent"],
            "eventSha256": submission["eventSha256"],
            "artifactSha256": submission["artifact"]["sha256"],
        })),
        "artifact": artifact_current.map_or_else(|| json!({"status":"not_revalidated"}), |current| json!({"status": if current {"current"} else {"drifted"}})),
        "checks": checks,
        "review": review_value,
        "acceptance": acceptance.map_or_else(|| json!({"status":"absent"}), |submission| json!({
            "status": "accepted",
            "eventSha256": submission["eventSha256"],
            "artifactSha256": submission["artifact"]["sha256"],
        })),
        "evidence": {
            "eventCount": state["events"].as_array().map_or(0, Vec::len),
            "submissionCount": submissions.len(),
            "checkCount": state["checks"].as_array().map_or(0, Vec::len),
            "protectionCount": state["protections"].as_array().map_or(0, Vec::len),
        },
    }))
}

/// Explain a run or selected protection event.
pub(crate) fn explain_with_artifact(
    state: &Value,
    event_id: Option<&str>,
    artifact_current: bool,
) -> Result<Value, String> {
    let status = status(state, Some(artifact_current))?;
    let empty_protections = Vec::new();
    let protections = state["protections"]
        .as_array()
        .unwrap_or(&empty_protections);
    let selected = event_id.and_then(|wanted| {
        protections
            .iter()
            .find(|event| event["eventSha256"].as_str() == Some(wanted))
    });
    if event_id.is_some() && selected.is_none() {
        return Err("explanation target is not a protection event in this run".into());
    }
    Ok(json!({
        "version": VALUE_REPORT_VERSION,
        "runId": state["runId"],
        "status": status,
        "protection": selected.map_or(Value::Null, Clone::clone),
        "guidance": if selected.is_some() {
            "rerun the configured check in its host and report the actual result for every current worker completion; repair or rework if needed, then request review and acceptance again; a passing report still requires authority"
        } else {
            "inspect the exact evidence references before choosing repair, rework, or acceptance"
        },
    }))
}

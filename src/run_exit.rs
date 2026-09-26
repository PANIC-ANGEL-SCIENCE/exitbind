//! Typed exit-state kernel.
//!
//! This module is the only owner of current-attempt evidence, subject
//! freshness, acceptance gating, and the canonical wire outcome.  Consumers
//! may project these facts, but must not classify the exit independently.

use crate::run_value::{self, CheckPolicy, PreservationRequirement};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckTargetStatus {
    Missing,
    Passed,
    Failed,
}

impl CheckTargetStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckTarget {
    pub(crate) target_event_sha256: String,
    pub(crate) requirement_id: Option<String>,
    pub(crate) requirement_text: Option<String>,
    pub(crate) status: CheckTargetStatus,
    pub(crate) check_event_sha256: Option<String>,
    pub(crate) exit_code: Option<u64>,
    pub(crate) result: Option<Value>,
    pub(crate) acquisition: Option<String>,
}

impl CheckTarget {
    pub(crate) fn value(&self) -> Value {
        let mut value = json!({
            "targetEventSha256": self.target_event_sha256,
            "status": self.status.as_str(),
        });
        if let Some(id) = &self.requirement_id {
            value["kind"] = json!("preservation");
            value["requirementId"] = json!(id);
        } else {
            value["kind"] = json!("check");
        }
        if let Some(text) = &self.requirement_text {
            value["requirementText"] = json!(text);
        }
        if let Some(check_event_sha256) = &self.check_event_sha256 {
            value["checkEventSha256"] = json!(check_event_sha256);
        }
        if let Some(exit_code) = self.exit_code {
            value["exitCode"] = json!(exit_code);
        }
        if let Some(result) = &self.result {
            value["result"] = result.clone();
        }
        if let Some(acquisition) = &self.acquisition {
            value["acquisition"] = json!(acquisition);
        }
        value
    }

    pub(crate) fn protection_value(&self) -> Value {
        let mut value = json!({
            "targetEventSha256": self.target_event_sha256,
            "status": self.status.as_str(),
        });
        if let Some(id) = &self.requirement_id {
            value["requirementId"] = json!(id);
        }
        if let Some(check_event_sha256) = &self.check_event_sha256 {
            value["checkEventSha256"] = json!(check_event_sha256);
        }
        if let Some(result) = &self.result {
            value["result"] = result.clone();
        } else if let Some(exit_code) = self.exit_code {
            value["exitCode"] = json!(exit_code);
        }
        value
    }

    pub(crate) fn is_missing(&self) -> bool {
        self.status == CheckTargetStatus::Missing
    }

    pub(crate) fn is_failed(&self) -> bool {
        self.status == CheckTargetStatus::Failed
    }

    pub(crate) fn is_preservation(&self) -> bool {
        self.requirement_id.is_some()
    }

    pub(crate) fn is_observed(&self) -> bool {
        self.check_event_sha256.is_some()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CheckAssessment {
    pub(crate) policy: Option<CheckPolicy>,
    pub(crate) targets: Vec<CheckTarget>,
    pub(crate) incomplete: bool,
    /// Required check targets per completed worker: the functional check plus
    /// one per accepted preservation requirement.
    pub(crate) targets_per_worker: usize,
}

impl CheckAssessment {
    fn unconfigured() -> Self {
        Self {
            policy: None,
            targets: Vec::new(),
            incomplete: false,
            targets_per_worker: 0,
        }
    }

    fn configured(
        policy: CheckPolicy,
        targets: Vec<CheckTarget>,
        incomplete: bool,
        targets_per_worker: usize,
    ) -> Self {
        Self {
            policy: Some(policy),
            targets,
            incomplete,
            targets_per_worker,
        }
    }

    pub(crate) fn reason(&self) -> Option<&'static str> {
        if self
            .targets
            .iter()
            .any(|target| target.is_preservation() && target.is_missing())
        {
            Some("preservation_missing")
        } else if self
            .targets
            .iter()
            .any(|target| target.is_preservation() && target.is_failed())
        {
            Some("preservation_failed")
        } else if self.targets.iter().any(CheckTarget::is_missing) {
            Some("check_missing")
        } else if self.targets.iter().any(CheckTarget::is_failed) {
            Some("check_failed")
        } else {
            None
        }
    }

    pub(crate) fn is_blocked(&self) -> bool {
        self.incomplete || self.reason().is_some()
    }

    pub(crate) fn has_refusal_evidence(&self) -> bool {
        !self.incomplete && self.reason().is_some()
    }

    pub(crate) fn targets_for_receipt(&self) -> Vec<Value> {
        self.targets.iter().map(CheckTarget::value).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitDecision {
    Ready,
    Refused(&'static str),
    Blocked(&'static str),
    InProgress,
    NotApplicable(&'static str),
}

/// True for the one terminal exit state, so no other module has to spell the
/// wire label. `ExitDecision::wire` remains its single owner.
pub(crate) fn is_ready(state: &str) -> bool {
    state == ExitDecision::Ready.wire().0
}

pub(crate) fn receipt_decision(valid: bool) -> ExitDecision {
    if valid {
        ExitDecision::Ready
    } else {
        ExitDecision::Refused("receipt_mismatch")
    }
}

impl ExitDecision {
    pub(crate) fn wire(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Ready => (
                "READY",
                "accepted",
                "current checked evidence, review, and lead acceptance are bound",
            ),
            Self::Refused("check_failed") => (
                "REFUSED",
                "check_failed",
                "the frozen check reported failure",
            ),
            Self::Refused("preservation_failed") => (
                "REFUSED",
                "preservation_failed",
                "a preservation check reported failure",
            ),
            Self::Refused("lead_rejected") => (
                "REFUSED",
                "lead_rejected",
                "the lead rejected the current transition",
            ),
            Self::Refused(code) => ("REFUSED", code, "the current transition was refused"),
            Self::Blocked("check_missing") => (
                "BLOCKED",
                "check_missing",
                "a required check or transition is missing or unresolved",
            ),
            Self::Blocked("preservation_missing") => (
                "BLOCKED",
                "preservation_missing",
                "a required preservation check is missing or unresolved",
            ),
            Self::Blocked(code) => ("BLOCKED", code, "a required transition is unresolved"),
            Self::InProgress => (
                "IN_PROGRESS",
                "prerequisites_incomplete",
                "current Exit Path prerequisites are incomplete",
            ),
            Self::NotApplicable("unchecked_run") => (
                "NOT_APPLICABLE",
                "unchecked_run",
                "the v5 checked Exit Contract is not configured for this run",
            ),
            Self::NotApplicable("historical_run") => (
                "NOT_APPLICABLE",
                "historical_run",
                "historical run acceptance remains readable without current Exit Contract progress",
            ),
            Self::NotApplicable(code) => {
                ("NOT_APPLICABLE", code, "current progress is unavailable")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExitState {
    pub(crate) version: u64,
    pub(crate) status: String,
    pub(crate) attempt: u64,
    #[allow(dead_code)]
    pub(crate) subject_sha256: Option<String>,
    pub(crate) inputs_sha256: Option<String>,
    pub(crate) assessment: CheckAssessment,
    pub(crate) current_submissions: Vec<Value>,
    pub(crate) worker_total: usize,
    pub(crate) reviewer_total: usize,
    pub(crate) worker_completed: usize,
    pub(crate) reviewer_completed: usize,
    pub(crate) review_required: bool,
    pub(crate) review_decision_sha256: Option<String>,
    /// A reviewer finding cycle the Lead resolved by `defer` or `reject`.
    pub(crate) review_resolution: Option<Value>,
    pub(crate) scope_completed: bool,
}

impl ExitState {
    pub(crate) fn reduce(state: &Value) -> Result<Self, String> {
        let version = state["version"]
            .as_u64()
            .ok_or("run state version is invalid")?;
        let status = state["status"]
            .as_str()
            .ok_or("run state status is invalid")?;
        if state.get("checkPolicy").is_none() {
            return Ok(Self {
                version,
                status: status.to_owned(),
                attempt: state["attempt"].as_u64().unwrap_or_default(),
                subject_sha256: state["subject"]["sha256"].as_str().map(str::to_owned),
                inputs_sha256: state["inputsSha256"].as_str().map(str::to_owned),
                assessment: CheckAssessment::unconfigured(),
                current_submissions: Vec::new(),
                worker_completed: 0,
                reviewer_completed: 0,
                scope_completed: false,
                worker_total: 0,
                reviewer_total: 0,
                review_required: true,
                review_decision_sha256: None,
                review_resolution: None,
            });
        }
        let attempt = state["attempt"]
            .as_u64()
            .ok_or("run state attempt is invalid")?;
        let all_submissions = state["submissions"]
            .as_array()
            .ok_or("run state submissions are invalid")?
            .clone();
        let current_submissions = all_submissions
            .iter()
            .filter(|event| event["attempt"] == attempt)
            .cloned()
            .collect::<Vec<_>>();
        let (worker_total, planned_reviewers) = planned_counts(state)?;
        let review_decision_sha256 = state["reviewPolicy"]["sha256"].as_str().map(str::to_owned);
        let review_required = state["reviewPolicy"]["decision"] != "omitted";
        let reviewer_total = if review_required {
            planned_reviewers
        } else {
            0
        };
        let assessment = assess(state)?;
        Ok(Self {
            version,
            status: status.to_owned(),
            attempt,
            subject_sha256: state["subject"]["sha256"].as_str().map(str::to_owned),
            inputs_sha256: state["inputsSha256"].as_str().map(str::to_owned),
            worker_completed: current_submissions
                .iter()
                .filter(|event| event["role"] == "worker" && event["outcome"] == "completed")
                .count(),
            reviewer_completed: current_submissions
                .iter()
                .filter(|event| approval_is_current(event, version, state["inputsSha256"].as_str()))
                .count(),
            scope_completed: all_submissions
                .iter()
                .any(|event| event["role"] == "lead" && event["outcome"] == "scoped"),
            worker_total,
            reviewer_total,
            review_required,
            review_decision_sha256,
            review_resolution: crate::run::disposition::current_resolution(state),
            assessment,
            current_submissions,
        })
    }

    pub(crate) fn decision(&self) -> ExitDecision {
        if self.version < 5 || self.assessment.policy.is_none() {
            return ExitDecision::NotApplicable(if self.version >= 5 {
                "unchecked_run"
            } else {
                "historical_run"
            });
        }
        if self.status == "accepted" {
            ExitDecision::Ready
        } else if matches!(
            self.assessment.reason(),
            Some("check_failed" | "preservation_failed")
        ) {
            ExitDecision::Refused(self.assessment.reason().unwrap_or("check_failed"))
        } else if self.status == "rejected" {
            ExitDecision::Refused("lead_rejected")
        } else if matches!(
            self.assessment.reason(),
            Some("check_missing" | "preservation_missing")
        ) || self.status == "blocked"
        {
            ExitDecision::Blocked(self.assessment.reason().unwrap_or("check_missing"))
        } else {
            ExitDecision::InProgress
        }
    }

    pub(crate) fn reviewer_approved(&self) -> bool {
        self.current_submissions
            .iter()
            .any(|event| approval_is_current(event, self.version, self.inputs_sha256.as_deref()))
    }

    pub(crate) fn lead_accepted(&self) -> bool {
        self.current_submissions
            .iter()
            .any(|event| event["role"] == "lead" && event["outcome"] == "accepted")
    }

    pub(crate) fn subject_is_current(&self, subject: &Value) -> bool {
        self.version < 5 || subject.as_str() == self.subject_sha256.as_deref()
    }

    pub(crate) fn acceptance_gate(&self) -> Result<(), String> {
        if self.version >= 5
            && self.assessment.policy.is_some()
            && self.review_required
            && !self.reviewer_approved()
            && self.review_resolution.is_none()
        {
            return Err("canonical acceptance requires reviewer approval".into());
        }
        if self.assessment.is_blocked() {
            let detail = self.assessment.reason().map_or(
                "worker completion or check result is not observed",
                |reason| reason,
            );
            return Err(format!(
                "canonical acceptance requires passing checks ({detail})"
            ));
        }
        Ok(())
    }
}

/// A reviewer approval counts only while it belongs to the result and, for v6
/// runs, to the tested inputs in force now.
fn approval_is_current(event: &Value, version: u64, inputs: Option<&str>) -> bool {
    event["role"] == "reviewer"
        && event["outcome"] == "approved"
        && (version < 6 || event["inputsSha256"].as_str() == inputs)
}

pub(crate) fn reduce(state: &Value) -> Result<ExitState, String> {
    ExitState::reduce(state)
}

pub(crate) fn assess(state: &Value) -> Result<CheckAssessment, String> {
    let Some(policy_value) = state.get("checkPolicy") else {
        return Ok(CheckAssessment::unconfigured());
    };
    let policy = run_value::policy_from_value(policy_value, 0)
        .map_err(|error| error.replacen("line 0", "state", 1))?;
    let preservation = state
        .get("preservation")
        .map(|value| run_value::preservation_from_value(value, 0))
        .transpose()
        .map_err(|error| error.replacen("line 0", "state", 1))?;
    let expected_workers = state["plan"]["stages"]
        .as_array()
        .ok_or("run state plan stages are invalid")?
        .iter()
        .map(|stage| {
            stage["agents"].as_array().map_or(0, |agents| {
                agents
                    .iter()
                    .filter(|agent| agent["role"] == "worker")
                    .count()
            })
        })
        .sum::<usize>();
    let attempt = state["attempt"]
        .as_u64()
        .ok_or("run state attempt is invalid")?;
    let submissions = state["submissions"]
        .as_array()
        .ok_or("run state submissions are invalid")?;
    let empty_checks = Vec::new();
    let checks = state["checks"].as_array().unwrap_or(&empty_checks);
    let current_subject = state["subject"]["sha256"].as_str();
    let current_inputs = state["inputsSha256"].as_str();
    let workers = submissions
        .iter()
        .filter(|submission| {
            submission["attempt"] == attempt
                && submission["role"] == "worker"
                && submission["outcome"] == "completed"
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut targets = Vec::new();
    for submission in &workers {
        targets.push(target_for(
            submission,
            checks,
            current_subject,
            current_inputs,
            state["version"].as_u64().unwrap_or_default(),
            None,
        )?);
        if let Some(preservation) = &preservation {
            for requirement in &preservation.requirements {
                targets.push(target_for(
                    submission,
                    checks,
                    current_subject,
                    current_inputs,
                    state["version"].as_u64().unwrap_or_default(),
                    Some(requirement),
                )?);
            }
        }
    }
    let incomplete = expected_workers == 0 || workers.len() < expected_workers;
    let targets_per_worker = 1 + preservation.map_or(0, |items| items.requirements.len());
    Ok(CheckAssessment::configured(
        policy,
        targets,
        incomplete,
        targets_per_worker,
    ))
}

fn target_for(
    submission: &Value,
    checks: &[Value],
    current_subject: Option<&str>,
    current_inputs: Option<&str>,
    version: u64,
    requirement: Option<&PreservationRequirement>,
) -> Result<CheckTarget, String> {
    let target = submission["eventSha256"].clone();
    let latest = checks.iter().rfind(|check| {
        check["targetEventSha256"] == target
            && (version < 5 || check["subjectSha256"].as_str() == current_subject)
            && (version < 6 || check["inputsSha256"].as_str() == current_inputs)
            && match requirement {
                Some(requirement) => check["requirementId"].as_str() == Some(&requirement.id),
                None => check.get("requirementId").is_none(),
            }
    });
    let target_event_sha256 = target
        .as_str()
        .map(str::to_owned)
        .ok_or("worker submission event hash is invalid")?;
    let requirement_id = requirement.map(|item| item.id.clone());
    let requirement_text = requirement.map(|item| item.text.clone());
    Ok(match latest {
        None => CheckTarget {
            target_event_sha256,
            requirement_id,
            requirement_text,
            status: CheckTargetStatus::Missing,
            check_event_sha256: None,
            exit_code: None,
            result: None,
            acquisition: None,
        },
        Some(check) if run_value::check_passed(check) => CheckTarget {
            target_event_sha256,
            requirement_id,
            requirement_text,
            status: CheckTargetStatus::Passed,
            check_event_sha256: check["eventSha256"].as_str().map(str::to_owned),
            exit_code: run_value::check_exit_code(check),
            result: run_value::check_result(check),
            acquisition: run_value::check_acquisition(check),
        },
        Some(check) => CheckTarget {
            target_event_sha256,
            requirement_id,
            requirement_text,
            status: CheckTargetStatus::Failed,
            check_event_sha256: check["eventSha256"].as_str().map(str::to_owned),
            exit_code: run_value::check_exit_code(check),
            result: run_value::check_result(check),
            acquisition: run_value::check_acquisition(check),
        },
    })
}

fn planned_counts(state: &Value) -> Result<(usize, usize), String> {
    let stages = state["plan"]["stages"]
        .as_array()
        .ok_or("run state plan stages are invalid")?;
    Ok((
        stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
            .filter(|agent| agent["role"] == "worker")
            .count(),
        stages
            .iter()
            .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
            .filter(|agent| agent["role"] == "reviewer")
            .count(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{reduce, ExitDecision};
    use serde_json::json;

    const SUBJECT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const WORKER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const CHECK: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

    fn checked(
        status: &str,
        submissions: serde_json::Value,
        checks: serde_json::Value,
    ) -> serde_json::Value {
        json!({
            "version": 5,
            "status": status,
            "attempt": 1,
            "subject": {"sha256": SUBJECT},
            "checkPolicy": {"version":1,"command":"true","commandSha256":crate::evidence::hash::text("true"),"origin":"local_report"},
            "plan": {"stages": [{"agents": [{"role":"worker"},{"role":"reviewer"}]}]},
            "submissions": submissions,
            "checks": checks,
        })
    }

    #[test]
    fn decision_preserves_not_applicable_and_current_check_boundaries() {
        let base = json!({
            "version": 5,
            "status": "running",
            "attempt": 2,
            "subject": {"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
            "plan": {"stages": [{"agents": [{"role": "worker"}]}]},
            "submissions": [], "checks": []
        });
        assert!(matches!(
            reduce(&base).unwrap().decision(),
            ExitDecision::NotApplicable("unchecked_run")
        ));
        let mut checked = base;
        checked["checkPolicy"] = json!({"version":1,"command":"true","commandSha256":crate::evidence::hash::text("true"),"origin":"local_report"});
        assert!(matches!(
            reduce(&checked).unwrap().decision(),
            ExitDecision::InProgress
        ));
        checked["status"] = json!("blocked");
        assert!(matches!(
            reduce(&checked).unwrap().decision(),
            ExitDecision::Blocked("check_missing")
        ));
    }

    #[test]
    fn every_checked_decision_has_a_paired_control() {
        let worker =
            json!({"attempt":1,"role":"worker","outcome":"completed","eventSha256":WORKER});
        let reviewer = json!({"attempt":1,"role":"reviewer","outcome":"approved"});
        let lead = json!({"attempt":1,"role":"lead","outcome":"accepted"});
        let passed = json!([{"targetEventSha256":WORKER,"subjectSha256":SUBJECT,"eventSha256":CHECK,"exitCode":0}]);
        let failed = json!([{"targetEventSha256":WORKER,"subjectSha256":SUBJECT,"eventSha256":CHECK,"exitCode":1}]);
        assert!(matches!(
            reduce(&checked(
                "accepted",
                json!([worker.clone(), reviewer, lead]),
                passed.clone()
            ))
            .unwrap()
            .decision(),
            ExitDecision::Ready
        ));
        assert!(matches!(
            reduce(&checked("running", json!([worker.clone()]), failed))
                .unwrap()
                .decision(),
            ExitDecision::Refused("check_failed")
        ));
        assert!(matches!(
            reduce(&checked("rejected", json!([]), json!([])))
                .unwrap()
                .decision(),
            ExitDecision::Refused("lead_rejected")
        ));
        assert!(matches!(
            reduce(&checked("running", json!([worker]), json!([])))
                .unwrap()
                .decision(),
            ExitDecision::Blocked("check_missing")
        ));
        assert!(matches!(
            reduce(&checked("running", json!([]), json!([])))
                .unwrap()
                .decision(),
            ExitDecision::InProgress
        ));
    }

    #[test]
    fn acceptance_gate_and_subject_freshness_fail_closed_with_controls() {
        let worker =
            json!({"attempt":1,"role":"worker","outcome":"completed","eventSha256":WORKER});
        let passed = json!([{"targetEventSha256":WORKER,"subjectSha256":SUBJECT,"eventSha256":CHECK,"exitCode":0}]);
        let missing_review =
            reduce(&checked("running", json!([worker.clone()]), passed.clone())).unwrap();
        assert_eq!(
            missing_review.acceptance_gate().unwrap_err(),
            "canonical acceptance requires reviewer approval"
        );
        let accepted = reduce(&checked("running", json!([worker]), passed)).unwrap();
        assert!(accepted.subject_is_current(&json!(SUBJECT)));
        assert!(!accepted.subject_is_current(&json!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )));
    }
}

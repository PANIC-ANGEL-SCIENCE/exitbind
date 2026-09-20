//! Human-readable rendering of the typed, validated run projection.

use serde_json::Value;
use std::path::Path;

use crate::run_human::{
    HumanCheckState, HumanCheckTarget, HumanExplanation, HumanIdentity, HumanLeadDecision,
    HumanLeadState, HumanProtection, HumanRecord, HumanReviewer, HumanStatus, HumanWorker,
};

pub(crate) fn print_status(
    status: &HumanStatus,
    session_goal: &Value,
    state_root: &Path,
    interactive: bool,
    closed_stdin: bool,
) -> bool {
    println!(
        "Run {}: {} (stage {}, attempt {})",
        inert(&status.run_id),
        inert(&status.status),
        status.stage,
        status.attempt
    );
    println!("Artifact: {}", inert(&status.artifact_status));
    println!("Claim: current worker claims are listed below.");
    print_workers(&status.workers);
    println!("Checks: {}", check_state(status.checks.state));
    print_checks(
        &status.checks.state,
        &status.checks.command,
        &status.checks.command_sha256,
        &status.checks.origin,
        status.checks.observed_capable,
        &status.checks.targets,
    );
    println!("Review: current reviewer outcomes are listed below.");
    print_reviewers(&status.reviewers);
    println!("Acceptance: see the actual lead decision below.");
    print_lead(&status.lead, &status.status, &status.protections);
    print_history(&status.history);
    print_protections(&status.protections);
    if let Some(guidance) = status.guidance {
        println!("Guidance: {guidance}");
    }
    if let Some(card) = crate::presentation_events::session_goal_card_for_human(
        state_root,
        session_goal,
        &status.progress,
        interactive,
        closed_stdin,
    ) {
        println!("{card}");
        return true;
    }
    if let Some(joke) = developer_joke(status, state_root, interactive, closed_stdin) {
        println!("{joke}");
    }
    false
}

pub(crate) fn print_explain(
    explanation: &HumanExplanation,
    session_goal: &Value,
    state_root: &Path,
    interactive: bool,
    closed_stdin: bool,
) -> bool {
    println!("Run {} explanation", inert(&explanation.status.run_id));
    println!(
        "Status: {} (artifact {})",
        inert(&explanation.status.status),
        inert(&explanation.status.artifact_status)
    );
    print_workers(&explanation.status.workers);
    print_checks(
        &explanation.status.checks.state,
        &explanation.status.checks.command,
        &explanation.status.checks.command_sha256,
        &explanation.status.checks.origin,
        explanation.status.checks.observed_capable,
        &explanation.status.checks.targets,
    );
    print_reviewers(&explanation.status.reviewers);
    print_lead(
        &explanation.status.lead,
        &explanation.status.status,
        &explanation.status.protections,
    );
    print_history(&explanation.status.history);
    match &explanation.protection {
        Some(protection) => print_protection("Protection", protection),
        None => println!("Protection: none selected"),
    }
    println!("Guidance: {}", explanation.guidance);
    if let Some(card) = crate::presentation_events::session_goal_card_for_human(
        state_root,
        session_goal,
        &explanation.status.progress,
        interactive,
        closed_stdin,
    ) {
        println!("{card}");
        return true;
    }
    if let Some(joke) = developer_joke(&explanation.status, state_root, interactive, closed_stdin) {
        println!("{joke}");
    }
    false
}

/// Optional human-only flavor. It is selected from typed terminal facts after
/// the full diagnostic and never enters JSON, ledgers, receipts, or evidence.
fn developer_joke(
    status: &HumanStatus,
    state_root: &Path,
    interactive: bool,
    closed_stdin: bool,
) -> Option<&'static str> {
    let blocked_transition = matches!(status.lead.state, HumanLeadState::Blocked)
        && status
            .lead
            .current
            .iter()
            .any(|record| record.outcome == "blocked");
    let refused_transition = status.protections.iter().any(|protection| {
        protection.current
            && matches!(
                protection.reason.as_str(),
                "check_missing" | "check_failed" | "preservation_missing" | "preservation_failed"
            )
    });
    if !interactive || closed_stdin || !(blocked_transition || refused_transition) {
        return None;
    }
    let transition = status
        .protections
        .iter()
        .find(|protection| protection.current)
        .map(|protection| format!("refusal:{}", protection.event_sha256))
        .or_else(|| {
            status
                .lead
                .current
                .iter()
                .find(|record| record.outcome == "blocked")
                .map(|record| format!("blocked:{}", record.event_sha256))
        })
        .unwrap_or_else(|| format!("{}:blocked", status.run_id));
    crate::presentation_events::failure_joke_once(
        state_root,
        &transition,
        interactive,
        closed_stdin,
    )
    .then_some("Developer special: the bug has requested a second opinion.")
}

fn print_workers(workers: &[HumanWorker]) {
    if workers.is_empty() {
        println!("Worker claim: none planned");
        return;
    }
    for worker in workers {
        match &worker.current {
            Some(record) => println!(
                "Worker claim: {} outcome={} attempt={} event={} artifact={}",
                identity(&worker.identity),
                inert(&record.outcome),
                record.attempt,
                inert(&record.event_sha256),
                inert(&record.artifact_sha256)
            ),
            None => println!(
                "Worker claim: {} outcome=pending attempt={} event=missing artifact=missing",
                identity(&worker.identity),
                worker.attempt
            ),
        }
    }
}

fn print_checks(
    state: &HumanCheckState,
    command: &Option<String>,
    command_sha256: &Option<String>,
    origin: &Option<String>,
    observed_capable: bool,
    targets: &[HumanCheckTarget],
) {
    let product = if crate::producer::exitbind_surface() {
        "Exitbind"
    } else {
        "Soulmate"
    };
    match state {
        HumanCheckState::Unconfigured => {
            println!("Host-reported check: not configured (unchecked run)");
        }
        _ => {
            if targets.is_empty() || origin.is_none() {
                println!("Host-reported check: not observed; not executed by {product}");
            } else if !observed_capable {
                println!(
                    "Host-reported check: {}; not executed by {product}; origin={}",
                    check_state(*state),
                    optional(origin.as_deref(), "unknown")
                );
            } else {
                let acquisitions: Vec<_> = targets
                    .iter()
                    .filter_map(|target| target.acquisition.as_deref())
                    .collect();
                if acquisitions.first().is_some()
                    && acquisitions
                        .iter()
                        .all(|acquisition| *acquisition == "reported")
                {
                    println!(
                        "Host-reported check: {}; not executed by {product}; acquisition is shown per target{}; origin={}",
                        check_state(*state),
                        missing_observe_guidance(*state),
                        optional(origin.as_deref(), "unknown")
                    );
                } else if acquisitions.first().is_some()
                    && acquisitions
                        .iter()
                        .all(|acquisition| *acquisition == "observed")
                {
                    println!(
                        "Locally observed check: {}; acquisition is shown per target{}; origin={}",
                        check_state(*state),
                        missing_observe_guidance(*state),
                        optional(origin.as_deref(), "unknown")
                    );
                } else if acquisitions.first().is_some() {
                    println!(
                        "Configured check: {} (mixed acquisition; see targets){}; origin={}",
                        check_state(*state),
                        missing_observe_guidance(*state),
                        optional(origin.as_deref(), "unknown")
                    );
                } else if *state == HumanCheckState::NotObserved {
                    println!(
                        "Host-reported check: not observed; not executed by {product}; local observe-check is available; origin={}",
                        optional(origin.as_deref(), "unknown")
                    );
                } else {
                    println!(
                        "Configured check: {} (mixed acquisition; see targets); origin={}",
                        check_state(*state),
                        optional(origin.as_deref(), "unknown")
                    );
                }
            }
            println!(
                "  Frozen command: {}",
                optional(command.as_deref(), "missing")
            );
            println!(
                "  Command SHA-256: {}",
                optional(command_sha256.as_deref(), "missing")
            );
            for target in targets {
                let worker = target
                    .worker
                    .as_ref()
                    .map_or_else(|| "unknown".to_owned(), identity);
                if target.result.is_none() {
                    println!(
                        "  Check target: worker={} event={} status={} reported exit={} check={}",
                        worker,
                        optional(target.target_event_sha256.as_deref(), "missing"),
                        inert(&target.status),
                        target
                            .exit_code
                            .map_or_else(|| "missing".to_owned(), |exit| exit.to_string()),
                        optional(target.check_event_sha256.as_deref(), "missing")
                    );
                    continue;
                }
                if target.acquisition.as_deref() == Some("reported") {
                    println!(
                        "  Check target: worker={} event={} status={} reported exit={} acquisition=reported result={} check={}",
                        worker,
                        optional(target.target_event_sha256.as_deref(), "missing"),
                        inert(&target.status),
                        target
                            .exit_code
                            .map_or_else(|| "missing".to_owned(), |exit| exit.to_string()),
                        target.result.as_deref().unwrap_or("missing"),
                        optional(target.check_event_sha256.as_deref(), "missing")
                    );
                    continue;
                }
                let acquisition = target.acquisition.as_deref().unwrap_or("unknown");
                let result = target.result.clone().unwrap_or_else(|| {
                    target
                        .exit_code
                        .map_or_else(|| "missing".to_owned(), |exit| format!("exit code {exit}"))
                });
                println!(
                    "  Check target: worker={} event={} status={} acquisition={} result={} check={}",
                    worker,
                    optional(target.target_event_sha256.as_deref(), "missing"),
                    inert(&target.status),
                    acquisition,
                    result,
                    optional(target.check_event_sha256.as_deref(), "missing")
                );
            }
        }
    }
}

fn print_reviewers(reviewers: &[HumanReviewer]) {
    if reviewers.is_empty() {
        println!("Reviewer outcome: none planned");
        return;
    }
    for reviewer in reviewers {
        match &reviewer.current {
            Some(record) => println!(
                "Reviewer outcome: {} outcome={} attempt={} event={} artifact={}",
                identity(&reviewer.identity),
                inert(&record.outcome),
                record.attempt,
                inert(&record.event_sha256),
                inert(&record.artifact_sha256)
            ),
            None => println!("Reviewer outcome: {} pending", identity(&reviewer.identity)),
        }
    }
}

fn print_lead(lead: &HumanLeadDecision, run_status: &str, protections: &[HumanProtection]) {
    let state = match lead.state {
        HumanLeadState::Pending => "pending",
        HumanLeadState::Accepted => "accepted",
        HumanLeadState::Rejected => "rejected",
        HumanLeadState::Blocked => "blocked",
        HumanLeadState::TerminalWithoutLeadDecision => "not recorded",
    };
    let detail = match lead.state {
        HumanLeadState::TerminalWithoutLeadDecision => {
            format!(" (run status={})", inert(run_status))
        }
        HumanLeadState::Pending if protections.iter().any(|item| item.current) => {
            " (protocol refusal is not a lead rejection)".to_owned()
        }
        _ => String::new(),
    };
    println!("Lead decision: {state}{detail}");
    for record in &lead.current {
        println!(
            "  Lead outcome: {} outcome={} attempt={} event={} artifact={}",
            identity(&record.identity),
            inert(&record.outcome),
            record.attempt,
            inert(&record.event_sha256),
            inert(&record.artifact_sha256)
        );
    }
}

fn print_history(history: &[HumanRecord]) {
    if history.is_empty() {
        return;
    }
    println!("History: prior attempts remain historical and are not current acceptance");
    for record in history {
        println!(
            "  Prior attempt {}: {} role={} outcome={} event={} artifact={}",
            record.attempt,
            identity(&record.identity),
            inert(&record.role),
            inert(&record.outcome),
            inert(&record.event_sha256),
            inert(&record.artifact_sha256)
        );
    }
}

fn print_protections(protections: &[HumanProtection]) {
    for protection in protections {
        print_protection("Protocol refusal", protection);
    }
}

fn print_protection(label: &str, protection: &HumanProtection) {
    println!(
        "{label}: attempt={} state={} attempted={} reason={} origin={} actor={} event={}",
        protection.attempt,
        if protection.current {
            "current"
        } else {
            "prior"
        },
        inert(&protection.attempted_outcome),
        inert(&protection.reason),
        inert(&protection.origin),
        inert(&protection.actor),
        inert(&protection.event_sha256)
    );
    for item in &protection.evidence {
        let worker = item
            .worker
            .as_ref()
            .map_or_else(|| "unknown".to_owned(), identity);
        if item.result.is_none() && item.acquisition.as_deref() == Some("reported") {
            println!(
                "  Evidence: worker={} target={} status={} reported exit={} check={}",
                worker,
                optional(item.target_event_sha256.as_deref(), "missing"),
                inert(&item.status),
                item.exit_code
                    .map_or_else(|| "missing".to_owned(), |exit| exit.to_string()),
                optional(item.check_event_sha256.as_deref(), "missing")
            );
            continue;
        }
        let acquisition = item.acquisition.as_deref().unwrap_or("unknown");
        let result = item.result.clone().unwrap_or_else(|| {
            item.exit_code
                .map_or_else(|| "missing".to_owned(), |exit| format!("exit code {exit}"))
        });
        println!(
            "  Evidence: worker={} target={} status={} acquisition={} result={} check={}",
            worker,
            optional(item.target_event_sha256.as_deref(), "missing"),
            inert(&item.status),
            acquisition,
            result,
            optional(item.check_event_sha256.as_deref(), "missing")
        );
    }
}

fn check_state(state: HumanCheckState) -> &'static str {
    match state {
        HumanCheckState::Unconfigured => "not configured",
        HumanCheckState::NotObserved => "not observed",
        HumanCheckState::Blocked => "blocked",
        HumanCheckState::Passed => "passed",
    }
}

fn missing_observe_guidance(state: HumanCheckState) -> &'static str {
    if state == HumanCheckState::NotObserved {
        "; local observe-check is available"
    } else {
        ""
    }
}

fn identity(value: &HumanIdentity) -> String {
    format!(
        "{} ({}) stage {}",
        inert(&value.display_name),
        inert(&value.name),
        value.stage
    )
}

fn optional(value: Option<&str>, missing: &str) -> String {
    value.map_or_else(|| missing.to_owned(), inert)
}

fn inert(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

//! Redacted aggregate reporting for checked runs.

use super::*;

/// Aggregate redacted run views.  Commands, goals, prompts, profiles, paths,
/// and artifact content are intentionally absent.  Synthetic and unclassified
/// runs are grouped separately and no incident or avoided-loss count is
/// inferred from them.  Missing policy metadata remains unclassified rather
/// than being inferred as a local report.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportGroup {
    runs: u64,
    checks: u64,
    protections: u64,
    failed_checks: u64,
    missing_checks: u64,
    duration_ms_reported: u64,
    duration_ms_reported_count: u64,
}

#[derive(Debug, Default, Serialize)]
struct ReportGroups {
    local_report: ReportGroup,
    synthetic: ReportGroup,
    unclassified: ReportGroup,
}

impl ReportGroups {
    fn get_mut(&mut self, origin: &str) -> Option<&mut ReportGroup> {
        match origin {
            "local_report" => Some(&mut self.local_report),
            "synthetic" => Some(&mut self.synthetic),
            "unclassified" => Some(&mut self.unclassified),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportMetrics {
    synthetic_incident_count: &'static str,
    user_confirmed_avoided_loss_count: &'static str,
    human_time_ms: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRedaction {
    goals: &'static str,
    commands: &'static str,
    prompts: &'static str,
    profiles: &'static str,
    paths: &'static str,
    artifact_content: &'static str,
    hashes_are_not_anonymization: bool,
}

#[derive(Debug, Serialize)]
struct ValueReport {
    version: u64,
    runs: u64,
    groups: ReportGroups,
    metrics: ReportMetrics,
    redaction: ReportRedaction,
}

impl ValueReport {
    fn new() -> Self {
        Self {
            version: VALUE_REPORT_VERSION,
            runs: 0,
            groups: ReportGroups::default(),
            metrics: ReportMetrics {
                synthetic_incident_count: "unknown",
                user_confirmed_avoided_loss_count: "unknown",
                human_time_ms: "unknown",
            },
            redaction: ReportRedaction {
                goals: "omitted",
                commands: "omitted",
                prompts: "omitted",
                profiles: "omitted",
                paths: "omitted",
                artifact_content: "omitted",
                hashes_are_not_anonymization: true,
            },
        }
    }
}

pub(crate) fn aggregate(states: &[Value]) -> Result<Value, String> {
    let mut seen_runs = std::collections::BTreeMap::new();
    let mut report = ValueReport::new();
    for state in states {
        let run_id = state["runId"]
            .as_str()
            .ok_or("report run is missing a runId")?;
        let state_hash = crate::evidence::hash::value(state);
        if let Some(previous) = seen_runs.insert(run_id.to_owned(), state_hash.clone()) {
            if previous != state_hash {
                return Err("duplicate runId has conflicting ledger evidence".into());
            }
            continue;
        }
        let origin = state["checkPolicy"]["origin"]
            .as_str()
            .unwrap_or("unclassified");
        if !matches!(origin, "local_report" | "synthetic" | "unclassified") {
            return Err("invalid report origin".into());
        }
        let empty_checks = Vec::new();
        let checks = state["checks"].as_array().unwrap_or(&empty_checks);
        let empty_protections = Vec::new();
        let protections = state["protections"]
            .as_array()
            .unwrap_or(&empty_protections);
        let assessment = crate::run_exit::assess(state)?;
        let group = report
            .groups
            .get_mut(origin)
            .ok_or("report groups are invalid")?;
        add_counter(&mut group.runs, 1)?;
        add_counter(&mut group.checks, checks.len() as u64)?;
        add_counter(&mut group.protections, protections.len() as u64)?;
        add_counter(
            &mut group.failed_checks,
            checks
                .iter()
                .filter(|check| check_exit_code(check) != Some(0))
                .count() as u64,
        )?;
        add_counter(
            &mut group.missing_checks,
            assessment
                .targets
                .iter()
                .filter(|target| target.is_missing())
                .count() as u64,
        )?;
        for check in checks {
            if check["acquisition"] != "observed" {
                if let Some(duration) = check["durationMs"].as_u64() {
                    add_counter(&mut group.duration_ms_reported, duration)?;
                    add_counter(&mut group.duration_ms_reported_count, 1)?;
                }
            }
        }
    }
    report.runs = seen_runs.len() as u64;
    serde_json::to_value(report).map_err(|error| error.to_string())
}

pub(crate) fn markdown(report: &Value) -> String {
    let groups = &report["groups"];
    format!(
        "Value proof report v{}\n\nRuns: {}\n\nLocal reports: {} runs, {} checks, {} protections\nSynthetic scenarios: {} runs, {} checks, {} protections\nUnclassified runs: {} runs, {} checks, {} protections\n\nSynthetic incident frequency, user-confirmed avoided loss, and human time are unknown.\nGoals, commands, prompts, profiles, paths, and artifact content are omitted. Hashes are not anonymization.\n",
        report["version"],
        report["runs"],
        groups["local_report"]["runs"],
        groups["local_report"]["checks"],
        groups["local_report"]["protections"],
        groups["synthetic"]["runs"],
        groups["synthetic"]["checks"],
        groups["synthetic"]["protections"],
        groups["unclassified"]["runs"],
        groups["unclassified"]["checks"],
        groups["unclassified"]["protections"],
    )
}

fn add_counter(current: &mut u64, amount: u64) -> Result<(), String> {
    let total = (*current)
        .checked_add(amount)
        .ok_or("report metric overflow")?;
    *current = total;
    Ok(())
}

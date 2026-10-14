//! Deterministic, token-free first value-proof scenario.
//!
//! The benchmark is deliberately a caller of the installed binary.  It does
//! not call run internals, execute arbitrary commands, or inspect the user's
//! project.  Every workflow action below is an explicit invocation of this
//! binary in a disposable portable fixture.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const SCENARIO_ID: &str = "false-completion-v1";
const SCENARIO_ORIGIN: &str = "synthetic";
const SOULMATE_CHECK_COMMAND: &str = "soulmate check --config verification.json";
const EXITBIND_CHECK_COMMAND: &str = "exitbind check --config verification.json";
const SOULMATE_CLAIM: &str = "In checked runs, Soulmate refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.";
const EXITBIND_CLAIM: &str = "In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.";
const EXPECTED_JSON: &str = include_str!("../proof/scenarios/false-completion-v1/expected.json");
const RUN_EVENT_V3_SCHEMA: &str = include_str!("../schema/run-event-v3.schema.json");
const RUN_EVENT_V4_SCHEMA: &str = include_str!("../schema/run-event-v4.schema.json");
const RUN_EVENT_V5_SCHEMA: &str = include_str!("../schema/run-event-v5.schema.json");
const VALUE_REPORT_V1_SCHEMA: &str = include_str!("../schema/value-report-v1.schema.json");
const VALUE_PROOF_V1_SCHEMA: &str = include_str!("../schema/value-proof-v1.schema.json");
const SCENARIO_README: &str = include_str!("../proof/scenarios/false-completion-v1/README.md");

fn exitbind_surface() -> bool {
    crate::producer::exitbind_surface()
}

fn config_name() -> &'static str {
    if exitbind_surface() {
        "exitbind.json"
    } else {
        "soulmate.json"
    }
}

fn state_namespace() -> &'static str {
    if exitbind_surface() {
        ".exitbind"
    } else {
        ".soulmate"
    }
}

fn check_command() -> &'static str {
    if exitbind_surface() {
        EXITBIND_CHECK_COMMAND
    } else {
        SOULMATE_CHECK_COMMAND
    }
}

fn claim() -> &'static str {
    if exitbind_surface() {
        EXITBIND_CLAIM
    } else {
        SOULMATE_CLAIM
    }
}

/// Run the one-command first value-proof scenario.
pub fn run(output: Option<&str>) -> Result<Value, String> {
    let started = Instant::now();
    let destination = output.map(resolve_output_destination).transpose()?;
    let mut invoker = Invoker::new()?;
    let fixture = TempFixture::create()?;
    if let Some(destination) = destination.as_ref() {
        reject_destination_inside_fixture(destination, fixture.path())?;
    }

    let evidence = execute_scenario(&mut invoker, fixture.path())?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let result = build_result(&invoker, &evidence, elapsed_ms)?;
    if let Some(destination) = destination {
        export_bundle(destination, &evidence, &result)?;
    }
    Ok(result)
}

/// Render the bounded human-facing proof summary without exposing raw ledger,
/// artifact, command, or path evidence.
pub fn render(value: &Value) -> Result<String, String> {
    if value["scenarioId"] != SCENARIO_ID || value["origin"] != SCENARIO_ORIGIN {
        return Err("benchmark result has an unexpected scenario identity".into());
    }
    let assertions = value["assertions"]
        .as_array()
        .ok_or("benchmark result has no assertions")?;
    let passed = assertions
        .iter()
        .filter(|item| item["passed"] == true)
        .count();
    if passed != assertions.len() {
        return Err("benchmark result contains a failed assertion".into());
    }
    let invocations = value["cliInvocations"]["total"]
        .as_u64()
        .ok_or("benchmark result has no CLI invocation count")?;
    let elapsed = value["automatedElapsedMs"]
        .as_u64()
        .ok_or("benchmark result has no automated elapsed time")?;
    let recovery = value["observedResult"]["protectedRecovery"]
        .as_object()
        .ok_or("benchmark result has no protected recovery observations")?;
    let repair_check_exit_code = recovery
        .get("repairCheckExitCode")
        .and_then(Value::as_i64)
        .ok_or("benchmark result has no repair check exit code")?;
    let passing_check_alone_accepted = recovery
        .get("passingCheckAloneAccepted")
        .and_then(Value::as_bool)
        .ok_or("benchmark result has no passing-check acceptance observation")?;
    let reviewer_approval_alone_accepted = recovery
        .get("reviewerApprovalAloneAccepted")
        .and_then(Value::as_bool)
        .ok_or("benchmark result has no reviewer acceptance observation")?;
    let final_status = recovery
        .get("finalStatus")
        .and_then(Value::as_str)
        .ok_or("benchmark result has no final recovery status")?;
    if repair_check_exit_code != 0
        || passing_check_alone_accepted
        || reviewer_approval_alone_accepted
        || final_status != "accepted"
    {
        return Err("benchmark result does not show the exercised recovery sequence".into());
    }
    Ok(format!(
        "False-completion proof passed ({passed}/{} assertions).\n\nTask: repair an incomplete project configuration. Scripted actors; real local checks.\nAttempt 1:\n  Worker claim: completed.\n  Host-reported check: failed (exit 1); synthetic caller report.\n  Lead decision: pending; protocol refusal recorded (not a lead rejection).\nRework: preserved the previous attempt for the next assignment.\nAttempt 2:\n  Worker claim: completed.\n  Host-reported check: passed (exit {repair_check_exit_code}); synthetic caller report.\nAfter the repair check, before final acceptance:\n  Current fresh reviewer assignment: pending.\n  Lead decision: pending.\n  A passing check alone did not accept the run.\nAfter review and lead acceptance:\n  Reviewer outcome: approved.\n  Lead decision: accepted.\nBenchmark driver ran the fixture check; run record-check only records the result.\n\nThe fixture ran without a model or setup in your project. To inspect the records, rerun with --output followed by a new directory path.\n\nSource: synthetic. CLI invocations: {invocations}. Automated elapsed time: {elapsed} ms. Human interaction time: unmeasured.\n",
        assertions.len()
    ))
}

struct Invoker {
    executable: PathBuf,
    total: u64,
    successful: u64,
    failed: u64,
    actions: BTreeMap<String, u64>,
}

struct ChildOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

struct TempFixture {
    path: PathBuf,
}

struct ScenarioEvidence {
    baseline_check_exit_code: i32,
    baseline_ledger: Vec<u8>,
    baseline_events: Vec<Value>,
    blocked_events: Vec<Value>,
    final_events: Vec<Value>,
    blocked_ledger: Vec<u8>,
    final_ledger: Vec<u8>,
    prior_attempt_preserved: bool,
    config_bytes: Vec<u8>,
    verification_invalid_bytes: Vec<u8>,
    verification_valid_bytes: Vec<u8>,
    artifact_files: Vec<FixtureFile>,
    failed_acceptance_exit_code: i32,
    repair_check_exit_code: i32,
    passing_check_alone_accepted: bool,
    reviewer_approval_alone_accepted: bool,
    report: Value,
    report_markdown: String,
}

struct FixtureFile {
    bundle_relative: String,
    bytes: Vec<u8>,
}

mod artifacts;
mod invocation;
mod result;
mod scenario;
use artifacts::*;
use invocation::*;
use result::*;
use scenario::*;

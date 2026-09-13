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
        "False-completion proof passed ({passed}/{} assertions).\n\nTask: repair an incomplete project configuration. Scripted actors; real local checks.\nAttempt 1:\n  Worker claim: completed.\n  Host-reported check: failed (exit 1); synthetic caller report.\n  Reviewer outcome: approved.\n  Lead decision: pending; protocol refusal recorded (not a lead rejection).\nRework: preserved the previous attempt for the next assignment.\nAttempt 2:\n  Worker claim: completed.\n  Host-reported check: passed (exit {repair_check_exit_code}); synthetic caller report.\nAfter the repair check, before final acceptance:\n  Current fresh reviewer assignment: pending.\n  Lead decision: pending.\n  A passing check alone did not accept the run.\nAfter review and lead acceptance:\n  Reviewer outcome: approved.\n  Lead decision: accepted.\nBenchmark driver ran the fixture check; run record-check only records the result.\n\nThe fixture ran without a model or setup in your project. To inspect the records, rerun with --output followed by a new directory path.\n\nSource: synthetic. CLI invocations: {invocations}. Automated elapsed time: {elapsed} ms. Human interaction time: unmeasured.\n",
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

impl Invoker {
    fn new() -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("benchmark cannot resolve its executable: {error}"))?;
        Ok(Self {
            executable,
            total: 0,
            successful: 0,
            failed: 0,
            actions: BTreeMap::new(),
        })
    }

    fn call<I, S>(&mut self, cwd: &Path, arguments: I) -> Result<ChildOutput, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let arguments = arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_os_string())
            .collect::<Vec<_>>();
        let action = action_label(&arguments)?;
        let output = Command::new(&self.executable)
            .args(&arguments)
            .current_dir(cwd)
            .output()
            .map_err(|error| format!("benchmark could not invoke its binary: {error}"))?;
        self.total += 1;
        if output.status.success() {
            self.successful += 1;
        } else {
            self.failed += 1;
        }
        *self.actions.entry(action).or_insert(0) += 1;
        Ok(ChildOutput {
            status: output.status,
            stdout: output.stdout,
        })
    }

    fn counts(&self) -> Value {
        json!({
            "total": self.total,
            "successful": self.successful,
            "failed": self.failed,
            "byAction": self.actions,
        })
    }
}

struct ChildOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

impl ChildOutput {
    fn exit_code(&self, label: &str) -> Result<i32, String> {
        self.status
            .code()
            .ok_or_else(|| format!("{label} terminated without an exit code"))
    }

    fn json(&self, label: &str) -> Result<Value, String> {
        serde_json::from_slice(&self.stdout)
            .map_err(|error| format!("{label} returned invalid JSON: {error}"))
    }
}

struct TempFixture {
    path: PathBuf,
}

impl TempFixture {
    fn create() -> Result<Self, String> {
        let base = std::env::temp_dir();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("benchmark clock is before the Unix epoch: {error}"))?
            .as_nanos();
        for attempt in 0..32u32 {
            let candidate = base.join(format!(
                "{}-value-proof-{}-{}-{}",
                if exitbind_surface() {
                    "exitbind"
                } else {
                    "soulmate"
                },
                std::process::id(),
                seed,
                attempt
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    if let Err(error) = set_dir_mode(&candidate, 0o700) {
                        let _ = fs::remove_dir(&candidate);
                        return Err(format!(
                            "benchmark fixture permissions cannot be set: {error}"
                        ));
                    }
                    let path = fs::canonicalize(&candidate).map_err(|error| {
                        let _ = fs::remove_dir(&candidate);
                        format!("benchmark fixture cannot be resolved: {error}")
                    })?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("benchmark fixture cannot be created: {}", error))
                }
            }
        }
        Err("benchmark fixture name could not be made unique".into())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
            .unwrap_or(true)
        {
            return;
        }
        let _ = fs::remove_dir_all(&self.path);
    }
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

fn execute_scenario(invoker: &mut Invoker, fixture: &Path) -> Result<ScenarioEvidence, String> {
    expect_success(
        invoker,
        fixture,
        args(["init", "--root", "."]),
        "portable fixture initialization",
    )?;
    let config_bytes = read_file(fixture.join(config_name()), "generated fixture config")?;
    let verification_invalid_bytes = b"{\"version\":1}\n".to_vec();
    write_file(
        fixture.join("verification.json"),
        &verification_invalid_bytes,
        "invalid verification fixture",
    )?;

    let baseline_check = expect_failure(
        invoker,
        fixture,
        args(["check", "--config", "verification.json"]),
        "baseline deterministic check",
    )?;
    let baseline_check_exit_code = baseline_check.exit_code("baseline deterministic check")?;
    if baseline_check_exit_code != 1 {
        return Err(format!(
            "baseline deterministic check returned {baseline_check_exit_code}; expected 1"
        ));
    }

    run_legacy_baseline(invoker, fixture)?;
    let baseline_ledger = read_ledger(fixture, "legacy.jsonl")?;
    let baseline_events = parse_ledger(&baseline_ledger, "legacy baseline")?;
    let baseline_submissions = submissions(&baseline_events);
    if baseline_submissions.len() != 4
        || distinct_artifact_count(&baseline_submissions) != 4
        || !has_submission(&baseline_submissions, "lead", "scoped", 1)
        || !has_submission(&baseline_submissions, "worker", "completed", 1)
        || !has_submission(&baseline_submissions, "reviewer", "approved", 1)
        || !has_submission(&baseline_submissions, "lead", "accepted", 1)
    {
        return Err("legacy baseline did not preserve four distinct role artifacts".into());
    }
    let baseline_view = expect_success(
        invoker,
        fixture,
        args([
            "run",
            "inspect",
            &format!("{}/runs/legacy.jsonl", state_namespace()),
            "--json",
            "--config",
            config_name(),
        ]),
        "legacy baseline inspection",
    )?
    .json("legacy baseline inspection")?;
    if baseline_view["status"] != "accepted" {
        return Err("legacy baseline did not reach its ordinary accepted state".into());
    }

    let protected_ledger = format!("{}/runs/protected.jsonl", state_namespace());
    expect_success(
        invoker,
        fixture,
        args_with([
            "run",
            "start",
            "change",
            "--goal",
            "synthetic checked completion fixture",
            "--ledger",
            protected_ledger.as_str(),
            "--check-command",
            check_command(),
            "--proof-origin",
            SCENARIO_ORIGIN,
            "--config",
            config_name(),
        ]),
        "protected checked run start",
    )?;

    let lead_one = next_assignment(invoker, fixture, &protected_ledger, "protected lead stage")?;
    require_assignment(&lead_one, "lead", 1, 1, "protected lead stage")?;
    let lead_one_path = write_state_artifact(
        fixture,
        "protected-lead-scoped.md",
        b"synthetic protected lead scope\n",
    )?;
    let lead_one_submit = submit(
        invoker,
        fixture,
        "lead",
        &protected_ledger,
        "scoped",
        &lead_one_path,
        true,
    )?;
    let lead_one_event = event_from_submit(&lead_one_submit, "protected lead submission")?;
    require_submission_event(
        &lead_one_event,
        "lead",
        "scoped",
        1,
        "protected lead submission",
    )?;

    let worker_one = next_assignment(
        invoker,
        fixture,
        &protected_ledger,
        "protected worker stage",
    )?;
    require_assignment(&worker_one, "worker", 2, 1, "protected worker stage")?;
    let worker_one_path = write_state_artifact(
        fixture,
        "protected-worker-attempt-1.md",
        b"synthetic worker completion attempt one\n",
    )?;
    let worker_one_submit = submit(
        invoker,
        fixture,
        "worker",
        &protected_ledger,
        "completed",
        &worker_one_path,
        true,
    )?;
    let worker_one_event = event_from_submit(&worker_one_submit, "protected worker attempt one")?;
    require_submission_event(
        &worker_one_event,
        "worker",
        "completed",
        1,
        "protected worker attempt one",
    )?;
    let worker_one_sha256 = string_field(
        &worker_one_event["artifact"],
        "sha256",
        "worker one artifact",
    )?
    .to_owned();
    let worker_one_bytes = read_state_artifact(fixture, &worker_one_path)?;

    let protected_check = expect_failure(
        invoker,
        fixture,
        args(["check", "--config", "verification.json"]),
        "protected deterministic check",
    )?;
    let protected_check_exit_code = protected_check.exit_code("protected deterministic check")?;
    if protected_check_exit_code != baseline_check_exit_code {
        return Err("baseline and protected check observations disagree".into());
    }
    let protected_check_value = record_check(
        invoker,
        fixture,
        &protected_ledger,
        event_sha(&worker_one_event, "worker one submission")?,
        protected_check_exit_code,
    )?;
    require_check_event(
        &protected_check_value["event"],
        event_sha(&worker_one_event, "worker one submission")?,
        protected_check_exit_code,
        "protected failed check record",
    )?;
    let after_failed_check = inspect(invoker, fixture, &protected_ledger, "after failed check")?;
    if after_failed_check["status"] != "running" {
        return Err("failed check changed canonical run status before acceptance".into());
    }

    let reviewer_one = next_assignment(
        invoker,
        fixture,
        &protected_ledger,
        "protected reviewer stage",
    )?;
    require_assignment(&reviewer_one, "reviewer", 3, 1, "protected reviewer stage")?;
    let reviewer_one_path = write_state_artifact(
        fixture,
        "protected-reviewer-attempt-1.md",
        b"synthetic reviewer approval attempt one\n",
    )?;
    let reviewer_one_submit = submit(
        invoker,
        fixture,
        "reviewer",
        &protected_ledger,
        "approved",
        &reviewer_one_path,
        true,
    )?;
    let reviewer_one_event =
        event_from_submit(&reviewer_one_submit, "protected reviewer attempt one")?;
    require_submission_event(
        &reviewer_one_event,
        "reviewer",
        "approved",
        1,
        "protected reviewer attempt one",
    )?;

    let lead_accept_one_path = write_state_artifact(
        fixture,
        "protected-lead-accept-attempt-1.md",
        b"synthetic lead acceptance attempt one\n",
    )?;
    let failed_acceptance_arguments = vec![
        OsString::from("run"),
        OsString::from("submit"),
        OsString::from("lead"),
        OsString::from(protected_ledger.as_str()),
        OsString::from("--outcome"),
        OsString::from("accepted"),
        OsString::from("--artifact"),
        OsString::from(lead_accept_one_path.as_str()),
        OsString::from("--artifact-root"),
        OsString::from("state"),
        OsString::from("--json"),
        OsString::from("--config"),
        OsString::from(config_name()),
    ];
    let failed_acceptance = invoker.call(fixture, failed_acceptance_arguments)?;
    let failed_acceptance_exit_code = failed_acceptance.exit_code("failed acceptance")?;
    if failed_acceptance.status.success() || failed_acceptance_exit_code != 1 {
        return Err("failed check did not refuse lead acceptance with exit code 1".into());
    }

    let blocked_ledger = read_ledger(fixture, "protected.jsonl")?;
    write_file(
        fixture.join("blocked-ledger.jsonl"),
        &blocked_ledger,
        "blocked ledger snapshot",
    )?;
    let blocked_events = parse_ledger(&blocked_ledger, "blocked protected run")?;
    let blocked_view = inspect(invoker, fixture, &protected_ledger, "blocked protected run")?;
    let blocked_assignment = next_assignment(
        invoker,
        fixture,
        &protected_ledger,
        "blocked protected lead stage",
    )?;
    let blocked_protection_count = protection_count(&blocked_events);
    if blocked_view["status"] != "running"
        || blocked_protection_count != 1
        || has_canonical_acceptance(&blocked_events)
        || !has_protection_reason(&blocked_events, "check_failed")
        || blocked_assignment["agent"] != "lead"
        || blocked_assignment["stage"] != 4
        || blocked_assignment["attempt"] != 1
    {
        return Err(format!(
            "blocked protected run did not preserve one factual refusal: status={:?} protections={blocked_protection_count} canonical={} reason={} pending={}",
            blocked_view["status"],
            has_canonical_acceptance(&blocked_events),
            has_protection_reason(&blocked_events, "check_failed"),
            blocked_assignment["agent"] == "lead"
                && blocked_assignment["stage"] == 4
                && blocked_assignment["attempt"] == 1,
        ));
    }

    let lead_rework = next_assignment(invoker, fixture, &protected_ledger, "lead rework stage")?;
    require_assignment(&lead_rework, "lead", 4, 1, "lead rework stage")?;
    let lead_rework_path = write_state_artifact(
        fixture,
        "protected-lead-rework.md",
        b"synthetic lead rework request\n",
    )?;
    let lead_rework_submit = submit(
        invoker,
        fixture,
        "lead",
        &protected_ledger,
        "rework",
        &lead_rework_path,
        true,
    )?;
    let lead_rework_event = event_from_submit(&lead_rework_submit, "lead rework")?;
    require_submission_event(&lead_rework_event, "lead", "rework", 1, "lead rework")?;

    let worker_two = next_assignment(
        invoker,
        fixture,
        &protected_ledger,
        "protected worker attempt two",
    )?;
    require_assignment(&worker_two, "worker", 2, 2, "protected worker attempt two")?;
    let worker_two_path = write_state_artifact(
        fixture,
        "protected-worker-attempt-2.md",
        b"synthetic repaired worker completion attempt two\n",
    )?;
    let worker_two_submit = submit(
        invoker,
        fixture,
        "worker",
        &protected_ledger,
        "completed",
        &worker_two_path,
        true,
    )?;
    let worker_two_event = event_from_submit(&worker_two_submit, "protected worker attempt two")?;
    require_submission_event(
        &worker_two_event,
        "worker",
        "completed",
        2,
        "protected worker attempt two",
    )?;
    let worker_two_sha256 = string_field(
        &worker_two_event["artifact"],
        "sha256",
        "worker two artifact",
    )?
    .to_owned();
    let worker_two_bytes = read_state_artifact(fixture, &worker_two_path)?;

    let verification_valid_bytes = config_bytes.clone();
    replace_file(
        fixture.join("verification.json"),
        &verification_valid_bytes,
        "repaired verification fixture",
    )?;
    let repaired_check = expect_success(
        invoker,
        fixture,
        args(["check", "--config", "verification.json"]),
        "repaired deterministic check",
    )?;
    let repaired_check_exit_code = repaired_check.exit_code("repaired deterministic check")?;
    if repaired_check_exit_code != 0 {
        return Err(format!(
            "repaired deterministic check returned {repaired_check_exit_code}; expected 0"
        ));
    }
    let repaired_check_value = record_check(
        invoker,
        fixture,
        &protected_ledger,
        event_sha(&worker_two_event, "worker two submission")?,
        repaired_check_exit_code,
    )?;
    require_check_event(
        &repaired_check_value["event"],
        event_sha(&worker_two_event, "worker two submission")?,
        repaired_check_exit_code,
        "repaired check record",
    )?;

    let after_passing_check = inspect(invoker, fixture, &protected_ledger, "after passing check")?;
    if after_passing_check["status"] != "running"
        || has_canonical_acceptance(&parse_ledger(
            &read_ledger(fixture, "protected.jsonl")?,
            "after passing check",
        )?)
    {
        return Err("a passing check alone advanced canonical acceptance".into());
    }
    let passing_check_alone_accepted = after_passing_check["status"] != "running";

    let reviewer_two = next_assignment(
        invoker,
        fixture,
        &protected_ledger,
        "protected reviewer attempt two",
    )?;
    require_assignment(
        &reviewer_two,
        "reviewer",
        3,
        2,
        "protected reviewer attempt two",
    )?;
    let reviewer_two_path = write_state_artifact(
        fixture,
        "protected-reviewer-attempt-2.md",
        b"synthetic reviewer approval attempt two\n",
    )?;
    let reviewer_two_submit = submit(
        invoker,
        fixture,
        "reviewer",
        &protected_ledger,
        "approved",
        &reviewer_two_path,
        true,
    )?;
    let reviewer_two_event =
        event_from_submit(&reviewer_two_submit, "protected reviewer attempt two")?;
    require_submission_event(
        &reviewer_two_event,
        "reviewer",
        "approved",
        2,
        "protected reviewer attempt two",
    )?;
    let after_review = inspect(invoker, fixture, &protected_ledger, "after repaired review")?;
    if after_review["status"] != "running"
        || has_canonical_acceptance(&parse_ledger(
            &read_ledger(fixture, "protected.jsonl")?,
            "after repaired review",
        )?)
    {
        return Err("reviewer approval alone advanced canonical acceptance".into());
    }
    let reviewer_approval_alone_accepted = after_review["status"] != "running";

    let lead_accept_two_path = write_state_artifact(
        fixture,
        "protected-lead-accepted.md",
        b"synthetic final lead acceptance\n",
    )?;
    let final_submit = submit(
        invoker,
        fixture,
        "lead",
        &protected_ledger,
        "accepted",
        &lead_accept_two_path,
        true,
    )?;
    let final_event = event_from_submit(&final_submit, "final lead acceptance")?;
    require_submission_event(&final_event, "lead", "accepted", 2, "final lead acceptance")?;
    let final_view = inspect(invoker, fixture, &protected_ledger, "final protected run")?;
    if final_view["status"] != "accepted" {
        return Err("repaired protected run did not reach final lead acceptance".into());
    }
    let final_ledger = read_ledger(fixture, "protected.jsonl")?;
    write_file(
        fixture.join("final-ledger.jsonl"),
        &final_ledger,
        "final ledger snapshot",
    )?;
    let final_events = parse_ledger(&final_ledger, "final protected run")?;

    if !has_event_for_target(
        &final_events,
        "check",
        event_sha(&worker_one_event, "worker one")?,
        1,
    ) || !has_event_for_target(
        &final_events,
        "check",
        event_sha(&worker_two_event, "worker two")?,
        0,
    ) {
        return Err("final ledger lost one of the immutable worker check targets".into());
    }
    let preserved_one = sha256(&worker_one_bytes) == worker_one_sha256
        && sha256(&worker_one_bytes)
            == artifact_sha_for_event(&final_events, event_sha(&worker_one_event, "worker one")?)?;
    let preserved_two = sha256(&worker_two_bytes) == worker_two_sha256
        && sha256(&worker_two_bytes)
            == artifact_sha_for_event(&final_events, event_sha(&worker_two_event, "worker two")?)?;
    if !preserved_one || !preserved_two {
        return Err("worker attempt bytes or hashes were not preserved".into());
    }
    let prior_attempt_preserved = preserved_one
        && preserved_two
        && read_state_artifact(fixture, &worker_one_path)? == worker_one_bytes
        && read_state_artifact(fixture, &worker_two_path)? == worker_two_bytes;
    if !prior_attempt_preserved {
        return Err("worker attempt bytes or hashes were not preserved".into());
    }

    let report_output = expect_success(
        invoker,
        fixture,
        args([
            "run",
            "report",
            protected_ledger.as_str(),
            "--json",
            "--config",
            config_name(),
        ]),
        "protected JSON report",
    )?;
    let report = report_output.json("protected JSON report")?;
    let report_markdown = expect_success(
        invoker,
        fixture,
        args([
            "run",
            "report",
            protected_ledger.as_str(),
            "--config",
            config_name(),
        ]),
        "protected Markdown report",
    )?;
    let report_markdown = String::from_utf8(report_markdown.stdout)
        .map_err(|error| format!("protected Markdown report is not UTF-8: {error}"))?;
    if !report_has_separate_origins(&report) || report_forbidden_fields(&report, false).is_some() {
        return Err(
            "protected JSON report did not preserve origin separation and redaction".into(),
        );
    }

    let artifact_files = fixture_artifacts(
        fixture,
        &[
            "legacy-lead.md",
            "legacy-worker.md",
            "legacy-reviewer.md",
            "legacy-accepted.md",
            &format!("{}/artifacts/protected-lead-scoped.md", state_namespace()),
            &format!(
                "{}/artifacts/protected-worker-attempt-1.md",
                state_namespace()
            ),
            &format!(
                "{}/artifacts/protected-reviewer-attempt-1.md",
                state_namespace()
            ),
            &format!(
                "{}/artifacts/protected-lead-accept-attempt-1.md",
                state_namespace()
            ),
            &format!("{}/artifacts/protected-lead-rework.md", state_namespace()),
            &format!(
                "{}/artifacts/protected-worker-attempt-2.md",
                state_namespace()
            ),
            &format!(
                "{}/artifacts/protected-reviewer-attempt-2.md",
                state_namespace()
            ),
            &format!("{}/artifacts/protected-lead-accepted.md", state_namespace()),
        ],
    )?;

    Ok(ScenarioEvidence {
        baseline_check_exit_code,
        baseline_ledger,
        baseline_events,
        blocked_events,
        final_events,
        blocked_ledger,
        final_ledger,
        prior_attempt_preserved,
        config_bytes,
        verification_invalid_bytes,
        verification_valid_bytes,
        artifact_files,
        failed_acceptance_exit_code,
        repair_check_exit_code: repaired_check_exit_code,
        passing_check_alone_accepted,
        reviewer_approval_alone_accepted,
        report,
        report_markdown,
    })
}

fn run_legacy_baseline(invoker: &mut Invoker, fixture: &Path) -> Result<(), String> {
    let ledger = format!("{}/runs/legacy.jsonl", state_namespace());
    expect_success(
        invoker,
        fixture,
        args_with([
            "run",
            "start",
            "change",
            "--goal",
            "synthetic legacy baseline",
            "--ledger",
            ledger.as_str(),
            "--config",
            config_name(),
        ]),
        "legacy baseline run start",
    )?;
    for (agent, outcome, path, content) in [
        (
            "lead",
            "scoped",
            "legacy-lead.md",
            b"synthetic legacy lead scope\n" as &[u8],
        ),
        (
            "worker",
            "completed",
            "legacy-worker.md",
            b"synthetic legacy worker completion\n",
        ),
        (
            "reviewer",
            "approved",
            "legacy-reviewer.md",
            b"synthetic legacy reviewer approval\n",
        ),
        (
            "lead",
            "accepted",
            "legacy-accepted.md",
            b"synthetic legacy lead acceptance\n",
        ),
    ] {
        let expected_stage = match (agent, outcome) {
            ("lead", "scoped") => 1,
            ("worker", "completed") => 2,
            ("reviewer", "approved") => 3,
            ("lead", "accepted") => 4,
            _ => return Err("invalid legacy fixture stage".into()),
        };
        let assignment = next_assignment(invoker, fixture, &ledger, "legacy stage")?;
        require_assignment(&assignment, agent, expected_stage, 1, "legacy stage")?;
        write_file(fixture.join(path), content, "legacy artifact")?;
        submit(invoker, fixture, agent, &ledger, outcome, path, false)?;
    }
    Ok(())
}

fn next_assignment(
    invoker: &mut Invoker,
    fixture: &Path,
    ledger: &str,
    label: &str,
) -> Result<Value, String> {
    let output = expect_success(
        invoker,
        fixture,
        args_with(["run", "next", ledger, "--json", "--config", config_name()]),
        label,
    )?;
    let value = output.json(label)?;
    value["assignments"]
        .as_array()
        .and_then(|assignments| assignments.first())
        .cloned()
        .ok_or_else(|| format!("{label} returned no pending assignment"))
}

fn require_assignment(
    assignment: &Value,
    agent: &str,
    stage: u64,
    attempt: u64,
    label: &str,
) -> Result<(), String> {
    if assignment["agent"] != agent
        || assignment["stage"] != stage
        || assignment["attempt"] != attempt
    {
        return Err(format!("{label} returned an unexpected assignment"));
    }
    Ok(())
}

fn submit(
    invoker: &mut Invoker,
    fixture: &Path,
    agent: &str,
    ledger: &str,
    outcome: &str,
    artifact: &str,
    state_root: bool,
) -> Result<Value, String> {
    let mut arguments = vec![
        OsString::from("run"),
        OsString::from("submit"),
        OsString::from(agent),
        OsString::from(ledger),
        OsString::from("--outcome"),
        OsString::from(outcome),
        OsString::from("--artifact"),
        OsString::from(artifact),
    ];
    if state_root {
        arguments.push(OsString::from("--artifact-root"));
        arguments.push(OsString::from("state"));
    }
    arguments.extend([OsString::from("--config"), OsString::from(config_name())]);
    expect_success(invoker, fixture, arguments, "run submission")?.json("run submission")
}

fn record_check(
    invoker: &mut Invoker,
    fixture: &Path,
    ledger: &str,
    target: &str,
    exit_code: i32,
) -> Result<Value, String> {
    let exit_code = exit_code.to_string();
    let arguments = vec![
        OsString::from("run"),
        OsString::from("record-check"),
        OsString::from(ledger),
        OsString::from("--target"),
        OsString::from(target),
        OsString::from("--check-command"),
        OsString::from(check_command()),
        OsString::from("--exit-code"),
        OsString::from(exit_code.as_str()),
        OsString::from("--json"),
        OsString::from("--config"),
        OsString::from(config_name()),
    ];
    expect_success(invoker, fixture, arguments, "run record-check")?.json("run record-check")
}

fn inspect(
    invoker: &mut Invoker,
    fixture: &Path,
    ledger: &str,
    label: &str,
) -> Result<Value, String> {
    expect_success(
        invoker,
        fixture,
        args_with([
            "run",
            "inspect",
            ledger,
            "--json",
            "--config",
            config_name(),
        ]),
        label,
    )?
    .json(label)
}

fn expect_success<I, S>(
    invoker: &mut Invoker,
    fixture: &Path,
    arguments: I,
    label: &str,
) -> Result<ChildOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = invoker.call(fixture, arguments)?;
    if !output.status.success() {
        return Err(format!(
            "{label} failed with exit code {}",
            output
                .status
                .code()
                .map_or_else(|| "signal".into(), |code| code.to_string())
        ));
    }
    Ok(output)
}

fn expect_failure<I, S>(
    invoker: &mut Invoker,
    fixture: &Path,
    arguments: I,
    label: &str,
) -> Result<ChildOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = invoker.call(fixture, arguments)?;
    if output.status.success() {
        return Err(format!("{label} unexpectedly succeeded"));
    }
    Ok(output)
}

fn event_from_submit(value: &Value, label: &str) -> Result<Value, String> {
    value
        .get("event")
        .filter(|event| event.is_object())
        .cloned()
        .ok_or_else(|| format!("{label} did not return an event"))
}

fn require_submission_event(
    event: &Value,
    agent: &str,
    outcome: &str,
    attempt: u64,
    label: &str,
) -> Result<(), String> {
    if event["action"] != "submit"
        || event["agent"] != agent
        || event["outcome"] != outcome
        || event["attempt"] != attempt
        || if exitbind_surface() {
            event["version"] != 5
        } else {
            !matches!(event["version"].as_u64(), Some(3 | 4))
        }
    {
        return Err(format!("{label} returned an unexpected event"));
    }
    Ok(())
}

fn require_check_event(
    event: &Value,
    target: &str,
    exit_code: i32,
    label: &str,
) -> Result<(), String> {
    if event["action"] != "check"
        || event["targetEventSha256"] != target
        || event["checkCommand"] != check_command()
        || event["origin"] != SCENARIO_ORIGIN
        || event_exit_code(event) != Some(exit_code as u64)
    {
        return Err(format!(
            "{label} did not retain its exact target, policy, or exit code"
        ));
    }
    Ok(())
}

fn build_result(
    invoker: &Invoker,
    evidence: &ScenarioEvidence,
    elapsed_ms: u64,
) -> Result<Value, String> {
    let expected = expected_contract()?;
    if expected["scenarioId"] != SCENARIO_ID || expected["origin"] != SCENARIO_ORIGIN {
        return Err("benchmark expected contract has the wrong scenario identity".into());
    }
    let baseline_submissions = submissions(&evidence.baseline_events);
    let blocked_protections = protection_count(&evidence.blocked_events);
    let final_worker_attempts = evidence
        .final_events
        .iter()
        .filter(|event| {
            event["action"] == "submit"
                && event["role"] == "worker"
                && event["outcome"] == "completed"
        })
        .count();
    let blocked_canonical = has_canonical_acceptance(&evidence.blocked_events);
    let baseline_status = blocked_status(&evidence.baseline_events);
    let blocked_status = blocked_status(&evidence.blocked_events);
    let final_status = final_status(&evidence.final_events)?;

    let assertions = vec![
        assertion(
            "baseline_nonzero_check_observed",
            evidence.baseline_check_exit_code == 1,
        ),
        assertion(
            "baseline_completed_with_four_distinct_artifacts",
            baseline_submissions.len() == 4 && distinct_artifact_count(&baseline_submissions) == 4,
        ),
        assertion(
            "protected_failed_check_observed",
            has_event_for_exit(&evidence.blocked_events, "check", 1),
        ),
        assertion(
            "failed_acceptance_refused",
            evidence.failed_acceptance_exit_code == 1
                && !blocked_canonical
                && blocked_status == "running",
        ),
        assertion(
            "no_canonical_acceptance_on_failed_check",
            !blocked_canonical,
        ),
        assertion("one_factual_protection_record", blocked_protections == 1),
        assertion(
            "blocked_ledger_snapshot_saved",
            !evidence.blocked_ledger.is_empty() && sha256(&evidence.blocked_ledger).len() == 64,
        ),
        assertion(
            "repair_check_observed_zero",
            evidence.repair_check_exit_code == 0
                && has_event_for_exit(&evidence.final_events, "check", 0),
        ),
        assertion(
            "passing_check_does_not_accept_alone",
            !evidence.passing_check_alone_accepted,
        ),
        assertion(
            "reviewer_approval_does_not_accept_alone",
            !evidence.reviewer_approval_alone_accepted,
        ),
        assertion("lead_acceptance_after_repair", final_status == "accepted"),
        assertion(
            "prior_attempt_bytes_preserved",
            evidence.prior_attempt_preserved,
        ),
        assertion(
            "report_redacted_and_origin_separated",
            report_has_separate_origins(&evidence.report)
                && report_forbidden_fields(&evidence.report, false).is_none(),
        ),
        assertion(
            "invocation_counts_observed",
            invoker.total > 0 && invoker.total == invoker.successful + invoker.failed,
        ),
    ];
    if assertions.iter().any(|item| item["passed"] != true) {
        let failed = assertions
            .iter()
            .filter(|item| item["passed"] != true)
            .filter_map(|item| item["id"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("benchmark assertion failed: {failed}"));
    }

    let expected_result = expected["expectedResult"].clone();
    let observed_result = json!({
        "baseline": {
            "checkExitCode": evidence.baseline_check_exit_code,
            "distinctArtifactCount": baseline_submissions.len(),
            "finalStatus": baseline_status,
        },
        "protectedFailedCheck": {
            "checkExitCode": evidence.baseline_check_exit_code,
            "acceptanceExitCodeNonzero": evidence.failed_acceptance_exit_code != 0,
            "canonicalAcceptedEvent": blocked_canonical,
            "protectionRecordCount": blocked_protections,
            "status": blocked_status,
        },
        "protectedRecovery": {
            "repairCheckExitCode": evidence.repair_check_exit_code,
            "passingCheckAloneAccepted": evidence.passing_check_alone_accepted,
            "reviewerApprovalAloneAccepted": evidence.reviewer_approval_alone_accepted,
            "finalStatus": final_status,
            "workerAttemptCount": final_worker_attempts,
            "priorAttemptPreserved": evidence.prior_attempt_preserved,
        },
    });
    if expected_result != observed_result {
        return Err("benchmark observed result differs from its stable expected result".into());
    }
    let expected_ids = expected["assertionIds"]
        .as_array()
        .ok_or("benchmark expected contract has no assertion IDs")?;
    let actual_ids = assertions
        .iter()
        .map(|item| item["id"].clone())
        .collect::<Vec<_>>();
    if expected_ids != &actual_ids {
        return Err("benchmark assertion IDs differ from the stable scenario contract".into());
    }

    Ok(json!({
        "version": 1,
        "scenarioId": SCENARIO_ID,
        "origin": SCENARIO_ORIGIN,
        "product": if exitbind_surface() { "Exitbind" } else { "Soulmate" },
        "producer": if exitbind_surface() { "exitbind" } else { "soulmate" },
        "formatVersion": if exitbind_surface() { 5 } else { 4 },
        "layout": {
            "config": config_name(),
            "state": state_namespace(),
        },
        "claim": claim(),
        "limitations": expected["limitations"],
        "assertions": assertions,
        "expectedResult": expected_result,
        "observedResult": observed_result,
        "cliInvocations": invoker.counts(),
        "manualJsonEdits": 0,
        "automatedElapsedMs": elapsed_ms,
        "humanInteractionMs": Value::Null,
        "report": evidence.report,
    }))
}

fn assertion(id: &str, passed: bool) -> Value {
    json!({"id": id, "expected": true, "passed": passed})
}

fn expected_contract() -> Result<Value, String> {
    serde_json::from_str(EXPECTED_JSON)
        .map_err(|error| format!("benchmark expected result is invalid JSON: {error}"))
}

fn final_status(events: &[Value]) -> Result<&str, String> {
    if has_canonical_acceptance(events) {
        Ok("accepted")
    } else {
        Err("final protected ledger has no canonical acceptance".into())
    }
}

fn blocked_status(events: &[Value]) -> &str {
    if has_canonical_acceptance(events) {
        "accepted"
    } else {
        "running"
    }
}

fn submissions(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event["action"] == "submit")
        .collect()
}

fn distinct_artifact_count(events: &[&Value]) -> usize {
    events
        .iter()
        .filter_map(|event| {
            Some((
                event["artifact"]["root"].as_str()?,
                event["artifact"]["path"].as_str()?,
                event["artifact"]["sha256"].as_str()?,
            ))
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn has_submission(events: &[&Value], agent: &str, outcome: &str, attempt: u64) -> bool {
    events.iter().any(|event| {
        event["agent"] == agent && event["outcome"] == outcome && event["attempt"] == attempt
    })
}

fn protection_count(events: &[Value]) -> usize {
    events
        .iter()
        .filter(|event| event["action"] == "protect")
        .count()
}

fn has_protection_reason(events: &[Value], reason: &str) -> bool {
    events
        .iter()
        .any(|event| event["action"] == "protect" && event["reason"] == reason)
}

fn has_canonical_acceptance(events: &[Value]) -> bool {
    events.iter().any(|event| {
        event["action"] == "submit" && event["role"] == "lead" && event["outcome"] == "accepted"
    })
}

fn has_event_for_exit(events: &[Value], action: &str, exit_code: u64) -> bool {
    events
        .iter()
        .any(|event| event["action"] == action && event_exit_code(event) == Some(exit_code))
}

fn has_event_for_target(events: &[Value], action: &str, target: &str, exit_code: u64) -> bool {
    events.iter().any(|event| {
        event["action"] == action
            && event["targetEventSha256"] == target
            && event_exit_code(event) == Some(exit_code)
    })
}

fn event_exit_code(event: &Value) -> Option<u64> {
    event["exitCode"].as_u64().or_else(|| {
        (event["result"]["kind"] == "exit")
            .then(|| event["result"]["code"].as_u64())
            .flatten()
    })
}

fn event_sha<'a>(event: &'a Value, label: &str) -> Result<&'a str, String> {
    event["eventSha256"]
        .as_str()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| format!("{label} has no valid event hash"))
}

fn artifact_sha_for_event(events: &[Value], target: &str) -> Result<String, String> {
    events
        .iter()
        .find(|event| event["action"] == "submit" && event["eventSha256"] == target)
        .and_then(|event| event["artifact"]["sha256"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("submission target {target} has no artifact hash"))
}

fn string_field<'a>(value: &'a Value, field: &str, label: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label} has no {field}"))
}

fn report_has_separate_origins(report: &Value) -> bool {
    let groups = report.get("groups").and_then(Value::as_object);
    groups.is_some_and(|groups| {
        groups.get("synthetic").is_some_and(Value::is_object)
            && groups.get("local_report").is_some_and(Value::is_object)
    })
}

fn report_forbidden_fields(value: &Value, in_redaction: bool) -> Option<String> {
    let forbidden = [
        "goal",
        "command",
        "commands",
        "prompt",
        "prompts",
        "profile",
        "profiles",
        "transcript",
        "transcripts",
        "path",
        "paths",
        "artifact",
        "artifacts",
        "artifactContent",
    ];
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if !in_redaction && forbidden.contains(&key.as_str()) {
                    return Some(key.clone());
                }
                if let Some(found) =
                    report_forbidden_fields(child, in_redaction || key == "redaction")
                {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| report_forbidden_fields(item, in_redaction)),
        _ => None,
    }
}

fn fixture_artifacts(fixture: &Path, paths: &[&str]) -> Result<Vec<FixtureFile>, String> {
    paths
        .iter()
        .map(|relative| {
            let bytes = read_file(fixture.join(relative), "fixture artifact")?;
            let bundle_relative = format!("fixtures/{relative}");
            Ok(FixtureFile {
                bundle_relative,
                bytes,
            })
        })
        .collect()
}

fn write_state_artifact(fixture: &Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    let relative = format!("{}/artifacts/{name}", state_namespace());
    write_file(fixture.join(&relative), bytes, "state artifact")?;
    Ok(relative)
}

fn read_state_artifact(fixture: &Path, relative: &str) -> Result<Vec<u8>, String> {
    read_file(fixture.join(relative), "state artifact")
}

fn read_ledger(fixture: &Path, name: &str) -> Result<Vec<u8>, String> {
    read_file(
        fixture.join(format!("{}/runs/{name}", state_namespace())),
        "run ledger",
    )
}

fn parse_ledger(bytes: &[u8], label: &str) -> Result<Vec<Value>, String> {
    let source =
        std::str::from_utf8(bytes).map_err(|error| format!("{label} is not UTF-8: {error}"))?;
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("{label} has invalid JSON: {error}"))
        })
        .collect()
}

fn write_file(path: PathBuf, bytes: &[u8], label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label} has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| format!("{label} cannot create parent: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("{label} cannot be created: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("{label} cannot be written: {error}"))?;
    file.flush()
        .map_err(|error| format!("{label} cannot be flushed: {error}"))?;
    set_file_mode(&path, 0o600)?;
    Ok(())
}

fn replace_file(path: PathBuf, bytes: &[u8], label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("{label} cannot be replaced: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} must be a regular file"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|error| format!("{label} cannot be replaced: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("{label} cannot be written: {error}"))?;
    file.flush()
        .map_err(|error| format!("{label} cannot be flushed: {error}"))?;
    set_file_mode(&path, 0o600)?;
    Ok(())
}

fn read_file(path: PathBuf, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("{label} cannot be read: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} must be a regular file"));
    }
    fs::read(path).map_err(|error| format!("{label} cannot be read: {error}"))
}

fn args<const N: usize>(values: [&str; N]) -> Vec<OsString> {
    values.into_iter().map(OsString::from).collect()
}

fn args_with<I, S>(values: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    values
        .into_iter()
        .map(|value| value.as_ref().to_os_string())
        .collect()
}

fn action_label(arguments: &[OsString]) -> Result<String, String> {
    match (arguments.first(), arguments.get(1)) {
        (Some(command), Some(action)) if command == "run" => Ok(format!(
            "run {}",
            action.to_str().ok_or("benchmark action is not UTF-8")?
        )),
        (Some(command), _) => Ok(command
            .to_str()
            .ok_or("benchmark command is not UTF-8")?
            .to_owned()),
        _ => Err("benchmark command is missing".into()),
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn resolve_output_destination(output: &str) -> Result<PathBuf, String> {
    if output.trim().is_empty() || output.contains('\0') {
        return Err("benchmark output path must be a non-empty UTF-8 path".into());
    }
    let requested = Path::new(output);
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("benchmark current directory cannot be resolved: {error}"))?
            .join(requested)
    };
    let parent = absolute
        .parent()
        .ok_or("benchmark output path has no parent")?;
    let real_parent = fs::canonicalize(parent)
        .map_err(|error| format!("benchmark output parent cannot be resolved: {error}"))?;
    let name = absolute
        .file_name()
        .ok_or("benchmark output path has no final directory name")?;
    let destination = real_parent.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() {
            return Err("benchmark output path must not be a symlink".into());
        }
        return Err("benchmark output path already exists; refusing overwrite".into());
    }
    Ok(destination)
}

fn reject_destination_inside_fixture(destination: &Path, fixture: &Path) -> Result<(), String> {
    if destination.starts_with(fixture) {
        Err("benchmark output path must be outside its disposable fixture".into())
    } else {
        Ok(())
    }
}

struct BundleGuard {
    path: PathBuf,
    created: bool,
}

impl Drop for BundleGuard {
    fn drop(&mut self) {
        if !self.created {
            return;
        }
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return;
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn export_bundle(
    destination: PathBuf,
    evidence: &ScenarioEvidence,
    result: &Value,
) -> Result<(), String> {
    if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
        return Err("benchmark output path already exists; refusing overwrite".into());
    }
    fs::create_dir(&destination)
        .map_err(|error| format!("benchmark output directory cannot be created: {error}"))?;
    set_dir_mode(&destination, 0o700)?;
    let guard = BundleGuard {
        path: destination.clone(),
        created: true,
    };
    let result_bytes = pretty_json(result)?;
    write_bundle_file(&destination, "result.json", result_bytes.as_bytes())?;
    write_bundle_file(
        &destination,
        "result.md",
        result_markdown(result).as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "report.json",
        pretty_json(&evidence.report)?.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "report.md",
        evidence.report_markdown.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "scenario/README.md",
        SCENARIO_README.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "scenario/expected.json",
        pretty_json(&expected_contract()?)?.as_bytes(),
    )?;
    write_bundle_file(
        &destination,
        "ledgers/blocked.jsonl",
        &evidence.blocked_ledger,
    )?;
    write_bundle_file(
        &destination,
        "ledgers/baseline.jsonl",
        &evidence.baseline_ledger,
    )?;
    write_bundle_file(&destination, "ledgers/final.jsonl", &evidence.final_ledger)?;
    write_bundle_file(
        &destination,
        &format!("fixtures/{}", config_name()),
        &evidence.config_bytes,
    )?;
    write_bundle_file(
        &destination,
        "fixtures/verification-invalid.json",
        &evidence.verification_invalid_bytes,
    )?;
    write_bundle_file(
        &destination,
        "fixtures/verification-valid.json",
        &evidence.verification_valid_bytes,
    )?;
    for file in &evidence.artifact_files {
        write_bundle_file(&destination, &file.bundle_relative, &file.bytes)?;
    }
    for (name, schema) in [
        ("run-event-v3.schema.json", RUN_EVENT_V3_SCHEMA),
        ("run-event-v4.schema.json", RUN_EVENT_V4_SCHEMA),
        ("value-report-v1.schema.json", VALUE_REPORT_V1_SCHEMA),
        ("value-proof-v1.schema.json", VALUE_PROOF_V1_SCHEMA),
    ] {
        write_bundle_file(&destination, &format!("schema/{name}"), schema.as_bytes())?;
    }
    if exitbind_surface() {
        write_bundle_file(
            &destination,
            "schema/run-event-v5.schema.json",
            RUN_EVENT_V5_SCHEMA.as_bytes(),
        )?;
    }

    let manifest = hash_manifest(&destination)?;
    write_bundle_file(
        &destination,
        "hash-manifest.json",
        pretty_json(&manifest)?.as_bytes(),
    )?;
    std::mem::forget(guard);
    Ok(())
}

fn write_bundle_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        ensure_bundle_directory(root, parent)?;
    }
    write_file(path, bytes, "proof bundle file")
}

fn ensure_bundle_directory(root: &Path, directory: &Path) -> Result<(), String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "proof bundle path escapes bundle root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("proof bundle path is not a regular directory".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    format!("proof bundle directory cannot be created: {error}")
                })?;
                set_dir_mode(&current, 0o700)?;
            }
            Err(error) => return Err(format!("proof bundle directory cannot be read: {error}")),
        }
    }
    Ok(())
}

fn hash_manifest(root: &Path) -> Result<Value, String> {
    let mut files = Vec::new();
    collect_bundle_files(root, root, &mut files)?;
    files.sort_by(|left, right| {
        left["path"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["path"].as_str().unwrap_or_default())
    });
    Ok(json!({
        "version": 1,
        "files": files,
        "self": "excluded",
    }))
}

fn collect_bundle_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<Value>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("proof bundle cannot be read: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("proof bundle cannot be read: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("proof bundle entry cannot be read: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("proof bundle must not contain symlinks".into());
        }
        if metadata.is_dir() {
            collect_bundle_files(root, &path, files)?;
        } else if metadata.is_file()
            && path.file_name().and_then(OsStr::to_str) != Some("hash-manifest.json")
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "proof bundle entry escapes root")?
                .to_str()
                .ok_or("proof bundle entry is not valid UTF-8")?
                .replace('\\', "/");
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            files.push(json!({
                "path": relative,
                "sha256": sha256(&bytes),
                "bytes": bytes.len(),
            }));
        }
    }
    Ok(())
}

fn result_markdown(result: &Value) -> String {
    let assertions = result["assertions"].as_array().map_or(0, Vec::len);
    format!(
        "# False-completion v1\n\nClaim: {}\n\nOrigin: synthetic\n\nAssertions passed: {assertions}\n\nManual JSON edits: {}\nAutomated elapsed time (ms): {}\nHuman interaction time (ms): unmeasured\n\nThis synthetic result does not establish recurring user losses, avoided harm, novice success, market frequency, model compliance, or human time saved.\n",
        result["claim"],
        result["manualJsonEdits"],
        result["automatedElapsedMs"],
    )
}

fn pretty_json(value: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|value| format!("{value}\n"))
        .map_err(|error| format!("proof JSON cannot be serialized: {error}"))
}

fn set_file_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn set_dir_mode(path: &Path, mode: u32) -> Result<(), String> {
    set_file_mode(path, mode)
}

use super::*;

pub(super) fn execute_scenario(
    invoker: &mut Invoker,
    fixture: &Path,
) -> Result<ScenarioEvidence, String> {
    expect_success(
        invoker,
        fixture,
        args(["init", "--mode", "portable", "--root", "."]),
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

pub(super) fn run_legacy_baseline(invoker: &mut Invoker, fixture: &Path) -> Result<(), String> {
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

pub(super) fn next_assignment(
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

pub(super) fn require_assignment(
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

pub(super) fn submit(
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

pub(super) fn record_check(
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

pub(super) fn inspect(
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

pub(super) fn expect_success<I, S>(
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

pub(super) fn expect_failure<I, S>(
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

pub(super) fn event_from_submit(value: &Value, label: &str) -> Result<Value, String> {
    value
        .get("event")
        .filter(|event| event.is_object())
        .cloned()
        .ok_or_else(|| format!("{label} did not return an event"))
}

pub(super) fn require_submission_event(
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
            event["version"] != crate::compatibility::profile().format_version
        } else {
            !matches!(event["version"].as_u64(), Some(3 | 4))
        }
    {
        return Err(format!("{label} returned an unexpected event"));
    }
    Ok(())
}

pub(super) fn require_check_event(
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

use serde_json::Value;
use sha2::{Digest, Sha256};
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CHECK: &str = "soulmate check --config verification.json";

fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let output = invoke(&root, &["init", "--root", "."]);
    assert!(output.status.success(), "{}", text(&output));
    root
}

fn invoke(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn invoke_owned(root: &Path, arguments: Vec<String>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; output: {}", text(output)))
}

fn state_artifact(root: &Path, name: &str, content: &str) -> String {
    let path = root.join(".soulmate/artifacts").join(name);
    fs::write(path, content).unwrap();
    format!(".soulmate/artifacts/{name}")
}

fn submit(root: &Path, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Value {
    let output = invoke(
        root,
        &[
            "run",
            "submit",
            agent,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            artifact,
            "--artifact-root",
            "state",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    json_output(&output)
}

fn checked_start(root: &Path, ledger: &str, origin: &str) -> Value {
    let output = invoke(
        root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "checked test",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            origin,
            "--config",
            "soulmate.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    json_output(&output)
}

fn record_check(root: &Path, ledger: &str, target: &str, exit_code: &str) -> Output {
    record_check_duration(root, ledger, target, exit_code, None)
}

fn record_check_duration(
    root: &Path,
    ledger: &str,
    target: &str,
    exit_code: &str,
    duration_ms: Option<&str>,
) -> Output {
    let mut arguments = vec![
        "run".into(),
        "record-check".into(),
        ledger.into(),
        "--target".into(),
        target.into(),
        "--check-command".into(),
        CHECK.into(),
        "--exit-code".into(),
        exit_code.into(),
    ];
    if let Some(duration_ms) = duration_ms {
        arguments.push("--duration-ms".into());
        arguments.push(duration_ms.into());
    }
    arguments.extend(["--json".into(), "--config".into(), "soulmate.json".into()]);
    invoke_owned(root, arguments)
}

fn event_hash(submission: &Value) -> String {
    submission["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn rehash(mut event: Value) -> Value {
    event.as_object_mut().unwrap().remove("eventSha256");
    let hash = digest(&event);
    event["eventSha256"] = serde_json::json!(hash);
    event
}

fn read_events(root: &Path, ledger: &str) -> Vec<Value> {
    fs::read_to_string(root.join(ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn write_events(root: &Path, ledger: &str, events: &[Value]) {
    let contents = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.join(ledger), format!("{contents}\n")).unwrap();
}

fn configure_workers(root: &Path, workers: &[&str]) {
    let config_path = root.join("soulmate.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let base = config["agents"]["worker"].clone();
    for worker in workers.iter().copied().filter(|worker| *worker != "worker") {
        let mut agent = base.clone();
        agent["profile"] = serde_json::json!(format!("soulmate/agents/{worker}.md"));
        agent["purpose"] = serde_json::json!(format!("Complete bounded work for {worker}."));
        config["agents"][worker] = agent;
        fs::write(
            root.join(format!("soulmate/agents/{worker}.md")),
            format!("# {worker}\n\nComplete bounded work.\n"),
        )
        .unwrap();
    }
    config["workflows"]["change"]["workers"] = serde_json::json!(workers);
    fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

fn split_worker_stages(root: &Path, ledger: &str) {
    let mut events = read_events(root, ledger);
    let mut start = events[0].clone();
    let stages = start["plan"]["stages"].as_array().unwrap().clone();
    assert_eq!(stages.len(), 4);
    let worker_agents = stages[1]["agents"].as_array().unwrap().clone();
    assert_eq!(worker_agents.len(), 3);
    let mut first_worker = stages[1].clone();
    first_worker["stage"] = serde_json::json!(2);
    first_worker["agents"] = serde_json::json!([worker_agents[0].clone()]);
    first_worker["dependsOn"] = serde_json::json!([1]);
    let mut later_workers = stages[1].clone();
    later_workers["stage"] = serde_json::json!(3);
    later_workers["agents"] =
        serde_json::json!([worker_agents[1].clone(), worker_agents[2].clone()]);
    later_workers["dependsOn"] = serde_json::json!([2]);
    let mut reviewer = stages[2].clone();
    reviewer["stage"] = serde_json::json!(4);
    reviewer["dependsOn"] = serde_json::json!([3]);
    let mut final_lead = stages[3].clone();
    final_lead["stage"] = serde_json::json!(5);
    final_lead["dependsOn"] = serde_json::json!([4]);
    start["plan"]["stages"] = serde_json::json!([
        stages[0].clone(),
        first_worker,
        later_workers,
        reviewer,
        final_lead
    ]);
    events[0] = rehash(start);
    write_events(root, ledger, &events);
}

#[test]
fn checked_packets_and_guard_keep_missing_and_passing_distinct() {
    let root = project("value-proof-guard");
    let ledger = ".soulmate/runs/checked.jsonl";
    let started = checked_start(&root, ledger, "local_report");
    assert_eq!(started["assignments"][0]["checkPolicy"]["command"], CHECK);
    assert_eq!(
        started["assignments"][0]["checkPolicy"]["origin"],
        "local_report"
    );

    let next = json_output(&invoke(
        &root,
        &["run", "next", ledger, "--json", "--config", "soulmate.json"],
    ));
    assert_eq!(next["assignments"][0]["checkPolicy"]["command"], CHECK);
    assert_eq!(next["assignments"][0]["checkPolicy"]["version"], 1);

    let lead = state_artifact(&root, "guard-lead.md", "lead scope\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "guard-worker.md", "worker completion\n");
    let worker_submission = submit(&root, "worker", ledger, "completed", &worker);
    let worker_event = event_hash(&worker_submission);
    let reviewer = state_artifact(&root, "guard-reviewer.md", "reviewer approval\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);

    let before_check = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(before_check["checks"]["status"], "not_observed");
    assert_eq!(before_check["checks"]["targetCount"], 1);
    assert_eq!(before_check["checks"]["observedCount"], 0);
    assert_eq!(before_check["checks"]["missingCount"], 1);
    let human_status = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(human_status.status.success(), "{}", text(&human_status));
    let human_status_text = String::from_utf8_lossy(&human_status.stdout);
    for expected in [
        "Claim:",
        "Worker claim: worker (worker) stage 2 outcome=completed",
        "Checks:",
        "Host-reported check: not observed; not executed by Soulmate",
        "Frozen command: soulmate check --config verification.json",
        "Review:",
        "Reviewer outcome: reviewer (reviewer) stage 3 outcome=approved",
        "Acceptance:",
        "Lead decision: pending",
        &worker_event,
    ] {
        assert!(human_status_text.contains(expected), "missing {expected}");
    }
    assert!(human_status_text.contains("actual result"));

    let lead_accept = state_artifact(&root, "guard-lead-accept.md", "lead acceptance\n");
    let refused = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead_accept,
            "--artifact-root",
            "state",
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    let blocked: Vec<Value> = fs::read_to_string(root.join(ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(blocked.last().unwrap()["action"], "protect");
    assert_eq!(blocked.last().unwrap()["reason"], "check_missing");
    assert!(blocked
        .iter()
        .all(|event| !(event["action"] == "submit" && event["outcome"] == "accepted")));
    let refused_status = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(refused_status.status.success(), "{}", text(&refused_status));
    let refused_status_text = String::from_utf8_lossy(&refused_status.stdout);
    assert!(refused_status_text.contains("Protocol refusal:"));
    assert!(refused_status_text.contains("protocol refusal is not a lead rejection"));
    let protection_hash = blocked.last().unwrap()["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let explanation = invoke_owned(
        &root,
        vec![
            "run".into(),
            "explain".into(),
            ledger.into(),
            "--event".into(),
            protection_hash,
            "--config".into(),
            "soulmate.json".into(),
        ],
    );
    assert!(explanation.status.success(), "{}", text(&explanation));
    let explanation_text = String::from_utf8_lossy(&explanation.stdout);
    assert!(explanation_text.contains("Protection:"));
    assert!(explanation_text.contains("reason=check_missing"));
    assert!(
        explanation_text.contains("Host-reported check: not observed; not executed by Soulmate")
    );
    assert!(explanation_text.contains(&worker_event));

    let passing = record_check(&root, ledger, &worker_event, "0");
    assert!(passing.status.success(), "{}", text(&passing));
    assert_eq!(json_output(&passing)["checks"]["status"], "passed");
    let still_running = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(still_running["status"], "running");
    assert_eq!(still_running["checks"]["status"], "passed");
    assert_eq!(still_running["checks"]["observedCount"], 1);
    let accepted = submit(&root, "lead", ledger, "accepted", &lead_accept);
    assert_eq!(accepted["status"], "accepted");
    let terminal_human = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(terminal_human.status.success(), "{}", text(&terminal_human));
    let terminal_text = String::from_utf8_lossy(&terminal_human.stdout);
    assert!(terminal_text.contains("Lead decision: accepted"));
    assert!(terminal_text.contains("Host-reported check: passed; not executed by Soulmate"));

    let report = json_output(&invoke(
        &root,
        &[
            "run",
            "report",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(report["groups"]["local_report"]["runs"], 1);
    assert_eq!(report["groups"]["local_report"]["checks"], 1);
    assert_eq!(report["groups"]["local_report"]["protections"], 1);
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains(CHECK));
    assert!(!serialized.contains("checked test"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn human_status_keeps_rework_history_and_prior_protection_visible() {
    let root = project("value-proof-human-rework");
    let ledger = ".soulmate/runs/rework.jsonl";
    checked_start(&root, ledger, "local_report");

    let scoped = state_artifact(&root, "rework-scope.md", "scope\n");
    submit(&root, "lead", ledger, "scoped", &scoped);
    let worker = state_artifact(&root, "rework-worker.md", "first completion\n");
    submit(&root, "worker", ledger, "completed", &worker);
    let reviewer = state_artifact(&root, "rework-reviewer.md", "approval\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);

    let accepted = state_artifact(&root, "rework-accepted.md", "acceptance\n");
    let refused = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &accepted,
            "--artifact-root",
            "state",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!refused.status.success(), "{}", text(&refused));

    let repair = state_artifact(&root, "rework-repair.md", "repair request\n");
    submit(&root, "lead", ledger, "rework", &repair);
    let before = fs::read(root.join(ledger)).unwrap();
    let status = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(status.status.success(), "{}", text(&status));
    let rendered = String::from_utf8_lossy(&status.stdout);
    for expected in [
        "Lead decision: pending",
        "History: prior attempts remain historical",
        "Prior attempt 1:",
        "Protocol refusal: attempt=1 state=prior",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
    let explanation = invoke(
        &root,
        &["run", "explain", ledger, "--config", "soulmate.json"],
    );
    assert!(explanation.status.success(), "{}", text(&explanation));
    let explanation_text = String::from_utf8_lossy(&explanation.stdout);
    assert!(explanation_text.contains("Lead decision: pending"));
    assert!(explanation_text.contains("History: prior attempts remain historical"));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn human_status_and_explain_render_terminal_lead_decisions() {
    for outcome in ["rejected", "blocked"] {
        let label = format!("value-proof-human-lead-{outcome}");
        let root = project(&label);
        let ledger = format!(".soulmate/runs/{outcome}.jsonl");
        checked_start(&root, &ledger, "local_report");

        let scoped = state_artifact(&root, &format!("{outcome}-scope.md"), "scope\n");
        submit(&root, "lead", &ledger, "scoped", &scoped);
        let worker = state_artifact(&root, &format!("{outcome}-worker.md"), "work\n");
        submit(&root, "worker", &ledger, "completed", &worker);
        let reviewer = state_artifact(&root, &format!("{outcome}-reviewer.md"), "review\n");
        submit(&root, "reviewer", &ledger, "approved", &reviewer);
        let decision = state_artifact(&root, &format!("{outcome}-decision.md"), "decision\n");
        let terminal = submit(&root, "lead", &ledger, outcome, &decision);
        assert_eq!(terminal["status"], outcome);

        let before = fs::read(root.join(&ledger)).unwrap();
        let status = invoke(
            &root,
            &["run", "status", &ledger, "--config", "soulmate.json"],
        );
        assert!(status.status.success(), "{}", text(&status));
        let status_text = String::from_utf8_lossy(&status.stdout);
        assert!(status_text.contains(&format!("Lead decision: {outcome}")));

        let explanation = invoke(
            &root,
            &["run", "explain", &ledger, "--config", "soulmate.json"],
        );
        assert!(explanation.status.success(), "{}", text(&explanation));
        let explanation_text = String::from_utf8_lossy(&explanation.stdout);
        assert!(explanation_text.contains(&format!("Lead decision: {outcome}")));
        assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn stale_and_cross_run_check_targets_fail_without_append() {
    let root = project("value-proof-targets");
    let first = ".soulmate/runs/first.jsonl";
    let second = ".soulmate/runs/second.jsonl";
    checked_start(&root, first, "local_report");
    checked_start(&root, second, "local_report");

    let first_lead = state_artifact(&root, "first-lead.md", "lead\n");
    submit(&root, "lead", first, "scoped", &first_lead);
    let second_lead = state_artifact(&root, "second-lead.md", "lead\n");
    submit(&root, "lead", second, "scoped", &second_lead);
    let first_worker = state_artifact(&root, "first-worker.md", "first\n");
    let first_submission = submit(&root, "worker", first, "completed", &first_worker);
    let first_target = event_hash(&first_submission);
    let second_worker = state_artifact(&root, "second-worker.md", "second\n");
    let second_submission = submit(&root, "worker", second, "completed", &second_worker);
    let second_target = event_hash(&second_submission);

    let before = fs::read(root.join(first)).unwrap();
    let stale = record_check(
        &root,
        first,
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0",
    );
    assert!(!stale.status.success(), "{}", text(&stale));
    assert_eq!(fs::read(root.join(first)).unwrap(), before);

    let cross_run = record_check(&root, first, &second_target, "0");
    assert!(!cross_run.status.success(), "{}", text(&cross_run));
    assert_eq!(fs::read(root.join(first)).unwrap(), before);
    assert_ne!(first_target, second_target);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn status_reports_artifact_drift_without_appending() {
    let root = project("value-proof-drift");
    let ledger = ".soulmate/runs/checked.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "drift-lead.md", "original\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let before = fs::read(root.join(ledger)).unwrap();
    fs::write(root.join(&lead), "substituted\n").unwrap();

    let status = invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(status.status.success(), "{}", text(&status));
    let value = json_output(&status);
    assert_eq!(value["artifact"]["status"], "drifted");
    assert_eq!(value["status"], "running");
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_report_keeps_missing_origin_unclassified() {
    let root = project("value-proof-legacy-report");
    let ledger = ".soulmate/runs/legacy.jsonl";
    let output = invoke(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "legacy report",
            "--ledger",
            ledger,
            "--config",
            "soulmate.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let synthetic = ".soulmate/runs/synthetic.jsonl";
    checked_start(&root, synthetic, "synthetic");
    let local = ".soulmate/runs/local.jsonl";
    checked_start(&root, local, "local_report");
    let report = json_output(&invoke(
        &root,
        &[
            "run",
            "report",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(report["groups"]["unclassified"]["runs"], 1);
    assert_eq!(report["groups"]["local_report"]["runs"], 0);
    assert_eq!(report["groups"]["synthetic"]["runs"], 0);
    let mixed = json_output(&invoke_owned(
        &root,
        vec![
            "run".into(),
            "report".into(),
            ledger.into(),
            synthetic.into(),
            local.into(),
            "--json".into(),
            "--config".into(),
            "soulmate.json".into(),
        ],
    ));
    assert_eq!(mixed["groups"]["unclassified"]["runs"], 1);
    assert_eq!(mixed["groups"]["synthetic"]["runs"], 1);
    assert_eq!(mixed["groups"]["local_report"]["runs"], 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn report_rejects_duration_overflow_instead_of_saturating() {
    let root = project("value-proof-overflow");
    let ledger = ".soulmate/runs/overflow.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "overflow-lead.md", "lead\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "overflow-worker.md", "worker\n");
    let worker_submission = submit(&root, "worker", ledger, "completed", &worker);
    let target = event_hash(&worker_submission);
    let max = u64::MAX.to_string();
    for _ in 0..2 {
        let recorded = record_check_duration(&root, ledger, &target, "0", Some(&max));
        assert!(recorded.status.success(), "{}", text(&recorded));
    }
    let report = invoke(
        &root,
        &[
            "run",
            "report",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!report.status.success(), "{}", text(&report));
    assert!(text(&report).contains("report metric overflow"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn human_status_escapes_frozen_check_command_without_appending() {
    let root = project("value-proof-human-escaping");
    let ledger = ".soulmate/runs/escaping.jsonl";
    let command = "printf \u{1b}[31mcheck";
    let started = invoke_owned(
        &root,
        vec![
            "run".into(),
            "start".into(),
            "change".into(),
            "--goal".into(),
            "escaped command".into(),
            "--ledger".into(),
            ledger.into(),
            "--check-command".into(),
            command.into(),
            "--config".into(),
            "soulmate.json".into(),
        ],
    );
    assert!(started.status.success(), "{}", text(&started));
    let before = fs::read(root.join(ledger)).unwrap();
    let status = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(status.status.success(), "{}", text(&status));
    let rendered = String::from_utf8_lossy(&status.stdout);
    assert!(rendered.contains("Host-reported check: not observed; not executed by Soulmate"));
    assert!(rendered.contains("Frozen command: printf"));
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_guard_covers_all_worker_stages_and_parallel_workers() {
    let root = project("value-proof-all-workers");
    configure_workers(&root, &["worker", "worker_two", "worker_three"]);
    let ledger = ".soulmate/runs/all-workers.jsonl";
    checked_start(&root, ledger, "local_report");
    split_worker_stages(&root, ledger);

    let lead = state_artifact(&root, "all-workers-lead.md", "lead\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker_one = state_artifact(&root, "all-workers-one.md", "one\n");
    let first = submit(&root, "worker", ledger, "completed", &worker_one);
    let first_target = event_hash(&first);

    let next = json_output(&invoke(
        &root,
        &["run", "next", ledger, "--json", "--config", "soulmate.json"],
    ));
    assert_eq!(next["assignments"].as_array().unwrap().len(), 2);
    assert!(next["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .any(|assignment| assignment["agent"] == "worker_two"));
    let worker_two = state_artifact(&root, "all-workers-two.md", "two\n");
    let second = submit(&root, "worker_two", ledger, "completed", &worker_two);
    let second_target = event_hash(&second);
    let worker_three = state_artifact(&root, "all-workers-three.md", "three\n");
    let third = submit(&root, "worker_three", ledger, "completed", &worker_three);
    let third_target = event_hash(&third);

    let reviewer = state_artifact(&root, "all-workers-reviewer.md", "reviewer\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);
    let before_checks = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(before_checks["checks"]["targetCount"], 3);
    assert_eq!(before_checks["checks"]["observedCount"], 0);
    assert_eq!(before_checks["checks"]["missingCount"], 3);
    assert_eq!(before_checks["checks"]["status"], "not_observed");
    let human_before = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(human_before.status.success(), "{}", text(&human_before));
    let human_before_text = String::from_utf8_lossy(&human_before.stdout);
    for worker in ["worker", "worker_two", "worker_three"] {
        assert!(human_before_text.contains(&format!("Worker claim: {worker}")));
    }
    assert!(human_before_text.contains("reported exit=missing"));

    assert!(record_check(&root, ledger, &first_target, "1")
        .status
        .success());
    let mixed = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(mixed.status.success(), "{}", text(&mixed));
    let mixed_text = String::from_utf8_lossy(&mixed.stdout);
    assert!(mixed_text.contains(&format!(
        "Check target: worker=worker (worker) stage 2 event={first_target} status=failed reported exit=1"
    )));
    assert!(mixed_text.contains("worker_two (worker_two) stage 3"));
    assert!(mixed_text.contains("reported exit=missing"));

    assert!(record_check(&root, ledger, &first_target, "0")
        .status
        .success());
    let one_check = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(one_check["checks"]["observedCount"], 1);
    assert_eq!(one_check["checks"]["missingCount"], 2);
    assert_eq!(one_check["checks"]["status"], "not_observed");
    let human_one = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(human_one.status.success(), "{}", text(&human_one));
    let human_one_text = String::from_utf8_lossy(&human_one.stdout);
    assert!(human_one_text.contains(&format!(
        "Check target: worker=worker (worker) stage 2 event={first_target} status=passed reported exit=0"
    )));
    assert!(human_one_text.contains("worker_two (worker_two) stage 3"));
    assert!(human_one_text.contains("worker_three (worker_three) stage 3"));
    assert!(record_check(&root, ledger, &second_target, "0")
        .status
        .success());
    assert!(record_check(&root, ledger, &third_target, "0")
        .status
        .success());
    let complete = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(complete["checks"]["targetCount"], 3);
    assert_eq!(complete["checks"]["observedCount"], 3);
    assert_eq!(complete["checks"]["missingCount"], 0);
    assert_eq!(complete["checks"]["status"], "passed");
    let human_complete = invoke(
        &root,
        &["run", "status", ledger, "--config", "soulmate.json"],
    );
    assert!(human_complete.status.success(), "{}", text(&human_complete));
    let human_complete_text = String::from_utf8_lossy(&human_complete.stdout);
    assert!(human_complete_text.contains("Host-reported check: passed; not executed by Soulmate"));
    assert!(human_complete_text.contains("Reviewer outcome:"));
    assert!(human_complete_text.contains("outcome=approved"));
    assert!(human_complete_text.contains("Lead decision: pending"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_runs_reject_no_worker_plans_and_early_lead_acceptance() {
    let no_worker = project("value-proof-no-worker");
    configure_workers(&no_worker, &[]);
    let rejected = invoke(
        &no_worker,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "no worker",
            "--ledger",
            ".soulmate/runs/no-worker.jsonl",
            "--check-command",
            CHECK,
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert!(!no_worker.join(".soulmate/runs/no-worker.jsonl").exists());
    fs::remove_dir_all(no_worker).unwrap();

    let root = project("value-proof-early-acceptance");
    let ledger = ".soulmate/runs/early.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "early-lead.md", "lead\n");
    let before = fs::read(root.join(ledger)).unwrap();
    let accepted = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead,
            "--artifact-root",
            "state",
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!accepted.status.success(), "{}", text(&accepted));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    let status = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(status["checks"]["status"], "not_observed");
    assert_eq!(status["checks"]["targetCount"], 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn latest_check_result_controls_acceptance_and_failure_protection() {
    let root = project("value-proof-latest-check");
    let ledger = ".soulmate/runs/latest.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "latest-lead.md", "lead\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "latest-worker.md", "worker\n");
    let worker_submission = submit(&root, "worker", ledger, "completed", &worker);
    let target = event_hash(&worker_submission);
    let reviewer = state_artifact(&root, "latest-reviewer.md", "reviewer\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);

    assert!(record_check(&root, ledger, &target, "0").status.success());
    let passed = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(passed["checks"]["status"], "passed");
    assert!(record_check(&root, ledger, &target, "9").status.success());
    let failed = json_output(&invoke(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "soulmate.json",
        ],
    ));
    assert_eq!(failed["checks"]["status"], "blocked");
    assert_eq!(failed["checks"]["observedCount"], 1);
    assert_eq!(failed["checks"]["failedCount"], 1);
    assert_eq!(failed["checks"]["targets"][0]["exitCode"], 9);

    let accepted = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead,
            "--artifact-root",
            "state",
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!accepted.status.success(), "{}", text(&accepted));
    let events = read_events(&root, ledger);
    assert_eq!(events.last().unwrap()["action"], "protect");
    assert_eq!(events.last().unwrap()["reason"], "check_failed");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn artifact_drift_blocks_check_and_protection_without_appending() {
    let root = project("value-proof-drift-no-append");
    let ledger = ".soulmate/runs/drift-no-append.jsonl";
    checked_start(&root, ledger, "local_report");
    let lead = state_artifact(&root, "drift-no-append-lead.md", "lead\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "drift-no-append-worker.md", "worker\n");
    let worker_submission = submit(&root, "worker", ledger, "completed", &worker);
    let target = event_hash(&worker_submission);
    let reviewer = state_artifact(&root, "drift-no-append-reviewer.md", "reviewer\n");
    submit(&root, "reviewer", ledger, "approved", &reviewer);
    fs::write(root.join(&worker), "changed after submission\n").unwrap();
    let before = fs::read(root.join(ledger)).unwrap();

    let check = record_check(&root, ledger, &target, "0");
    assert!(!check.status.success(), "{}", text(&check));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    let accepted = invoke(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &lead,
            "--artifact-root",
            "state",
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!accepted.status.success(), "{}", text(&accepted));
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_supersession_inherits_policy_and_seals_predecessor() {
    let root = project("value-proof-supersede");
    let old = ".soulmate/runs/old.jsonl";
    let new = ".soulmate/runs/new.jsonl";
    checked_start(&root, old, "synthetic");
    let output = invoke(
        &root,
        &[
            "run",
            "supersede",
            old,
            "--workflow",
            "change",
            "--goal",
            "successor",
            "--ledger",
            new,
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let successor = json_output(&invoke(
        &root,
        &["run", "inspect", new, "--json", "--config", "soulmate.json"],
    ));
    assert_eq!(successor["events"][0]["version"], 4);
    assert_eq!(successor["events"][0]["checkPolicy"]["command"], CHECK);
    assert_eq!(successor["events"][0]["checkPolicy"]["origin"], "synthetic");
    assert!(root.join(".soulmate/runs/old.jsonl.supersede").is_file());

    let worker = state_artifact(&root, "sealed-worker.md", "worker\n");
    let before = fs::read(root.join(old)).unwrap();
    let mutation = invoke(
        &root,
        &[
            "run",
            "submit",
            "worker",
            old,
            "--outcome",
            "completed",
            "--artifact",
            &worker,
            "--artifact-root",
            "state",
            "--json",
            "--config",
            "soulmate.json",
        ],
    );
    assert!(!mutation.status.success(), "{}", text(&mutation));
    assert_eq!(fs::read(root.join(old)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

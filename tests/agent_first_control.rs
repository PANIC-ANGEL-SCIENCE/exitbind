#[path = "support/agent_first_surface.rs"]
mod agent_first_surface;
mod support;

use agent_first_surface::assert_no_raw_protocol_fields;
use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};

const EVALUATION_DOC: &str = include_str!("../docs/agent-first-evaluation.md");
const BLOCKED_RESULT_BYTES: usize = 2 * 1024 * 1024;

struct Fixture {
    root: PathBuf,
    trace: RefCell<Vec<CallTrace>>,
}

struct CallTrace {
    args: Vec<String>,
    success: bool,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("agent-first-control");
        let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Self {
            root,
            trace: RefCell::new(Vec::new()),
        }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_soulmate"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("soulmate.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command.spawn().unwrap();
        if let Some(input) = input {
            child.stdin.take().unwrap().write_all(input).unwrap();
        }
        let output = child.wait_with_output().unwrap();
        self.trace.borrow_mut().push(CallTrace {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            success: output.status.success(),
        });
        output
    }

    fn blocked_return(&self, work: &str, assignment: &str, outcome: &str) -> (Child, ChildStdin) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_soulmate"));
        command
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                work,
                assignment,
                "--outcome",
                outcome,
                "--config",
            ])
            .arg(self.root.join("soulmate.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        (child, stdin)
    }

    fn prime_return(mut stdin: ChildStdin) -> ChildStdin {
        // The payload exceeds the anonymous pipe buffer.  write_all can
        // finish only after the child has reached read_to_end; keeping stdin
        // open then leaves that read blocked until the caller releases it.
        stdin.write_all(&vec![b'x'; BLOCKED_RESULT_BYTES]).unwrap();
        stdin
    }

    fn release_return(child: Child, stdin: ChildStdin) -> Output {
        drop(stdin);
        child.wait_with_output().unwrap()
    }

    fn value(&self, args: &[&str], input: Option<&[u8]>) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{args:?}: {error}; {output:?}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct WorkflowMetrics {
    soulmate_calls: usize,
    raw_identifier_occurrences: usize,
    bookkeeping_inputs: usize,
    protocol_errors_or_retries: usize,
    operator_interventions: usize,
    human_protocol_transfers: usize,
}

fn metrics(fixture: &Fixture) -> WorkflowMetrics {
    let trace = fixture.trace.borrow();
    WorkflowMetrics {
        soulmate_calls: trace.len(),
        raw_identifier_occurrences: trace
            .iter()
            .map(|call| raw_identifier_occurrences(&call.args))
            .sum(),
        bookkeeping_inputs: trace
            .iter()
            .map(|call| bookkeeping_inputs(&call.args))
            .sum(),
        protocol_errors_or_retries: trace.iter().filter(|call| !call.success).count(),
        // These are explicitly scripted-harness observations, not inferred
        // from command traces.
        operator_interventions: 0,
        human_protocol_transfers: 0,
    }
}

fn raw_identifier_occurrences(args: &[String]) -> usize {
    args.iter().filter(|arg| is_ledger_path(arg)).count()
        + args
            .windows(2)
            .filter(|window| matches!(window[0].as_str(), "--artifact" | "--target"))
            .count()
}

fn bookkeeping_inputs(args: &[String]) -> usize {
    args.iter()
        .filter(|arg| {
            matches!(
                arg.as_str(),
                "--ledger" | "--artifact" | "--artifact-root" | "--target"
            )
        })
        .count()
}

fn is_ledger_path(value: &str) -> bool {
    value.starts_with(".soulmate/runs/") && value.ends_with(".jsonl")
}

fn write_state_artifact(fixture: &Fixture, name: &str, contents: &[u8]) -> String {
    let relative = format!(".soulmate/artifacts/{name}");
    fs::write(fixture.root.join(&relative), contents).unwrap();
    relative
}

fn low_level_submit(
    fixture: &Fixture,
    ledger: &str,
    agent: &str,
    outcome: &str,
    artifact: &str,
) -> Value {
    fixture.value(
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
        ],
        None,
    )
}

fn event_trace(path: &std::path::Path) -> Vec<(String, String, String)> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| {
            let event: Value = serde_json::from_str(line).unwrap();
            (
                event["action"].as_str().unwrap_or("").to_owned(),
                event["role"].as_str().unwrap_or("").to_owned(),
                event["outcome"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

#[test]
fn checked_facade_hides_protocol_transport_and_preserves_core_binding() {
    let fixture = Fixture::new();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "facade task",
            "--check-command",
            "test -f marker",
        ],
        None,
    );
    assert_no_raw_protocol_fields(&begin);
    let work = begin["work"].as_str().unwrap().to_owned();
    let assignment = begin["next"]["assignment"].as_str().unwrap().to_owned();
    assert!(work.starts_with("smw_"));
    let lead_return = fixture.value(
        &["work", "return", &work, &assignment, "--outcome", "scoped"],
        Some(b"lead result"),
    );
    assert_no_raw_protocol_fields(&lead_return);
    let worker = fixture.value(&["work", "next", &work], None);
    assert_no_raw_protocol_fields(&worker);
    let worker_assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let worker_return = fixture.value(
        &[
            "work",
            "return",
            &work,
            &worker_assignment,
            "--outcome",
            "completed",
        ],
        Some(b"worker result"),
    );
    assert_no_raw_protocol_fields(&worker_return);
    let check_pending = fixture.value(&["work", "next", &work], None);
    assert_no_raw_protocol_fields(&check_pending);
    assert_eq!(check_pending["next"]["action"], "check");
    std::fs::write(fixture.root.join("marker"), b"ok").unwrap();
    let checked = fixture.value(&["work", "check", &work], None);
    assert_no_raw_protocol_fields(&checked);
    assert_eq!(checked["next"]["action"], "spawn");
    let reviewer = checked["next"]["assignment"].as_str().unwrap().to_owned();
    let reviewer_return = fixture.value(
        &["work", "return", &work, &reviewer, "--outcome", "approved"],
        Some(b"review result"),
    );
    assert_no_raw_protocol_fields(&reviewer_return);
    let lead = fixture.value(&["work", "next", &work], None);
    assert_no_raw_protocol_fields(&lead);
    let lead_assignment = lead["next"]["assignment"].as_str().unwrap().to_owned();
    let done = fixture.value(
        &[
            "work",
            "return",
            &work,
            &lead_assignment,
            "--outcome",
            "accepted",
        ],
        Some(b"accepted result"),
    );
    assert_no_raw_protocol_fields(&done);
    assert_eq!(done["next"]["action"], "done");

    let ledgers = std::fs::read_dir(fixture.root.join(".soulmate/runs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(ledgers.len(), 1);
    assert!(ledgers[0].is_file());
    let ledger = ledgers[0]
        .strip_prefix(&fixture.root)
        .unwrap()
        .to_str()
        .unwrap();
    let advanced = fixture.value(&["run", "inspect", ledger], None);
    assert_eq!(advanced["events"].as_array().unwrap().len(), 6);
}

#[test]
fn resume_is_explicit_for_zero_and_multiple_active_work_items() {
    let fixture = Fixture::new();
    let none = fixture.value(&["work", "resume"], None);
    assert_no_raw_protocol_fields(&none);
    assert_eq!(none["status"], "none");
    let first_begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "one",
            "--check-command",
            "true",
        ],
        None,
    );
    assert_no_raw_protocol_fields(&first_begin);
    let one = fixture.value(&["work", "resume"], None);
    assert_no_raw_protocol_fields(&one);
    assert_eq!(one["status"], "resumed");
    assert_eq!(one["work"], first_begin["work"]);
    assert_eq!(one["next"]["action"], "lead_decision");
    let second_begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "two",
            "--check-command",
            "true",
        ],
        None,
    );
    assert_no_raw_protocol_fields(&second_begin);
    // The focus written by the second begin selects it; the first stays
    // history rather than making resume ambiguous.
    let resumed = fixture.value(&["work", "resume"], None);
    assert_no_raw_protocol_fields(&resumed);
    assert_eq!(resumed["status"], "resumed");
    assert_eq!(resumed["work"], second_begin["work"]);
    assert_eq!(resumed["history"]["running"], 1);
    // A legacy project without a focus keeps the explicit ambiguous answer.
    let removed = [".exitbind", ".soulmate"]
        .iter()
        .filter(|state| {
            std::fs::remove_file(fixture.root.join(state).join("current-work.json")).is_ok()
        })
        .count();
    assert_eq!(removed, 1);
    let resumed = fixture.value(&["work", "resume"], None);
    assert_no_raw_protocol_fields(&resumed);
    assert_eq!(resumed["status"], "ambiguous");
    assert_eq!(resumed["works"].as_array().unwrap().len(), 2);
    assert!(resumed["next"].is_null());
}

#[test]
fn stale_blocked_work_return_cannot_submit_a_fresh_attempt() {
    let fixture = Fixture::new();
    let begin = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "stale return",
            "--check-command",
            "true",
        ],
        None,
    );
    let work = begin["work"].as_str().unwrap().to_owned();
    let _scoped = fixture.value(
        &[
            "work",
            "return",
            &work,
            begin["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    );
    let worker = fixture.value(&["work", "next", &work], None);
    let worker_assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let (first, first_stdin) = fixture.blocked_return(&work, &worker_assignment, "completed");
    let (second, second_stdin) = fixture.blocked_return(&work, &worker_assignment, "completed");
    let first_stdin = Fixture::prime_return(first_stdin);
    let second_stdin = Fixture::prime_return(second_stdin);

    let first_result = Fixture::release_return(first, first_stdin);
    assert!(first_result.status.success(), "{first_result:?}");
    let first_result: Value = serde_json::from_slice(&first_result.stdout).unwrap();
    assert_eq!(first_result["next"]["action"], "check");

    let checked = fixture.value(&["work", "check", &work], None);
    let reviewer_assignment = checked["next"]["assignment"].as_str().unwrap().to_owned();
    let reworked = fixture.value(
        &[
            "work",
            "return",
            &work,
            &reviewer_assignment,
            "--outcome",
            "rework",
        ],
        Some(b"review requests rework"),
    );
    assert_eq!(reworked["next"]["action"], "spawn");
    assert_eq!(reworked["next"]["requiresExpansion"], true);
    assert!(reworked["next"]["packet"].is_null());

    let artifact_dir = fixture.root.join(".soulmate/artifacts");
    let artifacts_before = fs::read_dir(&artifact_dir).unwrap().count();
    let ledger_path = fs::read_dir(fixture.root.join(".soulmate/runs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_file())
        .unwrap();
    let events_before = fs::read(&ledger_path).unwrap();

    let stale = Fixture::release_return(second, second_stdin);
    assert!(!stale.status.success(), "{stale:?}");
    let error = String::from_utf8_lossy(&stale.stderr);
    assert!(error.contains("assignment changed while work result was being read"));
    assert_eq!(fs::read(&ledger_path).unwrap(), events_before);
    assert_eq!(
        fs::read_dir(&artifact_dir).unwrap().count(),
        artifacts_before
    );

    let fresh = fixture.value(&["work", "next", &work], None);
    assert_eq!(fresh["next"]["action"], "spawn");
    assert_eq!(fresh["next"]["packet"]["attempt"], 2);
    let events = fs::read_to_string(ledger_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(!events
        .iter()
        .any(|event| { event["action"] == "submit" && event["attempt"] == 2 }));
}

#[test]
fn matched_workflows_measure_protocol_transport_without_product_overclaim() {
    let low_level = Fixture::new();
    let ledger = ".soulmate/runs/matched-low-level.jsonl";
    low_level.value(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "matched checked task",
            "--ledger",
            ledger,
            "--check-command",
            "test -f marker",
        ],
        None,
    );
    let lead_scope = write_state_artifact(&low_level, "matched-lead-scope.md", b"scope");
    low_level_submit(&low_level, ledger, "lead", "scoped", &lead_scope);
    let worker = write_state_artifact(&low_level, "matched-worker.md", b"worker result");
    let worker_result = low_level_submit(&low_level, ledger, "worker", "completed", &worker);
    let worker_event = worker_result["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::write(low_level.root.join("marker"), b"ok").unwrap();
    low_level.value(
        &["run", "observe-check", ledger, "--target", &worker_event],
        None,
    );
    let reviewer = write_state_artifact(&low_level, "matched-reviewer.md", b"approved");
    low_level_submit(&low_level, ledger, "reviewer", "approved", &reviewer);
    let acceptance = write_state_artifact(&low_level, "matched-acceptance.md", b"accepted");
    let low_level_done = low_level_submit(&low_level, ledger, "lead", "accepted", &acceptance);
    assert_eq!(low_level_done["status"], "accepted");

    let facade = Fixture::new();
    let begin = facade.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "matched checked task",
            "--check-command",
            "test -f marker",
        ],
        None,
    );
    assert_no_raw_protocol_fields(&begin);
    let work = begin["work"].as_str().unwrap().to_owned();
    let mut next = begin["next"].clone();
    next = facade.value(
        &[
            "work",
            "return",
            &work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        Some(b"scope"),
    )["next"]
        .clone();
    next = facade.value(
        &[
            "work",
            "return",
            &work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        Some(b"worker result"),
    )["next"]
        .clone();
    assert_eq!(next["action"], "check");
    fs::write(facade.root.join("marker"), b"ok").unwrap();
    next = facade.value(&["work", "check", &work], None)["next"].clone();
    next = facade.value(
        &[
            "work",
            "return",
            &work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        Some(b"approved"),
    )["next"]
        .clone();
    let facade_done = facade.value(
        &[
            "work",
            "return",
            &work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accepted"),
    );
    assert_no_raw_protocol_fields(&facade_done);
    assert_eq!(facade_done["next"]["action"], "done");

    let facade_ledger = fs::read_dir(facade.root.join(".soulmate/runs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_file())
        .unwrap();
    assert_eq!(
        event_trace(&low_level.root.join(ledger)),
        event_trace(&facade_ledger)
    );

    let baseline = metrics(&low_level);
    let candidate = metrics(&facade);
    let expected_baseline = WorkflowMetrics {
        soulmate_calls: 6,
        raw_identifier_occurrences: 11,
        bookkeeping_inputs: 10,
        protocol_errors_or_retries: 0,
        operator_interventions: 0,
        human_protocol_transfers: 0,
    };
    let expected_candidate = WorkflowMetrics {
        soulmate_calls: 6,
        raw_identifier_occurrences: 0,
        bookkeeping_inputs: 0,
        protocol_errors_or_retries: 0,
        operator_interventions: 0,
        human_protocol_transfers: 0,
    };
    assert_eq!(baseline, expected_baseline);
    assert_eq!(candidate, expected_candidate);
    assert_eq!(baseline.soulmate_calls, candidate.soulmate_calls);
    assert!(baseline.raw_identifier_occurrences > candidate.raw_identifier_occurrences);
    assert!(baseline.bookkeeping_inputs > candidate.bookkeeping_inputs);
    for (label, baseline_value, candidate_value) in [
        (
            "Exitbind process calls",
            baseline.soulmate_calls,
            candidate.soulmate_calls,
        ),
        (
            "Raw protocol identifier occurrences in command inputs",
            baseline.raw_identifier_occurrences,
            candidate.raw_identifier_occurrences,
        ),
        (
            "Caller-selected bookkeeping inputs",
            baseline.bookkeeping_inputs,
            candidate.bookkeeping_inputs,
        ),
        (
            "Observed protocol errors or retries",
            baseline.protocol_errors_or_retries,
            candidate.protocol_errors_or_retries,
        ),
        (
            "Operator interventions",
            baseline.operator_interventions,
            candidate.operator_interventions,
        ),
        (
            "Human protocol-state transfers",
            baseline.human_protocol_transfers,
            candidate.human_protocol_transfers,
        ),
    ] {
        let row = format!("| {label} | {baseline_value} | {candidate_value} |");
        assert!(EVALUATION_DOC.contains(&row), "missing {row}");
    }
}

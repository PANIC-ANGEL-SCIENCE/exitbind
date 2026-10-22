//! R55 F7: for every shape of observed check the governor can answer, whether
//! the governor appends new information, and which event it appends.
//!
//! The rows are the cartesian product of the check's admitted vocabularies
//! (protocol marker, acquisition, origin, result kind, subject, governor
//! phase). The self-tests read those vocabularies from the product sources
//! and count the rows another way, so a new value cannot be left out.

use super::super::recovery::after_event;
use super::FIELD;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const SEEN: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const COMMAND: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const INPUT: &str = "4444444444444444444444444444444444444444444444444444444444444444";

const MARKERS: [bool; 2] = [false, true];
const ACQUISITIONS: [&str; 2] = ["observed", "reported"];
const ORIGINS: [&str; 2] = ["local_report", "synthetic"];
/// Every admitted process result, plus one shape the product never admits.
const RESULTS: [&str; 4] = ["exit 0", "exit 1", "signal 9", "inadmissible"];
const SUBJECTS: [&str; 2] = [SEEN, NEW];
const PHASES: [&str; 4] = ["ready", "replan_required", "evidence_required", "blocked"];

fn result(label: &str) -> Value {
    match label {
        "exit 0" => json!({"kind": "exit", "code": 0}),
        "exit 1" => json!({"kind": "exit", "code": 1}),
        "signal 9" => json!({"kind": "signal", "signal": 9}),
        _ => json!({"kind": "timeout"}),
    }
}

fn status(label: &str) -> Value {
    match label {
        "exit 0" => json!({"exit": 0}),
        "exit 1" => json!({"exit": 1}),
        _ => json!({"signal": 9}),
    }
}

fn check(acquisition: &str, origin: &str, label: &str, subject: &str) -> Value {
    json!({
        "action": "check",
        "acquisition": acquisition,
        "origin": origin,
        "result": result(label),
        "checkCommandSha256": COMMAND,
        "subjectSha256": subject,
        "inputsSha256": INPUT,
        "targetEventSha256": "5".repeat(64),
        "eventSha256": format!("{:064x}", label.len() + subject.len() + acquisition.len()),
    })
}

fn governed(previous: Option<&Value>, payload: Value) -> Value {
    crate::context::event(previous, payload)
}

fn mutation(previous: &Value, checkpoint: u64) -> Value {
    governed(
        Some(previous),
        json!({"action": "mutation", "runId": "run", "subjectSha256": SEEN, "attempt": 1,
            "checkpoint": checkpoint, "inputSha256": INPUT, "lineageSha256": "lineage",
            "carryLineage": true, "unit": "worker-mutation",
            "operation": format!("edit {checkpoint}")}),
    )
}

/// A governor that has already observed the pass and the failure on the seen
/// subject, then reached `phase` through real transitions.
fn state(marker: bool, phase: &str) -> Value {
    let mut start = json!({"recoveryProtocol": 1, "runId": "run", "attempt": 1});
    if marker {
        start[FIELD] = json!(super::PROTOCOL);
    }
    let first = governed(
        None,
        json!({"action": "mutation", "runId": "run", "subjectSha256": SEEN, "attempt": 1,
            "checkpoint": 1, "inputSha256": INPUT, "lineageSha256": "lineage",
            "carryLineage": true, "unit": "worker-mutation", "operation": "edit 1"}),
    );
    let mut events = vec![first];
    for (index, label) in ["exit 0", "exit 1"].into_iter().enumerate() {
        let seen = check("observed", "local_report", label, SEEN);
        let key = super::key(&start, &seen, &status(label));
        events.push(governed(
            events.last(),
            json!({"action": "observation", "runId": "run", "subjectSha256": SEEN,
                "attempt": 1, "inputSha256": INPUT, "observationKey": key,
                "checkEventSha256": format!("{:064x}", index + 100)}),
        ));
    }
    events.push(mutation(events.last().unwrap(), 2));
    if phase != "ready" {
        events.push(mutation(events.last().unwrap(), 3));
    }
    if matches!(phase, "evidence_required" | "blocked") {
        events.push(governed(
            events.last(),
            json!({"action": "replan", "runId": "run", "subjectSha256": SEEN, "attempt": 1,
                "inputSha256": INPUT, "hypothesis": "a material re-plan"}),
        ));
        events.push(mutation(events.last().unwrap(), 4));
    }
    if phase == "blocked" {
        events.push(governed(
            events.last(),
            json!({"action": "blocked", "runId": "run", "subjectSha256": SEEN, "attempt": 1,
                "inputSha256": INPUT, "reason": "mutation requested without evidence"}),
        ));
    }
    let mut governor = crate::context::reduce_governor(&events).unwrap();
    assert_eq!(governor["state"], phase, "fixture phase");
    governor["enabled"] = json!(true);
    start["governor"] = governor;
    start["governorEvents"] = Value::Array(events);
    start
}

/// The oracle, written out: is this observed local check new information on
/// an open governor? The seen subject already has a pass and a failure.
fn new_information(marker: bool, label: &str, subject: &str) -> bool {
    let new_subject = subject == NEW;
    match (marker, label) {
        // Historical starts: the subject never enters the key.
        (false, "exit 0" | "exit 1") => false,
        // A pass on a new subject is new; the same failure stays the same.
        (true, "exit 0") => new_subject,
        (true, "exit 1") => false,
        (_, "signal 9") => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Appended {
    Nothing,
    Observation,
    CheckedSubmissionObservation,
}

fn appended(before: &Value, after: &Value) -> Appended {
    let count = |state: &Value| state["governorEvents"].as_array().unwrap().len();
    if count(after) == count(before) {
        return Appended::Nothing;
    }
    assert_eq!(count(after), count(before) + 1);
    let event = after["governorEvents"].as_array().unwrap().last().unwrap();
    assert_eq!(event["action"], "observation");
    match event["identityTransition"].as_str() {
        None => Appended::Observation,
        Some("checked_submission_v1") => Appended::CheckedSubmissionObservation,
        Some(other) => panic!("unexpected transition {other}"),
    }
}

type Row = (
    bool,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for marker in MARKERS {
        for acquisition in ACQUISITIONS {
            for origin in ORIGINS {
                for label in RESULTS {
                    for subject in SUBJECTS {
                        for phase in PHASES {
                            rows.push((marker, acquisition, origin, label, subject, phase));
                        }
                    }
                }
            }
        }
    }
    rows
}

#[test]
fn every_observed_check_shape_is_information_or_not_as_the_table_says() {
    let mut shapes = BTreeSet::new();
    for (marker, acquisition, origin, label, subject, phase) in rows() {
        let before = state(marker, phase);
        let mut after = before.clone();
        let event = check(acquisition, origin, label, subject);
        after_event(&mut after, &event).unwrap();
        let expected = acquisition == "observed"
            && origin == "local_report"
            && phase != "blocked"
            && new_information(marker, label, subject);
        let shape = appended(&before, &after);
        let row = format!("{marker} {acquisition} {origin} {label} {subject:.4} {phase}");
        assert_eq!(shape != Appended::Nothing, expected, "{row}");
        if expected {
            // New information resets the consecutive trajectory.
            assert_eq!(after["governor"]["state"], "ready", "{row}");
            assert_eq!(after["governor"]["noInformationStreak"], 0, "{row}");
            let transition = if subject == NEW {
                Appended::CheckedSubmissionObservation
            } else {
                Appended::Observation
            };
            assert_eq!(shape, transition, "{row}");
        } else {
            assert_eq!(after["governor"], before["governor"], "{row}");
        }
        shapes.insert(shape);
    }
    assert_eq!(
        shapes,
        BTreeSet::from([
            Appended::Nothing,
            Appended::Observation,
            Appended::CheckedSubmissionObservation
        ])
    );
}

/// The same pass on the same subject is a duplicate even with the marker; a
/// second pass on the new subject after the first is a duplicate too.
#[test]
fn a_repeated_pass_on_one_subject_stays_a_duplicate() {
    let mut state = state(true, "replan_required");
    let pass = check("observed", "local_report", "exit 0", NEW);
    after_event(&mut state, &pass).unwrap();
    let once = state.clone();
    after_event(&mut state, &pass).unwrap();
    assert_eq!(state, once);
}

fn quoted(text: &str) -> BTreeSet<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let from = source
        .find(start)
        .unwrap_or_else(|| panic!("missing {start}"));
    let rest = &source[from + start.len()..];
    &rest[..rest.find(end).unwrap()]
}

#[test]
fn the_table_enumerates_every_admitted_vocabulary_in_the_product_sources() {
    // Result kinds: the serde tags of the admitted check result.
    let gate = include_str!("../gate/value.rs");
    let kinds = quoted(between(gate, "enum CheckResult {", "\n}"))
        .into_iter()
        .filter(|item| item != "kind")
        .collect::<BTreeSet<_>>();
    let table_kinds = RESULTS
        .iter()
        .filter(|label| **label != "inadmissible")
        .map(|label| result(label)["kind"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(kinds, table_kinds);
    assert!(
        !kinds.contains("timeout"),
        "the inadmissible row must stay inadmissible"
    );
    // Acquisitions: every literal the run layer writes onto a check.
    let mut acquisitions = BTreeSet::new();
    for source in [
        include_str!("../observed_check.rs"),
        include_str!("../mod.rs"),
    ] {
        for piece in source.split("\"acquisition\": \"").skip(1) {
            acquisitions.insert(piece[..piece.find('"').unwrap()].to_owned());
        }
    }
    let table: BTreeSet<String> = ACQUISITIONS.iter().map(|x| (*x).to_owned()).collect();
    assert_eq!(acquisitions, table);
    // Origins: the check validator's admitted origins.
    let validation = include_str!("../gate/value/validation.rs");
    let origins = quoted(between(
        validation,
        "event[\"origin\"].as_str(), Some(",
        ")",
    ));
    let table: BTreeSet<String> = ORIGINS.iter().map(|x| (*x).to_owned()).collect();
    assert_eq!(origins, table);
    // Phases: every name the kernel parses.
    let kernel = include_str!("../../kernel/governor.rs");
    let phases = quoted(between(kernel, "pub(crate) fn parse", "_ =>"))
        .into_iter()
        .filter(|name| !name.contains(' '))
        .collect::<BTreeSet<_>>();
    let table: BTreeSet<String> = PHASES.iter().map(|x| (*x).to_owned()).collect();
    assert_eq!(phases, table);
}

#[test]
fn the_rows_are_the_whole_product_counted_another_way() {
    let rows = rows();
    let expected = MARKERS.len()
        * ACQUISITIONS.len()
        * ORIGINS.len()
        * RESULTS.len()
        * SUBJECTS.len()
        * PHASES.len();
    assert_eq!(rows.len(), expected);
    let distinct: BTreeSet<Row> = rows.iter().copied().collect();
    assert_eq!(distinct.len(), 2 * 2 * 2 * 4 * 2 * 4);
}

/// Every start path carries the marker: each source file that names the
/// recovery protocol field (start construction, successor copies, frozen
/// field comparisons, start intent, validation) also carries
/// `observationNoveltyProtocol`, so a new start path cannot drop it.
#[test]
fn every_file_that_carries_the_recovery_protocol_carries_the_novelty_marker() {
    fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let carriers: Vec<_> = files
        .iter()
        .filter(|path| {
            std::fs::read_to_string(path)
                .unwrap()
                .contains("\"recoveryProtocol\"")
        })
        .collect();
    assert!(carriers.len() >= 8, "{carriers:?}");
    for path in carriers {
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            text.contains("observationNoveltyProtocol") || text.contains("observation_novelty::"),
            "{} carries recoveryProtocol without the novelty marker",
            path.display()
        );
    }
}

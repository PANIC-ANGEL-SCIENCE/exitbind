//! FR55-15: a Goal journal grows by what changed, old journals read
//! unchanged, and recovery reads a complete journal past the former 16 MiB
//! bound (R55's was 17,987,800 B in 114 records when recovery refused it).

use super::*;
use crate::session_goal::{read_for_recovery, records_from_raw, sealed};

/// A complete Goal journal written by the release before change records
/// (`goal incorporate`, three `goal require` from one card, `goal cover`).
const COMPLETE_FIXTURE: &str =
    include_str!("../../../tests/fixtures/session-goal-complete-records.jsonl");

fn sha(seed: usize) -> String {
    format!("{seed:064x}")
}

fn work(seed: usize) -> String {
    format!("smw_{}", sha(seed))
}

/// Deterministic card-like text of about `bytes` bytes.
fn card(seed: usize, bytes: usize) -> String {
    let mut text = format!("# Task card {seed}\n\n");
    let mut line = 0;
    while text.len() < bytes {
        line += 1;
        text.push_str(&format!(
            "Step {line} of card {seed}: the requirement holds when its named check passes and the field report counts it.\n"
        ));
    }
    text
}

fn member(seed: usize, observed: &str) -> Value {
    json!({
        "configPath": "/fixture/control/exitbind.json",
        "configSha256": sha(1),
        "host": "claude",
        "localIdentitySha256": sha(2),
        "ownerIdentitySha256": sha(2),
        "ownerProjectId": "fixture_project",
        "parentGoalId": "R55",
        "parentObservedGoalEventSha256": observed,
        "productRoot": "/fixture/product",
        "projectIdentitySha256": sha(3),
        "sessionKey": sha(4),
        "startEventSha256": sha(1000 + seed),
        "stateRoot": "/fixture/state",
        "work": work(seed),
        "workGoalSha256": sha(5),
    })
}

fn admission(seed: usize) -> Value {
    let mut value = member(seed, &sha(6));
    let object = value.as_object_mut().unwrap();
    object.remove("startEventSha256");
    object.insert("startIntentSha256".into(), json!(sha(2000 + seed)));
    value
}

fn requirement(index: usize, text: &str) -> Value {
    json!({
        "history": [],
        "id": format!("S{index}"),
        "revision": 1,
        "source": {
            "coverageConfirmed": false,
            "ref": ".git/task-card.md",
            "sha256": crate::evidence::hash::text(text),
            "text": text,
        },
        "text": format!("Requirement S{index} holds when its check passes."),
    })
}

fn mapping(index: usize) -> Value {
    json!({
        "active": index % 3 != 0,
        "requirement": {
            "id": format!("S{}", index % 29),
            "revision": 1,
            "sourceSha256": sha(7),
            "text": format!("Mapping {index}: the Work proves its requirement with a named test and a counted field-report line."),
        },
        "startEventSha256": sha(3000 + index),
        "work": work(index % 22),
    })
}

/// The first record of an R55-shaped Goal: 29 requirements whose sources
/// are 11 distinct cards of 4-11 KB, 93 mappings, 22 journey members.
fn r55_shaped() -> Value {
    let cards = (0..11)
        .map(|seed| card(seed, 4_000 + seed * 650))
        .collect::<Vec<_>>();
    let goal = json!({
        "kind": "lead_session_goal",
        "version": 4,
        "goalId": "R55",
        "revision": 1,
        "goal": "R55-shaped fixture Goal",
        "source": {"owner": "lead", "kind": "explicit_incorporation"},
        "predecessor": null,
        "successorOf": null,
        "obligations": [],
        "findings": [],
        "blockers": [],
        "decisions": [],
        "externalActions": [],
        "categories": {"obligations": "considered"},
        "closure": {"closed": false, "revision": null, "resultRefs": []},
        "continuation": {
            "kind": "goal_requirements",
            "version": 2,
            "projectIdentity": sha(3),
            "source": {"coverageConfirmed": true, "ref": ".git/task-card.md", "sha256": sha(8), "text": cards[0]},
            "requirements": (0..29).map(|index| requirement(index, &cards[index % 11])).collect::<Vec<_>>(),
            "mappings": (0..93).map(mapping).collect::<Vec<_>>(),
        },
        "journeyMembershipProtocol": 2,
        "journeyMembers": (0..22).map(|seed| member(seed, &sha(9))).collect::<Vec<_>>(),
        "journeyIntegrations": [],
        "journeyAdmissions": [],
    });
    sealed(goal, None)
}

/// The next revision of `previous`, changed the way R55 revisions change
/// (mapping, member, admission, coverage, requirement history, finding).
fn revise(previous: &Value, step: usize) -> Value {
    let mut next = previous.clone();
    let revision = previous["revision"].as_u64().unwrap();
    next["revision"] = json!(revision + 1);
    next["predecessor"] = json!({"goalId": previous["goalId"], "revision": revision});
    let observed = previous["eventSha256"].clone();
    match step % 7 {
        0 => next["continuation"]["mappings"]
            .as_array_mut()
            .unwrap()
            .push(mapping(100 + step)),
        1 => next["continuation"]["mappings"][step % 93]["active"] = json!(step % 2 == 0),
        2 => next["journeyMembers"][step % 22]["parentObservedGoalEventSha256"] = observed,
        3 => next["journeyAdmissions"] = json!([admission(500 + step)]),
        4 => next["journeyAdmissions"] = json!([]),
        5 => {
            let confirmed = previous["continuation"]["source"]["coverageConfirmed"] != true;
            next["continuation"]["source"]["coverageConfirmed"] = json!(confirmed);
        }
        _ => {
            next["continuation"]["requirements"][step % 29]["history"]
                .as_array_mut()
                .unwrap()
                .push(json!({"revision": step, "text": format!("Earlier wording {step}.")}));
            next["findings"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": format!("F{step}"), "text": format!("Finding {step}."), "disposition": "open", "resultRefs": []}));
        }
    }
    let object = next.as_object_mut().unwrap();
    object.remove("eventSha256");
    object.remove("previousEventSha256");
    sealed(next, Some(previous))
}

/// The stored journal of logical `records`, written through `stored`.
fn journal(records: &[Value]) -> (String, Vec<usize>) {
    let mut raw = String::new();
    let mut sizes = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let line =
            serde_json::to_string(&stored(record, index.checked_sub(1).map(|i| &records[i])))
                .unwrap();
        sizes.push(line.len() + 1);
        raw.push_str(&line);
        raw.push('\n');
    }
    (raw, sizes)
}

#[test]
fn r55_shaped_revisions_store_only_what_changed() {
    let mut records = vec![r55_shaped()];
    for step in 0..100 {
        let next = revise(records.last().unwrap(), step);
        records.push(next);
    }
    let (raw, sizes) = journal(&records);
    let complete = serde_json::to_string(&records[100]).unwrap().len();
    let largest = sizes[1..].iter().copied().max().unwrap();
    eprintln!(
        "R55-shaped: first record {} B, latest complete record {complete} B; 100 revisions stored {} B, largest {largest} B, mean {} B",
        sizes[0],
        sizes[1..].iter().sum::<usize>(),
        sizes[1..].iter().sum::<usize>() / 100
    );
    assert!(complete > 250_000, "the fixture is R55-sized");
    assert!(largest < 10 * 1024, "a revision adds less than 10 KB");
    assert_eq!(records_from_raw(Some(&raw)).unwrap(), records);
}

#[test]
fn a_card_sourced_requirement_is_not_written_again() {
    let text = card(42, 9_400);
    let mut base = r55_shaped();
    let object = base.as_object_mut().unwrap();
    object.remove("eventSha256");
    object.remove("previousEventSha256");
    base["continuation"]["source"]["text"] = json!(text);
    base["continuation"]["requirements"] = json!([]);
    let mut records = vec![sealed(base, None)];
    for index in 0..12 {
        let previous = records.last().unwrap();
        let mut next = previous.clone();
        next["revision"] = json!(index + 2);
        next["predecessor"] = json!({"goalId": "R55", "revision": index + 1});
        next["continuation"]["requirements"]
            .as_array_mut()
            .unwrap()
            .push(requirement(index, &text));
        let object = next.as_object_mut().unwrap();
        object.remove("eventSha256");
        object.remove("previousEventSha256");
        let next = sealed(next, Some(previous));
        records.push(next);
    }
    let (raw, sizes) = journal(&records);
    let complete_growth = serde_json::to_string(&records[12]).unwrap().len()
        - serde_json::to_string(&records[11]).unwrap().len();
    eprintln!(
        "R56-shaped registration of a 9.4 KB card: complete record grows {complete_growth} B; stored change {} B",
        sizes[12]
    );
    assert!(complete_growth > 9_400);
    assert!(sizes[1..].iter().all(|size| *size < 1_024));
    assert_eq!(records_from_raw(Some(&raw)).unwrap(), records);
}

#[test]
fn a_journal_of_complete_records_reads_unchanged() {
    let records = records_from_raw(Some(COMPLETE_FIXTURE)).unwrap();
    let lines = COMPLETE_FIXTURE
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 5);
    assert_eq!(records, lines);
    // A new revision on top of it is stored as a change and reads back.
    let previous = records.last().unwrap();
    let mut next = previous.clone();
    let text = previous["continuation"]["requirements"][0]["source"]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    next["revision"] = json!(6);
    next["predecessor"] = json!({"goalId": previous["goalId"], "revision": 5});
    next["continuation"]["requirements"]
        .as_array_mut()
        .unwrap()
        .push(requirement(4, &text));
    let object = next.as_object_mut().unwrap();
    object.remove("eventSha256");
    object.remove("previousEventSha256");
    let next = sealed(next, Some(previous));
    let line = stored(&next, Some(previous));
    assert_eq!(line["kind"], CHANGE_KIND);
    let raw = format!("{COMPLETE_FIXTURE}{line}\n");
    let mut expected = lines;
    expected.push(next.clone());
    assert_eq!(records_from_raw(Some(&raw)).unwrap(), expected);
    // A complete record may still follow a change record.
    let mut after = next.clone();
    after["revision"] = json!(7);
    after["predecessor"] = json!({"goalId": next["goalId"], "revision": 6});
    let object = after.as_object_mut().unwrap();
    object.remove("eventSha256");
    object.remove("previousEventSha256");
    let after = sealed(after, Some(&next));
    let raw = format!("{raw}{after}\n");
    expected.push(after);
    assert_eq!(records_from_raw(Some(&raw)).unwrap(), expected);
}

#[test]
fn a_change_record_must_name_its_previous_record_and_rebuild_the_sealed_record() {
    let records = records_from_raw(Some(COMPLETE_FIXTURE)).unwrap();
    let first = serde_json::to_string(&records[0]).unwrap();
    let change = stored(&records[1], Some(&records[0]));
    assert_eq!(change["kind"], CHANGE_KIND);
    let read = |line: &Value| records_from_raw(Some(&format!("{first}\n{line}\n")));
    assert_eq!(read(&change).unwrap(), records[..2].to_vec());
    let altered = |edit: &dyn Fn(&mut Value)| {
        let mut line = change.clone();
        edit(&mut line);
        line
    };
    let refused = [
        altered(&|line| line["base"] = json!(sha(11))),
        altered(&|line| line["version"] = json!(2)),
        altered(&|line| line["extra"] = json!(true)),
        altered(&|line| line["changes"] = json!({})),
        altered(&|line| line["changes"] = json!([["move", ["goal"], "x"]])),
        altered(&|line| line["changes"] = json!([["set", ["closure", "missing", "x"], 1]])),
        altered(&|line| line["changes"] = json!([["copy", ["goal"], ["nowhere"]]])),
        altered(&|line| line["changes"] = json!([["del", ["nowhere"]]])),
        // A changed value no longer rebuilds the sealed record.
        altered(&|line| {
            line["changes"]
                .as_array_mut()
                .unwrap()
                .push(json!(["set", ["goal"], "changed"]))
        }),
        altered(&|line| line["changes"] = json!([])),
    ];
    for line in refused {
        assert!(read(&line).is_err(), "{line}");
    }
    // A change record cannot open a journal.
    assert!(records_from_raw(Some(&format!("{change}\n"))).is_err());
}

#[test]
fn recovery_reads_a_complete_journal_past_the_former_16_mib_bound() {
    let root = crate::test_root::temp("journal-form-recovery");
    let mut records = vec![r55_shaped()];
    let mut raw = format!("{}\n", records[0]);
    while raw.len() < 18_000_000 {
        let next = revise(records.last().unwrap(), records.len() - 1);
        raw.push_str(&format!("{next}\n"));
        records.push(next);
    }
    let path = root.join(crate::project::layout_types::state_namespace());
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("session-goal.jsonl"), &raw).unwrap();
    let started = std::time::Instant::now();
    let first = read_for_recovery(&root);
    eprintln!(
        "complete-record journal of {} records, {} B: first read_for_recovery {:?}, result {:?}",
        records.len(),
        raw.len(),
        started.elapsed(),
        first.as_ref().map(|record| record.is_some())
    );
    if !matches!(&first, Ok(Some(_))) {
        let started = std::time::Instant::now();
        let diagnostic = read_for_recovery(&root);
        eprintln!(
            "diagnostic second read_for_recovery {:?}, result {:?}",
            started.elapsed(),
            diagnostic.as_ref().map(|record| record.is_some())
        );
    }
    let read = first.unwrap().unwrap();
    assert_eq!(&read, records.last().unwrap());
}

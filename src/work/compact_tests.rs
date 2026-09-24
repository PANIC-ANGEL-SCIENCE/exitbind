//! Focused projection and recovery contract checks.

use super::{project, MAX_RESPONSE_BYTES};
use serde_json::json;
use std::path::Path;

#[test]
fn projection_keeps_decision_identity_subject_recorder_and_references() {
    let reference = json!({
        "id": "ref:history",
        "kind": "ledger_history",
        "work": "smw_work",
        "sha256": "a".repeat(64),
        "headEventSha256": "b".repeat(64),
        "eventCount": 2,
        "exact": true,
    });
    let response = json!({
        "status": "resumed",
        "work": "smw_work",
        "next": {
            "action": "spawn",
            "assignment": "sma_assignment",
            "role": "worker",
            "agent": "worker",
            "packet": {"context": {"expansions": [reference.clone()]}}
        },
        "residual": {
            "currentSubject": {"sha256": "c".repeat(64)},
            "context": {"expansions": [reference.clone()]},
            "humanHelp": {
                "nextAction": {"actor": "worker", "command": ["work", "next"]},
                "preservationAssignment": {"route": "FORMAL", "quality": "FULL"}
            },
            "large": "x".repeat(60_000)
        },
        "recorder": {"name": "exitbind", "version": "0.25.0", "commit": null},
        "ledgerProducer": {"name": "exitbind", "version": "0.25.0", "commit": null},
    });
    let compact = project(&response, Path::new("/tmp/exitbind.json"), "next").unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
    assert_eq!(compact["next"]["action"], "spawn");
    assert_eq!(compact["next"]["assignment"], "sma_assignment");
    assert_eq!(compact["nextAction"]["actor"], "worker");
    assert_eq!(
        compact["humanHelp"]["preservationAssignment"]["route"],
        "FORMAL"
    );
    assert_eq!(compact["currentSubject"]["sha256"], "c".repeat(64));
    assert_eq!(compact["recorder"]["name"], "exitbind");
    assert_eq!(compact["references"][0], reference);
    assert_eq!(compact["truncated"], false);
    assert!(compact["omitted"].as_array().unwrap().len() >= 2);
}

#[test]
fn oversized_exact_reference_set_keeps_representatives_and_counts_omissions() {
    let references = (0..200)
        .map(|index| {
            json!({
                "id": format!("ref:{index}"),
                "kind": "ledger_event",
                "work": "smw_work",
                "sha256": "a".repeat(64),
                "headEventSha256": "b".repeat(64),
                "eventCount": index,
                "selector": "c".repeat(64),
                "exact": true,
            })
        })
        .collect::<Vec<_>>();
    let response = json!({"work": "smw_work", "next": {"action": "check"}, "residual": {"context": {"expansions": references}}});
    let compact = project(&response, Path::new("/tmp/exitbind.json"), "next").unwrap();
    assert_eq!(compact["truncated"], true);
    assert_eq!(compact["referenceOmissions"], 199);
    assert_eq!(compact["references"].as_array().unwrap().len(), 1);
    assert_eq!(compact["references"][0]["kind"], "ledger_event");
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
}

#[test]
fn oversized_held_set_names_all_omissions_and_exact_full_route() {
    let held = (0..60)
        .map(|index| json!({"reference": format!("hld_{index}"), "detail": "x".repeat(400)}))
        .collect::<Vec<_>>();
    let response = json!({
        "work": "smw_work",
        "next": {"action": "spawn", "assignment": "sma_assignment", "heldResults": held},
        "residual": {"humanHelp": {"preservationAssignment": {"route": "FORMAL", "quality": "FULL"}}}
    });
    let compact = project(
        &response,
        Path::new("/tmp/other project/exitbind.json"),
        "next",
    )
    .unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
    assert_eq!(compact["truncated"], true);
    assert_eq!(compact["next"]["heldResultsCount"], 60);
    assert_eq!(compact["next"]["heldResultsOmissions"], 60);
    assert!(compact["next"].get("heldResults").is_none());
    assert_eq!(
        compact["humanHelp"]["preservationAssignment"]["quality"],
        "FULL"
    );
    assert_eq!(
        compact["fullCommand"][6],
        "/tmp/other project/exitbind.json"
    );
}

#[test]
fn resume_recovery_keeps_verb_and_long_config_path() {
    let path = format!("/tmp/{}/exitbind.json", "directory/".repeat(300));
    let response = json!({"status": "resumed", "work": "smw_work", "next": {"action": "check"}});
    let compact = project(&response, Path::new(&path), "resume").unwrap();
    assert_eq!(compact["fullCommand"][2], "resume");
    assert_eq!(compact["fullCommand"][5], path);
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
}

#[test]
fn config_path_over_budget_keeps_explicit_same_config_route() {
    let path = format!("/tmp/{}/exitbind.json", "directory/".repeat(1_000));
    let response = json!({
        "status": "resumed", "work": "smw_work",
        "next": {"action": "check", "assignment": "sma_assignment"},
    });
    let compact = project(&response, Path::new(&path), "next").unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() + 1 <= MAX_RESPONSE_BYTES);
    assert_eq!(compact["fullCommandSameConfigRequired"], true);
    assert_eq!(compact["fullCommand"][2], "next");
    assert_eq!(
        compact["fullCommand"].as_array().unwrap().last().unwrap(),
        "--config"
    );
}

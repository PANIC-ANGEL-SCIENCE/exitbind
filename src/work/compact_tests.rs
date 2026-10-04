//! Focused projection and recovery contract checks.

use super::{project, MAX_RESPONSE_BYTES};
use serde_json::json;
use std::path::Path;

#[test]
fn emergency_resume_keeps_navigation_with_current_action_detail() {
    for config_bytes in [32, 512, 6_000, 64_000] {
        let config = format!("/tmp/{}/exitbind.json", "c".repeat(config_bytes));
        let response = json!({
            "status": "resumed",
            "work": "smw_work",
            "selection": {"basis": "current_work_focus", "authority": "none"},
            "history": {"running": 1, "command": ["work", "resume", "--history"],
                "sameExecutableRequired": true, "sameConfigRequired": true},
            "focus": {"work": "smw_work", "resumable": true},
            "discovery": {"observed": 2, "complete": true},
            "next": {"action": "spawn", "current": {
                "action": "spawn", "binding": "binding", "readiness": "IN_PROGRESS",
                "details": {"grouped": {"command": ["work", "expand", "smw_work", "ref:detail"],
                    "sameExecutableRequired": true, "sameConfigRequired": true}},
                "actionForm": {"version": 1, "state": "pending_assignment",
                    "binding": "binding", "leadChoiceRequired": false,
                    "oversized": "x".repeat(16_000)}
            }},
            "continuation": {"oversized": "x".repeat(8_000)},
            "residual": {"humanHelp": {"whatHappened": "x".repeat(16_000)}}
        });
        let compact = project(&response, Path::new(&config), "resume").unwrap();
        assert!(serde_json::to_vec(&compact).unwrap().len() < MAX_RESPONSE_BYTES);
        assert_eq!(compact["truncated"], true);
        for key in ["selection", "history", "focus", "discovery"] {
            assert_eq!(compact[key], response[key], "navigation field {key}");
        }
        assert_eq!(compact["current"]["binding"], "binding");
        assert!(compact["fullCommand"].is_array());
    }
}

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
fn compact_projection_keeps_product_owned_goal_and_result_readiness() {
    let response = json!({
        "status": "running",
        "work": "smw_work",
        "next": {"action": "spawn", "assignment": "sma_assignment"},
        "presentation": {
            "exitState": "IN_PROGRESS",
            "goalProgress": {
                "overall": "in_progress",
                "systemText": "Goal: IN PROGRESS | Tasks: 1/2 complete | Result readiness: BLOCKED",
                "decomposition": {"available": true, "total": 2, "complete": 1, "omitted": 0},
                "tasks": [
                    {"id": "A", "state": "complete", "performed": true, "current": true},
                    {"id": "B", "state": "in_progress", "performed": false, "current": null}
                ],
                "resultReadiness": {"state": "BLOCKED", "reason": "check_missing"}
            }
        }
    });
    let compact = project(&response, Path::new("/tmp/exitbind.json"), "next").unwrap();
    assert_eq!(
        compact["presentation"]["goalProgress"]["overall"],
        "in_progress"
    );
    assert_eq!(
        compact["presentation"]["goalProgress"]["resultReadiness"]["state"],
        "BLOCKED"
    );
    assert_eq!(
        compact["presentation"]["goalProgress"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn compact_goal_progress_stays_bounded_with_many_long_tasks() {
    let tasks = (0..32)
        .map(|index| {
            json!({
                "id": format!("task-{index}-{}", "x".repeat(500)),
                "state": "in_progress",
                "disposition": "open",
                "performed": false,
                "current": null,
                "currentReason": "currentness_unavailable",
            })
        })
        .collect::<Vec<_>>();
    let response = json!({
        "status": "running",
        "work": "smw_work",
        "next": {"action": "spawn", "assignment": "sma_assignment"},
        "presentation": {
            "exitState": "IN_PROGRESS",
            "goalProgress": {
                "overall": "in_progress",
                "goal": "g".repeat(500),
                "systemText": "s".repeat(5000),
                "decomposition": {"available": true, "total": 32, "complete": 0, "omitted": 0},
                "tasks": tasks,
                "resultReadiness": {"state": "BLOCKED", "reason": "check_missing"}
            }
        }
    });
    let compact = project(&response, Path::new("/tmp/exitbind.json"), "next").unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
    assert_eq!(
        compact["presentation"]["goalProgress"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(compact["presentation"]["goalProgress"]["taskOmissions"], 24);
    assert_eq!(
        compact["presentation"]["goalProgress"]["resultReadiness"]["state"],
        "BLOCKED"
    );
}

#[test]
fn compact_resume_keeps_other_candidates_and_unreadable_recovery_routes() {
    let works = json!([{
        "work": "smw_other",
        "progress": {"state": "running", "percent": 10},
        "command": ["exitbind", "work", "next", "smw_other", "--json"],
    }]);
    let unreadable = json!([{
        "work": "smw_old",
        "reason": "candidate_unreadable",
        "error": "historical replay is incomplete",
        "command": ["exitbind", "run", "inspect", ".exitbind/runs/work-old.jsonl", "--json"],
    }]);
    let response = json!({
        "status": "resumed",
        "work": "smw_focus",
        "works": works,
        "unreadable": unreadable,
        "next": {"action": "check"},
    });

    let compact = project(&response, Path::new("/tmp/exitbind.json"), "resume").unwrap();
    assert_eq!(compact["works"], works);
    assert_eq!(compact["unreadable"], unreadable);
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
}

#[test]
fn oversized_candidate_details_keep_counts_and_a_full_resume_route() {
    let works = (0..100)
        .map(|index| {
            json!({
                "work": format!("smw_{index}"),
                "command": ["exitbind", "work", "next", format!("smw_{index}")],
                "detail": "x".repeat(400),
            })
        })
        .collect::<Vec<_>>();
    let unreadable = (0..100)
        .map(|index| {
            json!({
                "work": format!("smw_bad_{index}"),
                "reason": "candidate_unreadable",
                "command": ["exitbind", "run", "inspect", format!(".exitbind/runs/{index}.jsonl")],
                "error": "x".repeat(400),
            })
        })
        .collect::<Vec<_>>();
    let response = json!({
        "status": "resumed",
        "work": "smw_focus",
        "works": works,
        "unreadable": unreadable,
        "next": {"action": "check", "assignment": "sma_assignment"},
    });

    let compact = project(&response, Path::new("/tmp/exitbind.json"), "resume").unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() <= MAX_RESPONSE_BYTES);
    assert_eq!(compact["candidateCounts"]["works"], 100);
    assert_eq!(compact["candidateCounts"]["unreadable"], 100);
    assert_eq!(compact["omitted"][0], "candidate details; use fullCommand");
    assert_eq!(compact["fullCommand"][2], "resume");
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
    assert!(serde_json::to_vec(&compact).unwrap().len() < MAX_RESPONSE_BYTES);
    assert_eq!(compact["fullCommandSameConfigRequired"], true);
    assert_eq!(compact["fullCommand"][2], "next");
    assert_eq!(
        compact["fullCommand"].as_array().unwrap().last().unwrap(),
        "--config"
    );
}

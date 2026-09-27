use crate::{run_exit, run_progress};
use serde_json::json;

#[test]
fn progress_distinguishes_current_refusal_and_block() {
    let base = json!({
        "version": 5,
        "status":"running",
        "attempt":2,
        "subject":{"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
        "checkPolicy": {
            "version": 1,
            "command": "true",
            "commandSha256": crate::evidence::hash::text("true"),
            "origin": "local_report"
        },
        "plan":{"stages":[{"agents":[{"role":"worker"}]}]},
        "submissions":[],
        "checks":[],
        "protections":[]
    });
    assert_eq!(run_progress::project(&base)["state"], "IN_PROGRESS");
    let mut failed = base.clone();
    failed["submissions"] = json!([{
        "attempt":2,
        "role":"worker",
        "outcome":"completed",
        "eventSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    }]);
    failed["checks"] = json!([{
        "targetEventSha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "subjectSha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "exitCode":1
    }]);
    assert_eq!(run_progress::project(&failed)["state"], "REFUSED");
    assert_eq!(
        run_progress::project(&failed)["reason"]["code"],
        "check_failed"
    );
    let mut missing = base;
    missing["submissions"] = failed["submissions"].clone();
    assert_eq!(run_progress::project(&missing)["state"], "BLOCKED");
    let mut reworked = missing;
    reworked["attempt"] = json!(3);
    assert_eq!(run_progress::project(&reworked)["state"], "IN_PROGRESS");
}

#[test]
fn historical_and_unchecked_acceptance_are_not_exit_ready_progress() {
    let historical = json!({
        "version": 4,
        "status": "accepted",
        "attempt": 1,
        "submissions": [{"role": "lead", "outcome": "accepted"}]
    });
    let unchecked = {
        let mut value = historical.clone();
        value["version"] = json!(5);
        value
    };
    for (state, code) in [
        (&historical, "historical_run"),
        (&unchecked, "unchecked_run"),
    ] {
        let progress = run_progress::project(state);
        assert_eq!(progress["applicable"], false);
        assert!(progress["percent"].is_null());
        assert_eq!(progress["state"], "NOT_APPLICABLE");
        assert_eq!(progress["reason"]["code"], code);
        assert_eq!(progress["runStatus"], "accepted");
    }
}

#[test]
fn stale_subject_checks_are_missing_and_old_refusal_recovers() {
    let old_subject = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let current_subject = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let worker_a = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let worker_b = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let mut state = json!({
        "version": 5,
        "status": "running",
        "attempt": 1,
        "subject": {"sha256": current_subject},
        "checkPolicy": {
            "version": 1,
            "command": "true",
            "commandSha256": crate::evidence::hash::text("true"),
            "origin": "local_report"
        },
        "plan": {"stages": [{"agents": [
            {"role": "worker"}, {"role": "worker"}
        ]}]},
        "submissions": [
            {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_a},
            {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_b}
        ],
        "checks": [
            {"targetEventSha256": worker_a, "subjectSha256": old_subject, "exitCode": 0},
            {"targetEventSha256": worker_b, "subjectSha256": current_subject, "exitCode": 0}
        ],
        "protections": [{"attempt": 1, "reason": "check_failed"}]
    });
    let assessment = run_exit::assess(&state).unwrap();
    assert_eq!(assessment.reason(), Some("check_missing"));
    assert_eq!(run_progress::project(&state)["state"], "BLOCKED");
    assert_eq!(run_progress::project(&state)["percent"], 32);

    state["checks"] = json!([
        {"targetEventSha256": worker_a, "subjectSha256": current_subject, "exitCode": 0},
        {"targetEventSha256": worker_b, "subjectSha256": current_subject, "exitCode": 0}
    ]);
    let assessment = run_exit::assess(&state).unwrap();
    assert_eq!(assessment.reason(), None);
    assert_eq!(run_progress::project(&state)["state"], "IN_PROGRESS");
    assert_eq!(run_progress::project(&state)["percent"], 40);
}

#[test]
fn progress_allocates_named_weights_across_planned_workers_reviewers_and_lead() {
    let worker_a = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let worker_b = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let state = json!({
        "version": 5,
        "status": "running",
        "attempt": 1,
        "subject": {"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
        "checkPolicy": {
            "version": 1,
            "command": "true",
            "commandSha256": crate::evidence::hash::text("true"),
            "origin": "local_report"
        },
        "plan": {"stages": [{"agents": [
            {"role": "lead"}, {"role": "worker"}, {"role": "worker"},
            {"role": "reviewer"}, {"role": "reviewer"}
        ]}]},
        "submissions": [
            {"attempt": 1, "role": "lead", "outcome": "scoped"},
            {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_a},
            {"attempt": 1, "role": "worker", "outcome": "completed", "eventSha256": worker_b},
            {"attempt": 1, "role": "reviewer", "outcome": "approved", "subjectSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "eventSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
        ],
        "checks": [
            {"targetEventSha256": worker_a, "subjectSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "exitCode": 0, "eventSha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"},
            {"targetEventSha256": worker_b, "subjectSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "exitCode": 0, "eventSha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"}
        ],
        "events": [
            {"action": "check", "eventSha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"},
            {"action": "check", "eventSha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"},
            {"action": "submit", "eventSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "subjectSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
        ],
        "protections": []
    });
    let progress = run_progress::project(&state);
    assert_eq!(progress["applicable"], true);
    assert_eq!(progress["percent"], 65);
    assert_eq!(progress["weights"]["worker"], 25);
    assert_eq!(progress["weights"]["review"], 20);
    assert_eq!(progress["components"]["worker"]["total"], 2);
    assert_eq!(progress["components"]["check"]["completed"], 2);
    assert_eq!(progress["components"]["review"]["total"], 2);
    assert_eq!(progress["components"]["review"]["completed"], 1);
    assert_eq!(progress["components"]["lead"]["earned"], 0);
}

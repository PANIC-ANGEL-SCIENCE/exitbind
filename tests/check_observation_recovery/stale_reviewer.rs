//! An observed check whose tested inputs change after it ended: the pending
//! reviewer may only return rework or blocked, a marked Work routes rework
//! through the Lead, and an unmarked Work keeps the plain check-only path.
use super::*;

#[test]
fn stale_observed_reviewer_recovery_preserves_ordinary_work_protocols() {
    exercise_stale_reviewer_recovery(None, true, true);
    exercise_stale_reviewer_recovery(
        Some(
            r#"{"version":1,"constraints":["preserve history"],"openZones":["worker implementation"],"decisiveCases":["return a contradiction"]}"#,
        ),
        true,
        true,
    );
    exercise_stale_reviewer_recovery(None, true, false);
    exercise_unmarked_stale_check_remains_check_only();
}

fn exercise_stale_reviewer_recovery(basis: Option<&str>, marked: bool, drift: bool) {
    let f = Fixture::new("exit 0\n");
    let work = if marked {
        f.begin_with_optional_basis(basis)
    } else {
        let begun = f.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "repair the checker",
                "--check-command",
                "exec sh checker.sh",
            ],
            b"",
        );
        let work = begun["work"].as_str().unwrap().to_owned();
        let worker = f.give(&work, &begun["next"], "scoped")["next"].clone();
        f.give(&work, &worker, "completed");
        work
    };
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    let reviewer = f.next(&work);
    assert_eq!(reviewer["role"], "reviewer");

    if !drift {
        assert!(reviewer.get("staleEndedCheck").is_none());
        let detail = f.ok(&["work", "detail", &work, "--json"], b"");
        let choices = detail["actionForms"]["choices"].as_array().unwrap();
        assert!(choices.iter().any(|choice| choice["label"] == "approved"));
        let lead = f.give(&work, &reviewer, "approved")["next"].clone();
        f.give(&work, &lead, "accepted");
        assert_eq!(f.next(&work)["status"], "accepted");
        return;
    }

    fs::write(f.root.join("checker.sh"), "printf changed-inputs\\n\n").unwrap();
    let stale = f.next(&work);
    assert_eq!(stale["role"], "reviewer");
    assert_eq!(stale["assignment"], reviewer["assignment"]);
    assert_eq!(
        stale["staleEndedCheck"]["staleReason"],
        "tested_inputs_changed"
    );
    assert_eq!(stale["outcomes"], json!(["rework", "blocked"]));
    let detail = f.ok(&["work", "detail", &work, "--json"], b"");
    let choices = detail["actionForms"]["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 2);
    let before = fs::read(f.ledger(&work)).unwrap();
    for outcome in ["approved", "unavailable"] {
        let refused = f.call(
            &[
                "work",
                "return",
                &work,
                stale["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            outcome.as_bytes(),
        );
        assert!(!refused.status.success(), "{outcome}: {refused:?}");
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    }

    let disposition = f.give(&work, &stale, "rework")["next"].clone();
    let worker = if marked {
        assert_eq!(disposition["role"], "lead");
        assert_eq!(disposition["action"], "lead_decision");
        let detail = f.ok(&["work", "detail", &work, "--json"], b"");
        assert_eq!(detail["actionForms"]["state"], "pending_review_finding");
        assert!(detail["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|choice| choice["label"] == "repair"));
        f.repair(&work)
    } else {
        assert_eq!(disposition["role"], "worker");
        disposition
    };
    assert_eq!(worker["role"], "worker");
    fs::write(f.root.join("checker.sh"), "exit 0\n").unwrap();
    f.give(&work, &worker, "completed");
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    let fresh_reviewer = f.next(&work);
    assert_eq!(fresh_reviewer["role"], "reviewer");
    let lead = f.give(&work, &fresh_reviewer, "approved")["next"].clone();
    assert_eq!(lead["role"], "lead");
    f.give(&work, &lead, "accepted");
    assert_eq!(f.next(&work)["status"], "accepted");
}

fn exercise_unmarked_stale_check_remains_check_only() {
    let f = Fixture::new("exit 0\n");
    let begun = f.ok(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "repair the checker",
            "--check-command",
            "exec sh checker.sh",
        ],
        b"",
    );
    let work = begun["work"].as_str().unwrap().to_owned();
    let worker = f.give(&work, &begun["next"], "scoped")["next"].clone();
    f.give(&work, &worker, "completed");
    f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    let reviewer = f.next(&work);
    assert_eq!(reviewer["role"], "reviewer");

    fs::write(f.root.join("checker.sh"), "printf changed-inputs\\n\n").unwrap();
    let current = f.next(&work);
    assert_eq!(current["action"], "check");
    assert!(current.get("staleEndedCheck").is_none());
    assert!(current.get("outcomes").is_none());
    let before = fs::read(f.ledger(&work)).unwrap();
    let refused = f.call(
        &[
            "work",
            "return",
            &work,
            reviewer["assignment"].as_str().unwrap(),
            "--outcome",
            "approved",
        ],
        b"attempt approval without an observed-check recovery route",
    );
    assert!(!refused.status.success(), "{refused:?}");
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
}

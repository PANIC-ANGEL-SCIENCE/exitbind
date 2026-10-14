use super::*;

#[test]
fn invalid_deadlines_make_no_admission_or_execution() {
    let f = Fixture::new("printf ran > .exitbind/ran\n");
    let work = f.begin();
    let before = f.events(&work);
    for timeout in ["0", "invalid", "18446744073709551616", "86400001"] {
        assert!(!f
            .call(&["work", "check", &work, "--timeout-ms", timeout], b"")
            .status
            .success());
    }
    assert_eq!(f.events(&work), before);
    assert!(!f.root.join(".exitbind/ran").exists());
}

#[test]
fn changed_binding_during_observation_is_failure_and_not_success() {
    let f = Fixture::new("printf changed >> checker.sh; exit 0\n");
    let work = f.begin();
    let failure = f.call(&["work", "check", &work], b"");
    assert!(!failure.status.success());
    let facts = &value(&failure)["observationFailure"]["facts"];
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["storageStage"], "commit");
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    assert_eq!(f.next(&work)["action"], "lead_decision");
}

#[test]
fn storage_failure_preserves_observed_facts_and_unresolved_admission_without_retry() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("chmod u-w .exitbind/runs; printf captured; exit 7\n");
    let work = f.begin();
    let failed = f.call(&["work", "check", &work], b"");
    // Restore only this fixture's storage, even if the assertions below fail.
    fs::set_permissions(
        f.root.join(".exitbind/runs"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(!failed.status.success(), "{failed:?}");
    let failed = value(&failed);
    assert_eq!(failed["effect"], "unknown");
    assert_eq!(failed["durableFailureRecorded"], false);
    assert_eq!(failed["checkRecorded"], false);
    assert_eq!(failed["observation"]["process"]["code"], 7);
    assert_eq!(failed["observation"]["capture"], "complete");
    assert_eq!(failed["observation"]["storageStage"], "commit");
    assert_eq!(
        failed["observation"]["partialCaptures"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(failed["nextAction"]["command"].is_array());
    let next = f.next(&work);
    assert_eq!(next["outcomes"], json!(["blocked", "rejected"]));
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert!(!f
        .call(
            &[
                "work",
                "return",
                &work,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                "rework"
            ],
            b"unsafe retry"
        )
        .status
        .success());
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check_observation")
            .count(),
        1
    );
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    f.give(&work, &next, "blocked");
}

#[test]
fn replay_refuses_changed_configuration_and_a_new_policy_needs_supersession() {
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; exit 0\n");
    let work = f.begin();
    f.ok(&["work", "check", &work], b"");
    let original = fs::read(f.root.join("exitbind.json")).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    fs::write(f.root.join("exitbind.json"), changed).unwrap();
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    fs::write(f.root.join("exitbind.json"), original).unwrap();
    let relative = f
        .ledger(&work)
        .strip_prefix(&f.root)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!f
        .call(
            &[
                "run",
                "supersede",
                &relative,
                "--workflow",
                "change",
                "--goal",
                "new policy",
                "--ledger",
                ".exitbind/runs/new-policy.jsonl",
                "--check-command",
                "exit 9"
            ],
            b""
        )
        .status
        .success());
    assert!(!f.root.join(".exitbind/runs/new-policy.jsonl").exists());
    assert_eq!(f.ok(&["work", "check", &work], b"")["recovered"], true);
    f.ok(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "authorized successor with preserved policy",
            "--ledger",
            ".exitbind/runs/preserved-policy.jsonl",
        ],
        b"",
    );
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
}

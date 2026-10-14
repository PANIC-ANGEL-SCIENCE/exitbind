use super::*;

#[test]
fn missing_check_logs_invalidate_support_without_erasing_history() {
    let f = Fixture::with_preservation_command("w2ch-missing-check-log", "printf check-evidence");
    f.ok(
        json!({"action":"init","sourceRef":"fixture:requirements-v1",
        "sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}),
    );
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let source_sha = f.view()["source"]["sha256"].as_str().unwrap().to_owned();
    f.ok(
        json!({"action":"cover","expectedRevision":2,"bindingRevision":1,
        "sourceSha256":source_sha}),
    );
    f.return_stage("scoped", "Bounded scope");
    f.return_stage("completed", "A completed worker result");
    assert!(f.call(&["work", "check", &f.work]).status.success());
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let event: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let view = f.view();
    f.ok(
        json!({"action":"support","expectedRevision":3,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["event"]["eventSha256"],
        "conditionsSha256":view["currentConditionsSha256"]}),
    );
    assert_eq!(f.view()["requirements"][0]["unresolved"], false);
    let completion = f.call(&[
        "goal",
        "incorporate",
        "--direct",
        "--goal-id",
        &f.work,
        "--goal",
        "Finish two requirements on one work",
        "--obligation",
        "first",
    ]);
    assert!(completion.status.success(), "{completion:?}");
    let exact = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        event["eventSha256"].as_str().unwrap(),
    ]);
    assert!(exact.status.success(), "{exact:?}");
    let exact: Value = serde_json::from_slice(&exact.stdout).unwrap();
    let path = exact["event"]["stdout"]["path"].as_str().unwrap();
    fs::remove_file(f.root.join(path)).unwrap();
    let after = f.view();
    assert_eq!(
        after["requirements"][0]["support"][0]["integrityCurrent"],
        false
    );
    assert_eq!(after["requirements"][0]["unresolved"], true);
    assert_eq!(after["wholeGoalReady"], false);
    let close = f.call(&["goal", "close", "--direct", "--goal-id", &f.work]);
    assert!(!close.status.success());
    assert!(
        String::from_utf8_lossy(&close.stderr)
            .contains("continuation support is stale or incomplete"),
        "{close:?}"
    );
    let before = f.history();
    let refusal = f.record(
        json!({"action":"support","expectedRevision":5,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["event"]["eventSha256"],
        "conditionsSha256":after["currentConditionsSha256"]}),
    );
    assert!(!refusal.status.success());
    assert!(String::from_utf8_lossy(&refusal.stderr)
        .contains("check evidence is missing or unverified"));
    assert_eq!(f.history(), before);
}

#[test]
fn missing_accepted_result_artifact_invalidates_whole_goal_readiness() {
    let f = Fixture::new("w2ch-missing-accepted-artifact");
    f.ok(
        json!({"action":"init","sourceRef":"fixture:one-requirement",
        "sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}),
    );
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let source_sha = f.view()["source"]["sha256"].as_str().unwrap().to_owned();
    f.ok(
        json!({"action":"cover","expectedRevision":2,"bindingRevision":1,
        "sourceSha256":source_sha}),
    );
    f.return_stage("scoped", "Bounded scope");
    f.return_stage("completed", "A completed worker result");
    assert!(f.call(&["work", "check", &f.work]).status.success());
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let event: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let conditions = f.view()["currentConditionsSha256"].clone();
    f.ok(
        json!({"action":"support","expectedRevision":3,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event["event"]["eventSha256"],
        "conditionsSha256":conditions}),
    );
    let accepted = loop {
        let next = f.call(&["work", "next", &f.work, "--full"]);
        assert!(next.status.success(), "{next:?}");
        let next: Value = serde_json::from_slice(&next.stdout).unwrap();
        let stage = &next["next"];
        if stage["outcomes"]
            .as_array()
            .is_some_and(|outcomes| outcomes.iter().any(|outcome| outcome == "accepted"))
        {
            break f.return_stage("accepted", "Accepted worker result");
        }
        match stage["action"].as_str().unwrap() {
            "check" => assert!(f.call(&["work", "check", &f.work]).status.success()),
            "spawn" if stage["role"] == "reviewer" => {
                f.return_stage("approved", "Reviewed worker result");
            }
            other => panic!("unexpected stage before acceptance: {other} {stage}"),
        }
    };
    let inspected = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        accepted["eventSha256"].as_str().unwrap(),
    ]);
    assert!(inspected.status.success(), "{inspected:?}");
    let inspected: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    let artifact = inspected["event"]["artifact"]["path"].as_str().unwrap();
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Finish two requirements on one work",
        "--obligation",
        "first",
        "--disposition",
        "accepted",
        "--result-ref",
        &f.work,
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let closed = f.call(&[
        "goal",
        "close",
        "--goal-id",
        &f.work,
        "--result-ref",
        &f.work,
    ]);
    assert!(closed.status.success(), "{closed:?}");
    assert_eq!(f.view()["wholeGoalReady"], true);
    assert!(motif_active(&world(&f), "exit_sign"));
    let before = f.history();
    fs::remove_file(f.root.join(artifact)).unwrap();
    assert_eq!(f.view()["wholeGoalReady"], false);
    assert!(!motif_active(&world(&f), "exit_sign"));
    assert_eq!(f.history(), before);
}

#[test]
fn caller_reported_check_cannot_be_observed_requirement_support() {
    let f = Fixture::new("w2ch-reported-check");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    f.return_stage("scoped", "Bounded scope");
    let worker = f.return_stage("completed", "A completed worker result");
    assert!(f.call(&["work", "check", &f.work]).status.success());
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        f.work.strip_prefix("smw_").unwrap()
    );
    let reported = f.call(&[
        "run",
        "record-check",
        &ledger,
        "--target",
        worker["eventSha256"].as_str().unwrap(),
        "--requirement",
        "first",
        "--check-command",
        "true",
        "--exit-code",
        "0",
    ]);
    assert!(reported.status.success(), "{reported:?}");
    let reported: Value = serde_json::from_slice(&reported.stdout).unwrap();
    let conditions = f.view()["currentConditionsSha256"].clone();
    let before = f.history();
    let refusal = f.record(
        json!({"action":"support","expectedRevision":2,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":reported["event"]["eventSha256"],
        "conditionsSha256":conditions}),
    );
    assert!(!refusal.status.success());
    assert!(String::from_utf8_lossy(&refusal.stderr)
        .contains("support requires a passing check bound to the named requirement"));
    assert_eq!(f.history(), before);
}

#[test]
fn successor_goal_does_not_inherit_a_different_works_continuation() {
    let f = Fixture::new("w2ch-successor");
    f.initialize();
    let changed = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        "goal-b",
        "--goal",
        "New goal",
        "--obligation",
        "new obligation",
    ]);
    assert!(changed.status.success(), "{changed:?}");
    let changed: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert!(changed["continuation"].is_null());
    assert!(f.call(&["work", "next", &f.work]).status.success());
}

#[test]
fn long_requirement_history_has_bounded_exact_read_and_refinement_limit() {
    let f = Fixture::new("w2ch-history-expansion");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let prior_text = "First requirement.";
    for index in 0..32_u64 {
        f.ok(json!({"action":"refine","expectedRevision":index + 2,
            "bindingRevision":1,"id":"first",
            "text":format!("revision-{index}-{}", "\n".repeat(1000)),
            "sourceRef":format!("operator:{index}-{}", "\n".repeat(240)),
            "expectedRequirementRevision":index + 1}));
    }
    let before = f.history();
    let rejected = f.record(json!({"action":"refine","expectedRevision":34,
        "bindingRevision":1,"id":"first","text":"one revision too many",
        "sourceRef":"operator:limit","expectedRequirementRevision":33}));
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr)
        .contains("requirement refinement history bound exceeded"));
    assert_eq!(f.history(), before);

    let summary = f.call(&["work", "continuation", &f.work]);
    assert!(summary.status.success(), "{summary:?}");
    assert!(summary.stdout.len() <= 64 * 1024);
    let summary: Value = serde_json::from_slice(&summary.stdout).unwrap();
    assert_eq!(summary["requiresExpansion"], true);

    let section = f.call(&["work", "continuation", &f.work, "--section", "requirements"]);
    assert!(section.status.success(), "{section:?}");
    assert!(section.stdout.len() <= 64 * 1024);
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    assert_eq!(section["requiresExpansion"], true);

    let item = f.call(&[
        "work",
        "continuation",
        &f.work,
        "--section",
        "requirements",
        "--index",
        "0",
    ]);
    assert!(item.status.success(), "{item:?}");
    assert!(item.stdout.len() <= 64 * 1024);
    let item: Value = serde_json::from_slice(&item.stdout).unwrap();
    assert_eq!(item["requiresExpansion"], true);
    assert_eq!(item["historyCount"], 32);

    let history = f.call(&[
        "work",
        "continuation",
        &f.work,
        "--section",
        "requirements",
        "--index",
        "0",
        "--history-index",
        "0",
    ]);
    assert!(history.status.success(), "{history:?}");
    assert!(history.stdout.len() <= 64 * 1024);
    let history: Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(history["historyEntry"]["text"], prior_text);
    assert_eq!(history["historyIndex"], 0);
    fs::copy(f.root.join("exitbind.json"), f.root.join("alternate.json")).unwrap();
    let alternate = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "continuation",
            &f.work,
            "--section",
            "requirements",
            "--index",
            "0",
            "--config",
            "alternate.json",
        ])
        .output()
        .unwrap();
    assert!(alternate.status.success(), "{alternate:?}");
    let alternate: Value = serde_json::from_slice(&alternate.stdout).unwrap();
    assert_eq!(alternate["historyReadSameConfigRequired"], false);
    assert_eq!(alternate["historyReadSameExecutableRequired"], false);
    let argv = alternate["historyRead"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            if part == "N" {
                "0"
            } else {
                part.as_str().unwrap()
            }
        })
        .collect::<Vec<_>>();
    let read = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(read.status.success(), "{read:?}");
    let read: Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(read["historyEntry"]["text"], prior_text);
    assert_eq!(f.history(), before);
}

use super::*;

#[test]
fn host_handoff_fences_stale_writers_and_preserves_uncertain_operations() {
    let f = Fixture::new("w2ch-binding");
    f.initialize();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(f.root.join(".exitbind/session-goal.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    let view = f.view();
    assert_eq!(view["source"]["coverageConfirmed"], false);
    assert_eq!(view["requirements"].as_array().unwrap().len(), 1);
    assert_eq!(view["wholeGoalReady"], false);

    let first = f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-1","hostVersion":"0.156.1"}),
    );
    assert_eq!(first["goalRevision"], 2);
    assert_eq!(first["effect"], "appended");
    assert_eq!(f.view()["binding"]["revision"], 1);
    let before = f.history();
    let duplicate = f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-1","hostVersion":"0.156.1"}),
    );
    assert_eq!(duplicate["goalRevision"], 2);
    assert_eq!(duplicate["effect"], "unchanged");
    assert_eq!(f.history(), before);
    assert!(!f
        .record(
            json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"claude","session":"claude-1","hostVersion":"2.1.280"})
        )
        .status
        .success());
    assert_eq!(f.history(), before);

    f.ok(
        json!({"action":"correct","expectedRevision":2,"bindingRevision":1,
        "id":"no-publication","text":"Do not publish this fixture","scope":"publication",
        "sourceRef":"operator:fixture","supersedes":null}),
    );
    let params = sha("read-only probe v1");
    f.ok(
        json!({"action":"intent","expectedRevision":3,"bindingRevision":1,
        "id":"probe","parametersSha256":params,"description":"Read-only native probe"}),
    );
    let before = f.history();
    f.ok(
        json!({"action":"intent","expectedRevision":3,"bindingRevision":1,
        "id":"probe","parametersSha256":params,"description":"Read-only native probe"}),
    );
    assert_eq!(f.history(), before);
    assert!(!f
        .record(
            json!({"action":"intent","expectedRevision":4,"bindingRevision":1,
        "id":"probe","parametersSha256":sha("different"),"description":"Read-only native probe"})
        )
        .status
        .success());
    assert_eq!(f.history(), before);

    f.ok(
        json!({"action":"bind","expectedRevision":4,"expectedBindingRevision":1,
        "host":"claude","session":"claude-1","hostVersion":"2.1.280"}),
    );
    assert!(!f
        .record(
            json!({"action":"intent","expectedRevision":5,"bindingRevision":1,
        "id":"stale","parametersSha256":params,"description":"stale writer"})
        )
        .status
        .success());
    assert!(!f
        .record(
            json!({"action":"observe","expectedRevision":5,"bindingRevision":2,
        "id":"probe","status":"failed","handle":null,"resultEventSha256":null})
        )
        .status
        .success());

    f.ok(json!({"action":"child","expectedRevision":5,"bindingRevision":2,
        "assignment":"inspect second","nativeChild":"claude-child-1",
        "resultText":"Second requirement is still open.","resultSha256":sha("Second requirement is still open.")}));
    f.ok(
        json!({"action":"bind","expectedRevision":6,"expectedBindingRevision":2,
        "host":"codex","session":"codex-2","hostVersion":"0.156.1"}),
    );
    let view = f.view();
    assert_eq!(view["corrections"][0]["id"], "no-publication");
    assert_eq!(view["operations"][0]["status"], "uncertain");
    assert!(motif_active(&world(&f), "low_resolution_poster"));
    assert!(!motif_active(&world(&f), "distant_payphone"));
    assert_eq!(view["children"][0]["nativeChild"], "claude-child-1");
    assert_eq!(view["binding"]["revision"], 3);
    assert_eq!(view["wholeGoalReady"], false);
    let entry = f.call(&["work", "next", &f.work]);
    assert!(entry.status.success(), "{entry:?}");
    assert!(entry.stdout.len() <= 8 * 1024);
    let entry: Value = serde_json::from_slice(&entry.stdout).unwrap();
    let before = f.history();
    let continuation = if entry["continuation"]["requiresExpansion"] == true {
        let argv = entry["continuation"]["command"].as_array().unwrap();
        let expanded = super::support::git_topology::command(argv[0].as_str().unwrap())
            .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
            .output()
            .unwrap();
        assert!(expanded.status.success(), "{expanded:?}");
        serde_json::from_slice::<Value>(&expanded.stdout).unwrap()
    } else {
        entry["continuation"].clone()
    };
    assert_eq!(continuation["corrections"][0]["id"], "no-publication");
    assert_eq!(f.history(), before);
}

#[test]
fn exact_check_supports_one_revision_without_closing_the_whole_goal() {
    let f = Fixture::new("w2ch-support");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-1","hostVersion":"0.156.1"}),
    );
    f.return_stage("scoped", "Bounded scope");
    f.return_stage("completed", "A completed worker result");
    let first_check = f.call(&["work", "check", &f.work]);
    assert!(first_check.status.success(), "{first_check:?}");
    let first_check: Value = serde_json::from_slice(&first_check.stdout).unwrap();
    let checked = f.call(&["work", "check", &f.work]);
    assert!(checked.status.success(), "{checked:?}");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let event = &checked["event"];
    assert_eq!(event["action"], "check");
    assert_eq!(event["result"]["code"], 0);
    let event_sha = event["eventSha256"].as_str().unwrap();
    let exact = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        event_sha,
    ]);
    assert!(exact.status.success(), "{exact:?}");
    let exact_event: Value = serde_json::from_slice(&exact.stdout).unwrap();
    assert_eq!(exact_event["event"]["requirementId"], "first");
    let current = f.view();
    assert_eq!(
        exact_event["event"]["inputsSha256"],
        current["currentInputsSha256"]
    );
    let conditions = current["currentConditionsSha256"].as_str().unwrap();
    let before = f.history();
    assert!(!f
        .record(
            json!({"action":"support","expectedRevision":2,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":first_check["event"]["eventSha256"],"conditionsSha256":conditions})
        )
        .status
        .success());
    assert_eq!(f.history(), before);
    f.ok(
        json!({"action":"support","expectedRevision":2,"bindingRevision":1,
        "requirementId":"first","requirementRevision":1,
        "resultEventSha256":event_sha,"conditionsSha256":conditions}),
    );
    let view = f.view();
    assert_eq!(view["requirements"][0]["unresolved"], false);
    assert_eq!(view["wholeGoalReady"], false);
    let before = f.history();
    let exact_again = f.call(&[
        "run",
        "inspect",
        &format!(
            ".exitbind/runs/work-{}.jsonl",
            f.work.strip_prefix("smw_").unwrap()
        ),
        "--event",
        event_sha,
    ]);
    assert!(exact_again.status.success(), "{exact_again:?}");
    assert_eq!(exact.stdout, exact_again.stdout);
    assert_eq!(f.history(), before);
    f.ok(
        json!({"action":"refine","expectedRevision":3,"bindingRevision":1,
        "id":"first","text":"First requirement with changed condition.",
        "sourceRef":"operator:revision","expectedRequirementRevision":1}),
    );
    let revised = f.view();
    assert_eq!(revised["requirements"][0]["requirement"]["revision"], 2);
    assert_eq!(revised["requirements"][0]["unresolved"], true);
    assert_eq!(revised["source"]["text"], "First requirement.");
    let before = f.history();
    let refusal = f.record(
        json!({"action":"support","expectedRevision":4,"bindingRevision":1,
            "requirementId":"first","requirementRevision":2,
            "resultEventSha256":event_sha,
            "conditionsSha256":revised["currentConditionsSha256"]}),
    );
    assert!(!refusal.status.success());
    assert!(String::from_utf8_lossy(&refusal.stderr)
        .contains("check predates the current requirement revision"));
    assert_eq!(f.history(), before);
    assert_eq!(f.view()["requirements"][0]["unresolved"], true);
}

#[test]
fn native_child_text_is_exact_and_retries_cannot_replace_it() {
    let f = Fixture::new("w2ch-child-exact");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"claude","session":"claude-1","hostVersion":"2.1.280"}),
    );
    let text = "Observed child result. ".repeat(180);
    assert!(text.len() > 2048);
    let input = json!({"action":"child","expectedRevision":2,"bindingRevision":1,
        "assignment":"inspect second","nativeChild":"native-agent-1",
        "resultText":text,"resultSha256":sha(&text)});
    f.ok(input.clone());
    let before = f.history();
    let view = f.view();
    assert_eq!(view["children"][0]["resultText"], text);
    f.ok(input);
    assert_eq!(f.history(), before);
    assert!(!f
        .record(
            json!({"action":"child","expectedRevision":3,"bindingRevision":1,
        "assignment":"inspect second","nativeChild":"native-agent-1",
        "resultText":"different","resultSha256":sha("different")})
        )
        .status
        .success());
    assert_eq!(f.history(), before);
}

#[test]
fn child_ids_are_scoped_to_native_sessions() {
    let f = Fixture::new("w2ch-child-origin");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"s1","hostVersion":"0.156.1"}),
    );
    let first = "first child result";
    f.ok(
        json!({"action":"child","expectedRevision":2,"bindingRevision":1,
        "assignment":"inspect","nativeChild":"local-1","resultText":first,
        "resultSha256":sha(first)}),
    );
    f.ok(
        json!({"action":"bind","expectedRevision":3,"expectedBindingRevision":1,
        "host":"claude","session":"s2","hostVersion":"2.1.280"}),
    );
    let second = "different child result";
    let input = json!({"action":"child","expectedRevision":4,"bindingRevision":2,
        "assignment":"inspect","nativeChild":"local-1","resultText":second,
        "resultSha256":sha(second)});
    f.ok(input.clone());
    let view = f.view();
    assert_eq!(view["children"].as_array().unwrap().len(), 2);
    assert_ne!(
        view["children"][0]["childKey"],
        view["children"][1]["childKey"]
    );
    assert_eq!(view["children"][0]["origin"]["session"], "s1");
    assert_eq!(view["children"][1]["origin"]["session"], "s2");
    let before = f.history();
    f.ok(input);
    assert_eq!(f.history(), before);
}

#[test]
fn accumulated_children_keep_a_bounded_read_only_route() {
    let f = Fixture::new("w2ch-child-overflow");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"claude","session":"s1","hostVersion":"2.1.280"}),
    );
    let text = "X".repeat(8192);
    for index in 0..8 {
        let recorded = f.record(
            json!({"action":"child","expectedRevision":2+index,"bindingRevision":1,
            "assignment":"inspect","nativeChild":format!("child-{index}"),
            "resultText":text,"resultSha256":sha(&text)}),
        );
        assert!(recorded.status.success(), "{recorded:?}");
        assert!(
            recorded.stdout.len() <= 8 * 1024,
            "{}",
            recorded.stdout.len()
        );
    }
    let corrected = f.record(json!({"action":"correct","expectedRevision":10,
        "bindingRevision":1,"id":"one","text":"Keep the current correction.",
        "scope":"task","sourceRef":"operator:current"}));
    assert!(corrected.status.success(), "{corrected:?}");
    assert!(
        corrected.stdout.len() <= 8 * 1024,
        "{}",
        corrected.stdout.len()
    );
    let corrected: Value = serde_json::from_slice(&corrected.stdout).unwrap();
    let argv = corrected["nextAction"]["command"].as_array().unwrap();
    assert_eq!(corrected["nextAction"]["sameConfigRequired"], false);
    assert_eq!(corrected["nextAction"]["sameExecutableRequired"], false);
    let recovered = super::support::git_topology::command(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["goalRevision"], 11);
    fs::copy(f.root.join("exitbind.json"), f.root.join("alternate.json")).unwrap();
    let entry = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args(["work", "next", &f.work, "--config", "alternate.json"])
        .output()
        .unwrap();
    assert!(entry.status.success(), "{entry:?}");
    let entry: Value = serde_json::from_slice(&entry.stdout).unwrap();
    let route = &entry["continuation"];
    assert_eq!(route["requiresExpansion"], true);
    assert_eq!(route["sameConfigRequired"], false);
    assert_eq!(route["sameExecutableRequired"], false);
    let argv = route["command"].as_array().unwrap();
    let expanded = super::support::git_topology::command(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(expanded.status.success(), "{expanded:?}");
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    let children = expanded["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|section| section["name"] == "children")
        .unwrap();
    let argv = children["command"].as_array().unwrap();
    let section = super::support::git_topology::command(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(section.status.success(), "{section:?}");
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    let argv = section["items"][7]["command"].as_array().unwrap();
    let item = super::support::git_topology::command(argv[0].as_str().unwrap())
        .args(argv[1..].iter().map(|part| part.as_str().unwrap()))
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(item.status.success(), "{item:?}");
    let item: Value = serde_json::from_slice(&item.stdout).unwrap();
    assert_eq!(item["item"]["resultText"], text);
    let view = f.view();
    assert_eq!(view["requiresExpansion"], true);
    assert_eq!(view["sections"].as_array().unwrap().len(), 8);
    assert!(f.call(&["work", "next", &f.work]).status.success());
    assert!(f.call(&["work", "resume"]).status.success());
    let section = f.call(&["work", "continuation", &f.work, "--section", "children"]);
    assert!(section.status.success(), "{section:?}");
    let section: Value = serde_json::from_slice(&section.stdout).unwrap();
    assert_eq!(section["requiresExpansion"], true);
    assert_eq!(section["count"], 8);
    let item = f.call(&[
        "work",
        "continuation",
        &f.work,
        "--section",
        "children",
        "--index",
        "7",
    ]);
    assert!(item.status.success(), "{item:?}");
    let item: Value = serde_json::from_slice(&item.stdout).unwrap();
    assert_eq!(item["item"]["resultText"], text);
    assert_eq!(item["item"]["resultSha256"], sha(&text));
}

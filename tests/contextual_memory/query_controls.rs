//! Matched recall controls for the current recipient's authorized memory.
use super::*;

#[test]
fn same_four_questions_and_budget_expose_omissions_and_missing_source_uncertainty() {
    let f = Fixture::new(false);
    let source_scope = "project-source.v1";
    let mut config = f.config();
    config["memory"]["protocolScopes"] = json!(["project-lessons.v1", source_scope]);
    config["agents"]["worker"]["memoryRead"] = json!(["project-lessons.v1", source_scope]);
    for right in [
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryRevoke",
    ] {
        config["agents"]["lead"][right] = json!(["project-lessons.v1", source_scope]);
    }
    f.save(&config);
    let mut lesson = f.lesson();
    lesson["appliesTo"]["taskTerms"] = json!([]);
    lesson["fact"] =
        json!("Flag: --capture-mode=manual. Decision changed: check after the worker return.");
    f.write("summary.json", &lesson);
    f.accept("lead", "summary.json", "project-lessons.v1", LEDGER, "lead");
    let lines = [
        "Flag: --capture-mode=manual.",
        "Late request: retain the return transcript.",
        "Decision changed: check after the worker return.",
        "Rare negative: never enable historical scope.",
        "Known CLI literal: --scope.",
    ];
    fs::write(f.root.join("original.txt"), lines.join("\n") + "\n").unwrap();
    f.accept(
        "lead",
        "original.txt",
        source_scope,
        ".exitbind/memory/original.jsonl",
        "lead",
    );
    let work = f.worker(TASK);
    let detail = f.ok(&["work", "detail", &work, "--json"]);
    let route = &detail["sections"]["assignment"]["memoryQuery"];
    let mut command = route["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let query = command
        .iter_mut()
        .find(|word| word.as_str() == "--query=<LITERAL_QUERY>")
        .expect("prepared route carries one inline literal slot");
    *query = "--query=--scope".to_owned();
    if route["sameExecutableRequired"] == true {
        command.insert(0, env!("CARGO_BIN_EXE_exitbind").to_owned());
    }
    if route["sameConfigRequired"] == true {
        command.push(f.root.join("exitbind.json").to_str().unwrap().to_owned());
    }
    let filled = support::git_topology::command(&command[0])
        .current_dir(&f.root)
        .args(&command[1..])
        .env("EXITBIND_BINDINGS_DIR", f.root.join("fixture-bindings"))
        .output()
        .unwrap();
    assert!(filled.status.success(), "{filled:?}");
    let filled: Value = serde_json::from_slice(&filled.stdout).unwrap();
    assert_eq!(filled["query"], "--scope");
    assert!(
        filled["matches"].as_array().unwrap().iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains("Known CLI literal: --scope"))
        }),
        "{filled}"
    );
    let questions = [
        ("--capture-mode", "--capture-mode=manual", true),
        ("Late request", "retain the return transcript", false),
        ("Decision changed", "check after the worker return", true),
        ("Rare negative", "never enable historical scope", false),
    ];
    let query = |scope: &str, question: &str| {
        f.ok(&[
            "memory", "query", &work, "--query", question, "--scope", scope, "--json",
        ])
    };
    let contains = |answer: &Value, expected: &str| {
        answer["matches"].as_array().unwrap().iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(expected))
        })
    };
    let mut correct = [0usize; 3];
    let mut budget = None;
    for (question, expected, in_summary) in questions {
        for (index, scope) in ["source", "derived", "combined"].iter().enumerate() {
            let answer = query(scope, question);
            let this_budget = answer["resultByteBudget"].as_u64().unwrap();
            assert_eq!(
                *budget.get_or_insert(this_budget),
                this_budget,
                "{scope}: {answer}"
            );
            let found = contains(&answer, expected);
            let expected_found = *scope != "derived" || in_summary;
            assert_eq!(found, expected_found, "{scope} / {question}: {answer}");
            assert_eq!(
                answer["diagnostics"]["status"],
                if expected_found {
                    "matches"
                } else {
                    "no_match"
                },
                "{scope} / {question}: {answer}"
            );
            correct[index] += usize::from(found);
            assert_eq!(answer["omittedMatches"], 0, "{scope}: {answer}");
        }
    }
    assert_eq!(correct, [4, 2, 4], "four identical questions per scope");

    fs::remove_file(f.root.join("original.txt")).unwrap();
    for scope in ["source", "combined"] {
        let answer = query(scope, "Late request");
        assert_eq!(answer["diagnostics"]["status"], "incomplete");
        assert_eq!(answer["diagnostics"]["sourceStates"]["unavailable"], true);
        assert_eq!(answer["diagnostics"]["sourceStates"]["stale"], false);
        assert!(
            !contains(&answer, "retain the return transcript"),
            "{answer}"
        );
        assert!(
            answer["unavailable"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| {
                    item["identity"]["sourcePath"] == "original.txt"
                        && item["reason"] == "source_unavailable_or_changed"
                }),
            "missing source must be reported as uncertainty: {answer}"
        );
    }
    let still_known = query("combined", "--capture-mode");
    assert!(
        contains(&still_known, "--capture-mode=manual"),
        "{still_known}"
    );
    assert!(
        still_known["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["identity"]["sourcePath"] == "original.txt" }),
        "remaining derived matches do not make the absent original complete: {still_known}"
    );
}

#[test]
fn source_observations_distinguish_no_match_stale_missing_and_revoked() {
    let f = Fixture::new(false);
    let scope = "project-source.v1";
    let ledger = ".exitbind/memory/original.jsonl";
    let mut config = f.config();
    config["memory"]["protocolScopes"] = json!(["project-lessons.v1", scope]);
    config["agents"]["worker"]["memoryRead"] = json!(["project-lessons.v1", scope]);
    for right in [
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryRevoke",
    ] {
        config["agents"]["lead"][right] = json!(["project-lessons.v1", scope]);
    }
    f.save(&config);
    let mut lesson = f.lesson();
    lesson["appliesTo"]["taskTerms"] = json!([]);
    lesson["fact"] = json!("summary remains available");
    f.write("summary.json", &lesson);
    f.accept("lead", "summary.json", "project-lessons.v1", LEDGER, "lead");
    let original = "known original fact\n";
    fs::write(f.root.join("original.txt"), original).unwrap();
    f.accept("lead", "original.txt", scope, ledger, "lead");
    let work = f.worker(TASK);
    let query = |scope: &str, literal: &str| {
        f.ok(&[
            "memory", "query", &work, "--query", literal, "--scope", scope, "--json",
        ])
    };
    let absent = query("source", "not present");
    assert_eq!(absent["diagnostics"]["status"], "no_match", "{absent}");
    assert_eq!(absent["unavailable"], json!([]));
    let found = query("source", "known original");
    assert_eq!(found["diagnostics"]["status"], "matches", "{found}");
    let route = found["matches"][0]["provenance"][0]["readRoute"].clone();
    let read = || {
        let mut command = route["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if route["sameExecutableRequired"] == true {
            command.insert(0, env!("CARGO_BIN_EXE_exitbind").to_owned());
        }
        if route["sameConfigRequired"] == true {
            command.push(f.root.join("exitbind.json").to_str().unwrap().to_owned());
        }
        support::git_topology::command(&command[0])
            .current_dir(&f.root)
            .args(&command[1..])
            .env("EXITBIND_BINDINGS_DIR", f.root.join("fixture-bindings"))
            .output()
            .unwrap()
    };
    assert!(read().status.success());

    fs::write(f.root.join("original.txt"), "changed private bytes\n").unwrap();
    let stale = query("source", "known original");
    assert_eq!(stale["diagnostics"]["status"], "incomplete", "{stale}");
    assert_eq!(stale["diagnostics"]["sourceStates"]["stale"], true);
    assert_eq!(stale["diagnostics"]["sourceStates"]["unavailable"], false);
    assert_eq!(
        stale["unavailable"][0]["reason"],
        "source_unavailable_or_changed"
    );
    assert!(!stale.to_string().contains("changed private bytes"));
    assert!(!read().status.success(), "stale bytes must not expand");
    let derived = query("derived", "summary remains");
    assert_eq!(derived["diagnostics"]["status"], "matches", "{derived}");
    assert_eq!(derived["diagnostics"]["sourceStates"]["stale"], false);
    let combined = query("combined", "summary remains");
    assert_eq!(
        combined["diagnostics"]["status"], "incomplete",
        "{combined}"
    );
    assert_eq!(combined["diagnostics"]["sourceStates"]["stale"], true);
    assert!(combined.to_string().contains("summary remains available"));

    fs::remove_file(f.root.join("original.txt")).unwrap();
    let missing = query("source", "known original");
    assert_eq!(missing["diagnostics"]["status"], "incomplete", "{missing}");
    assert_eq!(missing["diagnostics"]["sourceStates"]["stale"], false);
    assert_eq!(missing["diagnostics"]["sourceStates"]["unavailable"], true);
    assert_eq!(
        missing["unavailable"][0]["reason"],
        "source_unavailable_or_changed"
    );
    assert!(!read().status.success(), "missing source must not expand");

    std::os::unix::fs::symlink("summary.json", f.root.join("original.txt")).unwrap();
    let unsafe_source = f.call(&[
        "memory",
        "query",
        &work,
        "--query",
        "summary remains",
        "--scope",
        "source",
        "--json",
    ]);
    assert!(!unsafe_source.status.success(), "unsafe source must refuse");
    assert!(!String::from_utf8_lossy(&unsafe_source.stdout).contains("summary remains available"));
    fs::remove_file(f.root.join("original.txt")).unwrap();

    fs::write(f.root.join("original.txt"), original).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let path = f.root.join("original.txt");
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
        if fs::File::open(&path).is_err() {
            let unreadable = query("source", "known original");
            assert_eq!(unreadable["diagnostics"]["status"], "incomplete");
            assert_eq!(
                unreadable["diagnostics"]["sourceStates"]["unavailable"],
                true
            );
            assert_eq!(unreadable["diagnostics"]["sourceStates"]["stale"], false);
            assert!(
                !read().status.success(),
                "unreadable source must not expand"
            );
        } else {
            eprintln!("host privileges bypass file mode; OS unreadability was not exercised");
        }
        fs::set_permissions(path, permissions).unwrap();
    }
    f.ok(&["memory", "revoke", "lead", ledger]);
    let revoked = query("source", "known original");
    assert_eq!(revoked["diagnostics"]["status"], "incomplete", "{revoked}");
    assert_eq!(
        revoked["diagnostics"]["sourceStates"]["noLongerEligible"],
        true
    );
    assert_eq!(revoked["unavailable"][0]["reason"], "no_longer_eligible");
    assert!(!revoked.to_string().contains("known original fact"));
    assert!(!read().status.success(), "revoked source must not expand");
}

#[test]
fn source_query_denial_discloses_no_hidden_item_and_keeps_frozen_rights() {
    let f = Fixture::new(false);
    let scope = "project-source.v1";
    let mut config = f.config();
    config["memory"]["protocolScopes"] = json!(["project-lessons.v1", scope]);
    for right in ["memoryWrite", "memoryReview", "memoryPromote"] {
        config["agents"]["lead"][right] = json!(["project-lessons.v1", scope]);
    }
    f.save(&config);
    let mut lesson = f.lesson();
    lesson["appliesTo"]["taskTerms"] = json!([]);
    lesson["fact"] = json!("visible shared marker");
    f.write("summary.json", &lesson);
    f.accept("lead", "summary.json", "project-lessons.v1", LEDGER, "lead");
    let hidden = "shared marker hidden value\n";
    let ledger = ".exitbind/memory/hidden.jsonl";
    fs::write(f.root.join("hidden-source.txt"), hidden).unwrap();
    f.accept("lead", "hidden-source.txt", scope, ledger, "lead");
    let hidden_item = f.ok(&["memory", "inspect", ledger, "--json"])["items"][0]["itemId"]
        .as_str()
        .unwrap()
        .to_owned();
    let work = f.worker(TASK);
    let query = |scope: &str| {
        f.ok(&[
            "memory",
            "query",
            &work,
            "--query",
            "shared marker",
            "--scope",
            scope,
            "--json",
        ])
    };
    let denied = query("source");
    assert_eq!(denied["diagnostics"]["status"], "access_denied", "{denied}");
    assert_eq!(denied["diagnostics"]["scopeAccess"]["source"], "denied");
    assert_eq!(denied["matches"], json!([]));
    for secret in [
        "hidden-source.txt",
        "hidden.jsonl",
        "hidden value",
        &hidden_item,
        &digest(hidden.as_bytes()),
    ] {
        assert!(!denied.to_string().contains(secret), "{denied}");
    }
    let ledger_path = f.root.join(ledger);
    let saved_ledger = fs::read(&ledger_path).unwrap();
    fs::remove_file(&ledger_path).unwrap();
    let mut without_hidden = query("source");
    let mut with_hidden = denied.clone();
    with_hidden.as_object_mut().unwrap().remove("observedAt");
    without_hidden.as_object_mut().unwrap().remove("observedAt");
    assert_eq!(
        with_hidden, without_hidden,
        "denied query must not disclose item existence"
    );
    fs::write(&ledger_path, saved_ledger).unwrap();
    let combined = query("combined");
    assert_eq!(
        combined["diagnostics"]["status"], "incomplete",
        "{combined}"
    );
    assert_eq!(combined["diagnostics"]["scopeAccess"]["source"], "denied");
    assert_eq!(combined["diagnostics"]["scopeAccess"]["derived"], "allowed");
    assert!(combined.to_string().contains("visible shared marker"));
    assert!(!combined.to_string().contains("hidden-source.txt"));
    assert!(!combined.to_string().contains("hidden value"));
    assert!(!combined.to_string().contains(&hidden_item));

    config["agents"]["worker"]["memoryRead"] = json!(["project-lessons.v1", scope]);
    f.save(&config);
    let widened = query("source");
    assert_eq!(
        widened["diagnostics"]["status"], "access_denied",
        "{widened}"
    );
    assert!(!widened.to_string().contains("hidden-source.txt"));
    assert!(!widened.to_string().contains("hidden value"));
}

#[test]
fn result_budget_omission_is_not_reported_as_no_match() {
    let f = Fixture::new(false);
    let scope = "project-source.v1";
    let mut config = f.config();
    config["memory"]["maxBytes"] = json!(1200);
    config["memory"]["protocolScopes"] = json!([scope]);
    config["agents"]["worker"]["memoryRead"] = json!([scope]);
    for right in ["memoryWrite", "memoryReview", "memoryPromote"] {
        config["agents"]["lead"][right] = json!([scope]);
    }
    f.save(&config);
    fs::write(
        f.root.join("large.txt"),
        format!("NEEDLE{}\n", "x".repeat(1100)),
    )
    .unwrap();
    f.accept(
        "lead",
        "large.txt",
        scope,
        ".exitbind/memory/large.jsonl",
        "lead",
    );
    let work = f.worker(TASK);
    let result = f.ok(&[
        "memory", "query", &work, "--query", "NEEDLE", "--scope", "source", "--json",
    ]);
    assert_eq!(result["resultByteBudget"], 1200);
    assert!(result["omittedMatches"].as_u64().unwrap() > 0, "{result}");
    assert_eq!(result["matches"], json!([]));
    assert_eq!(result["diagnostics"]["status"], "incomplete", "{result}");
}

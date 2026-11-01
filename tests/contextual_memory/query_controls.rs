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
            correct[index] += usize::from(found);
            assert_eq!(answer["omittedMatches"], 0, "{scope}: {answer}");
        }
    }
    assert_eq!(correct, [4, 2, 4], "four identical questions per scope");

    fs::remove_file(f.root.join("original.txt")).unwrap();
    for scope in ["source", "combined"] {
        let answer = query(scope, "Late request");
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

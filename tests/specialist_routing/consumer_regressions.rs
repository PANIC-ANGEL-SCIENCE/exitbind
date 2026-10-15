//! Public consumer counterexamples from independent review; no real learning claims.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn with_generic() -> Fixture {
    let f = Fixture::new();
    let mut c = f.config();
    for (actor, rights) in [
        ("record-specialist", vec!["memoryRead", "memoryWrite"]),
        ("reviewer", vec!["memoryReview"]),
        ("lead", vec!["memoryPromote", "memoryRevoke"]),
    ] {
        for right in rights {
            c["agents"][actor][right] = json!(["specialist.record-specialist.v1", "ordinary"]);
        }
    }
    c["memory"]["protocolScopes"] = json!(["specialist.record-specialist.v1", "ordinary"]);
    c["memory"]["maxItems"] = json!(16);
    c["memory"]["maxBytes"] = json!(65536);
    f.save(&c);
    f
}

fn accept_generic(f: &Fixture, source: &str, ledger: &str) {
    f.ok(&[
        "memory",
        "propose",
        "record-specialist",
        source,
        "--scope",
        "ordinary",
        "--ledger",
        ledger,
    ]);
    f.qualify(ledger);
}

fn fake_provider(f: &Fixture) -> (PathBuf, PathBuf) {
    let capture = f.root.join("review-regression-prompt");
    let fake = f.root.join("review-regression-provider");
    fs::write(&fake, format!("#!/bin/sh\ncat > '{}'\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"review-fixture\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"fixture captured\\\",\\\"reason\\\":\\\"\\\"}}\"}}}}' '{{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}'\n", capture.display())).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    (fake, capture)
}

#[test]
fn duplicate_object_members_never_hide_revoked_reserved_source_from_unrelated_native_delivery() {
    let f = with_generic();
    f.draft("specialist.json", "r1");
    let source = fs::read_to_string(f.root.join("specialist.json")).unwrap();
    f.propose("specialist.json", ".exitbind/memory/specialist.jsonl");
    f.qualify(".exitbind/memory/specialist.jsonl");
    let duplicate = format!("{{\"payload\":{source},\"payload\":\"ordinary later value\"}}");
    for (i, text) in [
        duplicate.clone(),
        format!(
            "{{\"payload\":{},\"payload\":\"ordinary later value\"}}",
            serde_json::to_string(&source).unwrap()
        ),
        format!("[{{\"nested\":{duplicate}}}]"),
    ]
    .iter()
    .enumerate()
    {
        let path = format!("duplicate-{i}.json");
        fs::write(f.root.join(&path), text).unwrap();
        accept_generic(&f, &path, &format!(".exitbind/memory/duplicate-{i}.jsonl"));
    }
    let ordinary = "{\"note\":\"first ordinary member\",\"note\":\"second ordinary member\"}";
    fs::write(f.root.join("ordinary-duplicate.json"), ordinary).unwrap();
    accept_generic(
        &f,
        "ordinary-duplicate.json",
        ".exitbind/memory/ordinary.jsonl",
    );
    f.ok(&[
        "memory",
        "revoke",
        "lead",
        ".exitbind/memory/specialist.jsonl",
    ]);
    let resolved = f.ok(&[
        "memory",
        "resolve",
        "record-specialist",
        "--task",
        "unrelated formatting",
    ]);
    let work = f.worker("unrelated formatting");
    let (fake, capture) = fake_provider(&f);
    f.ok(&["work", "act", &work, "--codex-bin", fake.to_str().unwrap()]);
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(prompt.contains("first ordinary member"));
    assert!(prompt.contains("second ordinary member"));
    assert!(!prompt.contains("Track presence and sum independently."));
    assert!(!prompt.contains("MEMORY [ordinary] duplicate-"));
    assert_eq!(resolved["references"].as_array().unwrap().len(), 1);
    assert_eq!(
        resolved["references"][0]["sourcePath"],
        "ordinary-duplicate.json"
    );
    assert_eq!(resolved["references"][0]["byteLength"], ordinary.len());
}

#[test]
fn repeated_escaped_quotes_remain_valid_ordinary_memory() {
    let f = with_generic();
    let source = format!(
        "Ordinary escaped quote example: \"{}\"\n",
        "\\\"".repeat(4096)
    );
    assert_eq!(source.len(), 8227);
    fs::write(f.root.join("ordinary.txt"), &source).unwrap();
    accept_generic(&f, "ordinary.txt", ".exitbind/memory/ordinary.jsonl");
    let resolved = f.ok(&[
        "memory",
        "resolve",
        "record-specialist",
        "--task",
        "unrelated formatting",
    ]);
    assert_eq!(resolved["references"].as_array().unwrap().len(), 1);
    assert_eq!(resolved["references"][0]["sourcePath"], "ordinary.txt");
    assert_eq!(resolved["references"][0]["byteLength"], source.len());
    assert_eq!(
        resolved["references"][0]["sourceSha256"],
        "b3b7fa65d1bedbfa533ebabe6697e0f7cb7b71074d76d92a1ce6e1211ed8cc7c"
    );
}

#[test]
fn fenced_and_encoded_reserved_aliases_stay_out_of_actual_unrelated_native_delivery_after_revoke() {
    let f = with_generic();
    f.draft("specialist.json", "r1");
    let source = fs::read_to_string(f.root.join("specialist.json")).unwrap();
    let object: Value = serde_json::from_str(&source).unwrap();
    let pretty = serde_json::to_string_pretty(&object).unwrap();
    let mut key_alias = serde_json::Map::new();
    key_alias.insert(source.clone(), json!("ordinary lookup value"));
    let key_alias = Value::Object(key_alias);
    let quoted = |text: &str, prefix: &str| {
        let mut result = String::new();
        for line in text.lines() {
            result.push_str(prefix);
            result.push_str(line);
            result.push('\n');
        }
        result
    };
    f.propose("specialist.json", ".exitbind/memory/specialist.jsonl");
    f.qualify(".exitbind/memory/specialist.jsonl");
    let envelopes = [
        format!("```json\n{source}\n```\n"),
        format!("An ordinary introduction.\n~~~json\n{source}\n~~~\nAn ordinary conclusion."),
        format!("   ````json\n{source}\n   ````\n"),
        serde_json::to_string(&source).unwrap(),
        json!({"payload":object.clone()}).to_string(),
        json!([object.clone()]).to_string(),
        serde_json::to_string(&json!({"payload":[object]}).to_string()).unwrap(),
        json!({"payload":format!("```json\n{source}\n```\n")}).to_string(),
        quoted(&format!("```json\n{pretty}\n```"), "> "),
        quoted(&format!("~~~json\n{source}\n~~~"), "> > "),
        format!("```json\n{source}"),
        quoted(&format!("~~~json\n{pretty}"), "> "),
        format!("- ```json\n  {source}\n  ```\n"),
        format!("> 1. ```json\n>    {source}\n>    ```\n"),
        format!("```text\nExample source follows:\n{pretty}\nEnd of example.\n```\n"),
        format!("Prior record: `{source}` End of example."),
        quoted(&pretty, "    "),
        quoted(&format!("```json\n{pretty}\n```"), "> ").replace('\n', "\r\n"),
        json!({"payload":quoted(&format!("```json\n{pretty}"), "> ")}).to_string(),
        key_alias.to_string(),
        json!({"payload":[key_alias]}).to_string(),
    ];
    fs::write(f.root.join("ordinary.md"), "Ordinary loose field words: procedure, guards and taskTerms.\n> ```json\n> {\"payload\":[{\"procedure\":\"ordinary example\"}]}\n> ```\n- Ordinary list guidance.\n```text\nUnclosed ordinary code with procedure and guards.\n").unwrap();
    let mut ordinary = fs::read_to_string(f.root.join("ordinary.md")).unwrap();
    ordinary.push_str(
        &serde_json::to_string(&std::collections::BTreeMap::from([(
            json!({"procedure":"ordinary example"}).to_string(),
            "ordinary lookup",
        )]))
        .unwrap(),
    );
    fs::write(f.root.join("ordinary.md"), ordinary).unwrap();
    accept_generic(&f, "ordinary.md", ".exitbind/memory/ordinary.jsonl");
    f.ok(&[
        "memory",
        "revoke",
        "lead",
        ".exitbind/memory/specialist.jsonl",
    ]);
    // Keep each native probe below its independent presentation item ceiling;
    // otherwise that ceiling could mask an eligibility failure.
    for (group, envelopes) in envelopes.chunks(6).enumerate() {
        let mut ledgers = Vec::new();
        for (i, text) in envelopes.iter().enumerate() {
            let path = format!("wrapped-{group}-{i}.md");
            fs::write(f.root.join(&path), text).unwrap();
            let ledger = format!(".exitbind/memory/wrapped-{group}-{i}.jsonl");
            accept_generic(&f, &path, &ledger);
            ledgers.push(ledger);
        }
        let resolved = f.ok(&[
            "memory",
            "resolve",
            "record-specialist",
            "--task",
            "unrelated formatting",
        ]);
        let work = f.worker("unrelated formatting");
        let (fake, capture) = fake_provider(&f);
        f.ok(&["work", "act", &work, "--codex-bin", fake.to_str().unwrap()]);
        let prompt = fs::read_to_string(capture).unwrap();
        assert!(prompt.contains("Ordinary loose field words"));
        assert!(!prompt.contains("Track presence and sum independently."));
        assert!(!prompt.contains("wrapped-"));
        assert_eq!(resolved["references"].as_array().unwrap().len(), 1);
        assert_eq!(resolved["references"][0]["sourcePath"], "ordinary.md");
        for ledger in ledgers {
            f.ok(&["memory", "revoke", "lead", &ledger]);
        }
    }
}

#[test]
fn native_delivery_shares_one_total_item_and_byte_budget_without_dropping_ordinary_guidance() {
    for case in ["items", "bytes", "exact"] {
        let f = with_generic();
        f.draft("specialist.json", "r1");
        let ordinary = "Useful ordinary guidance remains selected.";
        fs::write(f.root.join("ordinary.txt"), ordinary).unwrap();
        let source_bytes = fs::read(f.root.join("specialist.json")).unwrap().len();
        let mut c = f.config();
        c["memory"]["maxItems"] = json!(if case == "items" { 1 } else { 2 });
        c["memory"]["maxBytes"] =
            json!(source_bytes + ordinary.len() - usize::from(case == "bytes"));
        f.save(&c);
        f.propose("specialist.json", ".exitbind/memory/specialist.jsonl");
        f.qualify(".exitbind/memory/specialist.jsonl");
        accept_generic(&f, "ordinary.txt", ".exitbind/memory/ordinary.jsonl");
        assert_eq!(
            f.ok(&[
                "memory",
                "resolve",
                "record-specialist",
                "--task",
                "record aggregation"
            ])["references"][0]["sourcePath"],
            "ordinary.txt"
        );
        let work = f.worker("record aggregation");
        let (fake, capture) = fake_provider(&f);
        let output = f.call(&["work", "act", &work, "--codex-bin", fake.to_str().unwrap()]);
        if case == "exact" {
            assert!(output.status.success(), "{output:?}");
            let prompt = fs::read_to_string(capture).unwrap();
            assert!(prompt.contains(ordinary));
            assert!(prompt.contains("Track presence and sum independently."));
        } else {
            assert!(!output.status.success(), "{case}");
            let message = String::from_utf8_lossy(&output.stdout).to_string()
                + &String::from_utf8_lossy(&output.stderr);
            assert!(
                message.contains("memory_budget_exceeded"),
                "{case}: {message}"
            );
            assert!(message.contains("attemptedItems=2"), "{message}");
            assert!(
                !capture.exists(),
                "provider started despite combined overflow"
            );
            assert!(!f.call(&["work", "detail", &work]).status.success());
        }
    }
}

#[test]
fn typed_source_preparation_checks_exact_inactive_bytes_and_guard_contract_without_lifecycle_effects(
) {
    let f = Fixture::new();
    let scope = "specialist.record-specialist.v1";
    let form = f.ok(&["memory", "form", "--scope", scope]);
    assert_eq!(form["fieldBounds"]["guards"]["maximumItems"], 4);
    assert!(form["fieldBounds"]["guards"]["element"]["path"].is_string());
    assert_eq!(form["validationRoute"]["command"][3], "<SOURCE>");
    f.draft("draft.json", "r1");
    let valid = f.ok(&["memory", "form", "draft.json", "--scope", scope]);
    assert_eq!(valid["validation"]["valid"], true);
    assert_eq!(valid["validation"]["qualification"], "not_evaluated");
    assert_eq!(
        valid["validation"]["source"]["bytes"],
        fs::read(f.root.join("draft.json")).unwrap().len()
    );
    assert!(!f.root.join(".exitbind/memory/skill.jsonl").exists());
    let original: Value =
        serde_json::from_slice(&fs::read(f.root.join("draft.json")).unwrap()).unwrap();
    for case in ["prose", "traversal", "count", "digest", "foreign"] {
        let mut source = original.clone();
        let guard = json!({"path":"docs/current.md","sha256":"0".repeat(64)});
        source["guards"] = match case {
            "prose" => json!(["Do not grant authority."]),
            "traversal" => json!([{"path":"../outside.md","sha256":"0".repeat(64)}]),
            "count" => json!(vec![guard.clone(); 5]),
            "digest" => json!([{"path":"docs/current.md","sha256":"not-a-digest"}]),
            _ => {
                source["projectIdentity"] = json!("foreign");
                json!([])
            }
        };
        fs::write(f.root.join("invalid.json"), source.to_string()).unwrap();
        assert!(
            !f.call(&["memory", "form", "invalid.json", "--scope", scope])
                .status
                .success(),
            "{case}"
        );
    }
    let mut bounded = original;
    bounded["guards"] = json!((0..4)
        .map(|i| json!({"path":format!("docs/guard-{i}.md"),"sha256":"0".repeat(64)}))
        .collect::<Vec<_>>());
    fs::write(f.root.join("bounded.json"), bounded.to_string()).unwrap();
    assert_eq!(
        f.ok(&["memory", "form", "bounded.json", "--scope", scope])["validation"]["guardCount"],
        4
    );
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
}

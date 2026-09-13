use serde_json::Value;
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn project() -> (PathBuf, String) {
    let root = support::temp("memory");
    let output = invoke(&["init", "--root", root.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output));
    let config = root.join("soulmate.json").to_string_lossy().into_owned();
    (root, config)
}

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn configure(root: &Path, config: &str, agent: &str, cross_context: &str, max_bytes: u64) {
    let path = root.join("soulmate.json");
    let mut value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    value["memory"] = serde_json::json!({
        "root": ".soulmate/memory",
        "maxItems": 8,
        "maxBytes": max_bytes,
        "protocolScopes": ["invariants"],
        "syntheticScopes": ["synthetic"]
    });
    value["agents"][agent]["memoryRead"] = serde_json::json!(["invariants"]);
    value["agents"][agent]["crossContext"] = serde_json::json!(cross_context);
    value["agents"]["worker"]["memoryWrite"] = serde_json::json!(["invariants"]);
    value["agents"]["worker"]["memoryReview"] = serde_json::json!(["invariants"]);
    value["agents"]["worker"]["memoryReject"] = serde_json::json!(["invariants"]);
    value["agents"]["lead"]["memoryPromote"] = serde_json::json!(["invariants"]);
    value["agents"]["lead"]["memoryRevoke"] = serde_json::json!(["invariants"]);
    fs::write(
        config,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    fs::create_dir_all(root.join(".soulmate/memory")).unwrap();
}

fn set_max_items(config: &str, max_items: u64) {
    let path = Path::new(config);
    let mut value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    value["memory"]["maxItems"] = serde_json::json!(max_items);
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
}

fn accepted(root: &Path, config: &str) -> String {
    accepted_entry(
        root,
        config,
        "memory.md",
        ".soulmate/memory/invariant.jsonl",
        "accepted invariant\n",
    )
}

fn accepted_entry(root: &Path, config: &str, source: &str, ledger: &str, content: &str) -> String {
    fs::write(root.join(source), content).unwrap();
    assert!(invoke(&[
        "memory",
        "propose",
        "worker",
        source,
        "--scope",
        "invariants",
        "--ledger",
        ledger,
        "--config",
        config,
    ])
    .status
    .success());
    assert!(
        invoke(&["memory", "review", "worker", ledger, "--config", config,])
            .status
            .success()
    );
    assert!(
        invoke(&["memory", "promote", "lead", ledger, "--config", config,])
            .status
            .success()
    );
    ledger.to_owned()
}

#[test]
fn absent_memory_keeps_brief_shape() {
    let (root, config) = project();
    let output = invoke(&[
        "brief", "worker", "--task", "bounded", "--json", "--config", &config,
    ]);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.get("memoryReferences").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn accepted_memory_is_scoped_and_source_is_revalidated() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let ledger = accepted(&root, &config);
    let output = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["references"].as_array().unwrap().len(), 1);
    fs::write(root.join("memory.md"), "changed\n").unwrap();
    let changed = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!changed.status.success());
    assert!(text(&changed).contains("memory source changed"));
    let _ = ledger;
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cross_context_and_budget_are_fail_closed() {
    let (root, config) = project();
    configure(&root, &config, "worker", "none", 32_768);
    let _ledger = accepted(&root, &config);
    let denied = invoke(&["memory", "resolve", "worker", "--json", "--config", &config]);
    assert!(denied.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&denied.stdout).unwrap()["references"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    configure(&root, &config, "worker", "protocol-only", 1);
    let over = invoke(&["memory", "resolve", "worker", "--json", "--config", &config]);
    assert!(!over.status.success());
    let diagnostic = text(&over);
    assert!(diagnostic.contains("memory_budget_exceeded"));
    assert!(diagnostic.contains("itemId="));
    assert!(diagnostic.contains("attemptedItems=1 attemptedBytes=19 maxItems=8 maxBytes=1"));
    assert!(!diagnostic.contains("accepted invariant"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn count_budget_diagnostic_reports_attempted_count_and_limits_without_content() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    set_max_items(&config, 1);
    accepted_entry(
        &root,
        &config,
        "first.md",
        ".soulmate/memory/first.jsonl",
        "first invariant\n",
    );
    accepted_entry(
        &root,
        &config,
        "second.md",
        ".soulmate/memory/second.jsonl",
        "second invariant\n",
    );

    let over = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!over.status.success());
    let diagnostic = text(&over);
    assert!(diagnostic.contains("memory_budget_exceeded: itemId="));
    assert!(diagnostic.contains("attemptedItems=2 attemptedBytes=33 maxItems=1 maxBytes=32768"));
    assert!(!diagnostic.contains("first invariant"));
    assert!(!diagnostic.contains("second invariant"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_items_are_rejected() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let ledger = accepted(&root, &config);
    fs::copy(
        root.join(ledger),
        root.join(".soulmate/memory/duplicate.jsonl"),
    )
    .unwrap();
    let output = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!output.status.success());
    assert!(text(&output).contains("duplicate memory item id"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn multiple_ledgers_resolve_in_stable_filename_order() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    for (ledger, source) in [
        ("z-last.jsonl", "z-last.md"),
        ("a-first.jsonl", "a-first.md"),
    ] {
        fs::write(root.join(source), format!("{source}\n")).unwrap();
        let ledger = format!(".soulmate/memory/{ledger}");
        assert!(invoke(&[
            "memory",
            "propose",
            "worker",
            source,
            "--scope",
            "invariants",
            "--ledger",
            &ledger,
            "--config",
            &config,
        ])
        .status
        .success());
        assert!(
            invoke(&["memory", "review", "worker", &ledger, "--config", &config])
                .status
                .success()
        );
        assert!(
            invoke(&["memory", "promote", "lead", &ledger, "--config", &config])
                .status
                .success()
        );
    }

    let resolved = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(resolved.status.success());
    let value: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    let paths: Vec<&str> = value["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["sourcePath"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["a-first.md", "z-last.md"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn run_freezes_memory_and_rejects_revocation_drift() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let ledger = accepted(&root, &config);
    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &config,
    ]);
    assert!(started.status.success(), "{}", text(&started));
    assert!(
        invoke(&["memory", "revoke", "lead", &ledger, "--config", &config,])
            .status
            .success()
    );
    let next = invoke(&[
        "run",
        "next",
        ".soulmate/run.jsonl",
        "--json",
        "--config",
        &config,
    ]);
    assert!(!next.status.success());
    let diagnostic: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(diagnostic["classification"], "memory_drift");
    assert_eq!(diagnostic["agent"], "lead");
    assert!(diagnostic["expectedMemorySetSha256"].as_str().is_some());
    assert!(diagnostic["currentMemorySetSha256"].as_str().is_some());
    assert!(!text(&next).contains("accepted invariant"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unaccepted_and_time_expired_items_are_not_recalled() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    fs::write(root.join("rejected.md"), "rejected candidate\n").unwrap();
    let rejected = ".soulmate/memory/rejected.jsonl";
    assert!(invoke(&[
        "memory",
        "propose",
        "worker",
        "rejected.md",
        "--scope",
        "invariants",
        "--ledger",
        rejected,
        "--config",
        &config,
    ])
    .status
    .success());
    assert!(
        invoke(&["memory", "review", "worker", rejected, "--config", &config,])
            .status
            .success()
    );
    assert!(
        invoke(&["memory", "reject", "worker", rejected, "--config", &config,])
            .status
            .success()
    );

    fs::write(root.join("expired.md"), "expired candidate\n").unwrap();
    let expired = ".soulmate/memory/expired.jsonl";
    assert!(invoke(&[
        "memory",
        "propose",
        "worker",
        "expired.md",
        "--scope",
        "invariants",
        "--ledger",
        expired,
        "--expires-at",
        "2000-01-01T00:00:00Z",
        "--config",
        &config,
    ])
    .status
    .success());
    assert!(
        invoke(&["memory", "review", "worker", expired, "--config", &config,])
            .status
            .success()
    );
    assert!(
        invoke(&["memory", "promote", "lead", expired, "--config", &config,])
            .status
            .success()
    );

    let resolved = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(resolved.status.success(), "{}", text(&resolved));
    let value: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert!(value["references"].as_array().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn accepted_memory_survives_later_config_and_actor_profile_changes() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let _ledger = accepted(&root, &config);

    let config_path = root.join("soulmate.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    value["orchestration"]["maxParallel"] = serde_json::json!(1);
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    fs::write(
        root.join("soulmate/agents/worker.md"),
        "# Worker\n\nChanged after the memory was accepted.\n",
    )
    .unwrap();

    let resolved = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(resolved.status.success(), "{}", text(&resolved));
    let value: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert_eq!(value["references"].as_array().unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recall_outputs_references_without_copying_memory_content() {
    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let _ledger = accepted(&root, &config);

    let resolved = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(resolved.status.success(), "{}", text(&resolved));
    assert!(!text(&resolved).contains("accepted invariant"));
    let plain = invoke(&["memory", "resolve", "lead", "--config", &config]);
    assert!(plain.status.success(), "{}", text(&plain));
    assert!(String::from_utf8_lossy(&plain.stdout).contains(" memory.md "));
    assert!(!String::from_utf8_lossy(&plain.stdout).contains('"'));
    assert!(!text(&plain).contains("accepted invariant"));

    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &config,
    ]);
    assert!(started.status.success(), "{}", text(&started));
    assert!(!text(&started).contains("accepted invariant"));
    assert!(!fs::read_to_string(root.join(".soulmate/run.jsonl"))
        .unwrap()
        .contains("accepted invariant"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn malformed_and_symlinked_recall_evidence_fails_closed() {
    use std::os::unix::fs::symlink;

    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    fs::write(root.join(".soulmate/memory/broken.jsonl"), "not json\n").unwrap();
    let malformed = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!malformed.status.success());
    assert!(text(&malformed).contains("invalid memory ledger"));

    fs::remove_file(root.join(".soulmate/memory/broken.jsonl")).unwrap();
    fs::write(root.join(".soulmate/target.jsonl"), "not used\n").unwrap();
    symlink(
        root.join(".soulmate/target.jsonl"),
        root.join(".soulmate/memory/linked.jsonl"),
    )
    .unwrap();
    let linked = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!linked.status.success());
    assert!(text(&linked).contains("must not be a symlink"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn recall_source_rejects_a_symlinked_parent_directory() {
    use std::os::unix::fs::symlink;

    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let source_dir = root.join("memory-source");
    fs::create_dir(&source_dir).unwrap();
    fs::write(source_dir.join("invariant.md"), "accepted invariant\n").unwrap();
    let ledger = ".soulmate/memory/parent-link.jsonl";
    assert!(invoke(&[
        "memory",
        "propose",
        "worker",
        "memory-source/invariant.md",
        "--scope",
        "invariants",
        "--ledger",
        ledger,
        "--config",
        &config,
    ])
    .status
    .success());
    assert!(
        invoke(&["memory", "review", "worker", ledger, "--config", &config])
            .status
            .success()
    );
    assert!(
        invoke(&["memory", "promote", "lead", ledger, "--config", &config])
            .status
            .success()
    );

    let outside = root.with_extension("outside-memory-source");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("invariant.md"), "accepted invariant\n").unwrap();
    fs::remove_dir_all(&source_dir).unwrap();
    symlink(&outside, &source_dir).unwrap();

    let resolved = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!resolved.status.success());
    assert!(!text(&resolved).contains("accepted invariant"));

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn recall_source_rejects_a_fifo_without_blocking() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let (root, config) = project();
    configure(&root, &config, "lead", "protocol-only", 32_768);
    let _ledger = accepted(&root, &config);
    let source = root.join("memory.md");
    fs::remove_file(&source).unwrap();
    let source_name = CString::new(source.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(source_name.as_ptr(), 0o600) }, 0);

    let resolved = invoke(&["memory", "resolve", "lead", "--json", "--config", &config]);
    assert!(!resolved.status.success());
    assert!(text(&resolved).contains("must be a regular file"));
    assert!(!text(&resolved).contains("accepted invariant"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recall_policy_rejects_wildcards_overlap_and_project_root_scans() {
    let (root, config) = project();
    let config_path = root.join("soulmate.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    value["memory"] = serde_json::json!({
        "root": ".soulmate/memory",
        "maxItems": 8,
        "maxBytes": 32_768,
        "protocolScopes": ["invariants"],
        "syntheticScopes": ["synthetic"]
    });
    value["agents"]["lead"]["memoryRead"] = serde_json::json!(["*"]);
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    let wildcard = invoke(&["check", "--config", &config]);
    assert!(!wildcard.status.success());
    assert!(text(&wildcard).contains("unique, non-empty exact scopes"));

    value["agents"]["lead"]["memoryRead"] = serde_json::json!(["invariants"]);
    value["memory"]["syntheticScopes"] = serde_json::json!(["invariants"]);
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    let overlap = invoke(&["check", "--config", &config]);
    assert!(!overlap.status.success());
    assert!(text(&overlap).contains("must not overlap"));

    value["memory"]["syntheticScopes"] = serde_json::json!(["synthetic"]);
    value["memory"]["root"] = serde_json::json!(".");
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    let broad = invoke(&["check", "--config", &config]);
    assert!(!broad.status.success());
    assert!(text(&broad).contains("must stay inside the project"));
    fs::remove_dir_all(root).unwrap();
}

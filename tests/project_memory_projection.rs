//! A new work item in the same project starts with the Lead's role and current
//! accepted project memory. A correction replaces the revoked rule; rejected,
//! expired, and changed items never project; another project sees none of it;
//! and one work's check never carries into the next.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod support;

fn call(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn ok_with(root: &Path, args: &[&str], input: &[u8]) -> Value {
    let output = call(root, args, input);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn ok(root: &Path, args: &[&str]) -> Value {
    ok_with(root, args, b"")
}

/// An initialized project with opt-in memory whose Lead may read and manage
/// the `project-rules` and `findings` scopes.
fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let lead = &mut config["agents"]["lead"];
    lead["crossContext"] = json!("same-scope");
    lead["retention"] = json!("until-revoked");
    for right in [
        "memoryRead",
        "memoryWrite",
        "memoryReview",
        "memoryPromote",
        "memoryReject",
        "memoryRevoke",
        "memoryExpire",
    ] {
        lead[right] = json!(["project-rules", "findings"]);
    }
    config["memory"] = json!({"root":"memory","maxItems":16,"maxBytes":16384,
        "protocolScopes":[],"syntheticScopes":[]});
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    fs::create_dir_all(root.join("memory")).unwrap();
    fs::create_dir_all(root.join("rules")).unwrap();
    root
}

fn propose(root: &Path, file: &str, text: &str, scope: &str, ledger: &str) {
    fs::write(root.join(file), text).unwrap();
    ok(
        root,
        &[
            "memory", "propose", "lead", file, "--scope", scope, "--ledger", ledger,
        ],
    );
}

fn accept(root: &Path, file: &str, text: &str, scope: &str, ledger: &str) {
    propose(root, file, text, scope, ledger);
    ok(root, &["memory", "review", "lead", ledger]);
    ok(root, &["memory", "promote", "lead", ledger]);
}

fn session_context(root: &Path) -> String {
    let cwd = root.canonicalize().unwrap().display().to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!(r#"{{"hook_event_name":"SessionStart","cwd":{cwd:?}}}"#).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Accepted item IDs by source path, as the memory lifecycle resolves them.
fn accepted(root: &Path) -> BTreeMap<String, String> {
    ok(root, &["memory", "resolve", "lead", "--json"])["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| {
            (
                reference["sourcePath"].as_str().unwrap().to_owned(),
                reference["itemId"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// Item IDs recorded in the new work's Lead assignment.
fn lead_items(root: &Path, work: &str) -> Vec<String> {
    let next = ok(root, &["work", "next", work, "--full"]);
    let mut items = next["next"]["packet"]["memoryReferences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["itemId"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    items.sort();
    items
}

fn begin(root: &Path, goal: &str) -> String {
    ok(
        root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "true",
        ],
    )["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn new_work_receives_role_and_current_memory_and_a_correction_replaces_the_old_rule() {
    let root = project("memory-projection");
    accept(
        &root,
        "rules/docs.md",
        "Rule: document every CLI flag in README.md.\n",
        "project-rules",
        "memory/docs.jsonl",
    );
    accept(
        &root,
        "rules/finding.md",
        "Finding: tests run with python3 -m unittest.\n",
        "findings",
        "memory/finding.jsonl",
    );
    let context = session_context(&root);
    assert!(context.contains("Project role for this session: lead (Own the goal"));
    assert!(context.contains("profile exitbind/agents/lead.md (sha256 "));
    assert!(context.contains("not a check, approval, or permission"));
    assert!(context.contains("Rule: document every CLI flag in README.md."));
    assert!(context.contains("Finding: tests run with python3 -m unittest."));

    let before = accepted(&root);
    let work_b = begin(&root, "Work B");
    let mut expected = before.values().cloned().collect::<Vec<_>>();
    expected.sort();
    assert_eq!(lead_items(&root, &work_b), expected);

    // The owner corrects the rule: revoke the old item, accept the new one.
    let old = before["rules/docs.md"].clone();
    ok(&root, &["memory", "revoke", "lead", "memory/docs.jsonl"]);
    accept(
        &root,
        "rules/docs-v2.md",
        "Rule: document every CLI flag in README.md with an example.\n",
        "project-rules",
        "memory/docs-v2.jsonl",
    );
    let context = session_context(&root);
    assert!(context.contains("with an example."));
    assert!(!context.contains("rules/docs.md:"));
    let after = accepted(&root);
    assert!(!after.values().any(|item| *item == old));
    let work_c = begin(&root, "Work C");
    let mut expected = after.values().cloned().collect::<Vec<_>>();
    expected.sort();
    assert_eq!(lead_items(&root, &work_c), expected);
    assert!(!lead_items(&root, &work_c).contains(&old));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn rejected_expired_and_changed_items_never_project() {
    let root = project("memory-exclusions");
    propose(
        &root,
        "rules/rejected.md",
        "Rule: rejected text.\n",
        "project-rules",
        "memory/rejected.jsonl",
    );
    ok(
        &root,
        &["memory", "review", "lead", "memory/rejected.jsonl"],
    );
    ok(
        &root,
        &["memory", "reject", "lead", "memory/rejected.jsonl"],
    );
    fs::write(root.join("rules/expired.md"), "Rule: expired text.\n").unwrap();
    let expires = (chrono::Utc::now() + chrono::Duration::seconds(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    ok(
        &root,
        &[
            "memory",
            "propose",
            "lead",
            "rules/expired.md",
            "--scope",
            "project-rules",
            "--ledger",
            "memory/expired.jsonl",
            "--expires-at",
            &expires,
        ],
    );
    ok(&root, &["memory", "review", "lead", "memory/expired.jsonl"]);
    ok(
        &root,
        &["memory", "promote", "lead", "memory/expired.jsonl"],
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    let context = session_context(&root);
    assert!(!context.contains("rejected text"));
    assert!(!context.contains("expired text"));

    // A changed accepted source is reported as a bounded error, never
    // projected with its new or old content.
    accept(
        &root,
        "rules/live.md",
        "Rule: live text.\n",
        "project-rules",
        "memory/live.jsonl",
    );
    assert!(session_context(&root).contains("live text"));
    fs::write(root.join("rules/live.md"), "Rule: edited text.\n").unwrap();
    let context = session_context(&root);
    assert!(context.contains("could not be projected"));
    assert!(!context.contains("edited text"));
    assert!(!context.contains("live text"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn another_project_with_the_same_agent_label_sees_none_of_it() {
    let first = project("memory-isolation-a");
    let second = project("memory-isolation-b");
    accept(
        &first,
        "rules/private.md",
        "Rule: project A only.\n",
        "project-rules",
        "memory/private.jsonl",
    );
    assert!(session_context(&first).contains("project A only"));
    let context = session_context(&second);
    assert!(!context.contains("project A only"));
    assert!(!context.contains("Accepted project memory"));
    assert!(accepted(&second).is_empty());
    let work = begin(&second, "Project B work");
    assert!(lead_items(&second, &work).is_empty());
    fs::remove_dir_all(&first).unwrap();
    fs::remove_dir_all(&second).unwrap();
}

#[test]
fn a_passing_check_on_one_work_does_not_carry_into_the_next() {
    let root = project("memory-no-inheritance");
    let work_a = begin(&root, "Work A");
    let lead = ok(&root, &["work", "next", &work_a, "--full"]);
    let assignment = lead["next"]["assignment"].as_str().unwrap().to_owned();
    ok(
        &root,
        &[
            "work",
            "return",
            &work_a,
            &assignment,
            "--outcome",
            "scoped",
        ],
    );
    let worker = ok(&root, &["work", "next", &work_a, "--full"]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    ok_with(
        &root,
        &[
            "work",
            "return",
            &work_a,
            &assignment,
            "--outcome",
            "completed",
        ],
        b"done",
    );
    let checked = ok(&root, &["work", "check", &work_a]);
    assert_eq!(checked["result"]["code"], 0, "{checked}");
    let a_next = ok(&root, &["work", "next", &work_a, "--full"]);
    assert_eq!(
        a_next["next"]["progress"]["components"]["check"]["completed"],
        1
    );

    let work_b = begin(&root, "Work B");
    assert_ne!(work_a, work_b);
    let b_next = ok(&root, &["work", "next", &work_b, "--full"]);
    assert_eq!(b_next["next"]["action"], "lead_decision");
    let components = &b_next["next"]["progress"]["components"];
    for component in ["check", "lead", "worker", "scope"] {
        assert_eq!(components[component]["completed"], 0, "{component}");
    }
    fs::remove_dir_all(&root).unwrap();
}

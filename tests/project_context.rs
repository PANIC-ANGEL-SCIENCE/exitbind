//! Current project references and native assignment context stay distinct.

#![cfg(unix)]
mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::Path,
    process::{Command, Stdio},
};

fn project(label: &str) -> std::path::PathBuf {
    let root = support::temp(label);
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    root
}

fn call(root: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn call_input(root: &Path, args: &[&str], input: &Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .args(["--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn hook(root: &Path, agent: &str) -> Option<String> {
    let payload = json!({"hook_event_name":"SubagentStart", "cwd":root,
        "agent_type":agent});
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
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    if output.stdout.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .map(str::to_owned)
}

fn child_hook(root: &Path, agent: &str, session: &str, child: &str) -> String {
    let payload = json!({"hook_event_name":"SubagentStart", "cwd":root,
        "agent_type":agent, "session_id":session, "agent_id":child});
    let mut process = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn session_hook(root: &Path) -> String {
    let payload = json!({"hook_event_name":"SessionStart", "cwd":root});
    let mut process = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn project_context_reports_current_rule_reference_and_refuses_unsafe_source() {
    let root = project("project-context-rule");
    fs::write(root.join("AGENTS.md"), "Rule: keep evidence current.\n").unwrap();
    let value = call(&root, &["project", "context", "--json"]);
    assert_eq!(value["project"]["identity"]["kind"], "location");
    assert_eq!(value["focus"]["state"], "none");
    assert_eq!(value["rules"][0]["state"], "current");
    assert_eq!(value["rules"][0]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["memory"]["references"].as_array().unwrap().len(), 0);
    assert_eq!(value["resources"]["version"], 1);
    assert_eq!(value["resources"]["os"]["value"], std::env::consts::OS);
    assert_eq!(
        value["resources"]["architecture"]["value"],
        std::env::consts::ARCH
    );
    assert_eq!(
        value["resources"]["filesystemAvailableBytes"]["state"],
        "known"
    );
    assert!(value["resources"]["filesystemAvailableBytes"]["value"]
        .as_u64()
        .is_some());

    fs::remove_file(root.join("AGENTS.md")).unwrap();
    let outside = support::temp("project-context-outside");
    fs::write(outside.join("rule.md"), "outside private rule").unwrap();
    symlink(outside.join("rule.md"), root.join("AGENTS.md")).unwrap();
    let value = call(&root, &["project", "context", "--json"]);
    assert_eq!(value["rules"][0]["state"], "unavailable");
    assert!(value["rules"][0]["sha256"].is_null());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn large_project_keeps_the_whole_session_context_bounded_and_jit_visible() {
    let root = project("project-context-large-session");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let worker = config["agents"]["worker"].clone();
    let workflow = config["workflows"]["change"].clone();
    for index in 0..100 {
        config["agents"][format!("extra_agent_{index:03}")] = worker.clone();
        config["workflows"][format!("extra_workflow_{index:03}")] = workflow.clone();
    }
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let context = session_hook(&root);
    assert!(context.len() <= 3072, "{} bytes", context.len());
    assert!(context.contains("exitbind project context --json"));
    assert!(context.contains("Preserve the existing root host conversation"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_native_projection_maps_profiles_without_claiming_launch() {
    let root = project("native-agent-projection");
    let initial = call(&root, &["project", "agents", "--json"]);
    assert!(initial["projections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["state"] == "missing"));
    let applied = call(&root, &["project", "agents", "--apply", "--json"]);
    assert!(applied["projections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["state"] == "current"));
    assert!(applied["evidence"]
        .as_str()
        .unwrap()
        .contains("native discovery and launch are separate"));
    let codex = fs::read_to_string(root.join(".codex/agents/worker.toml")).unwrap();
    assert!(codex.starts_with("# exitbind-managed-agent:v1\nname = \"worker\""));
    assert!(codex.contains("developer_instructions = \"# Worker\\n"));
    let claude = fs::read_to_string(root.join(".claude/agents/worker.md")).unwrap();
    assert!(claude.contains("<!-- exitbind-managed-agent:v1 -->\n# Worker"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_projection_refuses_external_profile_before_any_write() {
    let root = project("native-agent-external");
    fs::create_dir_all(root.join(".codex/agents")).unwrap();
    let external = root.join(".codex/agents/worker.toml");
    fs::write(&external, "# managed elsewhere\nname = \"worker\"\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["project", "agents", "--apply", "--config", "exitbind.json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(external).unwrap(),
        "# managed elsewhere\nname = \"worker\"\n"
    );
    assert!(!root.join(".claude/agents/worker.md").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_profile_requires_the_pending_named_assignment_and_current_source() {
    let root = project("project-context-assignment");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["nativeName"] = json!("sonic");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let begun = call(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Named worker",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    );
    let work = begun["work"].as_str().unwrap();
    let lead = call(&root, &["work", "next", work, "--full"]);
    let lead_assignment = lead["next"]["assignment"].as_str().unwrap();
    call(
        &root,
        &[
            "work",
            "return",
            work,
            lead_assignment,
            "--outcome",
            "scoped",
        ],
    );
    let worker = call(&root, &["work", "next", work, "--full"]);
    let assignment = worker["next"]["assignment"].as_str().unwrap();
    let profile = hook(&root, "sonic").unwrap();
    assert!(
        profile.contains(&format!(
            "Current assignment: work {work}, assignment {assignment}"
        )),
        "{profile}"
    );
    assert!(profile.contains("Presented profile SHA-256:"));
    assert!(hook(&root, "worker").is_none());
    let reviewer_context = hook(&root, "reviewer").unwrap();
    assert!(reviewer_context.contains("assignment context mismatch"));
    let conflict = json!({"hook_event_name":"SubagentStart", "cwd":root,
        "agent_name":"sonic", "agent_type":"worker"});
    let mut process = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(conflict.to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());

    fs::write(
        root.join("exitbind/agents/worker.md"),
        "changed after assignment\n",
    )
    .unwrap();
    let stale = hook(&root, "sonic").unwrap();
    assert!(stale.contains("profile acquisition is unavailable"));
    assert!(!stale.contains("changed after assignment"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn selected_perspective_expires_on_source_change_and_rebind() {
    let root = project("project-context-perspective");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["nativeName"] = json!("sonic");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let perspective_path = root.join("exitbind/perspectives");
    fs::create_dir_all(&perspective_path).unwrap();
    let qa = perspective_path.join("qa.md");
    fs::write(&qa, "Check unavailable resources separately from zero.\n").unwrap();
    let begun = call(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Scoped QA",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    );
    let work = begun["work"].as_str().unwrap();
    let lead = call(&root, &["work", "next", work, "--full"]);
    call(
        &root,
        &[
            "work",
            "return",
            work,
            lead["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
    );
    call_input(
        &root,
        &["work", "record", work],
        &json!({
            "action":"init", "sourceRef":"owner:request", "sourceText":"Scoped QA: Check resource failures",
            "requirements":[{"id":"qa", "text":"Check resource failures"}]
        }),
    );
    let token = call(&root, &["work", "continuation", work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &root,
        &[
            "work",
            "bind",
            work,
            "--context",
            &token,
            "--host",
            "claude",
            "--session",
            "parent-1",
            "--host-version",
            "0.157.1",
        ],
    );
    let token = call(&root, &["work", "continuation", work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    for id in ["p2", "p3", "p4"] {
        fs::write(
            perspective_path.join(format!("{id}.md")),
            "extra perspective\n",
        )
        .unwrap();
    }
    let four = r#"[{"id":"qa","path":"exitbind/perspectives/qa.md"},{"id":"p2","path":"exitbind/perspectives/p2.md"},{"id":"p3","path":"exitbind/perspectives/p3.md"},{"id":"p4","path":"exitbind/perspectives/p4.md"}]"#;
    let too_many = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "child",
            "prepare",
            work,
            "resource-observer",
            "--context",
            &token,
            "--agent-type",
            "sonic",
            "--perspectives",
            four,
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(!too_many.status.success());
    let selected = r#"[{"id":"qa","path":"exitbind/perspectives/qa.md"}]"#;
    let prepared = call(
        &root,
        &[
            "work",
            "child",
            "prepare",
            work,
            "resource-observer",
            "--context",
            &token,
            "--agent-type",
            "sonic",
            "--perspectives",
            selected,
        ],
    );
    let intent = prepared["intent"].as_str().unwrap();
    let delivered = call(&root, &["work", "child", "context", work, intent]);
    assert_eq!(delivered["nativeTaskName"], "sonic");
    assert!(delivered["profile"]["content"]
        .as_str()
        .unwrap()
        .contains("# Worker"));
    assert!(delivered["perspectives"][0]["content"]
        .as_str()
        .unwrap()
        .contains("unavailable resources separately from zero"));
    assert_eq!(delivered["perspectives"][0]["coverage"], "complete");
    let acquired = child_hook(&root, "sonic", "parent-1", "native-child-1");
    assert!(
        acquired.contains("Selected task perspectives for this assignment:"),
        "{acquired}"
    );
    assert!(acquired.contains("unavailable resources separately from zero"));

    let other = call(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Other pending worker",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    );
    let other_work = other["work"].as_str().unwrap();
    let other_lead = call(&root, &["work", "next", other_work, "--full"]);
    call(
        &root,
        &[
            "work",
            "return",
            other_work,
            other_lead["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
    );
    let mixed = child_hook(&root, "sonic", "parent-1", "native-child-1");
    assert!(
        !mixed.contains("unavailable resources separately from zero"),
        "{mixed}"
    );
    assert!(
        !mixed.contains(&format!("Current assignment: work {other_work}")),
        "{mixed}"
    );
    call(&root, &["work", "focus", work]);
    let token = call(&root, &["work", "continuation", work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let rebound = call(
        &root,
        &[
            "work",
            "child",
            "prepare",
            work,
            "resource-observer-rebound",
            "--context",
            &token,
            "--agent-type",
            "sonic",
            "--perspectives",
            selected,
        ],
    );
    assert_eq!(rebound["effect"], "prepared");
    call(&root, &["work", "focus", &other_work]);
    let rejected = child_hook(&root, "sonic", "parent-1", "native-child-rebound");
    assert!(
        rejected.contains("rejected the prepared child context before presentation"),
        "{rejected}"
    );
    assert!(
        !rejected.contains(&format!("Current assignment: work {other_work}")),
        "{rejected}"
    );
    assert!(
        !rejected.contains("unavailable resources separately from zero"),
        "{rejected}"
    );
    let repeated = child_hook(&root, "sonic", "parent-1", "native-child-rebound");
    assert!(
        repeated.contains("rejected the prepared child context before presentation"),
        "{repeated}"
    );
    assert!(
        !repeated.contains(&format!("Current assignment: work {other_work}")),
        "{repeated}"
    );

    call(&root, &["work", "focus", work]);
    let token = call(&root, &["work", "continuation", work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let prepared_other = call(
        &root,
        &[
            "work",
            "child",
            "prepare",
            work,
            "resource-observer-new-work",
            "--context",
            &token,
            "--agent-type",
            "sonic",
            "--perspectives",
            selected,
        ],
    );
    assert_eq!(prepared_other["effect"], "prepared");
    call(&root, &["work", "focus", &other_work]);
    let repeated_with_new_intent = child_hook(&root, "sonic", "parent-1", "native-child-rebound");
    assert!(
        repeated_with_new_intent
            .contains("rejected the prepared child context before presentation"),
        "{repeated_with_new_intent}"
    );
    assert!(
        !repeated_with_new_intent.contains(&format!("Current assignment: work {other_work}")),
        "{repeated_with_new_intent}"
    );
    assert!(!repeated_with_new_intent.contains("unavailable resources separately from zero"));
    let intents_lock = root.join(".exitbind/child-intents.json.lock");
    fs::write(&intents_lock, "held by focused lock regression").unwrap();
    let lock_error = child_hook(&root, "sonic", "parent-1", "native-child-lock");
    fs::remove_file(&intents_lock).unwrap();
    assert!(
        lock_error.contains("could not verify the prepared child context"),
        "{lock_error}"
    );
    assert!(
        !lock_error.contains(&format!("Current assignment: work {other_work}")),
        "{lock_error}"
    );
    assert!(!lock_error.contains("unavailable resources separately from zero"));
    call(&root, &["work", "focus", work]);

    fs::write(&qa, "changed perspective\n").unwrap();
    let stale = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "child",
            "context",
            work,
            intent,
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    fs::write(&qa, "Check unavailable resources separately from zero.\n").unwrap();
    let token = call(&root, &["work", "continuation", work])["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &root,
        &[
            "work",
            "bind",
            work,
            "--context",
            &token,
            "--host",
            "claude",
            "--session",
            "parent-2",
            "--host-version",
            "0.157.1",
        ],
    );
    let stale = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "work",
            "child",
            "context",
            work,
            intent,
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    fs::remove_dir_all(root).unwrap();
}

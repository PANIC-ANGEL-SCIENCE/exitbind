//! Native work receives complete current project rules and role-scoped memory.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{}", text(&init));
        let config_path = root.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        config["memory"] = json!({
            "root": "memory",
            "maxItems": 8,
            "maxBytes": 32_768,
            "protocolScopes": ["invariants"],
            "syntheticScopes": []
        });
        config["agents"]["worker"]["memoryRead"] = json!(["invariants"]);
        config["agents"]["worker"]["crossContext"] = json!("protocol-only");
        config["agents"]["worker"]["memoryWrite"] = json!(["invariants"]);
        config["agents"]["worker"]["memoryReview"] = json!(["invariants"]);
        config["agents"]["lead"]["memoryPromote"] = json!(["invariants"]);
        config["agents"]["lead"]["memoryRevoke"] = json!(["invariants"]);
        fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        fs::create_dir_all(root.join("memory")).unwrap();
        fs::write(root.join("AGENTS.md"), "current project rule\n").unwrap();
        fs::write(root.join("CLAUDE.md"), "second current rule\n").unwrap();
        Self { root }
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn value(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn accept(&self, source: &str, content: &str, ledger: &str) {
        fs::write(self.root.join(source), content).unwrap();
        self.value(
            &[
                "memory",
                "propose",
                "worker",
                source,
                "--scope",
                "invariants",
                "--ledger",
                ledger,
            ],
            b"",
        );
        self.value(&["memory", "review", "worker", ledger], b"");
        self.value(&["memory", "promote", "lead", ledger], b"");
    }

    fn fake_codex(&self, capture: &Path) -> PathBuf {
        let executable = self.root.join("fake-codex");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\ncat > '{}'\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"context-thread\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"captured\\\",\\\"reason\\\":\\\"\\\"}}\"}}}}' '{{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}'\n",
                capture.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        executable
    }

    fn run_worker(&self, goal: &str, executable: &Path) -> (String, PathBuf, Output) {
        let started = self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                goal,
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--review-policy",
                "required",
            ],
            b"",
        );
        let work = started["work"].as_str().unwrap().to_owned();
        let lead = started["next"]["assignment"].as_str().unwrap();
        let scoped = self.call(
            &["work", "return", &work, lead, "--outcome", "scoped"],
            b"scope",
        );
        assert!(scoped.status.success(), "{}", text(&scoped));
        let worker = self.value(&["work", "next", &work, "--full"], b"")["next"].clone();
        let output = self.call(
            &[
                "work",
                "act",
                &work,
                "--codex-bin",
                executable.to_str().unwrap(),
            ],
            b"",
        );
        assert!(output.status.success(), "{}", text(&output));
        (
            work,
            PathBuf::from(worker["assignment"].as_str().unwrap()),
            output,
        )
    }
}

fn stable_rules(prompt: &str) -> String {
    let start = prompt
        .find("CURRENT STABLE PROJECT RULES (verified complete current bytes):")
        .unwrap();
    let end = start
        + prompt[start..]
            .find("CURRENT VOLATILE NATIVE PROJECT CONTEXT")
            .unwrap();
    prompt[start..end].to_owned()
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn configured_product_managed_form_executes_and_stale_binding_refuses_before_provider() {
    let fixture = Fixture::new("native-action-form");
    let config_path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["runtime"] = json!({"host": "codex", "fallback": "none"});
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let capture = fixture.root.join("form-prompt");
    let fake = fixture.fake_codex(&capture);
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "execute emitted native form",
            "--check-command",
            "true",
            "--review-policy",
            "required",
            "--proof-origin",
            "synthetic",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    fixture.value(
        &[
            "work",
            "return",
            work,
            started["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
        ],
        b"complete scoped artifact",
    );
    let detail = fixture.value(&["work", "detail", work], b"");
    let native = &detail["actionForms"]["productManaged"];
    assert_eq!(native["host"], "codex");
    assert_eq!(
        native["profileSha256"],
        detail["sections"]["assignment"]["assignment"]["profileSha256"]
    );
    let run = |command: &Value, execute: bool| {
        assert_eq!(command["sameExecutableRequired"], false);
        assert_eq!(command["sameConfigRequired"], false);
        let argv = command["argv"].as_array().unwrap();
        let mut process = support::git_topology::command(argv[0].as_str().unwrap());
        process
            .current_dir(&fixture.root)
            .args(argv[1..].iter().map(|a| a.as_str().unwrap()));
        if execute {
            process.arg("--codex-bin").arg(&fake);
        }
        process.output().unwrap()
    };
    let inspected = run(&native["inspect"]["command"], false);
    assert!(inspected.status.success(), "{}", text(&inspected));
    assert!(!capture.exists());
    let executed = run(&native["command"], true);
    assert!(executed.status.success(), "{}", text(&executed));
    assert!(fs::read_to_string(&capture)
        .unwrap()
        .contains("execute emitted native form"));
    let repeated = run(&native["command"], true);
    assert!(
        !repeated.status.success(),
        "a revoked form must not execute a later assignment"
    );
    assert!(text(&repeated).contains("binding is stale"));
    let reviewer = fixture.value(&["work", "check", work], b"");
    assert_eq!(reviewer["next"]["role"], "reviewer");
    let host_managed = fixture.value(&["work", "detail", work], b"");
    assert!(host_managed["actionForms"].get("productManaged").is_none());
    assert!(!host_managed["actionForms"]["choices"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn native_context_carries_current_rules_and_role_memory_across_works() {
    let fixture = Fixture::new("agent-context-delivery");
    fixture.accept("old.md", "old accepted rule\n", "memory/old.jsonl");
    let first_capture = fixture.root.join("first-prompt");
    let first_exe = fixture.fake_codex(&first_capture);
    let _ = fixture.run_worker("first current context", &first_exe);
    let first = fs::read_to_string(&first_capture).unwrap();
    assert!(first.contains("NATIVE CURRENT CONTEXT"));
    assert!(first.contains("CURRENT PROJECT RULES"));
    assert!(
        first.find("CURRENT PROJECT RULES").unwrap()
            < first.find("NATIVE CURRENT CONTEXT").unwrap()
    );
    assert!(first.contains("current project rule"));
    assert!(first.contains("second current rule"));
    assert!(first.contains("old accepted rule"));
    assert!(first.contains("requestedNativeBinding"));
    assert!(first.contains("currentWork"));
    assert!(first.contains("supplied managed file tool"));
    assert!(first.contains("edit PATH"));
    assert!(!first.contains("--expected-sha256"));
    assert!(first.contains("full replacement content as UTF-8 bytes on stdin"));
    assert!(first.contains("never replay the write blindly"));

    let same_context_capture = fixture.root.join("same-context-prompt");
    let same_context_exe = fixture.fake_codex(&same_context_capture);
    let _ = fixture.run_worker("different Work and assignment", &same_context_exe);
    let same_context = fs::read_to_string(&same_context_capture).unwrap();
    assert_eq!(stable_rules(&first), stable_rules(&same_context));

    let revoked = fixture.value(&["memory", "revoke", "lead", "memory/old.jsonl"], b"");
    assert_eq!(revoked["action"], "revoke");
    fixture.accept("new.md", "corrected accepted rule\n", "memory/new.jsonl");
    let second_capture = fixture.root.join("second-prompt");
    let second_exe = fixture.fake_codex(&second_capture);
    let _ = fixture.run_worker("corrected current context", &second_exe);
    let second = fs::read_to_string(&second_capture).unwrap();
    assert!(second.contains("corrected accepted rule"));
    assert!(!second.contains("old accepted rule"));
    fs::write(
        fixture.root.join("AGENTS.md"),
        "changed current project rule\n",
    )
    .unwrap();
    let changed_rules_capture = fixture.root.join("changed-rules-prompt");
    let changed_rules_exe = fixture.fake_codex(&changed_rules_capture);
    let _ = fixture.run_worker("changed stable rule", &changed_rules_exe);
    let changed_rules = fs::read_to_string(&changed_rules_capture).unwrap();
    assert_ne!(stable_rules(&first), stable_rules(&changed_rules));

    let isolated = Fixture::new("agent-context-isolation");
    fs::write(isolated.root.join("old.md"), "old accepted rule\n").unwrap();
    let isolated_capture = isolated.root.join("isolated-prompt");
    let isolated_exe = isolated.fake_codex(&isolated_capture);
    let _ = isolated.run_worker("isolated project context", &isolated_exe);
    let isolated_prompt = fs::read_to_string(isolated_capture).unwrap();
    assert!(!isolated_prompt.contains("old accepted rule"));
}

#[test]
fn reported_long_rule_survives_start_detail_and_native_prompt() {
    let fixture = Fixture::new("agent-context-long-rule");
    let rule = format!("{}TAIL-UNIQUE-123456", "é".repeat(9_451));
    assert_eq!(rule.len(), 18_920);
    fs::write(fixture.root.join("AGENTS.md"), &rule).unwrap();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "deliver long current rule",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--detail",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["recipientContext"]["rules"][0]["content"], rule);
    let detail = fixture.value(&["work", "detail", work], b"");
    assert_eq!(detail["recipientContext"]["rules"][0]["content"], rule);
    let grouped_reference = detail["reference"].as_str().unwrap();
    let grouped = fixture.value(&["work", "expand", work, grouped_reference], b"");
    assert_eq!(grouped["recipientContext"]["rules"][0]["content"], rule);

    let lead = started["recipient"]["assignment"].as_str().unwrap();
    fixture.value(
        &["work", "return", work, lead, "--outcome", "scoped"],
        b"scope",
    );
    let capture = fixture.root.join("long-rule-prompt");
    let executable = fixture.fake_codex(&capture);
    let worker = fixture.call(
        &[
            "work",
            "act",
            work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(worker.status.success(), "{}", text(&worker));
    assert!(stable_rules(&fs::read_to_string(capture).unwrap()).contains(&rule));
}

#[test]
fn aggregate_rule_limit_keeps_the_recorded_work_and_refuses_partial_detail() {
    let fixture = Fixture::new("agent-context-aggregate-limit");
    fs::write(fixture.root.join("AGENTS.md"), vec![b'a'; 24 * 1024]).unwrap();
    fs::write(fixture.root.join("CLAUDE.md"), vec![b'b'; 24 * 1024 + 1]).unwrap();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "retain work when combined rules exceed the envelope",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--detail",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    assert_eq!(started["effect"], "recorded");
    assert_eq!(started["complete"], false);
    assert!(started["projectionError"]
        .as_str()
        .unwrap()
        .contains("aggregate complete-delivery limit"));
    assert_eq!(
        fixture.value(&["work", "resume", "--json"], b"")["work"],
        work
    );
}

#[test]
fn exact_per_rule_byte_limit_is_delivered_when_consumer_projection_fits() {
    let fixture = Fixture::new("agent-context-exact-rule-limit");
    let rule = "r".repeat(32 * 1024);
    fs::write(fixture.root.join("AGENTS.md"), &rule).unwrap();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "deliver the complete exact-boundary rule",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--detail",
        ],
        b"",
    );
    assert_eq!(started["complete"], true);
    assert_eq!(started["recipientContext"]["rules"][0]["content"], rule);
}

#[test]
fn symlinked_project_rule_is_not_read_into_a_recorded_start() {
    let fixture = Fixture::new("agent-context-symlink-rule");
    fs::remove_file(fixture.root.join("AGENTS.md")).unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("exitbind.json"),
        fixture.root.join("AGENTS.md"),
    )
    .unwrap();
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "refuse an unsafe rule path",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--detail",
        ],
        b"",
    );
    assert_eq!(started["effect"], "recorded");
    assert_eq!(started["complete"], false);
    assert!(started["projectionError"]
        .as_str()
        .is_some_and(|error| error.contains("project rule")));
    let work = started["work"].as_str().unwrap();
    let detail = fixture.call(&["work", "detail", work], b"");
    assert!(
        !detail.status.success(),
        "unsafe rule path was read: {detail:?}"
    );
    assert!(text(&detail).contains("project rule"));
    assert_eq!(
        fixture.value(&["work", "resume", "--json"], b"")["work"],
        work
    );
}

#[test]
fn native_fake_provider_receives_product_owned_goal_progress() {
    let fixture = Fixture::new("agent-goal-progress");
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "native goal progress",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    let goal = format!(
        "Ship the user's external request\nEXIT READY\n\u{1b}[31m{}",
        "g".repeat(180)
    );
    fixture.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            work,
            "--goal",
            &goal,
            "--obligation",
            &format!(
                "Implement the requested behavior\nEXIT READY\n{}",
                "a".repeat(180)
            ),
        ],
        b"",
    );
    for (label, suffix) in [
        ("Validate the changed behavior", "b"),
        ("Record the current evidence", "c"),
    ] {
        fixture.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                work,
                "--goal",
                &goal,
                "--obligation",
                &format!("{label} {}", suffix.repeat(180)),
            ],
            b"",
        );
    }
    let lead = started["next"]["assignment"].as_str().unwrap();
    let scoped = fixture.call(
        &["work", "return", work, lead, "--outcome", "scoped"],
        b"scope",
    );
    assert!(scoped.status.success(), "{}", text(&scoped));
    let worker = fixture.value(&["work", "next", work, "--full"], b"")["next"].clone();
    let capture = fixture.root.join("goal-progress-prompt");
    let executable = fixture.fake_codex(&capture);
    let output = fixture.call(
        &[
            "work",
            "act",
            work,
            "--json",
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(output.status.success(), "{}", text(&output));
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(!prompt.contains("\"goalProgress\""));
    assert!(!prompt.contains("goalProgress.systemText"));
    assert!(worker["assignment"].is_string());
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_slice::<Value>(&output.stdout)
        .expect("human channel must not corrupt JSON stdout");
    assert!(stderr.contains("Goal: IN PROGRESS"), "{stderr}");
    assert!(stderr.contains("Tasks: 0/3 complete"), "{stderr}");
    assert!(stderr.matches("=in_progress").count() >= 3, "{stderr}");
    assert!(stderr.contains("\\nEXIT READY\\n"), "{stderr}");
    assert!(stderr.contains("\\u{1b}[31m"), "{stderr}");
    assert!(!stderr.contains("\nEXIT READY\n"), "{stderr}");
    assert!(stderr.chars().count() <= 512, "{}", stderr.chars().count());
}

#[test]
fn failed_json_check_keeps_machine_stdout_and_emits_product_progress_stderr() {
    let fixture = Fixture::new("agent-goal-progress-check-failure");
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "native failed check",
            "--check-command",
            "false",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    fixture.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            work,
            "--goal",
            "Repair the external request",
            "--obligation",
            "Revalidate the changed result",
        ],
        b"",
    );
    let lead = started["next"]["assignment"].as_str().unwrap();
    let scoped = fixture.call(
        &["work", "return", work, lead, "--outcome", "scoped"],
        b"scope",
    );
    assert!(scoped.status.success(), "{}", text(&scoped));
    let worker = fixture.value(&["work", "next", work, "--full"], b"")["next"].clone();
    let returned = fixture.call(
        &[
            "work",
            "return",
            work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        b"worker artifact",
    );
    assert!(returned.status.success(), "{}", text(&returned));
    let failed = fixture.call(&["work", "check", work, "--json"], b"");
    assert!(!failed.status.success(), "failed check unexpectedly passed");
    serde_json::from_slice::<Value>(&failed.stdout)
        .expect("failed JSON check must keep stdout parseable");
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("Goal: IN PROGRESS"), "{stderr}");
    assert!(stderr.contains("Result readiness:"), "{stderr}");
}

#[test]
fn oversized_project_rule_refuses_native_launch_before_provider_spawn() {
    let fixture = Fixture::new("agent-context-oversized-rule");
    fs::write(fixture.root.join("AGENTS.md"), vec![b'x'; 32 * 1024 + 1]).unwrap();
    let capture = fixture.root.join("should-not-exist");
    let executable = fixture.fake_codex(&capture);
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "oversized rule",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    let lead = started["next"]["assignment"].as_str().unwrap();
    assert!(fixture
        .call(
            &["work", "return", work, lead, "--outcome", "scoped"],
            b"scope"
        )
        .status
        .success());
    let output = fixture.call(
        &[
            "work",
            "act",
            work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!output.status.success());
    assert!(text(&output).contains("instructions are not truncated"));
    assert!(!capture.exists());
}

#[test]
fn configured_non_codex_binding_is_refused_before_provider_spawn() {
    let fixture = Fixture::new("agent-context-binding");
    let config_path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["worker"]["runtime"]["host"] = json!("claude");
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let capture = fixture.root.join("should-not-exist");
    let executable = fixture.fake_codex(&capture);
    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "configured binding",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    let lead = started["next"]["assignment"].as_str().unwrap();
    assert!(fixture
        .call(
            &["work", "return", work, lead, "--outcome", "scoped"],
            b"scope"
        )
        .status
        .success());
    let output = fixture.call(
        &[
            "work",
            "act",
            work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(!output.status.success());
    assert!(text(&output).contains("runtime.host=codex"));
    assert!(!capture.exists());
}

fn text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn native_resolves_readable_scope_and_tasks_before_provider_launch() {
    for (index, scope) in [
        "é".repeat(20000).into_bytes(),
        vec![0xff, 0],
        vec![b'x'; 65537],
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new("native-resolved-detail");
        let started = fixture.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "complete coherent assignment",
                "--check-command",
                "true",
                "--review-policy",
                "required",
                "--proof-origin",
                "synthetic",
            ],
            b"",
        );
        let work = started["work"].as_str().unwrap();
        fixture.value(
            &[
                "goal",
                "incorporate",
                "--goal-id",
                work,
                "--goal",
                "whole outcome",
                "--obligation",
                "companion consumer stays coherent",
            ],
            b"",
        );
        fixture.value(
            &[
                "work",
                "return",
                work,
                started["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            &scope,
        );
        let capture = fixture.root.join("resolved-prompt");
        let executable = fixture.fake_codex(&capture);
        let output = fixture.call(
            &[
                "work",
                "act",
                work,
                "--codex-bin",
                executable.to_str().unwrap(),
            ],
            b"",
        );
        if index == 0 {
            assert!(output.status.success(), "{}", text(&output));
            let prompt = fs::read_to_string(capture).unwrap();
            assert!(prompt.contains(&String::from_utf8(scope).unwrap()));
            assert!(prompt.contains("companion consumer stays coherent"));
            assert!(prompt.contains("CURRENT READABLE ASSIGNMENT, EVIDENCE AND TASKS"));
            assert!(!prompt.contains("contentHex"));
        } else {
            assert!(
                !output.status.success(),
                "unreadable required evidence launched provider"
            );
            assert!(
                text(&output).contains("not completely readable"),
                "{}",
                text(&output)
            );
            assert!(!capture.exists());
        }
    }
}

#[test]
fn native_corrupt_or_missing_scope_never_launches_provider() {
    for missing in [false, true] {
        let fixture = Fixture::new("native-invalid-detail");
        let started = fixture.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "valid required scope",
                "--check-command",
                "true",
                "--review-policy",
                "required",
                "--proof-origin",
                "synthetic",
            ],
            b"",
        );
        let work = started["work"].as_str().unwrap();
        fixture.value(
            &[
                "work",
                "return",
                work,
                started["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"exact scope",
        );
        let artifact = fs::read_dir(fixture.root.join(".exitbind/artifacts"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_file())
            .unwrap();
        if missing {
            fs::remove_file(artifact).unwrap();
        } else {
            fs::write(artifact, b"corrupt scope").unwrap();
        }
        let capture = fixture.root.join("never-launched");
        let executable = fixture.fake_codex(&capture);
        let output = fixture.call(
            &[
                "work",
                "act",
                work,
                "--codex-bin",
                executable.to_str().unwrap(),
            ],
            b"",
        );
        assert!(!output.status.success(), "invalid scope launched provider");
        assert!(!capture.exists());
    }
}

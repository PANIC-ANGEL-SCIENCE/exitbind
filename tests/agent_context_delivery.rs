//! Native work receives complete current project rules and role-scoped memory.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
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

    fn run_worker(&self, goal: &str, executable: &Path) -> (String, PathBuf) {
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
        (work, PathBuf::from(worker["assignment"].as_str().unwrap()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn native_context_carries_current_rules_and_role_memory_across_works() {
    let fixture = Fixture::new("agent-context-delivery");
    fixture.accept("old.md", "old accepted rule\n", "memory/old.jsonl");
    let first_capture = fixture.root.join("first-prompt");
    let first_exe = fixture.fake_codex(&first_capture);
    fixture.run_worker("first current context", &first_exe);
    let first = fs::read_to_string(&first_capture).unwrap();
    assert!(first.contains("NATIVE CURRENT CONTEXT"));
    assert!(first.contains("CURRENT PROJECT RULES"));
    assert!(first.contains("current project rule"));
    assert!(first.contains("second current rule"));
    assert!(first.contains("old accepted rule"));
    assert!(first.contains("requestedNativeBinding"));
    assert!(first.contains("currentWork"));
    assert!(first.contains("exitbind work write"));
    assert!(first.contains("full replacement content as UTF-8 bytes on stdin"));
    assert!(first.contains("never replay the write blindly"));

    let revoked = fixture.value(&["memory", "revoke", "lead", "memory/old.jsonl"], b"");
    assert_eq!(revoked["action"], "revoke");
    fixture.accept("new.md", "corrected accepted rule\n", "memory/new.jsonl");
    let second_capture = fixture.root.join("second-prompt");
    let second_exe = fixture.fake_codex(&second_capture);
    fixture.run_worker("corrected current context", &second_exe);
    let second = fs::read_to_string(&second_capture).unwrap();
    assert!(second.contains("corrected accepted rule"));
    assert!(!second.contains("old accepted rule"));

    let isolated = Fixture::new("agent-context-isolation");
    fs::write(isolated.root.join("old.md"), "old accepted rule\n").unwrap();
    let isolated_capture = isolated.root.join("isolated-prompt");
    let isolated_exe = isolated.fake_codex(&isolated_capture);
    isolated.run_worker("isolated project context", &isolated_exe);
    let isolated_prompt = fs::read_to_string(isolated_capture).unwrap();
    assert!(!isolated_prompt.contains("old accepted rule"));
}

#[test]
fn oversized_project_rule_refuses_native_launch_before_provider_spawn() {
    let fixture = Fixture::new("agent-context-oversized-rule");
    fs::write(fixture.root.join("AGENTS.md"), vec![b'x'; 16 * 1024 + 1]).unwrap();
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

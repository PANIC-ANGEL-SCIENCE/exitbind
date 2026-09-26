//! A prepared child is claimed at SubagentStart and finalized at SubagentStop
//! from the host's own fields; the parent never relays the child ID or text.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod support;

fn run(root: &Path, args: &[&str], input: &[u8], config: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command.current_dir(root).args(args);
    if config {
        command.args(["--config", "exitbind.json"]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn ok(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args, b"", true);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

struct Fixture {
    root: PathBuf,
    work: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let begun = ok(
            &root,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Capture",
                "--check-command",
                "true",
            ],
        );
        let work = begun["work"].as_str().unwrap().to_owned();
        let init = json!({"action":"init","sourceRef":"owner:request","sourceText":"Check docs.",
            "requirements":[{"id":"docs","text":"Check docs."}]});
        let output = run(
            &root,
            &["work", "record", &work],
            init.to_string().as_bytes(),
            true,
        );
        assert!(output.status.success(), "{output:?}");
        let fixture = Self { root, work };
        fixture.bind("parent-1");
        fixture
    }

    fn token(&self) -> String {
        let view = ok(&self.root, &["work", "continuation", &self.work]);
        view["mutationContext"]["token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn bind(&self, session: &str) {
        let token = self.token();
        ok(
            &self.root,
            &[
                "work",
                "bind",
                &self.work,
                "--context",
                &token,
                "--host",
                "claude",
                "--session",
                session,
                "--host-version",
                "2.1.283",
            ],
        );
    }

    fn prepare(&self, assignment: &str) -> Output {
        let token = self.token();
        run(
            &self.root,
            &[
                "work",
                "child",
                "prepare",
                &self.work,
                assignment,
                "--context",
                &token,
            ],
            b"",
            true,
        )
    }

    fn hook(&self, payload: Value) {
        let mut payload = payload;
        payload["cwd"] = json!(self.root.canonicalize().unwrap().display().to_string());
        let output = run(
            &self.root,
            &["hook-run"],
            payload.to_string().as_bytes(),
            false,
        );
        assert!(output.status.success(), "{output:?}");
    }

    fn start(&self, session: &str, child: &str) {
        self.hook(
            json!({"hook_event_name":"SubagentStart","session_id":session,
            "agent_id":child,"agent_type":"Explore"}),
        );
    }

    fn stop(&self, session: &str, child: &str, message: &str) {
        self.hook(
            json!({"hook_event_name":"SubagentStop","session_id":session,
            "agent_id":child,"agent_type":"Explore","last_assistant_message":message}),
        );
    }

    fn view(&self) -> Value {
        ok(&self.root, &["work", "continuation", &self.work])
    }

    fn history(&self) -> Vec<u8> {
        fs::read(self.root.join(".exitbind/session-goal.jsonl")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const RESULT: &str = "Findings:\n\n**1.** README has no --json section.\n";

#[test]
fn prepared_child_is_recorded_from_host_fields_exactly() {
    let f = Fixture::new("capture-exact");
    let prepared = f.prepare("check README docs");
    assert!(prepared.status.success(), "{prepared:?}");
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["effect"], "prepared");
    // An identical retry is harmless; a different pending child is refused.
    let retry: Value = serde_json::from_slice(&f.prepare("check README docs").stdout).unwrap();
    assert_eq!(retry["effect"], "unchanged");
    assert!(!f.prepare("another child").status.success());

    f.start("parent-1", "agent-a1");
    f.stop("parent-1", "agent-a1", RESULT);
    let view = f.view();
    assert_eq!(view["children"][0]["resultText"], RESULT);
    assert_eq!(view["children"][0]["nativeChild"], "agent-a1");
    assert_eq!(view["children"][0]["assignment"], "check README docs");
    assert_eq!(
        view["children"][0]["sourceClass"],
        "host_reported_native_child"
    );
    assert_eq!(view["children"][0]["origin"]["session"], "parent-1");
    assert_eq!(view["preparedChildren"][0]["state"], "recorded");

    // A duplicate stop leaves history alone; a different message is flagged.
    let before = f.history();
    f.stop("parent-1", "agent-a1", RESULT);
    f.stop("parent-1", "agent-a1", "something else");
    assert_eq!(f.history(), before);
    assert!(f.view()["preparedChildren"][0]["conflict"].is_object());
}

#[test]
fn unprepared_or_foreign_subagents_are_never_recorded() {
    let f = Fixture::new("capture-unprepared");
    f.start("parent-1", "agent-free");
    f.stop("parent-1", "agent-free", RESULT);
    assert!(f.view()["children"].as_array().unwrap().is_empty());

    assert!(f.prepare("check README docs").status.success());
    f.start("other-session", "agent-x");
    f.stop("other-session", "agent-x", RESULT);
    let view = f.view();
    assert!(view["children"].as_array().unwrap().is_empty());
    assert_eq!(view["preparedChildren"][0]["state"], "prepared");
}

#[test]
fn oversized_or_rebound_results_stay_inspectable_without_a_record() {
    let f = Fixture::new("capture-oversized");
    assert!(f.prepare("large output").status.success());
    f.start("parent-1", "agent-big");
    f.stop("parent-1", "agent-big", &"x".repeat(9 * 1024));
    let view = f.view();
    assert!(view["children"].as_array().unwrap().is_empty());
    assert_eq!(view["preparedChildren"][0]["state"], "failed");
    assert!(view["preparedChildren"][0]["failure"]
        .as_str()
        .unwrap()
        .contains("not truncated"));

    let g = Fixture::new("capture-rebound");
    assert!(g.prepare("check README docs").status.success());
    g.start("parent-1", "agent-r");
    g.bind("parent-2");
    g.stop("parent-1", "agent-r", RESULT);
    let view = g.view();
    assert!(view["children"].as_array().unwrap().is_empty());
    assert_eq!(view["preparedChildren"][0]["state"], "failed");
    assert!(view["preparedChildren"][0]["failure"]
        .as_str()
        .unwrap()
        .contains("binding changed"));
}

#[test]
fn managed_claude_hooks_include_subagent_stop() {
    let home = support::temp("capture-host-install");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["host", "install", "--json"])
        .env("HOME", &home)
        .env(
            "PATH",
            format!(
                "{}:{}",
                Path::new(env!("CARGO_BIN_EXE_exitbind"))
                    .parent()
                    .unwrap()
                    .display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let settings: Value =
        serde_json::from_slice(&fs::read(home.join(".claude/settings.json")).unwrap()).unwrap();
    for event in ["SessionStart", "SubagentStart", "SubagentStop"] {
        assert!(
            settings["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("exitbind hook-run"),
            "{event}"
        );
    }
    let _ = fs::remove_dir_all(&home);
}

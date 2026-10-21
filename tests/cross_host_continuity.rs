mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Output, Stdio},
};

struct Fixture {
    root: PathBuf,
    work: String,
}

#[path = "cross_host_continuity/world_projection.rs"]
mod world_projection;
use world_projection::{motif_active, world};

impl Fixture {
    fn new(label: &str) -> Self {
        Self::with_preservation_command(label, "true")
    }

    fn with_preservation_command(label: &str, preservation_command: &str) -> Self {
        let root = support::temp(label);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let started = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&root)
            .args([
                "work",
                "begin",
                "change",
                "--goal",
                "Finish two requirements on one work",
                "--check-command",
                "true",
                "--preserve-requirement",
                "first:First requirement.",
                "--preservation-check-command",
                preservation_command,
                "--config",
                "exitbind.json",
            ])
            .output()
            .unwrap();
        assert!(started.status.success(), "{started:?}");
        let work = serde_json::from_slice::<Value>(&started.stdout).unwrap()["work"]
            .as_str()
            .unwrap()
            .to_owned();
        Self { root, work }
    }
    fn without_preservation(label: &str) -> Self {
        let root = support::temp(label);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let started = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&root)
            .args([
                "work",
                "begin",
                "change",
                "--goal",
                "Work without preservation support",
                "--check-command",
                "true",
                "--config",
                "exitbind.json",
            ])
            .output()
            .unwrap();
        assert!(started.status.success(), "{started:?}");
        let work = serde_json::from_slice::<Value>(&started.stdout).unwrap()["work"]
            .as_str()
            .unwrap()
            .to_owned();
        Self { root, work }
    }
    fn call(&self, args: &[&str]) -> Output {
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .output()
            .unwrap()
    }
    fn call_with_stdin(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }
    fn record(&self, value: Value) -> Output {
        self.record_work(&self.work, value)
    }

    fn record_work(&self, work: &str, value: Value) -> Output {
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(["work", "record", work, "--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&value).unwrap())
        {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }
    fn ok(&self, value: Value) -> Value {
        let output = self.record(value);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn view(&self) -> Value {
        let output = self.call(&["work", "continuation", &self.work]);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn history(&self) -> Vec<u8> {
        fs::read(self.root.join(".exitbind/session-goal.jsonl")).unwrap()
    }
    fn initialize(&self) {
        self.ok(
            json!({"action":"init","sourceRef":"fixture:requirements-v1",
            "sourceText":"First requirement.",
            "requirements":[{"id":"first","text":"First requirement."}]}),
        );
    }
    fn return_stage(&self, outcome: &str, body: &str) -> Value {
        let next = self.call(&["work", "next", &self.work, "--full"]);
        assert!(next.status.success(), "{next:?}");
        let next: Value = serde_json::from_slice(&next.stdout).unwrap();
        let assignment = next["next"]["assignment"].as_str().unwrap();
        let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args([
                "work",
                "return",
                &self.work,
                assignment,
                "--outcome",
                outcome,
                "--config",
                "exitbind.json",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(body.as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn sha(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn reseal_goal_record(record: &mut Value) {
    record.as_object_mut().unwrap().remove("eventSha256");
    record["eventSha256"] = json!(sha(&canonical(record)));
}

#[path = "cross_host_continuity/continuation_context.rs"]
mod continuation_context;
#[path = "cross_host_continuity/handoff_and_checks.rs"]
mod handoff_and_checks;
#[path = "cross_host_continuity/readiness_continuity.rs"]
mod readiness_continuity;

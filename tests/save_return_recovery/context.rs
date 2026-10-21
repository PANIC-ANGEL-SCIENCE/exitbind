#[path = "../support/mod.rs"]
mod support;

use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Output};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .env("EXITBIND_NATIVE_RECOVERY_TEST", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        use std::io::Write;
        // A command may refuse and exit before reading stdin; its output is still asserted.
if let Err(error) = child.stdin.take().unwrap().write_all(input) { assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}"); }
        child.wait_with_output().unwrap()
    }

    fn value(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn worker_journal(&self) -> (String, Vec<u8>) {
        self.worker_journal_with_submission(true)
    }

    fn pending_worker(&self, goal: &str) -> (String, Value) {
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
        let lead = started["next"]["assignment"].as_str().unwrap().to_owned();
        let scoped = self.call(
            &["work", "return", &work, &lead, "--outcome", "scoped"],
            b"scope",
        );
        assert!(scoped.status.success(), "{}", text(&scoped));
        let worker = self.value(&["work", "next", &work, "--full"], b"")["next"].clone();
        (work, worker)
    }

    fn worker_journal_with_submission(&self, submit: bool) -> (String, Vec<u8>) {
        let started = self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "same Work native return",
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
        let lead = started["next"].clone();
        let lead_return = self.call(
            &[
                "work",
                "return",
                &work,
                lead["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"scope",
        );
        assert!(lead_return.status.success(), "{}", text(&lead_return));
        let worker = self.value(&["work", "next", &work, "--full"], b"")["next"].clone();
        let body = br#"{"outcome":"completed","summary":"recorded","reason":""}"#.to_vec();
        if submit {
            let worker_return = self.call(
                &[
                    "work",
                    "return",
                    &work,
                    worker["assignment"].as_str().unwrap(),
                    "--outcome",
                    "completed",
                ],
                &body,
            );
            assert!(worker_return.status.success(), "{}", text(&worker_return));
        }
        let packet = worker["packet"].clone();
        let assignment = worker["assignment"].as_str().unwrap().to_owned();
        let directory = self.root.join(".exitbind/native-actions").join(&work);
        fs::create_dir_all(&directory).unwrap();
        let journal = json!({
            "version": 1,
            "status": "completed",
            "work": work,
            "assignment": assignment,
            "assignmentSha256": canonical_sha(&packet),
            "role": "worker",
            "agent": "worker",
            "stage": packet["stage"],
            "attempt": packet["attempt"],
            "threadId": "thread-r18",
            "result": hex_encode(&body),
        });
        fs::write(
            directory.join(format!("{assignment}.json")),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        (work, body)
    }

    fn marker(&self) -> PathBuf {
        let marker = self.root.join("provider-spawned");
        let executable = self.root.join("fake-codex");
        fs::write(
            &executable,
            format!("#!/bin/sh\nprintf spawned > '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        marker
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn canonical_sha(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
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
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

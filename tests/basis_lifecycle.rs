#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{Arc, Barrier},
    thread,
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
        assert!(init.status.success(), "{init:?}");
        Self { root }
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(input) = input {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }

    fn value(&self, args: &[&str]) -> Value {
        self.value_with_input(args, b"")
    }

    fn value_with_input(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, Some(input));
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{args:?}: invalid JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    fn artifact(&self, name: &str, body: &[u8]) -> String {
        let path = self.root.join(".exitbind/artifacts").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        format!(".exitbind/artifacts/{name}")
    }

    fn ledger(&self) -> &'static str {
        ".exitbind/runs/basis.jsonl"
    }

    fn artifact_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = fs::read_dir(self.root.join(".exitbind/artifacts"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| format!("{key:?}:{}", canonical(&object[key])))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn hash_value(value: &Value) -> String {
    let digest = Sha256::digest(canonical(value).as_bytes());
    format!("{digest:x}")
}

fn basis_with_hash(mut basis: Value) -> Value {
    basis["sha256"] = json!(hash_value(&basis));
    basis
}

fn disposition_with_hash(mut disposition: Value) -> Value {
    disposition["sha256"] = json!(hash_value(&disposition));
    disposition
}

fn ledger_events(fixture: &Fixture) -> Vec<Value> {
    ledger_events_at(fixture, fixture.ledger())
}

fn ledger_events_at(fixture: &Fixture, ledger: &str) -> Vec<Value> {
    fs::read_to_string(fixture.root.join(ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[path = "basis_lifecycle/basis_corrections.rs"]
mod basis_corrections;
#[path = "basis_lifecycle/reader_compatibility.rs"]
mod reader_compatibility;
#[path = "basis_lifecycle/supersede_policy.rs"]
mod supersede_policy;

mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("context-v022");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }

    fn call(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command.current_dir(&self.root).args(args);
        if args.first() == Some(&"work") && matches!(args.get(1), Some(&"next" | &"resume")) {
            command.arg("--full");
        }
        command.arg("--config").arg(&self.config).output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn return_body(&self, work: &str, assignment: &str, outcome: &str, body: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(["work", "return", work, assignment, "--outcome", outcome])
            .arg("--config")
            .arg(&self.config)
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }

    fn rewrite_last<F>(&self, ledger: &str, mutate: F)
    where
        F: FnOnce(&mut Value),
    {
        let path = self.root.join(ledger);
        let mut lines: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let event = lines.last_mut().unwrap();
        mutate(event);
        if let Some(governor) = event
            .get_mut("governorEvent")
            .and_then(Value::as_object_mut)
        {
            if governor.contains_key("eventSha256") {
                governor.remove("eventSha256");
                let digest = Sha256::digest(serde_json::to_vec(governor).unwrap());
                let mut digest_hex = String::with_capacity(digest.len() * 2);
                for byte in digest {
                    write!(&mut digest_hex, "{byte:02x}").unwrap();
                }
                governor.insert("eventSha256".into(), json!(digest_hex));
            }
        }
        event.as_object_mut().unwrap().remove("eventSha256");
        let digest = Sha256::digest(serde_json::to_vec(event).unwrap());
        let mut digest_hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut digest_hex, "{byte:02x}").unwrap();
        }
        event["eventSha256"] = json!(digest_hex);
        fs::write(
            path,
            lines
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
                + "\n",
        )
        .unwrap();
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
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn assert_no_transport_paths(value: &Value) {
    match value {
        Value::Object(object) => {
            for key in [
                "artifactPathHint",
                "artifactRootHint",
                "ledgerPath",
                "path",
                "profilePath",
                "root",
                "sourcePath",
            ] {
                assert!(!object.contains_key(key), "transport path leaked: {key}");
            }
            object.values().for_each(assert_no_transport_paths);
        }
        Value::Array(values) => values.iter().for_each(assert_no_transport_paths),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn assert_resolver_backed_references(value: &Value) {
    match value {
        Value::Object(object) => {
            if object
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("ref:"))
            {
                assert_eq!(object.get("exact"), Some(&json!(true)));
                assert!(matches!(
                    object.get("kind").and_then(Value::as_str),
                    Some(
                        "ledger_history"
                            | "ledger_event"
                            | "check_log"
                            | "evidence"
                            | "stdout"
                            | "stderr",
                    )
                ));
            }
            if object.contains_key("path") {
                assert!(!object
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with("ref:")));
            }
            object.values().for_each(assert_resolver_backed_references);
        }
        Value::Array(values) => values.iter().for_each(assert_resolver_backed_references),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn resolver_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                if id.starts_with("ref:") {
                    ids.push(id.to_owned());
                }
            }
            object.values().for_each(|value| resolver_ids(value, ids));
        }
        Value::Array(values) => values.iter().for_each(|value| resolver_ids(value, ids)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).unwrap(),
                            canonical(&object[key])
                        )
                    })
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

fn refresh_digest(value: &mut Value) {
    value.as_object_mut().unwrap().remove("digest");
    let digest = Sha256::digest(canonical(value).as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    value["digest"] = json!(encoded);
}

fn refresh_event_hash(value: &mut Value) {
    value.as_object_mut().unwrap().remove("eventSha256");
    let digest = Sha256::digest(canonical(value).as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    value["eventSha256"] = json!(encoded);
}

fn granted_worker_completion() -> (Fixture, String, String) {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "fingerprint-changing completion",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
    assert!(permit.status.success(), "{permit:?}");
    fs::write(fixture.root.join("product.txt"), b"fingerprint changed\n").unwrap();
    let resumed = fixture.json(&["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker result\n");
    assert!(completed.status.success(), "{completed:?}");
    (fixture, work, ledger)
}

#[path = "context_v022/context_projection.rs"]
mod context_projection;
#[path = "context_v022/governor_policy.rs"]
mod governor_policy;
#[path = "context_v022/mutation_completion.rs"]
mod mutation_completion;
#[path = "context_v022/request_replay.rs"]
mod request_replay;

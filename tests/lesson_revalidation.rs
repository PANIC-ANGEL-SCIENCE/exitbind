//! Compatible recipient evolution is explicit; old authority remains historical.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("lesson-revalidate");
        let out = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let f = Self { root };
        let mut config = f.config();
        config["memory"] = json!({"root":".exitbind/memory","maxItems":16,"maxBytes":32768,"protocolScopes":["project-lessons.v1"],"syntheticScopes":[]});
        for name in ["lead", "worker"] {
            config["agents"][name]["crossContext"] = json!("protocol-only");
        }
        for right in [
            "memoryRead",
            "memoryWrite",
            "memoryReview",
            "memoryPromote",
            "memoryRevoke",
        ] {
            config["agents"]["lead"][right] = json!(["project-lessons.v1"]);
        }
        f.save(&config);
        fs::copy(f.root.join("exitbind.json"), f.root.join("prior.json")).unwrap();
        fs::write(f.root.join("guard"), b"coupling").unwrap();
        let identity = f.ok(&["project", "context", "--json"])["project"]["memoryIdentity"].clone();
        let lesson = json!({"version":1,"id":"stable-lesson","fact":"Review the coupled surface.","projectIdentity":identity,"owner":"lead",
            "provenance":["fixture reviewed fact"],"createdRevision":"1207ce96c36674747a4bab54692c46b5e243ff20","revalidatedRevision":"1207ce96c36674747a4bab54692c46b5e243ff20",
            "appliesTo":{"agents":["worker"],"taskTerms":[]},"guards":[{"path":"guard","sha256":format!("{:x}",Sha256::digest(b"coupling"))}]});
        fs::write(
            f.root.join("lesson.json"),
            serde_json::to_vec(&lesson).unwrap(),
        )
        .unwrap();
        f.ok(&[
            "memory",
            "propose",
            "lead",
            "lesson.json",
            "--scope",
            "project-lessons.v1",
            "--expires-at",
            "2100-01-01T00:00:00Z",
            "--ledger",
            ".exitbind/memory/lesson.jsonl",
        ]);
        for action in ["review", "promote"] {
            f.ok(&["memory", action, "lead", ".exitbind/memory/lesson.jsonl"]);
        }
        f
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("exitbind.json")).unwrap()).unwrap()
    }
    fn save(&self, v: &Value) {
        fs::write(
            self.root.join("exitbind.json"),
            serde_json::to_vec(v).unwrap(),
        )
        .unwrap();
    }
    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.call(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn revalidate(&self, apply: bool) -> Output {
        let mut args = vec![
            "memory",
            "revalidate",
            "lead",
            ".exitbind/memory/lesson.jsonl",
            "--from-config",
            "prior.json",
            "--reason",
            "Reviewed additive recipient access.",
        ];
        if apply {
            args.push("--apply");
        }
        self.call(&args)
    }
    fn add_recipient(&self) {
        let mut c = self.config();
        c["agents"]["worker"]["memoryRead"] = json!(["project-lessons.v1"]);
        self.save(&c);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn reviewed_compatible_lineage_enables_retirement_and_correction_without_rewriting_history() {
    let f = Fixture::new();
    let ledger = f.root.join(".exitbind/memory/lesson.jsonl");
    let before = fs::read(&ledger).unwrap();
    f.add_recipient();
    assert!(!f
        .call(&["memory", "revoke", "lead", ".exitbind/memory/lesson.jsonl"])
        .status
        .success());
    assert!(!f
        .call(&["memory", "resolve", "worker", "--json"])
        .status
        .success());
    let preview = f.revalidate(false);
    assert!(preview.status.success(), "{preview:?}");
    let value: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["effect"], "no-change");
    assert_eq!(value["revalidation"]["differences"][0]["agent"], "worker");
    assert_eq!(fs::read(&ledger).unwrap(), before);
    let out = f.revalidate(true);
    assert!(out.status.success(), "{out:?}");
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["effect"], "recorded");
    assert_eq!(value["event"]["version"], 2);
    assert!(fs::read(&ledger).unwrap().starts_with(&before));
    assert_eq!(
        f.ok(&["memory", "resolve", "worker", "--json"])["references"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let item = f.ok(&[
        "memory",
        "inspect",
        ".exitbind/memory/lesson.jsonl",
        "--json",
    ])["items"][0]["itemId"]
        .clone();
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/lesson.jsonl"]);
    assert_eq!(
        f.ok(&["memory", "resolve", "worker", "--json"])["references"],
        json!([])
    );
    let mut corrected: Value =
        serde_json::from_slice(&fs::read(f.root.join("lesson.json")).unwrap()).unwrap();
    corrected["fact"] = json!("Corrected reviewed coupling.");
    corrected["supersedes"] = item;
    fs::write(
        f.root.join("corrected.json"),
        serde_json::to_vec(&corrected).unwrap(),
    )
    .unwrap();
    f.ok(&[
        "memory",
        "propose",
        "lead",
        "corrected.json",
        "--scope",
        "project-lessons.v1",
        "--expires-at",
        "2100-01-01T00:00:00Z",
        "--ledger",
        ".exitbind/memory/corrected.jsonl",
    ]);
    for action in ["review", "promote"] {
        f.ok(&["memory", action, "lead", ".exitbind/memory/corrected.jsonl"]);
    }
    assert_eq!(
        f.ok(&["memory", "resolve", "worker", "--json"])["references"][0]["sourcePath"],
        "corrected.json"
    );
    let old = f.ok(&[
        "memory",
        "inspect",
        ".exitbind/memory/lesson.jsonl",
        "--json",
    ]);
    assert_eq!(old["events"].as_array().unwrap().len(), 5);
    assert_eq!(old["items"][0]["state"], "revoked");
}

#[test]
fn incompatible_rights_foreign_config_and_changed_provenance_refuse_without_append() {
    let f = Fixture::new();
    let ledger = f.root.join(".exitbind/memory/lesson.jsonl");
    let before = fs::read(&ledger).unwrap();
    let base = f.config();
    for (agent, key, value) in [
        ("lead", "memoryRevoke", json!([])),
        ("worker", "write", json!(["src/**"])),
        ("lead", "memoryRead", json!([])),
    ] {
        let mut c = base.clone();
        c["agents"][agent][key] = value;
        f.save(&c);
        assert!(!f.revalidate(true).status.success());
        assert_eq!(fs::read(&ledger).unwrap(), before);
    }
    f.save(&base);
    f.add_recipient();
    let foreign = Fixture::new();
    let out = f.call(&[
        "memory",
        "revalidate",
        "lead",
        ".exitbind/memory/lesson.jsonl",
        "--from-config",
        foreign.root.join("exitbind.json").to_str().unwrap(),
        "--reason",
        "Foreign configuration.",
        "--apply",
    ]);
    assert!(!out.status.success());
    fs::write(f.root.join("lesson.json"), b"changed provenance").unwrap();
    assert!(!f.revalidate(true).status.success());
    assert_eq!(fs::read(&ledger).unwrap(), before);
}

#[test]
fn revalidation_does_not_make_a_stale_guard_current_or_transfer_work_authority() {
    let f = Fixture::new();
    f.add_recipient();
    fs::write(f.root.join("guard"), b"stale").unwrap();
    let out = f.revalidate(true);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        f.ok(&["memory", "resolve", "worker", "--json"])["references"],
        json!([])
    );
    assert!(!f.root.join(".exitbind/work_ledgers").exists());
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/lesson.jsonl"]);
}

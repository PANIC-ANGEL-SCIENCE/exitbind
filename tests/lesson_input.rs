//! Parser/form boundary parity; rejected drafts must not append lifecycle events.
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
        let root = support::temp("lesson-input");
        assert!(Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap()
            .success());
        let f = Self { root };
        let out = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--skip-skills", "--root"])
            .arg(&f.root)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let mut c: Value =
            serde_json::from_slice(&fs::read(f.root.join("exitbind.json")).unwrap()).unwrap();
        c["memory"] = json!({"root":".exitbind/memory","maxItems":8,"maxBytes":8192,"protocolScopes":["project-lessons.v1"],"syntheticScopes":[]});
        for right in ["memoryWrite", "memoryReview", "memoryPromote"] {
            c["agents"]["lead"][right] = json!(["project-lessons.v1"]);
        }
        fs::write(f.root.join("exitbind.json"), c.to_string()).unwrap();
        fs::write(f.root.join("guard"), b"kept").unwrap();
        f
    }
    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .output()
            .unwrap()
    }
    fn source(&self) -> Value {
        let out = self.call(&["memory", "form", "--scope", "project-lessons.v1", "--json"]);
        assert!(out.status.success(), "{out:?}");
        let form: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            form["fieldBounds"]["appliesTo.taskTerms"]["maximumItems"],
            4
        );
        assert_eq!(
            form["fieldBounds"]["guards"]["element"]["path"]["maximumBytes"],
            256
        );
        let mut s = form["template"].clone();
        s["createdRevision"] = json!("a".repeat(40));
        s["revalidatedRevision"] = json!("b".repeat(64));
        s["appliesTo"] = json!({"agents":["worker"],"taskTerms":["one","two","three","four"]});
        s["guards"] = json!([{"path":"guard","sha256":format!("{:x}",Sha256::digest(b"kept"))}]);
        s
    }
    fn propose(&self, source: &Value) -> Output {
        fs::write(self.root.join("draft.json"), source.to_string()).unwrap();
        self.call(&[
            "memory",
            "propose",
            "lead",
            "draft.json",
            "--scope",
            "project-lessons.v1",
            "--ledger",
            ".exitbind/memory/draft.jsonl",
            "--expires-at",
            "2100-01-01T00:00:00Z",
        ])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn four_terms_parse_and_five_terms_refuse_before_append_with_the_field() {
    let f = Fixture::new();
    let mut s = f.source();
    s["appliesTo"]["taskTerms"]
        .as_array_mut()
        .unwrap()
        .push(json!("five"));
    let out = f.propose(&s);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("appliesTo.taskTerms: requires 0..4 items"),
        "{out:?}"
    );
    assert!(!f.root.join(".exitbind/memory/draft.jsonl").exists());
    s["appliesTo"]["taskTerms"].as_array_mut().unwrap().pop();
    let out = f.propose(&s);
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn utf8_fact_and_term_limits_are_bytes_and_precise_refusals_do_not_append() {
    let f = Fixture::new();
    let mut s = f.source();
    s["fact"] = json!("é".repeat(257));
    let out = f.propose(&s);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("fact: must be nonblank"),
        "{out:?}"
    );
    assert!(!f.root.join(".exitbind/memory/draft.jsonl").exists());
    s["fact"] = json!("é".repeat(256));
    s["appliesTo"]["taskTerms"] = json!(["é".repeat(33)]);
    let out = f.propose(&s);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("appliesTo.taskTerms[0]"),
        "{out:?}"
    );
    assert!(!f.root.join(".exitbind/memory/draft.jsonl").exists());
    s["appliesTo"]["taskTerms"] = json!(["é".repeat(32)]);
    let out = f.propose(&s);
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn provenance_agent_guard_and_revision_errors_identify_the_exact_field() {
    let f = Fixture::new();
    let good = f.source();
    let cases = [
        ("provenance", json!([]), "provenance"),
        ("owner", json!("worker"), "owner"),
        ("createdRevision", json!("A".repeat(40)), "createdRevision"),
        (
            "appliesTo",
            json!({"agents":["worker","worker"],"taskTerms":[]}),
            "appliesTo.agents",
        ),
        (
            "guards",
            json!([{"path":"../elsewhere","sha256":"a".repeat(64)}]),
            "guards[0].path",
        ),
        ("supersedes", json!("A".repeat(64)), "supersedes"),
    ];
    for (field, value, error_field) in cases {
        let mut s = good.clone();
        s[field] = value;
        let out = f.propose(&s);
        assert!(!out.status.success(), "{field}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(error_field),
            "{field}: {out:?}"
        );
        assert!(!f.root.join(".exitbind/memory/draft.jsonl").exists());
    }
}

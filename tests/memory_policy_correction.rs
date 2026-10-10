//! The installed CLI can repair an explicitly selected recall policy without
//! source access, hidden roots, historical edits or transferred Work authority.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

const LEDGER: &str = ".exitbind/memory/lesson.jsonl";
const REASON: &str = "Owner selected recall and retirement for this exact project lesson.";
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::with_expiry("2100-01-01T00:00:00Z")
    }
    fn with_expiry(expiry: &str) -> Self {
        let root = support::temp("memory-policy-correction");
        let out = support::run(
            Command::new(env!("CARGO_BIN_EXE_exitbind"))
                .args(["init", "--mode", "portable", "--root"])
                .arg(&root),
        );
        assert!(out.status.success(), "{out:?}");
        let f = Self { root };
        let mut c = f.config();
        c["memory"] = json!({"root":".exitbind/memory","maxItems":4,"maxBytes":8192,
            "protocolScopes":["project-lessons.v1"],"syntheticScopes":[]});
        for right in [
            "memoryRead",
            "memoryWrite",
            "memoryReview",
            "memoryPromote",
            "memoryExpire",
        ] {
            c["agents"]["lead"][right] = json!(["project-lessons.v1"]);
        }
        c["agents"]["lead"]["commands"] = json!(["true"]);
        f.save(&c);
        fs::copy(f.root.join("exitbind.json"), f.root.join("prior.json")).unwrap();
        fs::write(f.root.join("guard"), b"fixture input").unwrap();
        let identity = f.ok(&["project", "context", "--json"])["project"]["memoryIdentity"].clone();
        let source = json!({"version":1,"id":"stored-field-lesson","fact":"Inspect the saved recovery result.",
            "projectIdentity":identity,"owner":"lead","provenance":["Reviewed fixture coupling"],
            "createdRevision":"1b3a0e97b5e7b8c974ae513b993e54df51b12560",
            "revalidatedRevision":"1b3a0e97b5e7b8c974ae513b993e54df51b12560",
            "appliesTo":{"agents":["lead"],"taskTerms":["recovery"]},
            "guards":[{"path":"guard","sha256":format!("{:x}",Sha256::digest(b"fixture input"))}]});
        fs::write(
            f.root.join("lesson.json"),
            serde_json::to_vec(&source).unwrap(),
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
            expiry,
            "--ledger",
            LEDGER,
        ]);
        for action in ["review", "promote"] {
            f.ok(&["memory", action, "lead", LEDGER]);
        }
        f
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("exitbind.json")).unwrap()).unwrap()
    }
    fn save(&self, c: &Value) {
        fs::write(
            self.root.join("exitbind.json"),
            serde_json::to_vec_pretty(c).unwrap(),
        )
        .unwrap();
    }
    fn call(&self, args: &[&str]) -> Output {
        support::run(
            Command::new(env!("CARGO_BIN_EXE_exitbind"))
                .current_dir(&self.root)
                .args(args)
                .args(["--config", "exitbind.json"]),
        )
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.call(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn corrected(&self) {
        let mut c = self.config();
        c["agents"]["lead"]["crossContext"] = json!("protocol-only");
        c["agents"]["lead"]["memoryRevoke"] = json!(["project-lessons.v1"]);
        self.save(&c);
    }
    fn correction(&self, apply: bool, decision: Option<&str>) -> Output {
        let mut args = vec![
            "memory",
            "correct-policy",
            LEDGER,
            "--from-config",
            "prior.json",
            "--reason",
            REASON,
        ];
        if apply {
            args.push("--apply");
        }
        if let Some(path) = decision {
            args.extend(["--owner-decision", path]);
        }
        self.call(&args)
    }
    fn decision(&self) -> Value {
        let out = self.correction(false, None);
        assert!(out.status.success(), "{out:?}");
        let preview: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(preview["effect"], "no-change");
        assert_eq!(preview["ownerDecision"]["approved"], false);
        preview["ownerDecision"].clone()
    }
    fn approve(&self) {
        let mut d = self.decision();
        d["approved"] = json!(true);
        self.save_decision(&d);
    }
    fn save_decision(&self, d: &Value) {
        fs::write(
            self.root.join("decision.json"),
            serde_json::to_vec_pretty(d).unwrap(),
        )
        .unwrap();
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(LEDGER)).unwrap()
    }
    fn resolve(&self, agent: &str, task: &str) -> Value {
        self.ok(&["memory", "resolve", agent, "--task", task, "--json"])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn approved_policy_correction_delivers_the_original_item_and_replay_recovers_without_append() {
    let f = Fixture::new();
    let stored = f.resolve("lead", "recovery");
    assert_eq!(stored["references"], json!([]));
    assert_eq!(stored["selection"]["reason"], "cross_context_disabled");
    let before = f.bytes();
    let item = f.ok(&["memory", "inspect", LEDGER, "--json"])["items"][0].clone();
    f.corrected();
    assert!(!f
        .call(&[
            "memory",
            "revalidate",
            "lead",
            LEDGER,
            "--from-config",
            "prior.json",
            "--reason",
            REASON,
            "--apply"
        ])
        .status
        .success());
    let preview = f.decision();
    assert_eq!(f.bytes(), before);
    assert_eq!(preview["itemId"], item["itemId"]);
    f.approve();
    let out = f.correction(true, Some("decision.json"));
    assert!(out.status.success(), "{out:?}");
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["status"], "recorded");
    assert_eq!(result["event"]["version"], 3);
    let after = f.bytes();
    assert!(after.starts_with(&before));
    let current = f.ok(&["memory", "inspect", LEDGER, "--json"]);
    assert_eq!(current["events"].as_array().unwrap().len(), 4);
    for key in ["itemId", "source", "expiresAt", "state"] {
        assert_eq!(current["items"][0][key], item[key]);
    }
    assert_eq!(
        f.resolve("lead", "recovery")["references"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let entry = f.ok(&["project", "context", "--task", "recovery", "--json"]);
    assert_eq!(entry["memory"]["references"][0]["itemId"], item["itemId"]);
    let id = item["itemId"].as_str().unwrap();
    let content = f.ok(&[
        "project", "context", "memory", id, "--task", "recovery", "--json",
    ]);
    let source: Value = serde_json::from_str(content["content"].as_str().unwrap()).unwrap();
    assert_eq!(source["fact"], "Inspect the saved recovery result.");
    assert!(!f
        .call(&[
            "project",
            "context",
            "memory",
            id,
            "--task",
            "unrelated",
            "--json"
        ])
        .status
        .success());
    assert_eq!(
        f.resolve("lead", "unrelated image task")["references"],
        json!([])
    );
    assert_eq!(f.resolve("worker", "recovery")["references"], json!([]));
    let replay = f.correction(true, Some("decision.json"));
    assert!(replay.status.success(), "{replay:?}");
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["status"], "existing_verified");
    assert_eq!(replay["effect"], "no-change");
    assert_eq!(f.bytes(), after);
    assert!(!f.root.join(".exitbind/work_ledgers").exists());
    let work = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "recovery contract",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--detail",
    ]);
    assert_eq!(
        work["recipientContext"]["projectLessons"]["items"][0]["lesson"]["fact"],
        "Inspect the saved recovery result."
    );
    let unrelated = f.ok(&[
        "work",
        "begin",
        "change",
        "--goal",
        "unrelated image task",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--detail",
    ]);
    assert!(unrelated["recipientContext"]
        .get("projectLessons")
        .is_none());
    f.ok(&["memory", "revoke", "lead", LEDGER]);
    assert_eq!(f.resolve("lead", "recovery")["references"], json!([]));
    assert!(!f.correction(true, Some("decision.json")).status.success());
}

#[test]
fn unapproved_wrong_owner_config_item_and_head_decisions_refuse_without_effect() {
    let f = Fixture::new();
    let before = f.bytes();
    f.corrected();
    assert!(!f.correction(true, None).status.success());
    let template = f.decision();
    f.save_decision(&template);
    assert!(!f.correction(true, Some("decision.json")).status.success());
    for key in [
        "owner",
        "itemId",
        "previousConfigSha256",
        "currentConfigSha256",
        "priorLedgerHeadSha256",
        "projectIdentity",
    ] {
        let mut d = template.clone();
        d["approved"] = json!(true);
        d[key] = json!("wrong");
        f.save_decision(&d);
        assert!(
            !f.correction(true, Some("decision.json")).status.success(),
            "{key}"
        );
        assert_eq!(f.bytes(), before);
    }
    let mut d = template;
    d["approved"] = json!(true);
    d["differences"] = json!([]);
    f.save_decision(&d);
    assert!(!f.correction(true, Some("decision.json")).status.success());
    assert_eq!(f.bytes(), before);
}

#[test]
fn unrelated_authority_root_ownership_and_all_scope_expansion_refuse() {
    let f = Fixture::new();
    let before = f.bytes();
    f.corrected();
    let base = f.config();
    for (name, key, value) in [
        ("worker", "write", json!(["src/**"])),
        (
            "lead",
            "memoryRead",
            json!(["project-lessons.v1", "secrets"]),
        ),
        ("lead", "crossContext", json!("same-scope")),
        ("lead", "retention", json!("until-revoked")),
    ] {
        let mut c = base.clone();
        c["agents"][name][key] = value;
        f.save(&c);
        assert!(!f.correction(false, None).status.success());
        assert_eq!(f.bytes(), before);
    }
    let mut c = base.clone();
    c["orchestration"]["lead"] = json!("worker");
    f.save(&c);
    assert!(!f.correction(false, None).status.success());
    let mut c = base.clone();
    c["memory"]["root"] = json!("hidden-memory");
    f.save(&c);
    assert!(!f.correction(false, None).status.success());
    assert_eq!(f.bytes(), before);
    f.save(&base);
    f.approve();
    let mut c = base;
    c["agents"]["lead"]["displayName"] = json!("Later name");
    f.save(&c);
    assert!(!f.correction(true, Some("decision.json")).status.success());
    assert_eq!(f.bytes(), before);
}

#[test]
fn changed_source_and_profile_refuse_and_changed_guard_stays_excluded() {
    let f = Fixture::new();
    let before = f.bytes();
    f.corrected();
    f.approve();
    let original = fs::read(f.root.join("lesson.json")).unwrap();
    fs::write(f.root.join("lesson.json"), b"changed").unwrap();
    assert!(!f.correction(true, Some("decision.json")).status.success());
    assert_eq!(f.bytes(), before);
    fs::write(f.root.join("lesson.json"), original).unwrap();
    let profile = f.config()["agents"]["lead"]["profile"]
        .as_str()
        .unwrap()
        .to_owned();
    let bytes = fs::read(f.root.join(&profile)).unwrap();
    fs::write(f.root.join(&profile), b"changed profile").unwrap();
    assert!(!f.correction(true, Some("decision.json")).status.success());
    assert_eq!(f.bytes(), before);
    fs::write(f.root.join(profile), bytes).unwrap();
    fs::write(f.root.join("guard"), b"changed guard").unwrap();
    let out = f.correction(true, Some("decision.json"));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(f.resolve("lead", "recovery")["references"], json!([]));
    fs::write(f.root.join("guard"), b"fixture input").unwrap();
    assert_eq!(
        f.resolve("lead", "recovery")["references"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::write(f.root.join("lesson.json"), b"changed after correction").unwrap();
    assert!(!f
        .call(&["memory", "resolve", "lead", "--task", "recovery", "--json"])
        .status
        .success());
}

#[test]
fn policy_correction_does_not_refresh_expiry_or_revive_a_retired_item() {
    let f = Fixture::with_expiry("2000-01-01T00:00:00Z");
    f.corrected();
    f.approve();
    let out = f.correction(true, Some("decision.json"));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(f.resolve("lead", "recovery")["references"], json!([]));
    f.ok(&["memory", "expire", "lead", LEDGER]);
    let bytes = f.bytes();
    assert!(!f.correction(true, Some("decision.json")).status.success());
    assert_eq!(f.bytes(), bytes);
}

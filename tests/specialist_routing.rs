//! Real CLI routing and the existing memory lifecycle, without a distributor.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Output};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("specialist-routing");
        support::git_topology::repository(&root);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--skip-skills", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let f = Self { root };
        let mut c = f.config();
        let mut worker = c["agents"]
            .as_object_mut()
            .unwrap()
            .remove("worker")
            .unwrap();
        worker["purpose"] = json!("Bounded record aggregation");
        worker["nativeName"] = json!("record_specialist");
        worker["runtime"] = json!({"host":"codex","fallback":"none"});
        worker["commands"] = json!(["python3"]);
        worker["memoryRead"] = json!(["specialist.record-specialist.v1"]);
        worker["memoryWrite"] = json!(["specialist.record-specialist.v1"]);
        worker["crossContext"] = json!("protocol-only");
        c["agents"]["record-specialist"] = worker;
        c["workflows"]["change"]["workers"] = json!(["record-specialist"]);
        c["memory"] = json!({"root":".exitbind/memory","maxItems":8,"maxBytes":16384,
            "protocolScopes":["specialist.record-specialist.v1"],"syntheticScopes":[]});
        c["agents"]["reviewer"]["memoryReview"] = json!(["specialist.record-specialist.v1"]);
        c["agents"]["lead"]["memoryPromote"] = json!(["specialist.record-specialist.v1"]);
        c["agents"]["lead"]["memoryRevoke"] = json!(["specialist.record-specialist.v1"]);
        f.save(&c);
        f
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("exitbind.json")).unwrap()).unwrap()
    }
    fn save(&self, c: &Value) {
        fs::write(self.root.join("exitbind.json"), c.to_string()).unwrap();
    }
    fn call(&self, args: &[&str]) -> Output {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"]);
        if args.first() != Some(&"memory") {
            command.arg("--json");
        }
        command.output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.call(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn route(&self, task: &str) -> Value {
        self.ok(&[
            "project",
            "route",
            "--task",
            task,
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex",
        ])
    }
    fn draft(&self, source: &str, revision: &str) {
        let identity = self.ok(&["project", "context"])["project"]["memoryIdentity"].clone();
        let v = json!({"version":1,"id":"presence","revision":revision,"kind":"skill","author":"record-specialist",
            "projectIdentity":identity,"taskTerms":["aggregation"],"hosts":["codex"],
            "summary":"Separate observed zero from unavailable values.","preconditions":["Schema distinguishes absence."],
            "procedure":["Track presence and sum independently."],"verification":["Exercise all-missing and observed-zero groups."],
            "requiredCommands":["python3"],"evidence":["task-a-report.md"],"guards":[]});
        fs::write(self.root.join(source), v.to_string()).unwrap();
    }
    fn propose(&self, source: &str, ledger: &str) {
        self.ok(&[
            "memory",
            "propose",
            "record-specialist",
            source,
            "--scope",
            "specialist.record-specialist.v1",
            "--expires-at",
            "2100-01-01T00:00:00Z",
            "--ledger",
            ledger,
        ]);
    }
    fn qualify(&self, ledger: &str) {
        self.ok(&["memory", "review", "reviewer", ledger]);
        self.ok(&["memory", "promote", "lead", ledger]);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn draft_is_inactive_and_reviewed_owned_skill_is_current_and_bounded() {
    let f = Fixture::new();
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/r1.jsonl");
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    let denied = f.call(&[
        "memory",
        "review",
        "record-specialist",
        ".exitbind/memory/r1.jsonl",
    ]);
    assert!(!denied.status.success());
    f.qualify(".exitbind/memory/r1.jsonl");
    let first = f.route("record aggregation");
    assert_eq!(first["selected"], "record-specialist");
    assert_eq!(first["snapshot"]["experience"][0]["revision"], "r1");
    assert_eq!(first, f.route("record aggregation")); // lost-reply read is idempotent
    assert_eq!(
        f.route("format invoice documentation")["snapshot"]["experience"],
        json!([])
    );
    assert_eq!(
        first["snapshot"]["declaredBoundary"]["commands"],
        json!(["python3"])
    );
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/r1.jsonl"]);
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    assert!(!f
        .call(&["memory", "promote", "lead", ".exitbind/memory/r1.jsonl"])
        .status
        .success());
    fs::copy(f.root.join("draft.json"), f.root.join("relocated.json")).unwrap();
    f.propose("relocated.json", ".exitbind/memory/reintroduced.jsonl");
    f.qualify(".exitbind/memory/reintroduced.jsonl");
    let revived = f.call(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!revived.status.success());
    assert!(String::from_utf8(revived.stdout)
        .unwrap()
        .contains("revoked specialist source"));
}

#[test]
fn routing_ambiguity_no_match_and_incompatible_host_do_not_expand_rights() {
    let f = Fixture::new();
    let mut c = f.config();
    c["agents"]["other"] = c["agents"]["record-specialist"].clone();
    c["agents"]["other"]["nativeName"] = json!("other_specialist");
    c["workflows"]["change"]["workers"] = json!(["record-specialist", "other"]);
    f.save(&c);
    let ambiguous = f.route("record aggregation");
    assert_eq!(ambiguous["state"], "lead_choice");
    assert!(ambiguous["snapshot"].is_null());
    assert_eq!(
        f.ok(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex",
            "--agent",
            "record-specialist"
        ])["selected"],
        "record-specialist"
    );
    let none = f.ok(&[
        "project",
        "route",
        "--task",
        "paint",
        "--purpose",
        "painting",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert_eq!(none["state"], "lead_held");
    let incompatible = f.ok(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "claude",
    ]);
    assert_eq!(incompatible["state"], "lead_held");
    assert!(!f
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "reviewer",
            "--host",
            "codex",
            "--agent",
            "record-specialist"
        ])
        .status
        .success());
    assert_eq!(f.config(), c);
}

#[test]
fn wrong_recipient_and_foreign_project_never_receive_private_content() {
    let f = Fixture::new();
    let mut c = f.config();
    c["agents"]["other"] = c["agents"]["record-specialist"].clone();
    c["agents"]["other"]["nativeName"] = json!("other_specialist");
    c["agents"]["other"]["memoryRead"] = json!([]);
    c["workflows"]["change"]["workers"] = json!(["record-specialist", "other"]);
    f.save(&c);
    f.draft("draft.json", "r1");
    f.propose("draft.json", ".exitbind/memory/r1.jsonl");
    f.qualify(".exitbind/memory/r1.jsonl");
    let other = f.ok(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
        "--agent",
        "other",
    ]);
    let encoded = other.to_string();
    assert!(!encoded.contains("presence"));
    assert!(!encoded.contains("draft.json"));
    let foreign = Fixture::new();
    fs::copy(f.root.join("draft.json"), foreign.root.join("draft.json")).unwrap();
    foreign.propose("draft.json", ".exitbind/memory/r1.jsonl");
    foreign.qualify(".exitbind/memory/r1.jsonl");
    assert!(!foreign
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex"
        ])
        .status
        .success());
}

#[test]
fn drift_guard_expiry_and_concurrent_revisions_fail_closed() {
    let f = Fixture::new();
    f.draft("one.json", "r1");
    let mut v: Value = serde_json::from_slice(&fs::read(f.root.join("one.json")).unwrap()).unwrap();
    fs::write(f.root.join("prerequisite.txt"), "one").unwrap();
    v["guards"] = json!([{"path":"prerequisite.txt","sha256":"7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed"}]);
    fs::write(f.root.join("one.json"), v.to_string()).unwrap();
    f.propose("one.json", ".exitbind/memory/one.jsonl");
    f.qualify(".exitbind/memory/one.jsonl");
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::write(f.root.join("prerequisite.txt"), "two").unwrap();
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    f.draft("two.json", "r2");
    f.propose("two.json", ".exitbind/memory/two.jsonl");
    f.qualify(".exitbind/memory/two.jsonl");
    assert!(!f
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex"
        ])
        .status
        .success());
    f.ok(&["memory", "revoke", "lead", ".exitbind/memory/one.jsonl"]);
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"][0]["revision"],
        "r2"
    );
    fs::write(f.root.join("two.json"), "{}").unwrap();
    assert!(!f
        .call(&[
            "project",
            "route",
            "--task",
            "record aggregation",
            "--purpose",
            "record",
            "--role",
            "worker",
            "--host",
            "codex"
        ])
        .status
        .success());
}

#[test]
fn expired_items_and_symlinked_profile_preserve_host_layout() {
    let f = Fixture::new();
    f.draft("draft.json", "r1");
    f.ok(&[
        "memory",
        "propose",
        "record-specialist",
        "draft.json",
        "--scope",
        "specialist.record-specialist.v1",
        "--expires-at",
        "2000-01-01T00:00:00Z",
        "--ledger",
        ".exitbind/memory/old.jsonl",
    ]);
    f.qualify(".exitbind/memory/old.jsonl");
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    let profile = f.root.join("exitbind/agents/worker.md");
    let bytes = fs::read(&profile).unwrap();
    fs::remove_file(&profile).unwrap();
    fs::write(f.root.join("custom.md"), &bytes).unwrap();
    std::os::unix::fs::symlink(f.root.join("custom.md"), &profile).unwrap();
    let out = f.call(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!out.status.success());
    assert!(fs::symlink_metadata(&profile)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(f.root.join("custom.md")).unwrap(), bytes);
}

#[test]
fn unrelated_scopes_and_inaccessible_metadata_are_not_read_or_disclosed() {
    let f = Fixture::new();
    let mut c = f.config();
    c["agents"]["record-specialist"]["memoryRead"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["agents"]["record-specialist"]["memoryWrite"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["agents"]["reviewer"]["memoryReview"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["agents"]["lead"]["memoryPromote"] =
        json!(["specialist.record-specialist.v1", "private-other"]);
    c["memory"]["protocolScopes"] = json!(["specialist.record-specialist.v1", "private-other"]);
    f.save(&c);
    fs::write(
        f.root.join("private-filename.json"),
        "This is not specialist JSON",
    )
    .unwrap();
    f.ok(&[
        "memory",
        "propose",
        "record-specialist",
        "private-filename.json",
        "--scope",
        "private-other",
        "--ledger",
        ".exitbind/memory/other.jsonl",
    ]);
    f.qualify(".exitbind/memory/other.jsonl");
    // Even authorized but unrelated sources must never be opened by this consumer.
    fs::write(
        f.root.join("private-filename.json"),
        vec![b'x'; 1024 * 1024],
    )
    .unwrap();
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
    std::os::unix::fs::symlink(
        "missing",
        f.root.join(".exitbind/memory/private-ledger-name.jsonl"),
    )
    .unwrap();
    let out = f.call(&[
        "project",
        "route",
        "--task",
        "record aggregation",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!out.status.success());
    assert!(!String::from_utf8(out.stdout)
        .unwrap()
        .contains("private-ledger-name"));
    c["agents"]["record-specialist"]["memoryRead"] = json!([]);
    f.save(&c);
    assert_eq!(
        f.route("record aggregation")["snapshot"]["experience"],
        json!([])
    );
}

#[test]
fn oversized_own_source_refuses_without_private_name_and_help_names_route() {
    let f = Fixture::new();
    f.draft("private-source.json", "r1");
    f.propose("private-source.json", ".exitbind/memory/own.jsonl");
    f.qualify(".exitbind/memory/own.jsonl");
    fs::write(f.root.join("private-source.json"), vec![b'x'; 1024 * 1024]).unwrap();
    let out = f.call(&[
        "project",
        "route",
        "--task",
        "unrelated record task",
        "--purpose",
        "record",
        "--role",
        "worker",
        "--host",
        "codex",
    ]);
    assert!(!out.status.success());
    let error = String::from_utf8(out.stdout).unwrap();
    assert!(error.contains("4096"));
    assert!(!error.contains("private-source"));
    let help = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["project", "route", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .contains("--role worker|reviewer|adviser"));
}

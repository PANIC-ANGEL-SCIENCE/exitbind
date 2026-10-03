//! Historical failure shapes at the actual CLI and harness boundaries.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

const SCOPE: &str = "project-lessons.v1";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("recurrence");
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        let f = Self { root };
        let config = f.root.join("exitbind.json");
        let mut v: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        v["memory"] = json!({"root":".exitbind/memory","maxItems":16,"maxBytes":32768,"protocolScopes":[SCOPE],"syntheticScopes":[]});
        for name in ["lead", "worker"] {
            v["agents"][name]["memoryRead"] = json!([SCOPE]);
            v["agents"][name]["crossContext"] = json!("protocol-only");
            for right in [
                "memoryWrite",
                "memoryReview",
                "memoryPromote",
                "memoryRevoke",
            ] {
                v["agents"][name][right] = json!([SCOPE]);
            }
        }
        fs::write(config, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
        fs::create_dir_all(f.root.join(".exitbind/memory")).unwrap();
        fs::write(
            f.root.join("coupling.txt"),
            "recipient and native delivery share a contract\n",
        )
        .unwrap();
        f
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
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
    fn value(&self, args: &[&str]) -> Value {
        let output = self.call(args, b"");
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn lesson(&self, id: &str, fact: &str, terms: &[&str]) -> Value {
        let project =
            self.value(&["project", "context", "--json"])["project"]["memoryIdentity"].clone();
        json!({"version":1,"id":id,"fact":fact,"projectIdentity":project,"owner":"lead",
            "provenance":["reviewed fixture coupling"],"createdRevision":"82a77f9cb4675bb515c294ab16a1d70d6b5e97c4","revalidatedRevision":sha(b"reviewed revision"),
            "appliesTo":{"agents":["worker"],"taskTerms":terms},
            "guards":[{"path":"coupling.txt","sha256":sha(&fs::read(self.root.join("coupling.txt")).unwrap())}]})
    }
    fn propose(&self, stem: &str, lesson: &Value) -> Value {
        let source = format!(".exitbind/{stem}.json");
        fs::write(self.root.join(&source), serde_json::to_vec(lesson).unwrap()).unwrap();
        self.value(&[
            "memory",
            "propose",
            "lead",
            &source,
            "--scope",
            SCOPE,
            "--expires-at",
            "2100-01-01T00:00:00Z",
            "--ledger",
            &format!(".exitbind/memory/{stem}.jsonl"),
        ])
    }
    fn accept(&self, stem: &str, lesson: &Value) -> Value {
        self.propose(stem, lesson);
        let ledger = format!(".exitbind/memory/{stem}.jsonl");
        self.value(&["memory", "review", "lead", &ledger]);
        self.value(&["memory", "promote", "lead", &ledger])
    }
    fn worker(&self, goal: &str) -> (String, Value) {
        let v = self.value(&[
            "work",
            "begin",
            "change",
            "--goal",
            goal,
            "--check-command",
            "test -f coupling.txt",
            "--review-policy",
            "required",
        ]);
        let work = v["work"].as_str().unwrap().to_owned();
        let output = self.call(
            &[
                "work",
                "return",
                &work,
                v["next"]["assignment"].as_str().unwrap(),
                "--outcome",
                "scoped",
            ],
            b"bounded fixture scope\n",
        );
        assert!(output.status.success(), "{}", text(&output));
        let detail = self.value(&["work", "detail", &work, "--json"]);
        (work, detail)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn ordinary_recipient_gets_only_applicable_current_project_lessons() {
    let f = Fixture::new();
    f.accept(
        "coupling",
        &f.lesson(
            "recipient-coupling",
            "Inspect the native delivery companion before changing recipient context.",
            &["recipient"],
        ),
    );
    let (_, detail) = f.worker("repair recipient context");
    assert_eq!(
        detail["recipientContext"]["projectLessons"]["items"][0]["lesson"]["id"],
        "recipient-coupling"
    );
    assert_eq!(
        detail["recipientContext"]["projectLessons"]["omittedCount"],
        0
    );
    assert!(detail["actionForms"]["beforeEditing"]["command"]["argv"].is_array());
    let (_, other) = f.worker("update unrelated docs");
    assert!(other["recipientContext"].get("projectLessons").is_none());
    let other_project = Fixture::new();
    let (_, other) = other_project.worker("repair recipient context");
    assert!(other["recipientContext"].get("projectLessons").is_none());
    fs::write(f.root.join("coupling.txt"), "changed contract\n").unwrap();
    let (_, stale) = f.worker("repair recipient context after guard changes");
    assert!(stale["recipientContext"].get("projectLessons").is_none());
}

#[test]
fn correction_retirement_and_owner_rights_prevent_contradictory_durable_truth() {
    let f = Fixture::new();
    let first = f.accept(
        "first",
        &f.lesson("coupling", "Original durable fact.", &["recipient"]),
    );
    let second = f.lesson("coupling", "Corrected durable fact.", &["recipient"]);
    f.propose("second", &second);
    f.value(&["memory", "review", "lead", ".exitbind/memory/second.jsonl"]);
    let blocked = f.call(
        &["memory", "promote", "lead", ".exitbind/memory/second.jsonl"],
        b"",
    );
    assert!(!blocked.status.success());
    assert!(text(&blocked).contains("retire the accepted lesson"));
    let worker = f.call(
        &[
            "memory",
            "promote",
            "worker",
            ".exitbind/memory/second.jsonl",
        ],
        b"",
    );
    assert!(!worker.status.success());
    assert!(text(&worker).contains("configured Lead"));
    f.value(&["memory", "revoke", "lead", ".exitbind/memory/first.jsonl"]);
    f.value(&["memory", "promote", "lead", ".exitbind/memory/second.jsonl"]);
    let (_, detail) = f.worker("recipient repair");
    let items = detail["recipientContext"]["projectLessons"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["lesson"]["fact"], "Corrected durable fact.");
    assert_ne!(items[0]["reference"]["itemId"], first["itemId"]);
    f.value(&["memory", "revoke", "lead", ".exitbind/memory/second.jsonl"]);
    let (_, detail) = f.worker("recipient repair after retirement");
    assert!(detail["recipientContext"].get("projectLessons").is_none());
}

#[test]
fn lesson_delivery_is_bounded_and_emits_an_exact_inspection_route() {
    let f = Fixture::new();
    for i in 0..8 {
        f.accept(
            &format!("lesson-{i}"),
            &f.lesson(&format!("coupling-{i}"), &"f".repeat(480), &["recipient"]),
        );
    }
    let (_, detail) = f.worker("recipient repair");
    let lessons = &detail["recipientContext"]["projectLessons"];
    assert!(serde_json::to_vec(lessons).unwrap().len() <= 6144);
    let delivered = lessons["items"].as_array().unwrap().len();
    assert!(delivered <= 4);
    assert_eq!(
        delivered + lessons["omittedCount"].as_u64().unwrap() as usize,
        8
    );
    let argv: Vec<_> = lessons["detail"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    let out = Command::new(argv[0]).args(&argv[1..]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["references"].as_array().unwrap().len(), 8);
}

#[test]
fn native_delivery_uses_the_same_lesson_cap_without_rejecting_an_eligible_registry() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    for i in 0..9 {
        f.accept(
            &format!("native-{i}"),
            &f.lesson(&format!("native-{i}"), &"n".repeat(400), &["recipient"]),
        );
    }
    let (work, _) = f.worker("recipient native delivery");
    let outside = support::temp("native-lesson-capture");
    let capture = outside.join("prompt");
    let executable = outside.join("fake-codex");
    let source = format!("#!/bin/sh\ncat > '{}'\nprintf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"lesson-fixture\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"outcome\\\":\\\"completed\\\",\\\"summary\\\":\\\"bounded fixture\\\",\\\"reason\\\":\\\"\\\"}}\"}}}}' '{{\"type\":\"turn.completed\",\"status\":\"completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}'\n",capture.display());
    fs::write(&executable, source).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let output = f.call(
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
    let prompt = fs::read_to_string(&capture).unwrap();
    let section = prompt
        .split_once("CURRENT APPLICABLE PROJECT LESSONS:\n")
        .unwrap()
        .1
        .lines()
        .next()
        .unwrap();
    let value: Value = serde_json::from_str(section).unwrap();
    assert!(section.len() <= 6144);
    let count = value["items"].as_array().unwrap().len();
    assert!(count <= 4);
    assert_eq!(count + value["omittedCount"].as_u64().unwrap() as usize, 9);
    assert!(prompt.contains("no permission, check, review or acceptance authority"));
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn malformed_foreign_unproven_and_unexpiring_lessons_refuse_before_recording() {
    let f = Fixture::new();
    let original = f.lesson("coupling", "Durable fact.", &["recipient"]);
    for (key, value) in [
        ("projectIdentity", json!("root:other")),
        ("provenance", json!([])),
        ("fact", json!("f".repeat(513))),
        ("machinePath", json!("forbidden transient field")),
    ] {
        let mut lesson = original.clone();
        lesson[key] = value;
        fs::write(
            f.root.join(".exitbind/invalid.json"),
            serde_json::to_vec(&lesson).unwrap(),
        )
        .unwrap();
        let out = f.call(
            &[
                "memory",
                "propose",
                "lead",
                ".exitbind/invalid.json",
                "--scope",
                SCOPE,
                "--expires-at",
                "2100-01-01T00:00:00Z",
                "--ledger",
                ".exitbind/memory/invalid.jsonl",
            ],
            b"",
        );
        assert!(!out.status.success(), "accepted {key}");
        assert!(!f.root.join(".exitbind/memory/invalid.jsonl").exists());
    }
    fs::write(
        f.root.join(".exitbind/valid.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    let out = f.call(
        &[
            "memory",
            "propose",
            "lead",
            ".exitbind/valid.json",
            "--scope",
            SCOPE,
            "--ledger",
            ".exitbind/memory/invalid.jsonl",
        ],
        b"",
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("expires-at"));
}

#[test]
fn preflight_resolves_cargo_once_and_refuses_product_target_log_or_temp() {
    let outside = support::temp("preflight");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-local.sh");
    let cargo = std::env::var_os("CARGO").unwrap();
    let run = |field: &str, value: &std::path::Path| {
        Command::new(&script)
            .arg("--preflight")
            .env("PATH", "/usr/bin:/bin")
            .env("CARGO", &cargo)
            .env("CARGO_TARGET_DIR", outside.join("target"))
            .env("TMPDIR", &outside)
            .env("CI_LOG_ROOT", outside.join("logs"))
            .env(field, value)
            .output()
            .unwrap()
    };
    let good = run("TMPDIR", &outside);
    assert!(good.status.success(), "{}", text(&good));
    assert!(text(&good).contains("preflight=ready"));
    for name in ["CARGO_TARGET_DIR", "TMPDIR", "CI_LOG_ROOT"] {
        let inside = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("refused-{name}"));
        let out = run(name, &inside);
        assert!(!out.status.success(), "accepted {name}");
        assert!(text(&out).contains("outside the checkout"));
        assert!(!inside.exists());
    }
    let linked = outside.join("linked-product");
    std::os::unix::fs::symlink(env!("CARGO_MANIFEST_DIR"), &linked).unwrap();
    let out = run("CI_LOG_ROOT", &linked.join("self-observing-trace"));
    assert!(!out.status.success());
    assert!(!PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("self-observing-trace")
        .exists());
    let bad = run("CARGO", &outside.join("missing-cargo"));
    assert!(!bad.status.success());
    assert!(text(&bad).contains("CARGO must name an executable"));
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn current_action_forms_and_basis_help_remove_guesswork() {
    let f = Fixture::new();
    let (work, detail) = f.worker("current action fixture");
    let command = detail["actionForms"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap();
    let args: Vec<String> = command
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap()
                .replace("<OPERATION>", "current-action-probe")
        })
        .collect();
    let out = Command::new(&args[0]).args(&args[1..]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["allowed"],
        true
    );
    let help = f.call(&["work", "begin", "--help"], b"");
    assert!(help.status.success());
    assert!(text(&help).contains("--basis JSON"));
    let check = f.call(&["work", "check", &work], b"");
    assert!(
        !check.status.success(),
        "check accepted without a worker result"
    );
}

//! Recipient-sufficient detail and asymmetric approved setup facts.
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("agent-protocol");
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--skip-skills", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        fs::write(root.join("AGENTS.md"), "CURRENT_RULE_SENTINEL\n").unwrap();
        Self { root }
    }
    fn call(&self, args: &[&str], body: Option<&[u8]>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("exitbind.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(body) = body {
            child.stdin.take().unwrap().write_all(body).unwrap();
        }
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        let r = self.call(args, None);
        assert!(r.status.success(), "{args:?}: {r:?}");
        serde_json::from_slice(&r.stdout).unwrap()
    }
    fn begin(&self) -> String {
        self.json(&[
            "work",
            "begin",
            "change",
            "--goal",
            "recipient economy",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ])["work"]
            .as_str()
            .unwrap()
            .into()
    }
    fn detail(&self, work: &str) -> Value {
        self.json(&["work", "detail", work, "--json"])
    }
    fn return_result(&self, work: &str, outcome: &str, body: &[u8]) {
        let j = self.detail(work);
        let output = self.call(
            &[
                "work",
                "return",
                work,
                j["recipient"]["assignment"].as_str().unwrap(),
                "--current-binding",
                j["binding"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            Some(body),
        );
        assert!(output.status.success(), "{output:?}");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn one_detail_delivers_only_current_recipient_profile_rules_evidence_and_forms() {
    let f = Fixture::new();
    for role in ["lead", "worker", "reviewer"] {
        fs::write(
            f.root.join(format!("exitbind/agents/{role}.md")),
            format!("{role}_PROFILE_SENTINEL\n"),
        )
        .unwrap();
    }
    let w = f.begin();
    for (role, outcome, body) in [
        ("lead", "scoped", "lead artifact"),
        ("worker", "completed", "worker artifact"),
        ("reviewer", "approved", "review artifact"),
    ] {
        let j = f.detail(&w);
        assert_eq!(j["complete"], true);
        assert_eq!(j["recipientContext"]["agent"], role);
        assert_eq!(
            j["recipientContext"]["profile"]["content"],
            format!("{role}_PROFILE_SENTINEL\n")
        );
        assert_eq!(
            j["recipientContext"]["rules"][0]["content"],
            "CURRENT_RULE_SENTINEL\n"
        );
        let rendered = serde_json::to_string(&j).unwrap();
        for other in ["lead", "worker", "reviewer"] {
            if other != role {
                assert!(!rendered.contains(&format!("{other}_PROFILE_SENTINEL")));
            }
        }
        assert!(j["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["label"] == outcome));
        if role == "worker" {
            assert!(rendered.contains("lead artifact"));
        }
        if role == "reviewer" {
            assert!(rendered.contains("worker artifact"));
        }
        f.return_result(&w, outcome, body.as_bytes());
        if role == "worker" {
            f.json(&["work", "check", &w]);
        }
    }
    assert_eq!(f.detail(&w)["recipient"]["role"], "lead");
}

#[test]
fn stale_recipient_detail_and_corrupt_rules_are_refused() {
    let f = Fixture::new();
    let w = f.begin();
    let j = f.detail(&w);
    f.return_result(&w, "scoped", b"scope");
    let stale = f.call(
        &["work", "expand", &w, j["reference"].as_str().unwrap()],
        None,
    );
    assert!(!stale.status.success());
    fs::write(f.root.join("AGENTS.md"), [0xff]).unwrap();
    let corrupt = f.call(&["work", "detail", &w], None);
    assert!(!corrupt.status.success());
    assert!(String::from_utf8_lossy(&corrupt.stderr).contains("not UTF-8"));
}

#[test]
fn large_recipient_profile_has_explicit_incomplete_expansion() {
    let f = Fixture::new();
    fs::write(f.root.join("exitbind/agents/lead.md"), "p".repeat(17000)).unwrap();
    let w = f.begin();
    let j = f.detail(&w);
    assert_eq!(j["complete"], false);
    assert!(j["recipientContext"]["profile"]["content"].is_null());
    let route = j["recipientContext"]["profile"]["access"]["command"]
        .as_array()
        .unwrap();
    let r = Command::new(route[0].as_str().unwrap())
        .args(route[1..].iter().map(|v| v.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(r.status.success());
    assert_eq!(r.stdout.len(), 17000);
}

fn setup(root: &Path, apply: bool, extra: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command.args(["setup", "--root"]).arg(root).args([
        "--json",
        "--skip-skills",
        "--hosts",
        "codex",
        "--lead-observe",
        "README.md,src",
        "--lead-write",
        "none",
        "--lead-commands",
        "none",
        "--worker-observe",
        "README.md,src",
        "--worker-write",
        "src",
        "--worker-commands",
        "cargo test --locked",
        "--reviewer-observe",
        "README.md,src",
        "--reviewer-write",
        "none",
        "--reviewer-commands",
        "none",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--goal",
        "asymmetric setup",
    ]);
    if apply {
        command.arg("--apply");
    }
    command.args(extra);
    command.output().unwrap()
}

#[test]
fn approved_roles_apply_once_preserve_asymmetry_and_return_validated_begin() {
    let root = support::temp("approved-roles");
    let preview = setup(&root, false, &[]);
    assert!(preview.status.success(), "{preview:?}");
    assert!(!root.join("exitbind.json").exists());
    let first = setup(&root, true, &[]);
    assert!(first.status.success(), "{first:?}");
    let report: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["validation"]["valid"], true);
    assert_eq!(report["applyRequired"], false);
    assert_eq!(report["missingOwnerDecisions"], json!([]));
    let config = fs::read(root.join("exitbind.json")).unwrap();
    let modified = fs::metadata(root.join("exitbind.json"))
        .unwrap()
        .modified()
        .unwrap();
    let parsed: Value = serde_json::from_slice(&config).unwrap();
    for role in ["lead", "reviewer"] {
        assert_eq!(parsed["agents"][role]["write"], json!([]));
        assert_eq!(parsed["agents"][role]["commands"], json!([]));
    }
    assert_eq!(parsed["agents"]["worker"]["write"], json!(["src"]));
    let checked = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["check", "--json", "--config"])
        .arg(root.join("exitbind.json"))
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert_eq!(
        report["validation"],
        serde_json::from_slice::<Value>(&checked.stdout).unwrap()
    );
    let actions = report["next"]["argv"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0][1], "work");
    assert_eq!(actions[0][2], "begin");
    let repeat = setup(&root, true, &[]);
    assert!(repeat.status.success(), "{repeat:?}");
    let repeated: Value = serde_json::from_slice(&repeat.stdout).unwrap();
    assert_eq!(repeated["status"], "unchanged");
    assert_eq!(repeated["configuration"], report["configuration"]);
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), config);
    assert_eq!(
        fs::metadata(root.join("exitbind.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    let begin = actions[0].as_array().unwrap();
    let run = Command::new(begin[0].as_str().unwrap())
        .current_dir(std::env::temp_dir())
        .args(begin[1..].iter().map(|v| v.as_str().unwrap()))
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mixed_role_authority_representation_is_refused_before_any_write() {
    let root = support::temp("mixed-roles");
    let output = setup(&root, true, &["--scope", "worker"]);
    assert!(!output.status.success());
    assert!(!root.join("exitbind.json").exists());
    assert!(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .contains("cannot be combined"));
    fs::remove_dir_all(root).unwrap();
}

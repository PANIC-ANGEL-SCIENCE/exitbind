//! Real edits and observed checks; role verdicts here are deterministic fixtures.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
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
impl Fixture {
    fn new() -> Self {
        let root = support::temp("repeated-repair");
        support::git_topology::repository(&root);
        let p = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(p.status.success(), "{p:?}");
        let mut f = Self {
            root,
            work: String::new(),
        };
        let v = f.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Correct counter.txt only",
                "--check-command",
                "test \"$(cat counter.txt)\" = correct",
                "--review-policy",
                "required",
            ],
            b"",
        );
        f.work = v["work"].as_str().unwrap().into();
        f.choose(
            "scoped",
            b"Only counter.txt; preserve check, review and governor limits.",
        );
        f
    }
    fn call(&self, args: &[&str], body: &[u8]) -> Output {
        let mut p = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        p.stdin.take().unwrap().write_all(body).unwrap();
        p.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], body: &[u8]) -> Value {
        let p = self.call(args, body);
        assert!(p.status.success(), "{args:?}: {p:?}");
        value(&p)
    }
    fn detail(&self) -> Value {
        self.ok(&["work", "detail", &self.work, "--json"], b"")
    }
    fn execute(&self, form: &Value, body: &[u8]) -> Output {
        let args: Vec<String> = form["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| match x.as_str().unwrap() {
                "<REASON>" => {
                    "Only repair the exact counter value after its current failed check".into()
                }
                "<HYPOTHESIS>" => {
                    "Two edits failed the same exact-value check; use the literal required value"
                        .into()
                }
                "<SCOPE_DECISION>" => {
                    "counter.txt only, same frozen configuration and authority".into()
                }
                "<EVIDENCE_REQUEST>" => {
                    "Fresh observed exact-value check and independent review".into()
                }
                "<OPERATION>" => "correct_counter".into(),
                "<ARTIFACT_ROOT>" => "product".into(),
                "<ARTIFACT_PATH>" => "repair-evidence.txt".into(),
                x => x.into(),
            })
            .collect();
        let mut p = support::git_topology::command(&args[0])
            .args(&args[1..])
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        p.stdin.take().unwrap().write_all(body).unwrap();
        p.wait_with_output().unwrap()
    }
    fn chosen(d: &Value, label: &str) -> Value {
        d["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["label"] == label)
            .unwrap_or_else(|| panic!("missing {label}: {}", d["actionForms"]))
            .clone()
    }
    fn choose(&self, label: &str, body: &[u8]) -> Value {
        let p = self.execute(&Self::chosen(&self.detail(), label), body);
        assert!(p.status.success(), "{label}: {p:?}");
        value(&p)
    }
    fn edit(&self, text: &str) {
        let p = self.execute(&self.detail()["actionForms"]["beforeEditing"], b"");
        assert!(p.status.success(), "{p:?}");
        assert_eq!(value(&p)["allowed"], true);
        fs::write(self.root.join("counter.txt"), text).unwrap();
    }
    fn check(&self, pass: bool) {
        let d = self.detail();
        let p = self.execute(&d["actionForms"]["mechanicalAction"], b"");
        assert_eq!(p.status.success(), pass, "{p:?}");
        assert_eq!(value(&p)["effect"], "recorded");
    }
    fn failed_attempt(&self, n: u64) {
        self.edit(&format!("incorrect {n}"));
        self.choose("completed", format!("attempt {n}").as_bytes());
        self.check(false);
        self.choose(
            "rework",
            format!("Only counter.txt; current check failed on attempt {n}").as_bytes(),
        );
    }
    fn held_after_two(&self) -> Value {
        self.failed_attempt(1);
        self.failed_attempt(2);
        self.edit("correct");
        let assignment = self.detail()["actionForms"]["assignment"]
            .as_str()
            .unwrap()
            .to_owned();
        let held = self.ok(
            &[
                "work",
                "return",
                &self.work,
                &assignment,
                "--outcome",
                "completed",
            ],
            b"retained exact third result",
        );
        assert_eq!(held["effect"], "held");
        held
    }
    fn ledger(&self) -> PathBuf {
        self.root
            .join(format!(".exitbind/runs/work-{}.jsonl", &self.work[4..]))
    }
    fn events(&self) -> Vec<Value> {
        fs::read_to_string(self.ledger())
            .unwrap()
            .lines()
            .map(|x| serde_json::from_str(x).unwrap())
            .collect()
    }
    fn finish(&self) {
        self.check(true);
        self.choose(
            "approved",
            b"Fixture reviewer: exact counter and fresh check agree.",
        );
        self.choose(
            "accepted",
            b"Fixture Lead: bounded result accepted after applicable review.",
        );
        let before = fs::read(self.ledger()).unwrap();
        let receipt = self.ok(&["work", "closeout", &self.work, "--export"], b"");
        assert_eq!(
            receipt["receipt"]["verification"]["valid"], true,
            "{receipt}"
        );
        self.ok(&["work", "closeout", &self.work, "--export"], b"");
        assert_eq!(fs::read(self.ledger()).unwrap(), before);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained fixture {}", self.root.display());
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn value(p: &Output) -> Value {
    serde_json::from_slice(&p.stdout).unwrap_or_else(|_| panic!("{p:?}"))
}

#[test]
fn single_check_repair_keeps_fresh_review_and_closeout() {
    let f = Fixture::new();
    f.failed_attempt(1);
    f.edit("correct");
    f.choose("completed", b"single repair");
    f.finish();
}
#[test]
fn repeated_check_repair_uses_current_emitted_replan_and_retained_result_once() {
    let f = Fixture::new();
    let held = f.held_after_two();
    let before = f.events();
    assert_eq!(before.len(), 13);
    let d = f.detail();
    assert_eq!(d["actionForms"]["recovery"]["state"], "replan_required");
    assert!(d["actionForms"].get("beforeEditing").is_none());
    let form = Fixture::chosen(&d, "replan");
    let p = f.execute(&form, b"");
    assert!(p.status.success(), "{p:?}");
    let after = f.events();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(&after[..before.len()], &before);
    let lost = f.execute(&form, b"");
    assert!(!lost.status.success());
    assert_eq!(f.events().len(), after.len());
    let d = f.detail();
    let recovery = &d["actionForms"]["recovery"]["heldResults"][0];
    assert_eq!(recovery["reference"], held["held"]["reference"]);
    assert_eq!(recovery["sha256"], held["held"]["sha256"]);
    let p = f.execute(recovery, b"");
    assert!(p.status.success(), "{p:?}");
    assert_eq!(value(&p)["effect"], "recorded");
    let bytes = fs::read(f.ledger()).unwrap();
    let repeat = f.execute(recovery, b"");
    assert!(!repeat.status.success() || value(&repeat)["effect"] == "replayed");
    assert_eq!(fs::read(f.ledger()).unwrap(), bytes);
    f.finish();
    let e = f.events();
    let grant_count = e
        .iter()
        .filter(|e| e["action"] == "govern" && e["governorEvent"]["action"] == "mutation")
        .count();
    assert_eq!(grant_count, 3);
    assert_eq!(
        e.iter()
            .filter(|e| e["governorEvent"]["action"] == "replan")
            .count(),
        1
    );
}
#[test]
fn changed_configuration_and_wrong_assignment_refuse_replan_without_events() {
    let f = Fixture::new();
    f.held_after_two();
    let before = fs::read(f.ledger()).unwrap();
    let d = f.detail();
    let form = Fixture::chosen(&d, "replan");
    let mut wrong = form.clone();
    wrong["command"]["argv"][4] = json!("sma_".to_owned() + &"0".repeat(64));
    assert!(!f.execute(&wrong, b"").status.success());
    assert_eq!(fs::read(f.ledger()).unwrap(), before);
    let config = f.root.join("exitbind.json");
    let original = fs::read(&config).unwrap();
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["project"]["name"] = json!("changed task configuration");
    fs::write(&config, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(!f.execute(&form, b"").status.success());
    assert_eq!(fs::read(f.ledger()).unwrap(), before);
    fs::write(config, original).unwrap();
    assert!(f.execute(&form, b"").status.success());
}
#[test]
fn repeated_failure_after_replan_requires_new_evidence_and_never_renews_grants() {
    let f = Fixture::new();
    f.held_after_two();
    f.choose("replan", b"");
    fs::write(f.root.join("counter.txt"), "still incorrect").unwrap();
    f.choose("completed", b"third permitted attempt still fails");
    f.check(false);
    f.choose("rework", b"Need new evidence before another mutation.");
    // The existing post-replan bound allows one additional no-information
    // mutation. It is finite; the acknowledgement never spends a second grant.
    f.edit("fourth attempt still incorrect");
    let d = f.detail();
    assert_eq!(d["actionForms"]["recovery"]["state"], "evidence_required");
    assert!(d["actionForms"].get("beforeEditing").is_none());
    let before = fs::read(f.ledger()).unwrap();
    let assignment = d["actionForms"]["assignment"].as_str().unwrap();
    let p = f.call(
        &[
            "work",
            "permit",
            &f.work,
            assignment,
            "--operation",
            "fourth_without_evidence",
        ],
        b"",
    );
    assert!(!p.status.success());
    let after = f.events();
    assert_eq!(after.len(), before.split(|b| *b == b'\n').count());
    assert_eq!(after.last().unwrap()["governorEvent"]["action"], "blocked");
    let blocked_bytes = fs::read(f.ledger()).unwrap();
    let blocked = f.detail();
    assert_eq!(blocked["effectiveAction"]["exists"], false);
    assert_eq!(blocked["actionForms"]["recovery"]["state"], "blocked");
    let p = f.call(
        &[
            "work",
            "replan",
            &f.work,
            assignment,
            "--hypothesis",
            "another guess",
        ],
        b"",
    );
    assert!(!p.status.success());
    assert_eq!(fs::read(f.ledger()).unwrap(), blocked_bytes);
}

#[test]
fn emitted_evidence_form_binds_exact_new_evidence_without_another_grant() {
    let f = Fixture::new();
    f.held_after_two();
    f.choose("replan", b"");
    fs::write(f.root.join("counter.txt"), "still incorrect").unwrap();
    f.choose("completed", b"third permitted attempt still fails");
    f.check(false);
    f.choose("rework", b"Need exact new evidence.");
    f.edit("fourth attempt");
    fs::write(
        f.root.join("repair-evidence.txt"),
        "literal required value: correct",
    )
    .unwrap();
    let before = f.events();
    let result = f.choose("evidence", b"");
    assert_eq!(result["event"]["governorEvent"]["action"], "evidence");
    let after = f.events();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(&after[..before.len()], &before);
    assert_eq!(
        result["governor"]["spent"],
        before.last().unwrap()["governorEvent"]["checkpoint"]
    );
    let bytes = fs::read(f.ledger()).unwrap();
    f.choose(
        "completed",
        b"exact evidence supplied; no further mutation requested",
    );
    assert!(fs::read(f.ledger()).unwrap().starts_with(&bytes));
}

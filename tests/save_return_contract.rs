//! Focused R18 checks for the shared result contract and safe refusal paths.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = support::temp(label);
        let output = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        Self { root }
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

    fn json(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn begin(&self) -> (String, Value) {
        let value = self.json(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "exercise result contract",
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--review-policy",
                "required",
            ],
            b"",
        );
        (
            value["work"].as_str().unwrap().to_owned(),
            value["next"].clone(),
        )
    }

    fn return_result(&self, work: &str, action: &Value, outcome: &str, body: &[u8]) -> Output {
        self.call(
            &[
                "work",
                "return",
                work,
                action["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            body,
        )
    }

    fn next(&self, work: &str) -> Value {
        self.json(&["work", "next", work, "--full"], b"")["next"].clone()
    }

    fn reviewer(&self, work: &str, first: &Value) -> Value {
        let scoped = self.return_result(work, first, "scoped", b"scope");
        assert!(scoped.status.success(), "{}", text(&scoped));
        let worker = self.next(work);
        let completed = self.return_result(work, &worker, "completed", b"implementation");
        assert!(completed.status.success(), "{}", text(&completed));
        self.json(&["work", "check", work], b"")["next"].clone()
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

fn event_count(fixture: &Fixture, work: &str) -> usize {
    fs::read_to_string(
        fixture
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
    )
    .unwrap()
    .lines()
    .count()
}

#[test]
fn reviewer_rejected_lists_contract_outcomes_without_recording_result() {
    let fixture = Fixture::new("save-return-reviewer-outcome");
    let (work, first) = fixture.begin();
    let reviewer = fixture.reviewer(&work, &first);
    let before = event_count(&fixture, &work);

    let refused = fixture.return_result(&work, &reviewer, "rejected", b"wrong role");
    assert!(
        !refused.status.success(),
        "unexpected success: {}",
        text(&refused)
    );
    let rendered = text(&refused);
    assert!(rendered.contains("permitted outcomes: approved, rework, blocked, unavailable"));
    assert_eq!(event_count(&fixture, &work), before);
}

#[test]
fn repair_missing_terms_are_reported_together_with_supported_form() {
    let fixture = Fixture::new("save-return-repair-terms");
    let (work, first) = fixture.begin();
    let reviewer = fixture.reviewer(&work, &first);
    let rework = fixture.return_result(&work, &reviewer, "rework", b"review finding");
    assert!(rework.status.success(), "{}", text(&rework));
    let lead = fixture.next(&work);
    let before = event_count(&fixture, &work);

    let refused = fixture.call(
        &[
            "work",
            "disposition",
            &work,
            lead["assignment"].as_str().unwrap(),
            "--decision",
            "repair",
            "--reason",
            "repair the finding",
        ],
        b"",
    );
    assert!(
        !refused.status.success(),
        "unexpected success: {}",
        text(&refused)
    );
    let rendered = text(&refused);
    assert!(rendered.contains("--repair-boundary TEXT"));
    assert!(rendered.contains("--regression TEXT"));
    assert!(rendered.contains("supported form: --repair-boundary TEXT --regression TEXT"));
    assert_eq!(event_count(&fixture, &work), before);
}

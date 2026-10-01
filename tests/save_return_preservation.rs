//! R18 preserved state distinctions on a new, explicitly marked Work.
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
        assert!(output.status.success(), "{output:?}");
        Self { root }
    }

    fn call(&self, args: &[&str], body: &[u8], check_result: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .env("R18_CHECK_RESULT", check_result)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }

    fn ok(&self, args: &[&str], body: &[u8], check_result: &str) -> Value {
        let output = self.call(args, body, check_result);
        assert!(output.status.success(), "{args:?}: {}", display(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn begin(&self) -> String {
        let started = self.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "repair one header parser",
                "--check-command",
                "sh -c 'test \"$R18_CHECK_RESULT\" = pass'",
                "--review-policy",
                "required",
            ],
            b"",
            "pass",
        );
        let work = started["work"].as_str().unwrap().to_owned();
        let scope = self.ok(
            &[
                "work",
                "act",
                &work,
                "--outcome",
                "scoped",
                "--reason",
                "one bounded repair",
            ],
            b"",
            "pass",
        );
        assert_eq!(scope["next"]["role"], "worker");
        work
    }

    fn next(&self, work: &str) -> Value {
        self.ok(&["work", "next", work, "--full"], b"", "pass")["next"].clone()
    }

    fn give(&self, work: &str, role: &str, outcome: &str, body: &str) -> Value {
        let action = self.next(work);
        assert_eq!(action["role"], role, "{action}");
        self.ok(
            &[
                "work",
                "return",
                work,
                action["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            body.as_bytes(),
            "pass",
        )
    }

    fn check(&self, work: &str, result: &str) -> Value {
        let output = self.call(&["work", "check", work], b"", result);
        let value: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{}", display(&output)));
        assert_eq!(
            value["result"]["code"],
            if result == "pass" { 0 } else { 1 }
        );
        assert_eq!(
            output.status.success(),
            result == "pass",
            "{}",
            display(&output)
        );
        value
    }

    fn repair(&self, work: &str) -> Value {
        let action = self.next(work);
        assert_eq!(action["role"], "lead", "{action}");
        self.ok(
            &[
                "work",
                "disposition",
                work,
                action["assignment"].as_str().unwrap(),
                "--decision",
                "repair",
                "--reason",
                "Observed parser defect",
                "--repair-boundary",
                "Header parsing only",
                "--regression",
                "Run the frozen parser check",
            ],
            b"",
            "pass",
        )
    }

    fn events(&self, work: &str) -> Vec<Value> {
        fs::read_to_string(
            self.root
                .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
        )
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn display(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn adverse_review_repair_check_rereview_and_lead_acceptance_stay_distinct() {
    let fixture = Fixture::new("save-return-normal-repair");
    let work = fixture.begin();
    assert_eq!(fixture.events(&work)[0]["recoveryProtocol"], 1);
    fixture.give(&work, "worker", "completed", "header parser draft");
    let first_check = fixture.check(&work, "pass");
    assert_eq!(first_check["next"]["role"], "reviewer");
    fixture.give(&work, "reviewer", "rework", "CRLF case is missing");
    assert_eq!(fixture.next(&work)["role"], "lead");
    let repaired = fixture.repair(&work);
    assert_eq!(repaired["next"]["role"], "worker");
    let repaired_packet = fixture.next(&work)["packet"].clone();
    assert_eq!(
        repaired_packet["leadRepair"]["repairBoundary"],
        "Header parsing only"
    );
    assert_eq!(
        repaired_packet["leadRepair"]["decisiveRegression"],
        "Run the frozen parser check"
    );
    assert_eq!(
        repaired_packet["context"]["goal"],
        "repair one header parser"
    );
    fixture.give(&work, "worker", "completed", "CRLF case implemented");
    let checked = fixture.check(&work, "pass");
    assert_eq!(checked["next"]["role"], "reviewer");
    fixture.give(&work, "reviewer", "approved", "reviewed current result");
    let before_accept = fixture.events(&work);
    assert_eq!(
        before_accept
            .iter()
            .filter(|event| event["role"] == "lead" && event["outcome"] == "accepted")
            .count(),
        0
    );
    let accepted = fixture.ok(
        &[
            "work",
            "act",
            &work,
            "--outcome",
            "accepted",
            "--reason",
            "Current check and review passed",
        ],
        b"",
        "pass",
    );
    assert_eq!(accepted["next"]["action"], "done");
    let events = fixture.events(&work);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "check")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["role"] == "reviewer" && event["outcome"] == "rework")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["role"] == "reviewer" && event["outcome"] == "approved")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["role"] == "lead" && event["outcome"] == "accepted")
            .count(),
        1
    );
}

#[test]
fn observed_status_novelty_is_bounded_and_failed_check_cannot_qualify_acceptance() {
    let fixture = Fixture::new("save-return-observation");
    let work = fixture.begin();
    fixture.give(&work, "worker", "completed", "first parser result");
    let failed = fixture.check(&work, "fail");
    assert_eq!(failed["next"]["role"], "lead");
    let observations = fixture.next(&work)["packet"]["context"]["loop"]["observations"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(observations, 1);
    let repeat = fixture.check(&work, "fail");
    assert_eq!(repeat["next"]["role"], "lead");
    let after = fixture.next(&work);
    assert_eq!(
        after["packet"]["context"]["loop"]["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let refused = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--outcome",
            "accepted",
            "--reason",
            "premature",
        ],
        b"",
        "fail",
    );
    let refusal: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refusal["outcome"], "refused", "{}", display(&refused));
    assert_eq!(
        fixture
            .events(&work)
            .iter()
            .filter(|event| event["role"] == "lead" && event["outcome"] == "accepted")
            .count(),
        0
    );
}

#[test]
fn current_lead_repair_supplies_the_required_replan_once() {
    let fixture = Fixture::new("save-return-lead-replan");
    let work = fixture.begin();
    for attempt in 1..=3 {
        fixture.give(
            &work,
            "worker",
            "completed",
            &format!("parser attempt {attempt}"),
        );
        fixture.check(&work, "pass");
        fixture.give(
            &work,
            "reviewer",
            "rework",
            &format!("review finding {attempt}"),
        );
        let before = fixture.next(&work);
        if attempt == 3 {
            assert_eq!(
                before["packet"]["context"]["loop"]["state"],
                "replan_required"
            );
        }
        fixture.repair(&work);
    }
    let worker = fixture.next(&work);
    assert_eq!(worker["role"], "worker");
    assert_eq!(worker["packet"]["context"]["loop"]["state"], "ready");
    let allowed = fixture.ok(
        &[
            "work",
            "permit",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--operation",
            "fourth_parser_edit",
        ],
        b"",
        "pass",
    );
    assert_eq!(allowed["allowed"], true);
    let loop_state = &fixture.next(&work)["packet"]["context"]["loop"];
    assert_eq!(loop_state["spent"], 4);
    assert_eq!(loop_state["replanCount"], 1);
}

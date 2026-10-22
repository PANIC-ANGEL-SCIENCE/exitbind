//! R55 F7 (FR54-11, the general case): a passing check on a new commit is new
//! information, so the worker's next permit after a Lead rework keeps the
//! return offered instead of exhausting the no-information streak.
use super::*;

impl Fixture {
    fn labels(&self) -> Vec<String> {
        self.detail()["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["label"].as_str().map(str::to_owned))
            .collect()
    }
    fn assert_return_offered(&self) {
        let d = self.detail();
        assert_eq!(
            d["actionForms"]["recovery"]["state"],
            Value::Null,
            "{}",
            d["actionForms"]
        );
        let labels = self.labels();
        assert!(labels.iter().any(|l| l == "completed"), "{labels:?}");
        assert!(!labels.iter().any(|l| l == "replan"), "{labels:?}");
        assert!(!labels.iter().any(|l| l == "evidence"), "{labels:?}");
    }
    /// One checked worker result: permit, edit, return, frozen check.
    fn checked_result(&self, text: &str, pass: bool) {
        self.edit(text);
        self.assert_return_offered();
        self.choose("completed", format!("result {text}").as_bytes());
        self.check(pass);
    }
    /// The reviewer approves and the Lead still asks for more (int attempt 3).
    fn approved_then_lead_rework(&self) {
        self.choose("approved", b"Fixture reviewer: the result meets the check.");
        self.choose("rework", b"Fixture Lead: one more bounded change.");
    }
    fn worker_replans(&self) -> usize {
        self.events()
            .iter()
            .filter(|e| {
                e["governorEvent"]["action"] == "replan"
                    && e["operation"] != "authorized_repair_recovery_v1"
            })
            .count()
    }
}

/// Integration attempt 3: pass → Lead rework → pass on a new commit → Lead
/// rework. The third permit keeps the return offered; no re-plan is recorded.
#[test]
fn a_pass_on_a_new_commit_after_a_lead_rework_keeps_the_return_offered() {
    let f = Fixture::new();
    f.checked_result("correct", true);
    f.approved_then_lead_rework();
    f.checked_result("correct", true);
    f.approved_then_lead_rework();
    f.checked_result("correct", true);
    assert_eq!(f.worker_replans(), 0);
    f.choose("approved", b"Fixture reviewer: fresh check agrees.");
    f.choose("accepted", b"Fixture Lead: accepted.");
}

/// Integration attempt 4: after the attempt 3 shape, a reviewer rework and one
/// more permit keep the return offered; no evidence demand appears.
#[test]
fn a_fourth_result_after_passing_checks_needs_no_new_evidence() {
    let f = Fixture::new();
    for _ in 0..3 {
        f.checked_result("correct", true);
        f.approved_then_lead_rework();
    }
    f.checked_result("correct", true);
    assert_eq!(f.worker_replans(), 0);
    let evidence = f
        .events()
        .iter()
        .filter(|e| e["governorEvent"]["action"] == "evidence")
        .count();
    assert_eq!(evidence, 0);
}

/// Fail → fix → pass on a new commit, after an earlier pass: the repeated
/// passing status on the new subject is new information.
#[test]
fn a_fix_after_a_failure_passes_on_a_new_commit_and_keeps_the_return_offered() {
    let f = Fixture::new();
    f.checked_result("correct", true);
    f.approved_then_lead_rework();
    f.checked_result("incorrect", false);
    f.choose("rework", b"Fixture reviewer: the check failed.");
    f.checked_result("correct", true);
    f.approved_then_lead_rework();
    f.checked_result("correct", true);
    assert_eq!(f.worker_replans(), 0);
}

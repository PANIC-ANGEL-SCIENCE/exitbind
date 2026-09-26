//! A reviewer finding is evidence, not a requirement.  In marked governed work
//! a reviewer `rework` waits for the Lead, who repairs, defers, rejects, or
//! supersedes the basis; only repair and supersede start another attempt.
#![cfg(unix)]

mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

const SUGGESTION: &str = "Suggested fix: also add a retry cache for the parser";

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

    fn ok(&self, args: &[&str], input: &[u8]) -> Value {
        let output = self.call(args, input);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn begin(&self, extra: &[&str]) -> (String, Value) {
        let mut args = vec![
            "work",
            "begin",
            "change",
            "--goal",
            "parse the config header",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
        ];
        args.extend_from_slice(extra);
        let value = self.ok(&args, b"");
        (
            value["work"].as_str().unwrap().to_owned(),
            value["next"].clone(),
        )
    }

    fn give(&self, work: &str, action: &Value, outcome: &str, body: &str) -> Output {
        self.call(
            &[
                "work",
                "return",
                work,
                action["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            body.as_bytes(),
        )
    }

    fn give_ok(&self, work: &str, action: &Value, outcome: &str, body: &str) -> Value {
        let output = self.give(work, action, outcome, body);
        assert!(output.status.success(), "{outcome}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn next(&self, work: &str) -> Value {
        self.ok(&["work", "next", work, "--full"], b"")["next"].clone()
    }

    fn dispose(&self, work: &str, lead: &Value, decision: &str, extra: &[&str]) -> Output {
        let mut args = vec![
            "work",
            "disposition",
            work,
            lead["assignment"].as_str().unwrap(),
            "--decision",
            decision,
            "--reason",
            "Lead rationale for this finding",
        ];
        args.extend_from_slice(extra);
        self.call(&args, b"")
    }

    /// Scope, work, and check once, then return the reviewer assignment.
    fn to_review(&self, work: &str, first: Value) -> Value {
        let worker = self.give_ok(work, &first, "scoped", "scope")["next"].clone();
        self.give_ok(work, &worker, "completed", "implementation");
        let checked = self.ok(&["work", "check", work], b"");
        assert_eq!(checked["next"]["role"], "reviewer", "{checked}");
        checked["next"].clone()
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

fn refused(output: &Output, needle: &str) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        text(output)
    );
    assert!(text(output).contains(needle), "{needle}: {}", text(output));
}

fn worker_submissions(fixture: &Fixture, work: &str) -> usize {
    fixture
        .events(work)
        .iter()
        .filter(|event| event["action"] == "submit" && event["role"] == "worker")
        .count()
}

/// Reviewer `rework` pends a Lead disposition and offers no way around it.
fn pending_lead(fixture: &Fixture, work: &str) -> Value {
    let (work_owned, first) = (work.to_owned(), fixture.next(work));
    let reviewer = fixture.to_review(&work_owned, first);
    let after = fixture.give_ok(
        work,
        &reviewer,
        "rework",
        &format!("Finding: the header parser ignores CRLF. {SUGGESTION}"),
    );
    assert_eq!(
        after["next"]["role"], "lead",
        "no worker before the Lead: {after}"
    );
    let lead = fixture.next(work);
    assert_eq!(lead["action"], "lead_decision");
    assert_eq!(
        lead["outcomes"],
        json!(["disposition", "blocked", "rejected"])
    );
    let pending = &lead["packet"]["pendingDisposition"];
    assert_eq!(pending["kind"], "review_rework");
    assert_eq!(
        pending["decisions"],
        json!(["repair", "defer", "reject", "supersede"])
    );
    let compact = fixture.ok(&["work", "next", work], b"");
    assert_eq!(
        compact["presentation"]["state"]["review"],
        "finding_pending"
    );
    assert!(compact["humanHelp"]["nextAction"]["summary"]
        .as_str()
        .unwrap()
        .contains("work disposition"));
    assert_eq!(
        compact["obligations"]["remaining"],
        json!([{"obligation": "lead_disposition"}])
    );
    refused(
        &fixture.give(work, &lead, "accepted", "accept"),
        "not allowed",
    );
    refused(
        &fixture.give(work, &lead, "rework", "rework"),
        "not allowed",
    );
    lead
}

fn resolved_then_accepted(decision: &str) {
    let fixture = Fixture::new(&format!("disposition-{decision}"));
    let (work, _) = fixture.begin(&["--review-policy", "required"]);
    let lead = pending_lead(&fixture, &work);
    let basis_before = lead["packet"]["basisSha256"].clone();
    let workers_before = worker_submissions(&fixture, &work);
    refused(
        &fixture.dispose(&work, &lead, decision, &["--repair-boundary", "x"]),
        "records no repair terms",
    );

    let decided = fixture.dispose(&work, &lead, decision, &[]);
    assert!(decided.status.success(), "{}", text(&decided));
    let lead = fixture.next(&work);
    assert_eq!(lead["role"], "lead", "{decision} starts no worker");
    assert_eq!(worker_submissions(&fixture, &work), workers_before);
    assert_eq!(lead["packet"]["basisSha256"], basis_before);
    assert!(lead["packet"].get("pendingDisposition").is_none());
    assert!(lead["packet"].get("leadRepair").is_none());
    let resolution = &lead["packet"]["reviewResolution"];
    assert_eq!(resolution["state"], "resolved_by_lead_disposition");
    assert_eq!(resolution["decision"], decision);
    let review = &lead["progress"]["components"]["review"];
    assert_eq!(
        review["completed"], 0,
        "a resolved finding is not an approval"
    );
    assert_eq!(review["resolution"]["decision"], decision);
    let compact = fixture.ok(&["work", "next", &work], b"");
    assert_eq!(
        compact["presentation"]["state"]["review"],
        "resolved_by_lead_disposition"
    );
    assert!(compact["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap()
        .contains("not approved"));
    assert!(!serde_json::to_string(&lead["packet"])
        .unwrap()
        .contains(SUGGESTION));

    let accepted = fixture.give_ok(&work, &lead, "accepted", "accepted within scope");
    assert_eq!(accepted["next"]["action"], "done", "{accepted}");
    let events = fixture.events(&work);
    let disposition = events
        .iter()
        .find(|event| event["outcome"] == "disposition")
        .unwrap();
    assert_eq!(disposition["disposition"]["decision"], decision);
    assert!(disposition["disposition"].get("repairBoundary").is_none());
    assert!(events
        .iter()
        .any(|event| event["role"] == "reviewer" && event["outcome"] == "rework"));
    assert!(!events
        .iter()
        .any(|event| event["role"] == "reviewer" && event["outcome"] == "approved"));
    reports_resolution(&fixture, &work, decision, disposition);
}

/// Run status and the Exit Path receipt name the Lead resolution, never an
/// omission or approval, and the receipt verifies against the ledger.
fn reports_resolution(fixture: &Fixture, work: &str, decision: &str, disposition: &Value) {
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let expected = |review: &Value| {
        assert_eq!(review["status"], "resolved_by_lead_disposition", "{review}");
        assert_eq!(review["decision"], decision, "{review}");
        assert_eq!(
            review["dispositionSha256"], disposition["disposition"]["sha256"],
            "{review}"
        );
        assert!(review["dispositionSha256"].is_string(), "{review}");
        assert!(review.get("source").is_none(), "{review}");
    };
    let status = fixture.ok(&["run", "status", &ledger, "--json"], b"");
    assert_eq!(status["status"], "accepted");
    expected(&status["review"]);
    let receipt_path = ".exitbind/receipts/resolved.json";
    let receipt = fixture.ok(
        &["receipt", &ledger, "--output", receipt_path, "--json"],
        b"",
    );
    expected(&receipt["review"]);
    let verified = fixture.ok(&["verify", receipt_path], b"");
    assert_eq!(verified["valid"], true, "{verified}");

    // A receipt that claims a different Lead decision no longer verifies.
    let path = fixture.root.join(receipt_path);
    let mut tampered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    tampered["review"]["decision"] = json!(if decision == "defer" {
        "reject"
    } else {
        "defer"
    });
    fs::write(&path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
    let refused = fixture.call(&["verify", receipt_path], b"");
    assert!(!refused.status.success(), "tampered receipt verified");
    let refused: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refused["valid"], false, "{refused}");
}

#[test]
fn case_a_valid_out_of_scope_finding_is_deferred_without_a_worker() {
    resolved_then_accepted("defer");
}

#[test]
fn case_b_rejected_finding_keeps_evidence_and_starts_nothing() {
    resolved_then_accepted("reject");
}

#[test]
fn case_c_repair_carries_the_lead_boundary_and_needs_fresh_review() {
    let fixture = Fixture::new("disposition-repair");
    let (work, _) = fixture.begin(&["--review-policy", "required"]);
    let lead = pending_lead(&fixture, &work);
    refused(
        &fixture.dispose(&work, &lead, "repair", &[]),
        "--repair-boundary",
    );
    let boundary = "Only the header parser's line-ending handling";
    let regression = "a CRLF header test fails before the repair and passes after";
    let decided = fixture.dispose(
        &work,
        &lead,
        "repair",
        &["--repair-boundary", boundary, "--regression", regression],
    );
    assert!(decided.status.success(), "{}", text(&decided));

    let worker = fixture.next(&work);
    assert_eq!(worker["role"], "worker");
    assert_eq!(worker["packet"]["attempt"], 2);
    let repair = &worker["packet"]["leadRepair"];
    assert_eq!(repair["authority"], "lead");
    assert_eq!(repair["decision"], "repair");
    assert_eq!(repair["repairBoundary"], boundary);
    assert_eq!(repair["decisiveRegression"], regression);
    assert!(!serde_json::to_string(&worker["packet"])
        .unwrap()
        .contains(SUGGESTION));

    // The repaired result needs its own check and review.
    fixture.give_ok(&work, &worker, "completed", "repaired");
    let checked = fixture.ok(&["work", "check", &work], b"");
    let reviewer = checked["next"].clone();
    assert_eq!(reviewer["role"], "reviewer");
    assert_eq!(
        fixture.next(&work)["packet"]["leadRepair"]["repairBoundary"],
        boundary
    );

    let lead = fixture.give_ok(&work, &reviewer, "approved", "approved")["next"].clone();
    assert!(fixture.next(&work)["packet"]
        .get("reviewResolution")
        .is_none());
    let done = fixture.give_ok(&work, &lead, "accepted", "accepted");
    assert_eq!(done["next"]["action"], "done");
}

#[test]
fn a_repeated_finding_surfaces_prior_cycles_to_the_lead() {
    let fixture = Fixture::new("disposition-repeated");
    let (work, _) = fixture.begin(&["--review-policy", "required"]);
    let lead = pending_lead(&fixture, &work);
    assert!(lead["packet"]["pendingDisposition"].get("advice").is_none());
    let terms = [
        "--repair-boundary",
        "the parser only",
        "--regression",
        "CRLF test",
    ];
    assert!(fixture
        .dispose(&work, &lead, "repair", &terms)
        .status
        .success());
    let worker = fixture.next(&work);
    fixture.give_ok(&work, &worker, "completed", "repaired");
    let reviewer = fixture.ok(&["work", "check", &work], b"")["next"].clone();
    fixture.give_ok(&work, &reviewer, "rework", "Finding: still wrong");
    let lead = fixture.next(&work);
    let pending = &lead["packet"]["pendingDisposition"];
    assert_eq!(pending["priorCycles"].as_array().unwrap().len(), 1);
    assert_eq!(pending["priorCycles"][0]["decision"], "repair");
    assert_eq!(
        pending["priorCycles"][0]["repairBoundary"],
        "the parser only"
    );
    assert!(pending["advice"]
        .as_str()
        .unwrap()
        .contains("reconsider the basis"));
    assert_eq!(
        pending["decisions"],
        json!(["repair", "defer", "reject", "supersede"]),
        "the Lead still decides"
    );
}

#[test]
fn case_d_supersede_requires_an_explicit_successor_basis() {
    let fixture = Fixture::new("disposition-supersede");
    let basis = json!({
        "version": 1,
        "constraints": ["keep the header format"],
        "openZones": ["line endings", "encoding"],
        "decisiveCases": ["a header parses"]
    });
    let (work, _) = fixture.begin(&["--basis", &basis.to_string(), "--review-policy", "required"]);
    let lead = pending_lead(&fixture, &work);
    let old_basis = lead["packet"]["basisSha256"].clone();
    refused(
        &fixture.dispose(
            &work,
            &lead,
            "supersede",
            &["--repair-boundary", "b", "--regression", "r"],
        ),
        "successorBasis",
    );
    let mut dropped = basis.clone();
    dropped["constraints"] = json!([]);
    refused(
        &fixture.dispose(
            &work,
            &lead,
            "supersede",
            &[
                "--repair-boundary",
                "b",
                "--regression",
                "r",
                "--successor-basis",
                &dropped.to_string(),
            ],
        ),
        "constraint",
    );
    let mut successor = basis.clone();
    successor["constraints"] = json!(["keep the header format", "accept CRLF"]);
    successor["openZones"] = json!(["encoding"]);
    successor["decisiveCases"] = json!(["a header parses", "a CRLF header parses"]);
    let decided = fixture.dispose(
        &work,
        &lead,
        "supersede",
        &[
            "--repair-boundary",
            "Line endings under the new constraint only",
            "--regression",
            "a CRLF header parses",
            "--successor-basis",
            &successor.to_string(),
        ],
    );
    assert!(decided.status.success(), "{}", text(&decided));
    let worker = fixture.next(&work);
    assert_eq!(worker["role"], "worker");
    assert_ne!(worker["packet"]["basisSha256"], old_basis);
    assert_eq!(worker["packet"]["leadRepair"]["decision"], "supersede");
    assert_eq!(
        worker["packet"]["leadRepair"]["basisSha256"],
        worker["packet"]["basisSha256"]
    );
    let check = &worker["progress"]["components"]["check"];
    assert_eq!(check["completed"], 0, "the prior check no longer applies");
    let events = fixture.events(&work);
    let disposition = events
        .iter()
        .find(|event| event["outcome"] == "disposition")
        .unwrap();
    assert_eq!(
        disposition["disposition"]["successorBasis"]["constraints"],
        successor["constraints"]
    );
}

#[test]
fn case_e_unmarked_run_keeps_historical_rework_routing() {
    let fixture = Fixture::new("disposition-legacy");
    let (work, first) = fixture.begin(&[]);
    assert!(fixture.events(&work)[0].get("basisProtocol").is_none());
    let reviewer = fixture.to_review(&work, first);
    let after = fixture.give_ok(&work, &reviewer, "rework", "finding");
    assert_eq!(
        after["next"]["role"], "worker",
        "historical routing is unchanged"
    );
    let worker = fixture.next(&work);
    assert!(worker["packet"].get("leadRepair").is_none());
    refused(
        &fixture.dispose(&work, &worker, "defer", &[]),
        "not the current pending Lead decision",
    );
}

#[test]
fn case_f_direct_work_has_no_disposition_ceremony() {
    let fixture = Fixture::new("disposition-direct");
    let classified = fixture.ok(
        &[
            "work",
            "classify",
            "--material-consequence",
            "false",
            "--promotion-required",
            "false",
        ],
        b"",
    );
    assert_eq!(classified["assessment"]["activation"], "direct");
    assert!(!classified.to_string().contains("disposition"));
    let runs = fixture.root.join(".exitbind/runs");
    assert!(!runs.exists() || fs::read_dir(runs).unwrap().next().is_none());
}

#[test]
fn default_role_profiles_separate_findings_from_requirements() {
    let fixture = Fixture::new("disposition-profiles");
    let read = |role: &str| {
        fs::read_to_string(fixture.root.join(format!("exitbind/agents/{role}.md"))).unwrap()
    };
    assert!(read("lead").contains("evidence, not a requirement"));
    assert!(read("worker").contains("smallest faithful change"));
    assert!(read("reviewer").contains("valid without being in the current scope"));
}

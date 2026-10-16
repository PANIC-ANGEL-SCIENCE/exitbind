//! Ordinary consumer delivery, semantic discrimination and evidence drift.
mod support;

use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn execute(root: &Path, args: &[&str], input: &[u8], config: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command.current_dir(root).args(args);
    if config {
        command.args(["--config", "exitbind.json"]);
    }
    for key in [
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "EXITBIND_NATIVE_SESSION_ID",
        "EXITBIND_SESSION_GOALS_DIR",
        "CLAUDE_ENV_FILE",
        "XDG_STATE_HOME",
    ] {
        command.env_remove(key);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn value(root: &Path, args: &[&str], input: &[u8]) -> Value {
    let output = execute(root, args, input, true);
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

struct Fixture {
    root: PathBuf,
    work: String,
}
impl Fixture {
    fn new() -> Self {
        let root = support::temp("early-evidence");
        let initialized = execute(
            &root,
            &["init", "--root", root.to_str().unwrap(), "--skip-skills"],
            b"",
            false,
        );
        assert!(initialized.status.success(), "{initialized:?}");
        fs::write(root.join("probe.py"), "# approved behavior probe\n").unwrap();
        let start = value(
            &root,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Repeat the same operation once; reject a changed payload; preserve receipt text.",
                "--check-command",
                "true",
                "--review-policy",
                "omitted",
                "--detail",
            ],
            b"",
        );
        Self {
            root,
            work: start["work"].as_str().unwrap().into(),
        }
    }
    fn detail(&self) -> Value {
        value(&self.root, &["work", "detail", &self.work, "--json"], b"")
    }
    fn challenge(&self) -> Value {
        value(
            &self.root,
            &[
                "work",
                "challenge",
                &self.work,
                "--question",
                "Can response loss cause a repeated effect?",
                "--probe-command",
                "python3 probe.py",
                "--probe-source",
                "probe.py",
                "--json",
            ],
            b"",
        )
    }
    fn submit(&self, outcome: &str, artifact: Value) {
        let output = self.submit_output(outcome, artifact);
        assert!(output.status.success(), "{output:?}");
    }
    fn submit_output(&self, outcome: &str, artifact: Value) -> Output {
        self.submit_raw_output(outcome, artifact.to_string().as_bytes())
    }
    fn submit_raw_output(&self, outcome: &str, artifact: &[u8]) -> Output {
        let detail = self.detail();
        let form = detail["actionForms"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|choice| choice["label"] == outcome)
            .unwrap();
        let args = form["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .map(|arg| {
                arg.as_str()
                    .unwrap()
                    .replace("<REASON>", "Observed test result")
            })
            .collect::<Vec<_>>();
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        execute(&self.root, &args, artifact, false)
    }
    fn report(&self) -> Value {
        let mut report = self.challenge()["evidenceTemplate"].clone();
        report["contributions"] = json!([
            {"sessionRef":"host-session-a","claim":"Retries must preserve effects","evidenceRef":"first-pass-a",
                "whatWouldChange":"An atomic idempotency guard"},
            {"sessionRef":"host-session-b","claim":"Changed payload must be rejected","evidenceRef":"first-pass-b",
                "whatWouldChange":"A payload collision test"}]);
        report["probe"]["observation"] = json!({"outcome":"discriminated",
            "wrongCandidate":{"oldCheck":"passed","strengthenedCheck":"failed_semantic_assertion"},
            "correctAlternative":{"strengthenedCheck":"passed"}});
        report
    }
    fn validate(&self, report: &Value) -> Output {
        execute(
            &self.root,
            &["work", "challenge", "validate", &self.work, "--json"],
            report.to_string().as_bytes(),
            true,
        )
    }
    fn submit_events(&self) -> Vec<Value> {
        let id = self.work.strip_prefix("smw_").unwrap();
        fs::read_to_string(self.root.join(format!(".exitbind/runs/work-{id}.jsonl")))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|event: &Value| event["action"] == "submit")
            .collect()
    }
    fn result_path(&self, event: &Value) -> PathBuf {
        self.root.join(event["artifact"]["path"].as_str().unwrap())
    }
    fn accepted_result(&self) -> Value {
        let checked = execute(&self.root, &["work", "check", &self.work], b"", true);
        assert!(checked.status.success(), "{checked:?}");
        self.submit(
            "accepted",
            json!({"summary":"Accepted result without a report"}),
        );
        self.submit_events().pop().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn normal_lead_entry_prepares_conditional_packet_without_starting_any_effect() {
    let fixture = Fixture::new();
    let before = fixture.detail();
    let challenge = fixture.challenge();
    assert_eq!(challenge["readOnly"], true);
    assert_eq!(
        challenge["hostAssignment"]["firstPassesSeeEachOther"],
        false
    );
    assert_eq!(
        challenge["probeExecution"],
        "not executed; observe this permitted probe before reporting an outcome"
    );
    assert_eq!(
        before["sections"]["assignment"]["optionalChallenge"]["required"],
        false
    );
    assert_eq!(before["binding"], fixture.detail()["binding"]);
    assert_eq!(
        before["sections"]["evidence"]["earlyEvidence"]["state"],
        "not_selected"
    );
    assert_eq!(
        challenge["requirement"]["approvedText"],
        "Repeat the same operation once; reject a changed payload; preserve receipt text."
    );
    assert!(challenge["validation"]["route"]["command"].is_array());
}

#[test]
fn prepared_report_has_a_bounded_explicit_validator_with_precise_errors() {
    let fixture = Fixture::new();
    let report = fixture.report();
    let valid = fixture.validate(&report);
    assert!(valid.status.success(), "{valid:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&valid.stdout).unwrap()["valid"],
        true
    );

    let mut missing = report.clone();
    missing["contributions"][0]["evidenceRef"] = Value::Null;
    let invalid = fixture.validate(&missing);
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8_lossy(&invalid.stdout)
            .contains("earlyEvidence.contributions[0].evidenceRef"),
        "{invalid:?}"
    );

    let mut stale = report;
    stale["candidateInputsSha256"] = json!("0".repeat(64));
    let invalid = fixture.validate(&stale);
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8_lossy(&invalid.stdout).contains("candidateInputsSha256 does not match"),
        "{invalid:?}"
    );
}

#[test]
fn inconclusive_report_can_truthfully_record_no_probe() {
    let fixture = Fixture::new();
    let mut report = fixture.report();
    report["probe"]["command"] = json!("");
    report["probe"]["commandSha256"] = Value::Null;
    report["probe"]["source"]["path"] = json!("");
    report["probe"]["source"]["sha256"] = Value::Null;
    report["probe"]["observation"] =
        json!({"outcome":"inconclusive","wrongCandidate":null,"correctAlternative":null});
    let valid = fixture.validate(&report);
    assert!(valid.status.success(), "{valid:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&valid.stdout).unwrap()["probeSourceSha256"],
        Value::Null
    );
}

#[test]
fn current_result_reports_evidence_and_rejects_candidate_or_probe_drift() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let report = fixture.report();
    fixture.submit(
        "completed",
        json!({"summary":"Implemented approved behavior","earlyEvidence":report}),
    );
    let current = fixture.detail();
    let evidence = &current["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["state"], "reported_current");
    assert_eq!(evidence["finalCheck"], Value::Null);
    assert_eq!(evidence["leadDisposition"], Value::Null);
    assert_eq!(evidence["authority"], "none");
    fs::write(fixture.root.join("probe.py"), "# changed test premise\n").unwrap();
    let changed = fixture.detail();
    let evidence = &changed["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["state"], "historical");
    assert!(evidence["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("probe_source_changed_or_unavailable")));
    assert_ne!(changed["binding"], current["binding"]);
}

#[test]
fn unsupported_independence_or_requirement_edit_stays_unknown_or_historical() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let mut report = fixture.report();
    report["requirement"]["approvedText"] = json!("An invented user requirement");
    fixture.submit("completed", json!({"earlyEvidence":report}));
    let detail = fixture.detail();
    let evidence = &detail["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["state"], "historical");
    assert!(evidence["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("requirement_or_goal_revision_changed")));
}

#[test]
fn no_new_information_remains_an_honest_report_and_does_not_pass_final_check() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let mut report = fixture.report();
    report["probe"]["observation"] = json!({"outcome":"no_new_information"});
    fixture.submit("completed", json!({"earlyEvidence":report}));
    let detail = fixture.detail();
    let evidence = &detail["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(
        evidence["report"]["probe"]["observation"]["outcome"],
        "no_new_information"
    );
    assert_eq!(evidence["finalCheck"], Value::Null);
}

#[test]
fn malformed_selected_report_is_rejected_before_return_event() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let mut report = fixture.report();
    report["probe"]["commandSha256"] = Value::Null;
    let before = fixture.submit_events().len();
    let output = fixture.submit_output("completed", json!({"earlyEvidence":report}));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("earlyEvidence.probe.commandSha256"));
    assert_eq!(fixture.submit_events().len(), before);
}

#[test]
fn well_formed_historical_counterexample_remains_returnable_and_keeps_its_hash() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let mut report = fixture.report();
    report["candidateInputsSha256"] = json!("0".repeat(64));
    fixture.submit("completed", json!({"earlyEvidence":report}));
    let evidence = &fixture.detail()["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["state"], "historical");
    assert!(evidence["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("candidate_or_relevant_inputs_changed")));
    assert_eq!(evidence["report"]["candidateInputsSha256"], "0".repeat(64));
}

#[test]
fn missing_newest_result_keeps_next_and_world_readable_without_history_churn() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"earlyEvidence":fixture.report()}));
    let original_report_hash =
        fixture.submit_events().last().unwrap()["artifact"]["sha256"].clone();
    let output = fixture.submit_raw_output("completed", b"Ordinary UTF-8 result notes\n");
    assert!(output.status.success(), "{output:?}");
    let check = execute(&fixture.root, &["work", "check", &fixture.work], b"", true);
    assert!(check.status.success(), "{check:?}");
    let historical = &fixture.detail()["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(historical["state"], "historical");
    assert!(historical["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("newer_candidate_result_without_report")));
    assert_eq!(historical["artifactSha256"], original_report_hash);
    assert_eq!(
        historical["latestCandidateEventSha256"],
        fixture.submit_events().last().unwrap()["eventSha256"]
    );
    assert!(historical["finalCheck"]["eventSha256"].is_string());
    assert_eq!(historical["finalCheck"]["distinctFromReportedProbe"], true);
    let newest = fixture.accepted_result();
    let events = fixture.submit_events();
    let ledger = fixture.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        fixture.work.strip_prefix("smw_").unwrap()
    ));
    let before = fs::read(&ledger).unwrap();
    fs::remove_file(fixture.result_path(&newest)).unwrap();

    let next = value(
        &fixture.root,
        &["work", "next", &fixture.work, "--json"],
        b"",
    );
    assert!(next["current"]["binding"].is_string());
    let world = value(
        &fixture.root,
        &["work", "world", &fixture.work, "--json"],
        b"",
    );
    assert!(world["motifs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|motif| motif["name"] == "exit_sign" && motif["active"] == false));
    assert_eq!(fixture.submit_events().len(), events.len());
    assert_eq!(fs::read(&ledger).unwrap(), before);
    let detail = execute(
        &fixture.root,
        &["work", "detail", &fixture.work, "--json"],
        b"",
        true,
    );
    assert!(!detail.status.success());
    assert!(String::from_utf8_lossy(&detail.stdout).contains("artifact"));
}

#[test]
fn changed_newest_result_keeps_next_readable_while_detail_stays_strict() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"earlyEvidence":fixture.report()}));
    fixture.submit("completed", json!({"summary":"No challenge report here"}));
    let newest = fixture.accepted_result();
    let path = fixture.result_path(&newest);
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b' ');
    fs::write(path, bytes).unwrap();

    let next = value(
        &fixture.root,
        &["work", "next", &fixture.work, "--json"],
        b"",
    );
    assert!(next["current"]["binding"].is_string());
    let detail = execute(
        &fixture.root,
        &["work", "detail", &fixture.work, "--json"],
        b"",
        true,
    );
    assert!(!detail.status.success());
    assert!(String::from_utf8_lossy(&detail.stdout).contains("artifact bytes changed"));
}

#[test]
fn verified_submissions_without_a_report_remain_not_selected() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let evidence = &fixture.detail()["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["selected"], false);
    assert_eq!(evidence["state"], "not_selected");
}

#[test]
fn ordinary_plain_text_result_keeps_optional_report_absent() {
    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let output = fixture.submit_raw_output("completed", b"ordinary UTF-8 result notes\n");
    assert!(output.status.success(), "{output:?}");
    let evidence = &fixture.detail()["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["state"], "not_selected");
}

#[test]
fn large_ordinary_result_without_optional_report_stays_supported() {
    let fixture = Fixture::new();
    let payload = "x".repeat(96 * 1024);
    fixture.submit(
        "scoped",
        json!({"summary":"ordinary result", "payload":payload}),
    );
    let event = fixture.submit_events().pop().unwrap();
    assert!(event["artifact"]["bytes"].as_u64().unwrap() > 64 * 1024);
    assert_eq!(
        fixture.detail()["sections"]["evidence"]["earlyEvidence"]["state"],
        "not_selected"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_latest_result_remains_a_strict_refusal() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    fixture.submit("scoped", json!({"scope":"Approved task only"}));
    let newest = fixture.submit_events().pop().unwrap();
    let path = fixture.result_path(&newest);
    let target = path.with_extension("target");
    fs::rename(&path, &target).unwrap();
    symlink(&target, &path).unwrap();

    let output = execute(
        &fixture.root,
        &["work", "next", &fixture.work, "--json"],
        b"",
        true,
    );
    assert!(!output.status.success());
    let response = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let refused: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(refused["effect"], "no-change", "{response}");
    assert_eq!(refused["reason"]["code"], "corrupt_ledger", "{response}");
}

/// Seeded executable candidates, not a naturally discovered product defect.
#[test]
fn weak_check_survives_wrong_candidate_and_strong_oracle_accepts_correct_alternative() {
    let root = support::temp("early-evidence-oracle");
    let candidates = [
        ("wrong", "seen = {}\ncount = 0\ndef apply(key, payload):\n global count\n count += 1\n seen[key] = payload\n return count\n"),
        ("correct", "seen = {}\ncount = 0\ndef apply(key, payload):\n global count\n if key in seen:\n  if seen[key] != payload: raise ValueError('collision')\n  return count\n seen[key] = payload\n count += 1\n return count\n"),
        ("alternative", "seen = []\ncount = 0\ndef apply(key, payload):\n global count\n for old_key, old_payload in seen:\n  if key == old_key:\n   if payload != old_payload: raise ValueError('collision')\n   return count\n seen.append((key, payload))\n count = len(seen)\n return count\n"),
    ];
    let probe = "import runpy, sys\nm = runpy.run_path(sys.argv[1])\nf = m['apply']\nassert f('request-1', 'payload') == 1, 'first_effect'\nif sys.argv[2] == 'strong':\n assert f('request-1', 'payload') == 1, 'duplicate_effects'\n try:\n  f('request-1', 'different')\n except ValueError: pass\n else: raise AssertionError('payload_collision')\n assert f('request-2', 'payload') == 2, 'legitimate_second_effect'\n";
    fs::write(root.join("probe.py"), probe).unwrap();
    for (name, source) in candidates {
        let path = root.join(format!("{name}.py"));
        fs::write(&path, source).unwrap();
        for mode in ["old", "strong"] {
            let output = Command::new("python3")
                .arg(root.join("probe.py"))
                .arg(&path)
                .arg(mode)
                .output()
                .unwrap();
            if name == "wrong" && mode == "strong" {
                assert!(!output.status.success());
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("AssertionError: duplicate_effects"),
                    "{output:?}"
                );
            } else {
                assert!(output.status.success(), "{name}/{mode}: {output:?}");
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

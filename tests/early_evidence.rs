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
        let output = execute(&self.root, &args, artifact.to_string().as_bytes(), false);
        assert!(output.status.success(), "{output:?}");
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
    report["contributions"][1]["sessionRef"] = report["contributions"][0]["sessionRef"].clone();
    fixture.submit("completed", json!({"earlyEvidence":report}));
    let detail = fixture.detail();
    let evidence = &detail["sections"]["evidence"]["earlyEvidence"];
    assert_eq!(evidence["state"], "unknown");
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

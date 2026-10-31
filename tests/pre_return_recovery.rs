//! Public-seam interruption and exact recovery of an owned pre-return.
#![cfg(target_os = "linux")]

#[path = "pre_return_recovery/guidance.rs"]
mod guidance;
#[path = "pre_return_recovery/known_outcomes.rs"]
mod known_outcomes;
#[path = "pre_return_recovery/ownership.rs"]
mod ownership;
mod support;
#[path = "pre_return_recovery/unselected.rs"]
mod unselected;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    path::{Path, PathBuf},
    process::{Child, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const BODY: &[u8] = b"the complete worker result\n";
const LARGE_BODY: &[u8] = &[b'q'; 32_317];
const CHECKER: &str = "#!/bin/sh\nset -eu\nprintf x >> .exitbind/attempt-count\nprintf '%s\\n' \"$EXITBIND_VERIFICATION_SCRATCH\" >> .exitbind/scratch-paths\n: > .exitbind/check-started\nwhile test ! -f .exitbind/release-check; do sleep 0.05; done\ntest -f .exitbind/allow-check\n";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn command(root: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .arg("--config")
        .arg(root.join("exitbind.json"))
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(bytes) = input {
        if let Err(error) = child.stdin.take().unwrap().write_all(bytes) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
    }
    child.wait_with_output().unwrap()
}

fn successful(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the owned fixture"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

struct CallerGuard<'a> {
    child: Option<Child>,
    root: &'a Path,
}

impl Drop for CallerGuard<'_> {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = fs::write(self.root.join(".exitbind/release-check"), b"cleanup");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct Fixture {
    root: support::TempRoot,
    work: String,
    assignment: String,
    held: String,
    attempt: String,
    admission: Vec<u8>,
    body: &'static [u8],
}

impl Fixture {
    fn new(label: &str, grant: bool) -> Self {
        let root = support::temp(label);
        let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        value["project"]["verificationPreparation"] =
            json!({"version":1,"pythonBytecode":"disabled","scratch":"taskOwned"});
        fs::write(config, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        fs::write(root.join("checker.sh"), CHECKER).unwrap();
        let started = successful(command(
            &root,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "owned pre-return interruption",
                "--check-command",
                "test -f actual-product-check",
                "--proof-origin",
                "synthetic",
                "--pre-return-command",
                "sh ./checker.sh",
                "--pre-return-timeout-ms",
                "20000",
            ],
            None,
        ));
        let work = started["work"].as_str().unwrap().to_owned();
        let lead = started["next"]["assignment"].as_str().unwrap();
        let scoped = command(
            &root,
            &["work", "return", &work, lead, "--outcome", "scoped"],
            Some(b"scope"),
        );
        assert!(scoped.status.success(), "{scoped:?}");
        let detail = successful(command(&root, &["work", "detail", &work, "--json"], None));
        let assignment = detail["recipient"]["assignment"]
            .as_str()
            .unwrap()
            .to_owned();
        if grant {
            let permit = successful(command(
                &root,
                &[
                    "work",
                    "permit",
                    &work,
                    &assignment,
                    "--operation",
                    "fixture repair",
                ],
                None,
            ));
            assert_eq!(permit["allowed"], true, "{permit}");
        }
        let body = if grant { LARGE_BODY } else { BODY };
        let held = format!(
            "hld_{}_{}_{}",
            &work[4..],
            digest(assignment.as_bytes()),
            digest(body)
        );

        // The caller is a real Work return. It stores the held bytes and admits
        // the real checker before the checker signals that it is running.
        let caller = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&root)
            .args([
                "work",
                "return",
                &work,
                &assignment,
                "--outcome",
                "completed",
                "--config",
            ])
            .arg(root.join("exitbind.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut caller = CallerGuard {
            child: Some(caller),
            root: &root,
        };
        caller
            .child
            .as_mut()
            .unwrap()
            .stdin
            .take()
            .unwrap()
            .write_all(body)
            .unwrap();
        wait_until(|| root.join(".exitbind/check-started").is_file());
        let attempts = fs::read_dir(root.join(".exitbind/pre-return"))
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(".attempt.json"))
            .collect::<Vec<_>>();
        assert_eq!(attempts.len(), 1);
        let admission = fs::read(&attempts[0]).unwrap();
        let attempt = serde_json::from_slice::<Value>(&admission).unwrap()["attemptId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            fs::read(root.join(".exitbind/held").join(format!("{held}.bin"))).unwrap(),
            body
        );
        caller.child.as_mut().unwrap().kill().unwrap();
        let killed = caller.child.take().unwrap().wait_with_output().unwrap();
        assert_eq!(killed.status.signal(), Some(9), "{killed:?}");
        drop(caller);
        let fixture = Self {
            root,
            work,
            assignment,
            held,
            attempt,
            admission,
            body,
        };
        assert!(fixture.read_attempt(&fixture.attempt)["result"].is_null());
        fixture
    }

    fn call(&self, args: &[&str], input: Option<&[u8]>) -> Output {
        command(&self.root, args, input)
    }
    fn detail(&self) -> Value {
        successful(self.call(&["work", "detail", &self.work, "--json"], None))
    }
    fn preview(&self) -> Value {
        successful(self.call(&["work", "recover-pre-return", &self.work, "--json"], None))
    }
    fn read_attempt(&self, id: &str) -> Value {
        successful(self.call(
            &["run", "pre-return", "read", &self.work, id, "--json"],
            None,
        ))
    }
    fn apply(&self, decision: &Value) -> Output {
        self.call(
            &[
                "work",
                "recover-pre-return",
                &self.work,
                "--apply",
                "--current-binding",
                decision["currentBinding"].as_str().unwrap(),
                "--json",
            ],
            Some(&serde_json::to_vec(decision).unwrap()),
        )
    }
    fn ledger_path(&self) -> String {
        format!(".exitbind/runs/work-{}.jsonl", &self.work[4..])
    }
    fn ledger(&self) -> Vec<u8> {
        fs::read(self.root.join(self.ledger_path())).unwrap()
    }
    fn governor(&self) -> Value {
        successful(self.call(&["run", "inspect", &self.ledger_path(), "--json"], None))["governor"]
            .clone()
    }
    fn held_path(&self) -> PathBuf {
        self.root
            .join(".exitbind/held")
            .join(format!("{}.bin", self.held))
    }
    fn admission_path(&self) -> PathBuf {
        self.root
            .join(".exitbind/pre-return")
            .join(format!("pre-return-{}.attempt.json", self.attempt))
    }
    fn result_path(&self) -> PathBuf {
        self.root
            .join(".exitbind/pre-return")
            .join(format!("pre-return-{}.result.json", self.attempt))
    }
    fn count(&self) -> Vec<u8> {
        fs::read(self.root.join(".exitbind/attempt-count")).unwrap()
    }
    fn release(&self) -> Value {
        fs::write(self.root.join(".exitbind/allow-check"), b"yes").unwrap();
        fs::write(self.root.join(".exitbind/release-check"), b"go").unwrap();
        let mut last = Value::Null;
        wait_until(|| {
            last = self.preview();
            last["ownerDecision"]["snapshot"]["evidence"]["type"] == "owned_observer_v1"
        });
        last
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Never leave the owned checker waiting if an assertion fails. The
        // proof frame becomes complete only after the capsule drains its child.
        let _ = fs::write(self.root.join(".exitbind/release-check"), b"cleanup");
        let proof = self
            .root
            .join(".exitbind/pre-return")
            .join(format!("pre-return-{}.observer.raw", self.attempt));
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && fs::metadata(&proof).map_or(true, |m| m.len() < 100) {
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn approved(preview: &Value) -> Value {
    let mut decision = preview["ownerDecision"].clone();
    assert!(decision.is_object(), "{preview}");
    decision["approved"] = json!(true);
    decision["reason"] = json!("Observed the owned execution domain finish.");
    decision["executionDomain"] = json!("ended");
    decision["effects"] = json!("reconciled");
    decision["effectsReason"] = json!("The fixture checker effects were inspected.");
    decision
}

fn rendered(root: &Path, route: &Value, input: Option<&[u8]>) -> Output {
    let argv = route["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap())
        .collect::<Vec<_>>();
    let mut child = support::git_topology::command(argv[0])
        .current_dir(root)
        .args(&argv[1..])
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(bytes) = input {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn owned_observer_real_interruption_recovers_once_and_preserves_exact_held_result() {
    let f = Fixture::new("pre-return-owned-real", true);
    assert_eq!(f.body.len(), 32_317);
    let governor = f.governor();
    assert_eq!(governor["spent"], 1);
    let before = f.ledger();
    let original_events: Vec<Value> = std::str::from_utf8(&before)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let grant = original_events
        .iter()
        .find(|event| event["action"] == "govern" && event["governorEvent"]["action"] == "mutation")
        .expect("fixture must issue an actual mutation grant")["eventSha256"]
        .clone();
    let action = f.detail();
    assert_eq!(
        action["actionForms"]["state"], "interrupted_pre_return",
        "{action}"
    );
    assert!(!action["actionForms"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["label"] == "completed"));
    let live = f.preview();
    assert_eq!(live["state"], "evidence_required", "{live}");
    assert!(live["apply"].is_null());
    assert_eq!(f.ledger(), before);
    assert_eq!(f.count(), b"x");
    let changed = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
        ],
        None,
    );
    assert!(!changed.status.success(), "{changed:?}");
    assert_eq!(f.ledger(), before);
    assert_eq!(f.count(), b"x");

    let preview = f.release();
    let emitted = &action["actionForms"]["choices"][0]["command"];
    let emitted_preview = successful(rendered(&f.root, emitted, None));
    assert_eq!(
        emitted_preview["ownerDecision"]["snapshot"],
        preview["ownerDecision"]["snapshot"]
    );
    let decision = approved(&preview);
    let mut rejected = decision.clone();
    rejected["effects"] = json!("unknown");
    assert!(!f.apply(&rejected).status.success());
    rejected = decision.clone();
    rejected["agent"] = json!("worker");
    assert!(!f.apply(&rejected).status.success());
    rejected = decision.clone();
    rejected["snapshot"]["held"]["sha256"] = json!("0".repeat(64));
    assert!(!f.apply(&rejected).status.success());
    assert_eq!(f.ledger(), before);
    let checker = f.root.join("checker.sh");
    fs::write(&checker, format!("{CHECKER}# drift\n")).unwrap();
    assert!(!f.apply(&decision).status.success());
    fs::write(&checker, CHECKER).unwrap();
    let config = f.root.join("exitbind.json");
    let original_config = fs::read(&config).unwrap();
    let mut changed_config = original_config.clone();
    changed_config.push(b' ');
    fs::write(&config, changed_config).unwrap();
    assert!(!f.apply(&decision).status.success());
    fs::write(config, original_config).unwrap();
    assert_eq!(f.ledger(), before);
    assert_eq!(fs::read(f.held_path()).unwrap(), f.body);

    let emitted_apply = preview["apply"]["command"].clone();
    let decision_bytes = serde_json::to_vec(&decision).unwrap();
    let (a, b) = thread::scope(|scope| {
        let a = scope.spawn(|| rendered(&f.root, &emitted_apply, Some(&decision_bytes)));
        let b = scope.spawn(|| f.apply(&decision));
        (a.join().unwrap(), b.join().unwrap())
    });
    assert!(a.status.success() || b.status.success(), "{a:?} {b:?}");
    let replay = successful(f.apply(&decision));
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["checkRecorded"], false);
    assert_eq!(
        f.governor(),
        governor,
        "reconciliation changed governor accounting"
    );
    assert_eq!(f.count(), b"x");
    assert_eq!(fs::read(f.admission_path()).unwrap(), f.admission);
    assert!(!f.result_path().exists());
    assert_eq!(fs::read(f.held_path()).unwrap(), f.body);
    // Synthetic tampering is a negative control after the real interruption.
    let proof = f
        .root
        .join(".exitbind/pre-return")
        .join(format!("pre-return-{}.observer.raw", f.attempt));
    let reconciled_ledger = f.ledger();
    let original_proof = fs::read(&proof).unwrap();
    let mut altered_proof = original_proof.clone();
    altered_proof[0] ^= 1;
    fs::write(&proof, altered_proof).unwrap();
    let unavailable = f.detail();
    assert_eq!(
        unavailable["actionForms"]["state"], "pre_return_evidence_unavailable",
        "{unavailable}"
    );
    assert!(unavailable["actionForms"]["diagnostic"]["command"]["argv"].is_array());
    let refused = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
        ],
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(f.ledger(), reconciled_ledger);
    assert_eq!(f.count(), b"x");
    fs::write(&proof, original_proof).unwrap();
    let events: Vec<Value> = std::str::from_utf8(&f.ledger())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["action"] == "pre_return_reconciled")
            .count(),
        1
    );
    if let Ok(old) = std::env::var("EXITBIND_PRE_RETURN_OLD_READER") {
        let old = support::git_topology::command(old)
            .current_dir(&f.root)
            .args(["work", "detail", &f.work, "--json", "--config"])
            .arg(f.root.join("exitbind.json"))
            .output()
            .unwrap();
        assert!(
            !old.status.success(),
            "older reader accepted the new event: {old:?}"
        );
    } else {
        eprintln!("COVERAGE UNAVAILABLE: no earlier reader supplied");
    }

    let complete = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
        ],
        None,
    );
    assert!(complete.status.success(), "{complete:?}");
    assert_eq!(f.count(), b"xx");
    let scratches = fs::read_to_string(f.root.join(".exitbind/scratch-paths")).unwrap();
    let scratches: Vec<_> = scratches.lines().collect();
    assert_eq!(scratches.len(), 2);
    assert_ne!(scratches[0], scratches[1]);
    let events: Vec<Value> = std::str::from_utf8(&f.ledger())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let worker = events
        .iter()
        .find(|event| event["action"] == "submit" && event["role"] == "worker")
        .unwrap();
    assert_eq!(worker["governorEvent"]["authorizationMode"], "explicit");
    assert_eq!(worker["governorEvent"]["grantEventSha256s"], json!([grant]));
    assert_eq!(worker["governorEvent"]["checkpoint"], governor["spent"]);
    assert_eq!(worker["artifact"]["sha256"], digest(f.body));
    assert_eq!(
        fs::read(f.root.join(worker["artifact"]["path"].as_str().unwrap())).unwrap(),
        f.body
    );
    let attempts = fs::read_dir(f.root.join(".exitbind/pre-return"))
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".attempt.json"))
        .collect::<Vec<_>>();
    assert_eq!(attempts.len(), 2);
    let retry = attempts
        .iter()
        .map(|path| serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap())
        .find(|record| record["attemptId"] != f.attempt)
        .unwrap();
    let original: Value = serde_json::from_slice(&f.admission).unwrap();
    assert_ne!(
        retry["binding"]["verificationOperationId"],
        original["binding"]["verificationOperationId"]
    );
    let saved = f.read_attempt(retry["attemptId"].as_str().unwrap());
    assert_eq!(saved["result"]["state"], "passed");
    assert_eq!(f.count(), b"xx", "reading saved PASS reran the checker");
    assert!(!f.result_path().exists());
}

#[test]
fn live_unknown_allows_truthful_blocked_report_and_direct_submit_cannot_bypass() {
    let f = Fixture::new("pre-return-live-report", false);
    let before = f.ledger();
    let preview = f.preview();
    assert_eq!(preview["state"], "evidence_required", "{preview}");
    assert!(preview["blockedOption"]["command"]["argv"].is_array());
    let artifact = f.root.join(".exitbind/artifacts/alternate-result.md");
    fs::write(artifact, b"different result").unwrap();
    let direct = f.call(
        &[
            "run",
            "submit",
            "worker",
            &f.ledger_path(),
            "--outcome",
            "completed",
            "--artifact",
            ".exitbind/artifacts/alternate-result.md",
            "--artifact-root",
            "state",
        ],
        None,
    );
    assert!(!direct.status.success(), "{direct:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&direct.stdout),
        String::from_utf8_lossy(&direct.stderr)
    );
    assert!(diagnostic.contains("uncertain outcome"), "{diagnostic}");
    assert_eq!(f.ledger(), before);
    assert_eq!(f.count(), b"x");
    let mut blocked_route = preview["blockedOption"]["command"].clone();
    let mut fills = 0;
    for word in blocked_route["argv"].as_array_mut().unwrap() {
        if word == "<REASON>" {
            *word = json!("The check outcome is still unknown.");
            fills += 1;
        }
    }
    assert_eq!(fills, 1);
    let blocked = rendered(
        &f.root,
        &blocked_route,
        Some(b"Incomplete: awaiting observer evidence."),
    );
    assert!(blocked.status.success(), "{blocked:?}");
    let events: Vec<Value> = std::str::from_utf8(&f.ledger())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| event["action"] == "submit"
        && event["role"] == "worker"
        && event["outcome"] == "blocked"));
    assert!(!events.iter().any(|event| event["action"] == "submit"
        && event["role"] == "worker"
        && event["outcome"] == "completed"));
    assert_eq!(fs::read(f.held_path()).unwrap(), BODY);
    assert!(!f.result_path().exists());
    fs::write(f.root.join(".exitbind/release-check"), b"finish").unwrap();
}

#[test]
fn synthetic_terminal_records_cannot_launder_a_live_unknown_admission() {
    let f = Fixture::new("pre-return-invalid-terminal", false);
    let original: Value = serde_json::from_slice(&f.admission).unwrap();
    let binding = &original["binding"];
    let mut record = json!({"version":1,"kind":"pre_return_result","attemptId":f.attempt,
        "state":"passed","failureKind":null,"exitCode":0,
        "bindingSha256":digest(&serde_json::to_vec(binding).unwrap()),
        "policySha256":digest(&serde_json::to_vec(&original["policy"]).unwrap()),
        "producer":{"executableSha256":"0".repeat(64),"producer":binding["producer"]},
        "completedAt":"2026-01-01T00:00:00Z"});
    let ledger = f.ledger();
    for variant in 0..3 {
        if variant == 1 {
            record["producer"]["executableSha256"] = binding["checkerExecutableSha256"].clone();
        }
        if variant == 2 {
            record["exitCode"] = json!(9);
        }
        record.as_object_mut().unwrap().remove("recordSha256");
        record["recordSha256"] = json!(digest(&serde_json::to_vec(&record).unwrap()));
        fs::write(f.result_path(), serde_json::to_vec(&record).unwrap()).unwrap();
        fs::set_permissions(f.result_path(), fs::Permissions::from_mode(0o600)).unwrap();
        let refused = f.call(
            &[
                "work",
                "return",
                &f.work,
                &f.assignment,
                "--outcome",
                "completed",
                "--result-ref",
                &f.held,
            ],
            None,
        );
        assert!(!refused.status.success(), "variant {variant}: {refused:?}");
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&refused.stdout),
            String::from_utf8_lossy(&refused.stderr)
        );
        let expected = if variant == 1 {
            "pre-return result has an incomplete capture"
        } else {
            "pre-return result identity mismatch"
        };
        assert!(
            diagnostic.contains(expected),
            "variant {variant}: {diagnostic}"
        );
        assert_eq!(f.ledger(), ledger);
        assert_eq!(f.count(), b"x");
    }
}

#[test]
fn interrupted_fresh_retry_remains_unknown_and_never_authorizes_a_third_attempt() {
    let f = Fixture::new("pre-return-retry-crash", false);
    let decision = approved(&f.release());
    assert!(f.apply(&decision).status.success());
    let before = f.ledger();
    fs::remove_file(f.root.join(".exitbind/release-check")).unwrap();
    let child = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
            "--config",
        ])
        .arg(f.root.join("exitbind.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut caller = CallerGuard {
        child: Some(child),
        root: &f.root,
    };
    wait_until(|| {
        fs::read(f.root.join(".exitbind/attempt-count")).is_ok_and(|bytes| bytes == b"xx")
    });
    caller.child.as_mut().unwrap().kill().unwrap();
    let killed = caller.child.take().unwrap().wait_with_output().unwrap();
    assert_eq!(killed.status.signal(), Some(9), "{killed:?}");
    drop(caller);
    let attempts = fs::read_dir(f.root.join(".exitbind/pre-return"))
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".attempt.json"))
        .collect::<Vec<_>>();
    assert_eq!(attempts.len(), 2);
    let retry = attempts
        .iter()
        .map(|path| serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap())
        .find(|record| record["attemptId"] != f.attempt)
        .unwrap();
    let retry_id = retry["attemptId"].as_str().unwrap();
    let after = f.ledger();
    assert!(after.starts_with(&before));
    let suffix = std::str::from_utf8(&after[before.len()..]).unwrap();
    let lines = suffix.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "{suffix}");
    let admission: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(admission["action"], "pre_return_admitted");
    assert_eq!(admission["admission"]["attemptId"], retry_id);
    assert!(f.read_attempt(retry_id)["result"].is_null());
    let detail = f.detail();
    assert_eq!(
        detail["actionForms"]["state"], "pre_return_retry_interrupted",
        "{detail}"
    );
    let again = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
        ],
        None,
    );
    assert!(!again.status.success(), "{again:?}");
    assert_eq!(f.ledger(), after);
    assert_eq!(f.count(), b"xx");
    assert_eq!(fs::read(f.held_path()).unwrap(), f.body);
    fs::write(f.root.join(".exitbind/release-check"), b"finish").unwrap();
    let proof = f
        .root
        .join(".exitbind/pre-return")
        .join(format!("pre-return-{retry_id}.observer.raw"));
    wait_until(|| fs::metadata(&proof).is_ok_and(|metadata| metadata.len() == 100));
    thread::sleep(Duration::from_millis(50));
    assert!(f.read_attempt(retry_id)["result"].is_null());
}

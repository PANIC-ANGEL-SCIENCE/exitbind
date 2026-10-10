//! Real candidate Work transitions; deterministic role returns prove mechanics.
#![cfg(unix)]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new(script: &str) -> Self {
        let root = support::temp("check-observation");
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        fs::write(root.join("checker.sh"), script).unwrap();
        Self { root }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(args)
            .args(["--config", "exitbind.json"])
            .env("EXITBIND_NO_UPDATE_CHECK", "1");
        command
    }
    fn call(&self, args: &[&str], body: &[u8]) -> Output {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str], body: &[u8]) -> Value {
        let output = self.call(args, body);
        assert!(output.status.success(), "{args:?}: {output:?}");
        value(&output)
    }
    fn next(&self, work: &str) -> Value {
        self.ok(&["work", "next", work, "--full"], b"")["next"].clone()
    }
    fn give(&self, work: &str, action: &Value, outcome: &str) -> Value {
        self.ok(
            &[
                "work",
                "return",
                work,
                action["assignment"].as_str().unwrap(),
                "--outcome",
                outcome,
            ],
            outcome.as_bytes(),
        )
    }
    fn begin(&self) -> String {
        let start = self.ok(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "repair the checker",
                "--check-command",
                "exec sh checker.sh",
                "--review-policy",
                "required",
            ],
            b"",
        );
        let work = start["work"].as_str().unwrap().to_string();
        let worker = self.give(&work, &start["next"], "scoped")["next"].clone();
        self.give(&work, &worker, "completed");
        work
    }
    fn events(&self, work: &str) -> Vec<Value> {
        fs::read_to_string(self.ledger(work))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn ledger(&self, work: &str) -> PathBuf {
        self.root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..]))
    }
    fn repair(&self, work: &str) -> Value {
        let lead = self.next(work);
        self.ok(
            &[
                "work",
                "disposition",
                work,
                lead["assignment"].as_str().unwrap(),
                "--decision",
                "repair",
                "--reason",
                "checker defect inside accepted task",
                "--repair-boundary",
                "checker.sh only",
                "--regression",
                "short observed deadline and successful repaired checker",
            ],
            b"",
        )["next"]
            .clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained failed fixture: {}", self.root.display());
            return;
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}

fn rehash(value: &mut Value) {
    use sha2::{Digest, Sha256};
    value.as_object_mut().unwrap().remove("eventSha256");
    value["eventSha256"] = json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).unwrap())
    ));
}

fn storage_interruption() -> (Fixture, String, Value, Vec<u8>) {
    // A real post-admission storage failure on every supported Unix host,
    // including privileged test runners: make the ledger directory unavailable
    // while the checker runs, then restore the exact original bytes after its
    // observer has returned. No accepted event or process fact is constructed.
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; mv .exitbind/runs .exitbind/runs-retained; printf unavailable > .exitbind/runs; printf retained-output; exit 0\n");
    let work = f.begin();
    let failed = f.call(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert!(!failed.status.success(), "{failed:?}");
    let failure = value(&failed);
    assert_eq!(failure["durableFailureRecorded"], false);
    assert_eq!(
        failure["reason"]["code"],
        "observation_failure_storage_unavailable"
    );
    assert_eq!(failure["observation"]["termination"], "ended");
    fs::remove_file(f.root.join(".exitbind/runs")).unwrap();
    fs::rename(
        f.root.join(".exitbind/runs-retained"),
        f.root.join(".exitbind/runs"),
    )
    .unwrap();
    let prefix = fs::read(f.ledger(&work)).unwrap();
    let mut d = f.ok(&["work", "recover-check", &work, "--json"], b"")["ownerDecision"].clone();
    d["approved"] = json!(true);
    d["reason"] = json!(
        "Observed checker and capture readers ended; restore storage and record failure only"
    );
    d["response"] = json!(String::from_utf8(failed.stdout).unwrap());
    response_hash(&mut d);
    (f, work, d, prefix)
}

fn response_hash(d: &mut Value) {
    use sha2::{Digest, Sha256};
    d["responseSha256"] = json!(format!(
        "{:x}",
        Sha256::digest(d["response"].as_str().unwrap().as_bytes())
    ));
}

fn recover(f: &Fixture, work: &str, d: &Value) -> Output {
    f.call(
        &[
            "work",
            "recover-check",
            work,
            "--apply",
            "--current-binding",
            d["currentBinding"].as_str().unwrap(),
            "--json",
        ],
        &serde_json::to_vec(d).unwrap(),
    )
}

#[test]
fn ended_storage_failure_recovers_once_without_check_credit_or_accounting_reset() {
    let (f, work, d, prefix) = storage_interruption();
    let before = f.next(&work)["packet"]["context"]["loop"].clone();
    let recovered = recover(&f, &work, &d);
    assert!(recovered.status.success(), "{recovered:?}");
    let result = value(&recovered);
    assert_eq!(result["checkRecorded"], false);
    assert!(result["result"].is_null());
    assert_eq!(result["provenance"], "configured_lead_reported");
    // Even the real exit0 above supplies no passing check after capture loss.
    assert_eq!(result["observationFailure"]["facts"]["process"]["code"], 0);
    let bytes = fs::read(f.ledger(&work)).unwrap();
    assert!(bytes.starts_with(&prefix));
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check_observation_recovered")
            .count(),
        1
    );
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    assert_eq!(f.next(&work)["packet"]["context"]["loop"], before);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
    let replay = recover(&f, &work, &d);
    assert!(replay.status.success(), "{replay:?}");
    assert_eq!(value(&replay)["effect"], "no-change");
    assert_eq!(value(&replay)["eventSha256"], result["eventSha256"]);
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), bytes);
    let mut changed = d.clone();
    changed["reason"] = json!("different request");
    assert!(!recover(&f, &work, &changed).status.success());
    let worker = f.repair(&work);
    assert_eq!(worker["role"], "worker");
    assert!(!recover(&f, &work, &d).status.success());
    let permit = f.ok(
        &[
            "work",
            "permit",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--operation",
            "repair checker",
            "--request-id",
            "recovery-repair",
        ],
        b"",
    );
    assert_eq!(permit["allowed"], true);
    fs::write(f.root.join("checker.sh"), "exit 0\n").unwrap();
    f.give(&work, &worker, "completed");
    let check = f.ok(&["work", "check", &work, "--timeout-ms", "2000"], b"");
    assert_eq!(check["result"]["code"], 0);
    let reviewer = f.next(&work);
    assert_eq!(reviewer["role"], "reviewer");
    f.give(&work, &reviewer, "approved");
    let lead = f.next(&work);
    f.give(&work, &lead, "accepted");
}

#[test]
fn recovery_refuses_unapproved_foreign_stale_unknown_and_changed_evidence() {
    let (f, work, d, prefix) = storage_interruption();
    let mut cases = Vec::new();
    for (path, value) in [
        ("/approved", json!(false)),
        ("/agent", json!("worker")),
        ("/work", json!(format!("smw_{}", "a".repeat(64)))),
        ("/snapshot/admissionEventSha256", json!("a".repeat(64))),
        ("/snapshot/ledgerSourceSha256", json!("a".repeat(64))),
        ("/snapshot/timeoutMs", json!(1)),
        ("/snapshot/binding/subjectSha256", json!("a".repeat(64))),
        ("/snapshot/binding/inputsSha256", json!("a".repeat(64))),
        ("/snapshot/binding/configSha256", json!("a".repeat(64))),
        (
            "/snapshot/binding/checkCommandSha256",
            json!("a".repeat(64)),
        ),
        (
            "/snapshot/binding/observerExecutableSha256",
            json!("a".repeat(64)),
        ),
        ("/snapshot/binding/requirementId", json!("foreign")),
        ("/responseSha256", json!("a".repeat(64))),
    ] {
        let mut c = d.clone();
        *c.pointer_mut(path).unwrap() = value;
        cases.push(c);
    }
    for (path, value) in [
        ("/observation/termination", json!("unknown")),
        ("/observation/groupEnded", json!(false)),
        ("/observation/captureReadersEnded", json!(false)),
        ("/observation/cleanupErrorCount", json!(1)),
        ("/observation/remainingOwnedPathCount", json!(1)),
        ("/observation/partialCaptures", json!([])),
        ("/checkRecorded", json!(true)),
    ] {
        let mut c = d.clone();
        let mut r: Value = serde_json::from_str(c["response"].as_str().unwrap()).unwrap();
        *r.pointer_mut(path).unwrap() = value;
        c["response"] = json!(r.to_string());
        response_hash(&mut c);
        cases.push(c);
    }
    for c in cases {
        let refused = recover(&f, &work, &c);
        assert!(!refused.status.success(), "{c}: {refused:?}");
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    }
    let r: Value = serde_json::from_str(d["response"].as_str().unwrap()).unwrap();
    let partial = f.root.join(
        r["observation"]["partialCaptures"][0]["path"]
            .as_str()
            .unwrap(),
    );
    let saved = fs::read(&partial).unwrap();
    fs::write(&partial, b"changed").unwrap();
    assert!(!recover(&f, &work, &d).status.success());
    fs::write(partial, saved).unwrap();
    let checker = fs::read(f.root.join("checker.sh")).unwrap();
    fs::write(f.root.join("checker.sh"), b"exit 0 # changed input\n").unwrap();
    assert!(!recover(&f, &work, &d).status.success());
    fs::write(f.root.join("checker.sh"), checker).unwrap();
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
}

#[test]
fn canonical_recovery_rejects_rehashed_changed_binding_facts_and_prefix() {
    let (f, work, d, prefix) = storage_interruption();
    assert!(recover(&f, &work, &d).status.success());
    let original = f.events(&work).last().unwrap().clone();
    for (path, v) in [
        (
            "/observation/recovery/snapshot/binding/observerExecutableSha256",
            json!("b".repeat(64)),
        ),
        (
            "/observation/recovery/snapshot/admissionEventSha256",
            json!("b".repeat(64)),
        ),
        ("/observation/recovery/approved", json!(false)),
        ("/observation/recovery/agent", json!("worker")),
        ("/observation/facts/groupEnded", json!(false)),
    ] {
        let mut e = original.clone();
        *e.pointer_mut(path).unwrap() = v;
        rehash(&mut e);
        let mut bytes = prefix.clone();
        bytes.extend(serde_json::to_vec(&e).unwrap());
        bytes.push(b'\n');
        fs::write(f.ledger(&work), bytes).unwrap();
        assert!(
            !f.call(&["work", "next", &work], b"").status.success(),
            "accepted corrupt recovery {path}"
        );
    }
    // Shape-valid but wrong raw-prefix identity must fail the ledger owner too.
    let mut e = original;
    let c = &mut e["observation"]["recovery"];
    c["snapshot"]["ledgerSourceSha256"] = json!("b".repeat(64));
    use sha2::{Digest, Sha256};
    let operation=format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"identity":c["snapshot"]["binding"],"ledgerSourceSha256":c["snapshot"]["ledgerSourceSha256"]})).unwrap()));
    let mut r: Value = serde_json::from_str(c["response"].as_str().unwrap()).unwrap();
    for (i, p) in r["observation"]["partialCaptures"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        p["path"] = json!(format!(
            ".exitbind/artifacts/check-partial-{operation}-1-{i}.raw"
        ));
    }
    c["response"] = json!(r.to_string());
    response_hash(c);
    e["observation"]["facts"] = r["observation"].clone();
    rehash(&mut e);
    let mut bytes = prefix;
    bytes.extend(serde_json::to_vec(&e).unwrap());
    bytes.push(b'\n');
    fs::write(f.ledger(&work), bytes).unwrap();
    assert!(!f.call(&["work", "next", &work], b"").status.success());
}

#[test]
fn timeout_records_failure_then_lead_repair_recheck_required_review_and_acceptance() {
    let f = Fixture::new("printf before-timeout; exec sleep 3\n");
    let work = f.begin();
    let started = Instant::now();
    let failure = f.call(&["work", "check", &work, "--timeout-ms", "50"], b"");
    assert!(!failure.status.success(), "{failure:?}");
    // This includes debug executable hashing and concurrent fixture load.
    assert!(started.elapsed() < Duration::from_secs(10));
    let result = value(&failure);
    assert_eq!(result["checkRecorded"], false);
    let facts = &result["observationFailure"]["facts"];
    assert_eq!(facts["deadlineExceeded"], true);
    assert!(facts["durationMs"].as_u64().unwrap() < 1000);
    assert_eq!(facts["termination"], "ended");
    assert_eq!(facts["capture"], "incomplete");
    assert!(!facts["process"].is_null());
    assert_eq!(facts["partialCaptures"].as_array().unwrap().len(), 2);
    for partial in facts["partialCaptures"].as_array().unwrap() {
        use sha2::{Digest, Sha256};
        let bytes = fs::read(f.root.join(partial["path"].as_str().unwrap())).unwrap();
        assert_eq!(partial["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(partial["completeness"], "partial");
    }
    assert_eq!(f.next(&work)["action"], "lead_decision");
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    let repeated = f.call(&["work", "check", &work, "--timeout-ms", "50"], b"");
    assert!(!repeated.status.success());
    let worker = f.repair(&work);
    let permit = f.ok(
        &[
            "work",
            "permit",
            &work,
            worker["assignment"].as_str().unwrap(),
            "--operation",
            "repair-checker",
        ],
        b"",
    );
    assert_eq!(permit["allowed"], true);
    let spent = f.next(&work)["packet"]["context"]["loop"]["spent"].clone();
    fs::write(f.root.join("checker.sh"), "printf repaired; exit 0\n").unwrap();
    f.give(&work, &worker, "completed");
    let check = f.ok(&["work", "check", &work], b"");
    assert_eq!(check["result"]["code"], 0);
    assert_eq!(check["next"]["role"], "reviewer");
    let review = f.next(&work);
    let lead = f.give(&work, &review, "approved")["next"].clone();
    f.give(&work, &lead, "accepted");
    assert_eq!(f.next(&work)["status"], "accepted");
    let events = f.events(&work);
    let recovery = events
        .iter()
        .find(|e| e["operation"] == "authorized_repair_recovery_v1")
        .unwrap();
    assert_eq!(recovery["governorEvent"]["checkpoint"], spent);
    // Rehashed but unauthorised/stale/mismatched proposals still fail canonical validation.
    let index = events.iter().position(|e| e == recovery).unwrap();
    for field in ["dispositionSha256", "assignmentPacketSha256", "inputSha256"] {
        let mut proposal = recovery.clone();
        proposal["governorEvent"][field] = json!("0".repeat(64));
        rehash(&mut proposal["governorEvent"]);
        rehash(&mut proposal);
        let mut prefix = events[..index].to_vec();
        prefix.push(proposal);
        let ledger = format!(".exitbind/runs/invalid-{field}.jsonl");
        fs::write(
            f.root.join(&ledger),
            prefix
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let refused = f.call(&["run", "inspect", &ledger, "--json"], b"");
        assert!(!refused.status.success(), "{field}: {refused:?}");
    }

    assert!(recovery["inputsSha256"] != events[0]["inputsSha256"]);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["action"] == "check_observation_failed")
            .count(),
        1
    );
    assert_eq!(events.iter().filter(|e| e["action"] == "check").count(), 1);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["role"] == "worker" && e["outcome"] == "completed")
            .count(),
        2
    );
}

#[test]
fn sequential_preservation_requirement_uses_its_own_deadline_and_replays_exactly() {
    let f = Fixture::new("printf primary >> .exitbind/attempt-count; exit 0\n");
    let start = f.ok(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "two applicable checks",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "identity:keep the record",
            "--preservation-check-command",
            "printf preservation >> .exitbind/attempt-count; exit 0",
        ],
        b"",
    );
    let work = start["work"].as_str().unwrap();
    let worker = f.give(work, &start["next"], "scoped")["next"].clone();
    f.give(work, &worker, "completed");
    let primary = f.ok(&["work", "check", work, "--timeout-ms", "1000"], b"");
    assert_eq!(primary["result"]["code"], 0);
    let preservation = f.ok(&["work", "check", work, "--timeout-ms", "2000"], b"");
    assert_eq!(preservation["result"]["code"], 0);
    assert!(preservation["eventSha256"] != primary["eventSha256"]);
    let replay = f.ok(&["work", "check", work, "--timeout-ms", "2000"], b"");
    assert_eq!(replay["eventSha256"], preservation["eventSha256"]);
    assert_eq!(replay["recovered"], true);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"primarypreservation"
    );
}

#[test]
fn complete_failure_and_lost_response_replay_never_execute_twice() {
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; exit 7\n");
    let work = f.begin();
    let failed = f.call(&["work", "check", &work], b"");
    assert!(!failed.status.success());
    let first = value(&failed);
    assert_eq!(first["result"]["code"], 7);
    let replay = f.call(&["work", "check", &work], b"");
    assert!(!replay.status.success());
    let replay = value(&replay);
    assert_eq!(first["eventSha256"], replay["eventSha256"]);
    assert_eq!(replay["recovered"], true);
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        1
    );
}

#[test]
fn successful_lost_response_replays_and_changed_limits_and_inputs_refuse() {
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; exit 0\n");
    let work = f.begin();
    let first = f.ok(&["work", "check", &work], b"");
    let replay = f.ok(&["work", "check", &work], b"");
    assert_eq!(replay["eventSha256"], first["eventSha256"]);
    assert_eq!(replay["recovered"], true);
    assert!(!f
        .call(&["work", "check", &work, "--timeout-ms", "1000"], b"")
        .status
        .success());
    fs::write(f.root.join("checker.sh"), "exit 0 # drift\n").unwrap();
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
}

#[test]
fn simultaneous_requests_claim_once_and_unfinished_admission_blocks_supersession() {
    let f = Fixture::new("printf started > .exitbind/started; while [ ! -f .exitbind/release ]; do sleep 0.01; done; exit 0\n");
    let work = f.begin();
    let mut first = f
        .command(&["work", "check", &work, "--timeout-ms", "5000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.root.join(".exitbind/started").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(f.root.join(".exitbind/started").exists());
    let second = f.call(&["work", "check", &work, "--timeout-ms", "5000"], b"");
    assert!(!second.status.success());
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    let relative = f
        .ledger(&work)
        .strip_prefix(&f.root)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let refused = f.call(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "retry",
            "--ledger",
            ".exitbind/runs/successor.jsonl",
        ],
        b"",
    );
    assert!(!refused.status.success());
    assert!(!f.root.join(".exitbind/runs/successor.jsonl").exists());
    fs::write(f.root.join(".exitbind/release"), b"release").unwrap();
    assert!(first.wait().unwrap().success());
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check_observation")
            .count(),
        1
    );
}

fn select_fixture_contract(f: &Fixture) {
    // The combined candidate must retain this blocker with a selected contract.
    fs::write(f.root.join("architecture.json"), json!({"version": 1, "revision": "live-effect-v1",
        "responsibilities": [{"id": "checker", "summary": "owned checker", "paths": ["checker.sh"]}],
        "dependencies": [], "interfaces": [], "checks": []}).to_string()).unwrap();
    let preview = f.ok(
        &[
            "project",
            "architecture",
            "select",
            "architecture.json",
            "--decision",
            "reviewed",
            "--reason",
            "fixture project review",
            "--json",
        ],
        b"",
    );
    let args = preview["apply"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    let applied = support::git_topology::command(args[0])
        .args(&args[1..])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(applied.status.success(), "{applied:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn live_descendant_with_closed_streams_remains_blocked_and_is_owned_by_fixture() {
    if std::env::var("EXITBIND_OBSERVATION_REAPER_FIXTURE").as_deref() != Ok("1") {
        let root = support::temp("observation-reaper-controller");
        let stdout = fs::File::create(root.join("stdout")).unwrap();
        let stderr = fs::File::create(root.join("stderr")).unwrap();
        let mut command = support::git_topology::command(std::env::current_exe().unwrap());
        let mut child = command
            .args([
                "--exact",
                "live_descendant_with_closed_streams_remains_blocked_and_is_owned_by_fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EXITBIND_OBSERVATION_REAPER_FIXTURE", "1")
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!(
                    "isolated fixture deadline; captures retained at {}",
                    root.display()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let diagnostic = fs::read_to_string(root.join("stderr")).unwrap();
        assert!(
            status.success(),
            "{status}; {diagnostic}; captures retained at {}",
            root.display()
        );
        assert!(fs::metadata(root.join("stdout")).unwrap().len() < 65536);
        assert!(diagnostic.len() < 65536);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    struct OwnedDescendant(i32);
    impl Drop for OwnedDescendant {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                let waited = unsafe { libc::waitpid(self.0, std::ptr::null_mut(), libc::WNOHANG) };
                if waited == self.0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            eprintln!("owned descendant cleanup unresolved: {}", self.0);
        }
    }
    let f =
        Fixture::new("sleep 10 >/dev/null 2>&1 & printf '%s' $! > .exitbind/descendant; exit 0\n");
    select_fixture_contract(&f);
    let work = f.begin();
    let output = f.call(&["work", "check", &work], b"");
    let pid: i32 = fs::read_to_string(f.root.join(".exitbind/descendant"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(pid > 0);
    let descendant = OwnedDescendant(pid);
    assert!(!output.status.success(), "{output:?}");
    let output = value(&output);
    let facts = &output["observationFailure"]["facts"];
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["capture"], "complete");
    assert_eq!(facts["groupEnded"], false);
    assert_eq!(facts["termination"], "unknown");
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    assert!(!f
        .call(
            &[
                "work",
                "disposition",
                &work,
                lead["assignment"].as_str().unwrap(),
                "--decision",
                "repair",
                "--reason",
                "unsafe",
                "--repair-boundary",
                "checker.sh",
                "--regression",
                "retry"
            ],
            b""
        )
        .status
        .success());
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    drop(descendant);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

#[cfg(target_os = "linux")]
#[test]
fn escaped_descendant_holding_capture_streams_keeps_termination_unknown() {
    if std::env::var("EXITBIND_ESCAPED_CAPTURE_FIXTURE").as_deref() != Ok("1") {
        let root = support::temp("escaped-capture-controller");
        let mut child = support::git_topology::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "escaped_descendant_holding_capture_streams_keeps_termination_unknown",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EXITBIND_ESCAPED_CAPTURE_FIXTURE", "1")
            .stdout(fs::File::create(root.join("stdout")).unwrap())
            .stderr(fs::File::create(root.join("stderr")).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!(
                    "isolated escaped capture deadline; captures retained at {}",
                    root.display()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let diagnostic = fs::read_to_string(root.join("stderr")).unwrap();
        assert!(
            status.success(),
            "{status}; {diagnostic}; captures at {}",
            root.display()
        );
        assert!(fs::metadata(root.join("stdout")).unwrap().len() < 65536);
        assert!(diagnostic.len() < 65536);
        fs::remove_dir_all(root).unwrap();
        return;
    }
    // Only this isolated test process adopts descendants; the Cargo test
    // runner and other fixtures retain their original process topology.
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    struct OwnedDescendant(i32);
    impl Drop for OwnedDescendant {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if unsafe { libc::waitpid(self.0, std::ptr::null_mut(), libc::WNOHANG) } == self.0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            eprintln!("exact escaped descendant cleanup unresolved: {}", self.0);
        }
    }
    let f = Fixture::new("exec python3 escaped.py\n");
    fs::write(f.root.join("escaped.py"), "import os, time\nfrom pathlib import Path\npid=os.fork()\nif pid == 0:\n    os.setsid()\n    Path('.exitbind/escaped.pid').write_text(str(os.getpid()))\n    time.sleep(10)\n    os._exit(0)\nfor _ in range(200):\n    if Path('.exitbind/escaped.pid').exists():\n        os._exit(0)\n    time.sleep(.005)\nos._exit(1)\n").unwrap();
    select_fixture_contract(&f);
    let work = f.begin();
    // The deadline includes Python startup and fork readiness. Keep it below
    // the escaped child's ten-second lifetime so unfinished readers still
    // exercise unknown termination after the command itself exits successfully.
    let result = f.call(&["work", "check", &work, "--timeout-ms", "1000"], b"");
    let pid: i32 = fs::read_to_string(f.root.join(".exitbind/escaped.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(pid > 0);
    let descendant = OwnedDescendant(pid);
    assert!(!result.status.success(), "{result:?}");
    let failure = value(&result);
    let facts = &failure["observationFailure"]["facts"];
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["groupEnded"], true);
    assert_eq!(facts["captureReadersEnded"], false);
    assert_eq!(facts["capture"], "incomplete");
    assert_eq!(facts["termination"], "unknown");
    assert_eq!(facts["deadlineExceeded"], true);
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    let lead = f.next(&work);
    assert_eq!(lead["outcomes"], json!(["blocked", "rejected"]));
    let before = fs::read(f.ledger(&work)).unwrap();
    let repair = f.call(
        &[
            "work",
            "disposition",
            &work,
            lead["assignment"].as_str().unwrap(),
            "--decision",
            "repair",
            "--reason",
            "unsafe detached reader",
            "--repair-boundary",
            "checker.sh",
            "--regression",
            "retry",
        ],
        b"",
    );
    assert!(!repair.status.success(), "{repair:?}");
    let relative = format!(".exitbind/runs/work-{}.jsonl", &work[4..]);
    let superseded = f.call(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "unsafe successor",
            "--ledger",
            ".exitbind/runs/escaped-successor.jsonl",
        ],
        b"",
    );
    assert!(!superseded.status.success(), "{superseded:?}");
    assert!(!f
        .root
        .join(".exitbind/runs/escaped-successor.jsonl")
        .exists());
    assert!(!f
        .call(&["work", "check", &work, "--timeout-ms", "50"], b"")
        .status
        .success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), before);
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    // Rehashed persisted facts cannot turn unfinished readers into safe
    // termination and thus manufacture repair authority.
    let events = f.events(&work);
    let mut invalid = events.last().unwrap().clone();
    assert_eq!(invalid["action"], "check_observation_failed");
    invalid["observation"]["facts"]["termination"] = json!("ended");
    rehash(&mut invalid);
    let mut prefix = events[..events.len() - 1].to_vec();
    prefix.push(invalid);
    let invalid_ledger = ".exitbind/runs/invalid-reader-termination.jsonl";
    fs::write(
        f.root.join(invalid_ledger),
        prefix
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let invalid = f.call(&["run", "inspect", invalid_ledger, "--json"], b"");
    assert!(!invalid.status.success(), "{invalid:?}");
    drop(descendant);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

#[test]
fn invalid_deadlines_make_no_admission_or_execution() {
    let f = Fixture::new("printf ran > .exitbind/ran\n");
    let work = f.begin();
    let before = f.events(&work);
    for timeout in ["0", "invalid", "18446744073709551616", "86400001"] {
        assert!(!f
            .call(&["work", "check", &work, "--timeout-ms", timeout], b"")
            .status
            .success());
    }
    assert_eq!(f.events(&work), before);
    assert!(!f.root.join(".exitbind/ran").exists());
}

#[test]
fn changed_binding_during_observation_is_failure_and_not_success() {
    let f = Fixture::new("printf changed >> checker.sh; exit 0\n");
    let work = f.begin();
    let failure = f.call(&["work", "check", &work], b"");
    assert!(!failure.status.success());
    let facts = &value(&failure)["observationFailure"]["facts"];
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["storageStage"], "commit");
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    assert_eq!(f.next(&work)["action"], "lead_decision");
}

#[test]
fn storage_failure_preserves_observed_facts_and_unresolved_admission_without_retry() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("chmod u-w .exitbind/runs; printf captured; exit 7\n");
    let work = f.begin();
    let failed = f.call(&["work", "check", &work], b"");
    // Restore only this fixture's storage, even if the assertions below fail.
    fs::set_permissions(
        f.root.join(".exitbind/runs"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(!failed.status.success(), "{failed:?}");
    let failed = value(&failed);
    assert_eq!(failed["effect"], "unknown");
    assert_eq!(failed["durableFailureRecorded"], false);
    assert_eq!(failed["checkRecorded"], false);
    assert_eq!(failed["observation"]["process"]["code"], 7);
    assert_eq!(failed["observation"]["capture"], "complete");
    assert_eq!(failed["observation"]["storageStage"], "commit");
    assert_eq!(
        failed["observation"]["partialCaptures"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(failed["nextAction"]["command"].is_array());
    let next = f.next(&work);
    assert_eq!(next["outcomes"], json!(["blocked", "rejected"]));
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert!(!f
        .call(
            &[
                "work",
                "return",
                &work,
                next["assignment"].as_str().unwrap(),
                "--outcome",
                "rework"
            ],
            b"unsafe retry"
        )
        .status
        .success());
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check_observation")
            .count(),
        1
    );
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|e| e["action"] == "check")
            .count(),
        0
    );
    f.give(&work, &next, "blocked");
}

#[test]
fn replay_refuses_changed_configuration_and_a_new_policy_needs_supersession() {
    let f = Fixture::new("printf ran >> .exitbind/attempt-count; exit 0\n");
    let work = f.begin();
    f.ok(&["work", "check", &work], b"");
    let original = fs::read(f.root.join("exitbind.json")).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    fs::write(f.root.join("exitbind.json"), changed).unwrap();
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    fs::write(f.root.join("exitbind.json"), original).unwrap();
    let relative = f
        .ledger(&work)
        .strip_prefix(&f.root)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!f
        .call(
            &[
                "run",
                "supersede",
                &relative,
                "--workflow",
                "change",
                "--goal",
                "new policy",
                "--ledger",
                ".exitbind/runs/new-policy.jsonl",
                "--check-command",
                "exit 9"
            ],
            b""
        )
        .status
        .success());
    assert!(!f.root.join(".exitbind/runs/new-policy.jsonl").exists());
    assert_eq!(f.ok(&["work", "check", &work], b"")["recovered"], true);
    f.ok(
        &[
            "run",
            "supersede",
            &relative,
            "--workflow",
            "change",
            "--goal",
            "authorized successor with preserved policy",
            "--ledger",
            ".exitbind/runs/preserved-policy.jsonl",
        ],
        b"",
    );
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(
        fs::read(f.root.join(".exitbind/attempt-count")).unwrap(),
        b"ran"
    );
}

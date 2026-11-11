//! Controlled real caller/capsule split; no facts about the original incident.
use super::*;
use sha2::{Digest, Sha256};

struct ReleaseOnDrop(PathBuf);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = fs::write(self.0.join(".exitbind/release"), b"release");
    }
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn start() -> (Fixture, String, String, std::process::Child) {
    let f = Fixture::new("printf run >> .exitbind/launches; printf retained > .exitbind/effect; while [ ! -f .exitbind/release ]; do sleep 0.1; done; printf finished > .exitbind/finished\n");
    let started = f.ok(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "preserve unknown caller",
            "--check-command",
            "exec sh checker.sh",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap().to_owned();
    let worker = f.give(&work, &started["next"], "scoped")["next"].clone();
    let worker_assignment = worker["assignment"].as_str().unwrap().to_owned();
    f.give(&work, &worker, "completed");
    let child = f
        .command(&["work", "check", &work, "--timeout-ms", "60000"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f.root.join(".exitbind/effect").exists() {
        assert!(Instant::now() < deadline, "checker never started");
        std::thread::sleep(Duration::from_millis(25));
    }
    (f, work, worker_assignment, child)
}

pub(super) fn preview(f: &Fixture, work: &str) -> (Value, Value) {
    let result = f.ok(&["work", "report-lost-caller", work, "--json"], b"");
    let preview = result["preview"].clone();
    let mut decision = preview["ownerDecision"].clone();
    decision["approved"] = json!(true);
    decision["reason"] =
        json!("retain the caller-loss evidence without claiming execution results");
    let content = "Synthetic caller handle and interruption report for this test.";
    decision["evidence"] =
        json!([{"source":"controlled fixture", "content":content,"sha256":digest(content)}]);
    (preview, decision)
}

pub(super) fn apply(f: &Fixture, work: &str, decision: &Value) -> Output {
    f.call(
        &[
            "work",
            "report-lost-caller",
            work,
            "--apply",
            "--current-binding",
            decision["currentBinding"].as_str().unwrap(),
            "--json",
        ],
        &serde_json::to_vec(decision).unwrap(),
    )
}

fn release(f: &Fixture) {
    fs::write(f.root.join(".exitbind/release"), b"release").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.root.join(".exitbind/finished").exists() {
        assert!(
            Instant::now() < deadline,
            "controlled checker did not finish"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn real_lost_caller_is_reported_without_native_facts_retry_or_credit() {
    let (f, work, worker_assignment, mut caller) = start();
    let _release = ReleaseOnDrop(f.root.to_path_buf());
    let prefix = fs::read(f.ledger(&work)).unwrap();
    if let Ok(old) = std::env::var("EXITBIND_PRE_RETURN_OLD_READER") {
        let older = support::git_topology::command(old)
            .current_dir(&f.root)
            .args([
                "work",
                "detail",
                &work,
                "--json",
                "--config",
                "exitbind.json",
            ])
            .output()
            .unwrap();
        assert!(
            !older.status.success(),
            "old reader must refuse the unmodified context admission: {older:?}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    } else {
        eprintln!("COVERAGE UNAVAILABLE: old reader not selected for context admission");
    }
    let (view, mut decision) = preview(&f, &work);
    assert_eq!(view["legacyHostAttestationRequired"], false);
    assert!(
        !apply(&f, &work, &decision).status.success(),
        "live caller must refuse"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    caller.kill().unwrap();
    caller.wait().unwrap();
    let retained = fs::read(f.root.join(".exitbind/effect")).unwrap();
    let lead = f.next(&work);
    let before = lead["packet"]["context"]["loop"].clone();
    let invalid = [
        ("/approved", json!(false)),
        ("/currentBinding", json!("a".repeat(64))),
        ("/snapshot/admissionEventSha256", json!("a".repeat(64))),
        ("/hostContext/bootSha256", json!("a".repeat(64))),
        ("/reporter/executableSha256", json!("a".repeat(64))),
        ("/legacySameOriginAttested", json!(true)),
        ("/evidence/0/sha256", json!("a".repeat(64))),
    ];
    for (pointer, replacement) in invalid {
        let mut bad = decision.clone();
        *bad.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            !apply(&f, &work, &bad).status.success(),
            "must refuse {pointer}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    }
    // The alternate fixture binary reports the same version/commit but has
    // different bytes. The preview's exact reporting executable must win.
    let alternate = f.root.join(".exitbind/alternate-exitbind");
    fs::copy(env!("CARGO_BIN_EXE_exitbind"), &alternate).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&alternate)
        .unwrap()
        .write_all(b"synthetic-trailer")
        .unwrap();
    let mut child = support::git_topology::command(&alternate)
        .current_dir(&f.root)
        .args([
            "work",
            "report-lost-caller",
            &work,
            "--apply",
            "--current-binding",
            decision["currentBinding"].as_str().unwrap(),
            "--json",
            "--config",
            "exitbind.json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&decision).unwrap())
        .unwrap();
    assert!(
        !child.wait_with_output().unwrap().status.success(),
        "different reporter bytes must refuse"
    );
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), prefix);
    let applied = apply(&f, &work, &decision);
    assert!(applied.status.success(), "{applied:?}");
    let out = value(&applied);
    assert_eq!(out["execution"], "unknown");
    assert_eq!(out["checkRecorded"], false);
    let events = f.events(&work);
    let report = events.last().unwrap();
    assert_eq!(report["action"], "check_observation_caller_lost");
    assert!(report["observation"].get("facts").is_none());
    assert!(fs::read(f.ledger(&work)).unwrap().starts_with(&prefix));
    assert_eq!(f.next(&work)["packet"]["context"]["loop"], before);
    assert_eq!(fs::read(f.root.join(".exitbind/effect")).unwrap(), retained);
    assert_eq!(fs::read(f.root.join(".exitbind/launches")).unwrap(), b"run");
    let saved = fs::read(f.ledger(&work)).unwrap();
    let replay = apply(&f, &work, &decision);
    assert!(replay.status.success(), "{replay:?}");
    assert_eq!(value(&replay)["effect"], "no-change");
    decision["reason"] = json!("a different decision");
    assert!(!apply(&f, &work, &decision).status.success());
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert!(!f
        .call(
            &[
                "work",
                "permit",
                &work,
                &worker_assignment,
                "--operation",
                "retry",
                "--request-id",
                "no-retry"
            ],
            b""
        )
        .status
        .success());
    let admission = events
        .iter()
        .find(|event| event["action"] == "check_observation")
        .unwrap();
    let target = admission["observation"]["binding"]["targetEventSha256"]
        .as_str()
        .unwrap();
    assert!(!f
        .call(
            &[
                "run",
                "record-check",
                &format!(".exitbind/runs/work-{}.jsonl", &work[4..]),
                "--target",
                target,
                "--check-command",
                "exec sh checker.sh",
                "--exit-code",
                "0"
            ],
            b""
        )
        .status
        .success());
    let action = f.ok(
        &["work", "detail", &work, "--view", "action", "--json"],
        b"",
    );
    let choices = action["effectiveAction"]["choices"].as_array().unwrap();
    assert!(!choices.iter().any(|choice| matches!(
        choice["label"].as_str(),
        Some("environmental" | "repair" | "accepted")
    )));
    let detail = f.ok(&["work", "detail", &work, "--json"], b"");
    assert_eq!(detail["work"], work);
    let resumed = f.ok(&["work", "resume", "--json"], b"");
    assert_eq!(resumed["work"], work);
    assert_eq!(resumed["status"], "resumed");
    let lead = f.next(&work);
    let assignment = lead["assignment"].as_str().unwrap();
    assert!(!f
        .call(
            &[
                "work",
                "disposition",
                &work,
                assignment,
                "--decision",
                "repair",
                "--reason",
                "repair",
                "--repair-boundary",
                "none",
                "--regression",
                "none"
            ],
            b""
        )
        .status
        .success());
    assert!(!f
        .call(
            &["work", "return", &work, assignment, "--outcome", "accepted"],
            b"accept"
        )
        .status
        .success());
    assert!(!f
        .call(
            &["work", "supersede", &work, "--reason", "not allowed"],
            b""
        )
        .status
        .success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), saved);
    assert_eq!(fs::read(f.root.join(".exitbind/launches")).unwrap(), b"run");
    release(&f);
}

#[cfg(target_os = "linux")]
#[test]
fn recorded_context_mismatch_or_missing_start_refuses_even_with_attestation() {
    let (f, work, _, mut caller) = start();
    let _release = ReleaseOnDrop(f.root.to_path_buf());
    caller.kill().unwrap();
    caller.wait().unwrap();
    let mut events = f.events(&work);
    let admission = events.last_mut().unwrap();
    let local_context = admission["observation"]["observer"]["context"].clone();
    admission["observation"]["observer"]["context"]["bootSha256"] = json!("b".repeat(64));
    rehash(admission);
    let mut bytes = Vec::new();
    for event in &events {
        bytes.extend(serde_json::to_vec(event).unwrap());
        bytes.push(b'\n');
    }
    fs::write(f.ledger(&work), &bytes).unwrap();
    let (_, mut decision) = preview(&f, &work);
    assert!(!apply(&f, &work, &decision).status.success());
    decision["legacySameOriginAttested"] = json!(true);
    assert!(!apply(&f, &work, &decision).status.success());
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), bytes);
    let admission = events.last_mut().unwrap();
    admission["observation"]["observer"]["context"] = local_context;
    admission["observation"]["observer"]["startTimeTicks"] = Value::Null;
    rehash(admission);
    let mut missing_start = Vec::new();
    for event in &events {
        missing_start.extend(serde_json::to_vec(event).unwrap());
        missing_start.push(b'\n');
    }
    fs::write(f.ledger(&work), &missing_start).unwrap();
    let (_, decision) = preview(&f, &work);
    assert!(!apply(&f, &work, &decision).status.success());
    assert!(!f.call(&["work", "check", &work], b"").status.success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), missing_start);
    release(&f);
}

#[cfg(target_os = "linux")]
#[test]
fn completed_check_wins_over_stale_report_preview() {
    let (f, work, _, mut caller) = start();
    let _release = ReleaseOnDrop(f.root.to_path_buf());
    let (_, decision) = preview(&f, &work);
    release(&f);
    assert!(caller.wait().unwrap().success());
    let checked = fs::read(f.ledger(&work)).unwrap();
    assert!(f
        .events(&work)
        .iter()
        .any(|event| event["action"] == "check"));
    assert!(!apply(&f, &work, &decision).status.success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), checked);
}

#[cfg(target_os = "linux")]
#[test]
fn concurrent_conflicting_reporters_cannot_append_twice() {
    let (f, work, _, mut caller) = start();
    let _release = ReleaseOnDrop(f.root.to_path_buf());
    caller.kill().unwrap();
    caller.wait().unwrap();
    let (_, first) = preview(&f, &work);
    let mut second = first.clone();
    second["reason"] = json!("another incompatible report");
    let barrier = std::sync::Barrier::new(3);
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            apply(&f, &work, &first)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            apply(&f, &work, &second)
        });
        barrier.wait();
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_ne!(
        a.status.success(),
        b.status.success(),
        "one conflicting decision may commit: {a:?} {b:?}"
    );
    let (winner, loser) = if a.status.success() {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let saved = fs::read(f.ledger(&work)).unwrap();
    assert_eq!(
        f.events(&work)
            .iter()
            .filter(|event| event["action"] == "check_observation_caller_lost")
            .count(),
        1
    );
    assert!(apply(&f, &work, winner).status.success());
    assert!(!apply(&f, &work, loser).status.success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), saved);
    release(&f);
}

#[cfg(target_os = "linux")]
#[test]
fn legacy_host_attestation_is_explicit_and_old_reader_refuses_new_event() {
    let (f, work, _, mut caller) = start();
    let _release = ReleaseOnDrop(f.root.to_path_buf());
    caller.kill().unwrap();
    caller.wait().unwrap();
    let mut events = f.events(&work);
    let admission = events.last_mut().unwrap();
    admission["observation"]["observer"]
        .as_object_mut()
        .unwrap()
        .remove("context");
    rehash(admission);
    let mut original = Vec::new();
    for event in events {
        original.extend(serde_json::to_vec(&event).unwrap());
        original.push(b'\n');
    }
    fs::write(f.ledger(&work), &original).unwrap();
    let old_reader = std::env::var("EXITBIND_PRE_RETURN_OLD_READER").ok();
    if let Some(old) = &old_reader {
        let before = support::git_topology::command(old)
            .current_dir(&f.root)
            .args([
                "work",
                "detail",
                &work,
                "--json",
                "--config",
                "exitbind.json",
            ])
            .output()
            .unwrap();
        assert!(
            before.status.success(),
            "old reader must read the legacy prefix: {before:?}"
        );
    } else {
        eprintln!(
            "COVERAGE UNAVAILABLE: set EXITBIND_PRE_RETURN_OLD_READER to a pinned older executable"
        );
    }
    let (view, mut decision) = preview(&f, &work);
    assert_eq!(view["legacyHostAttestationRequired"], true);
    assert!(!apply(&f, &work, &decision).status.success());
    assert_eq!(fs::read(f.ledger(&work)).unwrap(), original);
    decision["legacySameOriginAttested"] = json!(true);
    let applied = apply(&f, &work, &decision);
    assert!(applied.status.success(), "{applied:?}");
    let saved = fs::read(f.ledger(&work)).unwrap();
    if let Some(old) = old_reader {
        let after = support::git_topology::command(old)
            .current_dir(&f.root)
            .args([
                "work",
                "detail",
                &work,
                "--json",
                "--config",
                "exitbind.json",
            ])
            .output()
            .unwrap();
        assert!(
            !after.status.success(),
            "old reader must refuse the additive action: {after:?}"
        );
        assert_eq!(fs::read(f.ledger(&work)).unwrap(), saved);
    }
    release(&f);
}

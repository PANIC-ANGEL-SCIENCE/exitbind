//! R55 S3: the governor reads a Lead repair as the new information that
//! admits the worker's return.
use super::*;

/// Attempt 1 returns a contradiction and the Lead records a repair.
fn repaired_after_contradiction() -> Fixture {
    let basis = json!({"version": 1, "constraints": ["counter.txt only"],
        "openZones": ["worker implementation"], "decisiveCases": ["exact value"]})
    .to_string();
    // The R54 Work ran its frozen check at return (pre-return).
    let f = Fixture::new_with(&[
        "--basis",
        &basis,
        "--pre-return-command",
        "test \"$(cat counter.txt)\" = correct",
    ]);
    f.edit("environment blocked");
    let assignment = f.detail()["actionForms"]["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    f.ok(
        &[
            "work",
            "return",
            &f.work,
            &assignment,
            "--outcome",
            "contradiction",
            "--reason",
            "The check environment was unavailable",
        ],
        b"The check environment was unavailable; the result is not comparable.",
    );
    let d = f.detail();
    let repair = Fixture::chosen(&d, "repair");
    let filled: Vec<String> = repair["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| match x.as_str().unwrap() {
            "<REASON>" => "The environment failure is repaired; redo the edit".into(),
            "<REPAIR_BOUNDARY>" => "counter.txt only".into(),
            "<REGRESSION>" => "the exact-value check passes".into(),
            x => x.into(),
        })
        .collect();
    let p = support::git_topology::command(&filled[0])
        .args(&filled[1..])
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(p.status.success(), "{p:?}");
    f
}

/// R55 S3 (FR54-11, the R54 case): attempt 1 returns a contradiction, the
/// Lead records a repair, and the attempt 2 permit exhausts the
/// no-information streak. The Lead's recorded repair is the new information:
/// the worker's view offers the return, the attempt 2 result passes its
/// check on the new commit, and no worker re-plan is recorded.
#[test]
fn a_lead_repair_after_a_contradiction_offers_the_return_without_a_replan() {
    let f = repaired_after_contradiction();
    f.edit("correct");
    let d = f.detail();
    assert_eq!(
        d["actionForms"]["recovery"]["state"],
        Value::Null,
        "{}",
        d["actionForms"]
    );
    let labels: Vec<&str> = d["actionForms"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"completed"),
        "{labels:?}: {}",
        d["actionForms"]
    );
    assert!(!labels.contains(&"replan"), "{labels:?}");
    f.choose("completed", b"attempt 2 on the new commit");
    f.check(true);
    let replans = f
        .events()
        .iter()
        .filter(|e| {
            e["governorEvent"]["action"] == "replan"
                && e["operation"] != "authorized_repair_recovery_v1"
        })
        .count();
    assert_eq!(replans, 0, "no worker re-plan was needed");
}

/// R55 F7 (W3 D3): an event appended after the caller's view and before the
/// submission makes the binding check refuse. The return passes its first
/// binding check, then waits on standard input; another call of the same
/// worker records a re-plan meanwhile. The return then refuses as stale and
/// appends nothing. (The pre-F7 code refused this interleaving too; its
/// unlocked window lay between its event count and the repair lock, where
/// no black-box test can stop. That window no longer exists:
/// `only_own_repair` in `src/run/repair_recovery.rs` names the one event.)
#[cfg(target_os = "linux")]
#[test]
fn an_event_after_the_callers_view_makes_the_binding_refuse() {
    let f = repaired_after_contradiction();
    f.edit("correct");
    let d = f.detail();
    let binding = d["actionForms"]["binding"].as_str().unwrap().to_owned();
    let assignment = d["actionForms"]["assignment"].as_str().unwrap().to_owned();
    let mut waiting = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&f.root)
        .args([
            "work",
            "return",
            &f.work,
            &assignment,
            "--outcome",
            "completed",
        ])
        .args(["--current-binding", &binding, "--config", "exitbind.json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = waiting.id();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let wchan = fs::read_to_string(format!("/proc/{pid}/wchan")).unwrap_or_default();
        let children =
            fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default();
        if wchan.contains("pipe_read") && children.trim().is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "return never waited on stdin"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let replan = f.call(
        &[
            "work",
            "replan",
            &f.work,
            &assignment,
            "--hypothesis",
            "Another call records the re-plan first",
            "--scope-decision",
            "counter.txt only",
            "--evidence-request",
            "the exact-value check passes",
        ],
        b"",
    );
    assert!(replan.status.success(), "{replan:?}");
    let before = fs::read(f.ledger()).unwrap();
    waiting
        .stdin
        .take()
        .unwrap()
        .write_all(b"attempt 2 on the new commit")
        .unwrap();
    let refused = waiting.wait_with_output().unwrap();
    assert!(!refused.status.success(), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("current action form binding is stale"),
        "{stderr}"
    );
    assert_eq!(fs::read(f.ledger()).unwrap(), before);
}

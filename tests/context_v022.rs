mod support;

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("context-v022");
        let init = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let config = root.join("exitbind.json");
        Self { root, config }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(&self.config)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn return_body(&self, work: &str, assignment: &str, outcome: &str, body: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(&self.root)
            .args(["work", "return", work, assignment, "--outcome", outcome])
            .arg("--config")
            .arg(&self.config)
            .stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn work_projection_is_thin_exact_and_recoverable_without_replay() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "preserve packet semantics",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    assert!(started["next"]["packet"]["context"]["digest"].is_string());
    let residual = fixture.json(&["work", "next", &work]);
    let context = &residual["residual"]["context"];
    assert_eq!(context["version"], 1);
    assert_eq!(context["role"], "lead");
    assert_eq!(context["run"]["workflow"], "change");
    assert!(context["goal"].is_string());
    assert!(context["obligations"].is_array());
    assert!(context["evidence"].is_array());
    assert!(context["expansions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| {
            item["exact"] == true && item["sha256"].is_string() && item["path"].is_string()
        }));
    assert!(context["digest"].as_str().unwrap().len() == 64);

    // Keep the saved packet under Exitbind's excluded state root so creating
    // the recovery artifact cannot change the tested-input fingerprint.
    let packet_path = fixture.root.join(".exitbind/residual.json");
    fs::write(
        &packet_path,
        serde_json::to_vec(&residual["residual"]).unwrap(),
    )
    .unwrap();
    let packet_path = packet_path.to_str().unwrap();
    let usable = fixture.json(&["work", "validate", &work, "--packet", packet_path]);
    assert_eq!(usable["result"], "usable");

    let mut tampered = residual["residual"].clone();
    tampered["context"]["goal"] = json!("weaker goal");
    fs::write(packet_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
    let refused = fixture.json(&["work", "validate", &work, "--packet", packet_path]);
    assert_eq!(refused["result"], "cannot_establish_applicability");
    assert_eq!(refused["reason"], "malformed_context_projection");

    let assignment = started["next"]["assignment"].as_str().unwrap();
    let mut returner = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    returner
        .current_dir(&fixture.root)
        .args(["work", "return", &work, assignment, "--outcome", "scoped"])
        .arg("--config")
        .arg(&fixture.config)
        .stdin(Stdio::null());
    let returned = returner.output().unwrap();
    assert!(returned.status.success(), "{returned:?}");
    fs::write(
        packet_path,
        serde_json::to_vec(&residual["residual"]).unwrap(),
    )
    .unwrap();
    let stale = fixture.json(&["work", "validate", &work, "--packet", packet_path]);
    assert_eq!(stale["result"], "refresh_required");
    // The canonical packet precedence remains intact: a ledger transition is
    // reported as ledger advancement even though its additive context is also
    // necessarily stale.
    assert_eq!(stale["reason"], "ledger_advanced");

    let resumed = fixture.json(&["work", "resume"]);
    assert_eq!(resumed["status"], "resumed");
    assert!(resumed["residual"]["context"]["digest"].is_string());

    // The cooperative governor is also persisted on the real run ledger: a
    // worker completion through the work façade advances its replayed state,
    // rather than living only in the standalone context reducer fixture.
    let worker = fixture.json(&["work", "next", &work]);
    let worker_assignment = worker["next"]["assignment"].as_str().unwrap();
    let mut completion = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    completion
        .current_dir(&fixture.root)
        .args([
            "work",
            "return",
            &work,
            worker_assignment,
            "--outcome",
            "completed",
        ])
        .arg("--config")
        .arg(&fixture.config)
        .stdin(Stdio::piped());
    let mut child = completion.spawn().unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"worker mutation\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 1);
}

#[test]
fn cooperative_checkpoint_cli_refuses_after_a_replayed_bound() {
    let fixture = Fixture::new();
    let events = fixture.root.join("governor.json");
    fs::write(&events, "[]").unwrap();
    let reduced = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["context", "reduce", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(reduced.status.success(), "{reduced:?}");
    let state: Value = serde_json::from_slice(&reduced.stdout).unwrap();
    assert_eq!(state["spent"], 0);
    assert_eq!(state["state"], "ready");

    let state_path = fixture.root.join("state.json");
    let proposal_path = fixture.root.join("proposal.json");
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    fs::write(&proposal_path, br#"{"unit":"worker-mutation"}"#).unwrap();
    let checkpoint = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&fixture.root)
        .args(["context", "checkpoint", "--state"])
        .arg(&state_path)
        .args(["--proposal"])
        .arg(&proposal_path)
        .output()
        .unwrap();
    assert!(checkpoint.status.success(), "{checkpoint:?}");
    let value: Value = serde_json::from_slice(&checkpoint.stdout).unwrap();
    assert_eq!(value["allowed"], true);
    assert_eq!(value["next"]["unit"], "worker-mutation");
}

#[test]
fn persisted_work_governor_refuses_a_fourth_worker_mutation() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "persist bounded worker mutations",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");

    for round in 0..4 {
        let worker = fixture.json(&["work", "next", &work]);
        let worker_assignment = worker["next"]["assignment"].as_str().unwrap();
        let body = format!("worker round {round}\n");
        let completed = fixture.return_body(&work, worker_assignment, "completed", body.as_bytes());
        if round == 3 {
            assert!(
                !completed.status.success(),
                "fourth worker mutation bypassed bound"
            );
            let ledger = format!(
                ".exitbind/runs/work-{}.jsonl",
                work.strip_prefix("smw_").unwrap()
            );
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], 3);
            assert_eq!(inspected["governor"]["state"], "checkpoint_required");
            break;
        }
        assert!(completed.status.success(), "{completed:?}");
        fixture.json(&["work", "check", &work]);
        let reviewer = fixture.json(&["work", "next", &work]);
        let reviewed = fixture.return_body(
            &work,
            reviewer["next"]["assignment"].as_str().unwrap(),
            "approved",
            b"review\n",
        );
        assert!(reviewed.status.success(), "{reviewed:?}");
        let accepting = fixture.json(&["work", "next", &work]);
        let reworked = fixture.return_body(
            &work,
            accepting["next"]["assignment"].as_str().unwrap(),
            "rework",
            b"rework\n",
        );
        assert!(reworked.status.success(), "{reworked:?}");
        let ledger = format!(
            ".exitbind/runs/work-{}.jsonl",
            work.strip_prefix("smw_").unwrap()
        );
        let inspected = fixture.json(&["run", "inspect", &ledger]);
        assert_eq!(inspected["governor"]["spent"], round + 1);
    }
}

#[test]
fn public_pre_mutation_permit_persists_before_product_edit_and_refuses_fourth() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "consume pre-mutation units atomically",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let mut tampered_assignment = assignment.clone();
    tampered_assignment.pop();
    tampered_assignment.push(if assignment.ends_with('0') { '1' } else { '0' });
    assert_ne!(tampered_assignment, assignment);
    let tampered = fixture.call(&[
        "work",
        "permit",
        &work,
        &tampered_assignment,
        "--operation",
        "edit",
    ]);
    assert!(
        !tampered.status.success(),
        "tampered assignment was accepted"
    );
    assert_eq!(
        fixture.json(&["run", "inspect", &ledger])["governor"]["spent"],
        0
    );
    for unit in 1..=4 {
        let before_permit = fs::read(fixture.root.join(&ledger)).unwrap();
        let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
        if unit < 4 {
            assert!(permit.status.success(), "{permit:?}");
            let value: Value = serde_json::from_slice(&permit.stdout).unwrap();
            assert_eq!(value["allowed"], true);
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], unit);
            fs::write(
                fixture.root.join("product.txt"),
                format!("permitted product edit {unit}\n"),
            )
            .unwrap();
            let resumed = fixture.json(&["work", "resume"]);
            assert_eq!(resumed["status"], "resumed");
        } else {
            assert!(!permit.status.success(), "fourth permit bypassed the bound");
            assert_eq!(
                fs::read(fixture.root.join("product.txt")).unwrap(),
                b"permitted product edit 3\n"
            );
            assert_eq!(fs::read(fixture.root.join(&ledger)).unwrap(), before_permit);
            let inspected = fixture.json(&["run", "inspect", &ledger]);
            assert_eq!(inspected["governor"]["spent"], 3);
            assert_eq!(inspected["governor"]["state"], "checkpoint_required");
        }
    }

    // The owning action refuses cross-run and malformed operation inputs
    // before it reaches the ledger append boundary.
    let other = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "cross-run permit refusal",
        "--check-command",
        "true",
    ]);
    let cross = fixture.call(&[
        "work",
        "permit",
        other["work"].as_str().unwrap(),
        &assignment,
        "--operation",
        "edit",
    ]);
    assert!(!cross.status.success());
    let malformed = fixture.call(&["work", "permit", &work, &assignment, "--operation", ""]);
    assert!(!malformed.status.success());
}

#[test]
fn pre_mutation_permits_are_not_double_counted_by_worker_completion() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "work",
        "begin",
        "change",
        "--goal",
        "complete after cooperative mutation permits",
        "--check-command",
        "true",
    ]);
    let work = started["work"].as_str().unwrap().to_owned();
    let scoped = fixture.return_body(
        &work,
        started["next"]["assignment"].as_str().unwrap(),
        "scoped",
        b"scope\n",
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let worker = fixture.json(&["work", "next", &work]);
    let assignment = worker["next"]["assignment"].as_str().unwrap().to_owned();
    let ledger = format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    );

    // Three independent CLI calls authorize product edits before any worker
    // completion. The third leaves the canonical governor at its hard bound.
    for unit in 1..=3 {
        let permit = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
        assert!(permit.status.success(), "{permit:?}");
        let inspected = fixture.json(&["run", "inspect", &ledger]);
        assert_eq!(inspected["governor"]["spent"], unit);
        fs::write(
            fixture.root.join("product.txt"),
            format!("permitted completion edit {unit}\n"),
        )
        .unwrap();
        let resumed = fixture.json(&["work", "resume"]);
        assert_eq!(resumed["status"], "resumed");
    }

    // Completion acknowledges the already-authorized third edit. It is a
    // fallback governor unit only when no pre-mutation permit exists, so this
    // valid result must not become an unreducible fourth mutation.
    let completed = fixture.return_body(&work, &assignment, "completed", b"worker completion\n");
    assert!(completed.status.success(), "{completed:?}");
    let inspected = fixture.json(&["run", "inspect", &ledger]);
    assert_eq!(inspected["governor"]["spent"], 3);
    let stale = fixture.call(&["work", "permit", &work, &assignment, "--operation", "edit"]);
    assert!(!stale.status.success(), "stale assignment was accepted");
}

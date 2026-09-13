mod support;

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const CHECK: &str = "test -f actual-product-check";

struct Fixture {
    root: PathBuf,
    ledger: String,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("checked UX's $(touch SHOULD_NOT_EXIST)");
        let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(stdout(&output).contains("--check-command"));
        Self {
            root,
            ledger: ".soulmate/runs/task '$(touch SHOULD_NOT_EXIST).jsonl".into(),
        }
    }

    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .current_dir(&self.root)
            .args(args)
            .arg("--config")
            .arg(self.root.join("soulmate.json"))
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn start(&self, checked: bool) -> Output {
        let mut args = vec![
            "run",
            "start",
            "change",
            "--goal",
            "Repair the product; keep prior work.",
            "--ledger",
            &self.ledger,
        ];
        if checked {
            args.extend(["--check-command", CHECK]);
        }
        let output = self.call(&args);
        assert!(output.status.success(), "{output:?}");
        let _: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(String::from_utf8_lossy(&output.stderr)
            .to_lowercase()
            .contains("check"));
        output
    }

    fn submit(&self, actor: &str, outcome: &str, name: &str, scalar: bool) -> Output {
        let path = format!(".soulmate/artifacts/{name}.md");
        fs::write(self.root.join(&path), format!("Private {name} body\n")).unwrap();
        let mut args = vec![
            "run",
            "submit",
            actor,
            &self.ledger,
            "--outcome",
            outcome,
            "--artifact",
            &path,
            "--artifact-root",
            "state",
        ];
        if scalar {
            args.push("--event-id");
        }
        self.call(&args)
    }

    fn advance(&self, actor: &str, outcome: &str, name: &str) -> Value {
        let output = self.submit(actor, outcome, name, false);
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn record(&self, target: &str, command: &str, exit: &str) -> Output {
        self.call(&[
            "run",
            "record-check",
            &self.ledger,
            "--target",
            target,
            "--check-command",
            command,
            "--exit-code",
            exit,
        ])
    }

    fn bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(&self.ledger)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn captured_submission_identity_stays_bound_through_rework_and_fresh_process_resume() {
    let f = Fixture::new();
    f.start(true);
    f.advance("lead", "scoped", "scope");
    let first = f.submit("worker", "completed", "first", true);
    assert!(first.status.success(), "{first:?}");
    let target = stdout(&first).trim().to_owned();
    assert_eq!(target.len(), 64);
    assert!(target.bytes().all(|b| b.is_ascii_hexdigit()));
    let event: Value = serde_json::from_slice(
        f.bytes()
            .split(|b| *b == b'\n')
            .rfind(|x| !x.is_empty())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(event["eventSha256"], target);
    f.advance("reviewer", "approved", "review-first");
    assert!(!f
        .submit("lead", "accepted", "missing-refused", false)
        .status
        .success());
    let before = f.bytes();
    assert!(!f.record(&target, "different command", "0").status.success());
    assert_eq!(f.bytes(), before);
    assert!(f.record(&target, CHECK, "1").status.success());
    assert!(!f
        .submit("lead", "accepted", "failed-refused", false)
        .status
        .success());
    f.advance("lead", "rework", "repair-request");
    let before = f.bytes();
    let resumed = f.call(&["run", "next", &f.ledger, "--text"]);
    assert!(resumed.status.success(), "{resumed:?}");
    let text = stdout(&resumed);
    for expected in [
        "Repair the product",
        "worker",
        "first.md",
        "repair-request.md",
        CHECK,
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(text.to_lowercase().contains("prior"));
    assert!(!text.contains("Private first body"));
    assert_eq!(f.bytes(), before);
    let next = f.ok(&["run", "next", &f.ledger]);
    assert_eq!(next["attempt"], 2);
    assert_eq!(
        next["assignments"][0]["upstreamArtifacts"][1]["attemptStatus"],
        "prior"
    );
    let second = f.submit("worker", "completed", "second", true);
    assert!(second.status.success());
    let fresh_target = stdout(&second).trim().to_owned();
    assert_ne!(target, fresh_target);
    let before = f.bytes();
    assert!(!f.record(&target, CHECK, "0").status.success());
    assert_eq!(f.bytes(), before);
    assert!(f.record(&fresh_target, CHECK, "0").status.success());
    f.advance("reviewer", "approved", "review-second");
    let final_event = f.advance("lead", "accepted", "accepted");
    assert_eq!(final_event["status"], "accepted");
    assert_eq!(
        fs::read_to_string(f.root.join(".soulmate/artifacts/first.md")).unwrap(),
        "Private first body\n"
    );
    let next = f.call(&["run", "next", &f.ledger, "--text"]);
    assert!(next.status.success());
    assert!(stdout(&next).contains("accepted"));
    assert_eq!(
        f.ok(&["run", "next", &f.ledger, "--json"])["assignments"],
        json!([])
    );
}

#[test]
fn output_conflicts_fail_before_submission_and_failed_scalar_capture_is_empty() {
    let f = Fixture::new();
    f.start(true);
    fs::write(f.root.join("scope.md"), "scope").unwrap();
    let before = f.bytes();
    let conflict = f.call(&[
        "run",
        "submit",
        "lead",
        &f.ledger,
        "--outcome",
        "scoped",
        "--artifact",
        "scope.md",
        "--event-id",
        "--json",
    ]);
    assert!(!conflict.status.success());
    assert_eq!(f.bytes(), before);
    let conflict = f.call(&["run", "next", &f.ledger, "--text", "--json"]);
    assert!(!conflict.status.success());
    assert_eq!(f.bytes(), before);
    let failed = f.submit("worker", "completed", "too-early", true);
    assert!(!failed.status.success());
    assert!(
        failed.stdout.is_empty(),
        "failed scalar output looks like a target: {failed:?}"
    );
    assert_eq!(f.bytes(), before);
}

#[test]
fn unchecked_runs_stay_compatible_and_captured_ids_cannot_cross_runs() {
    let f = Fixture::new();
    f.start(false);
    let status = f.ok(&["run", "status", &f.ledger, "--json"]);
    assert_eq!(status["checks"]["status"], "not_configured");
    f.advance("lead", "scoped", "scope");
    f.advance("worker", "completed", "worker");
    f.advance("reviewer", "approved", "review");
    assert_eq!(
        f.advance("lead", "accepted", "accept")["status"],
        "accepted"
    );
    let a = Fixture::new();
    let b = Fixture::new();
    for fixture in [&a, &b] {
        fixture.start(true);
        fixture.advance("lead", "scoped", "scope");
    }
    let first = a.submit("worker", "completed", "worker", true);
    let second = b.submit("worker", "completed", "worker", true);
    assert!(first.status.success() && second.status.success());
    let before = b.bytes();
    assert!(!b.record(stdout(&first).trim(), CHECK, "0").status.success());
    assert_eq!(b.bytes(), before);
}

#[cfg(unix)]
fn follow_hint(f: &Fixture, label: &str) -> String {
    let status = f.call(&["run", "status", &f.ledger]);
    assert!(status.status.success(), "{status:?}");
    let text = stdout(&status);
    let command = text
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("missing {label}: {text}"));
    // Use an isolated PATH alias to exercise the printed command exactly.
    let bin_dir = f.root.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    if !bin_dir.join("soulmate").exists() {
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_soulmate"), bin_dir.join("soulmate"))
            .unwrap();
    }
    let result = Command::new("/bin/sh")
        .args(["-c", command])
        .env("PATH", &bin_dir)
        .current_dir(&f.root)
        .output()
        .unwrap();
    assert!(result.status.success(), "hint {command}: {result:?}");
    assert!(!f.root.join("SHOULD_NOT_EXIST").exists());
    stdout(&result)
}

#[cfg(unix)]
#[test]
fn quoted_next_hints_execute_as_data_and_drift_or_terminal_guides_to_inspect() {
    let f = Fixture::new();
    f.start(true);
    let before = f.bytes();
    assert!(follow_hint(&f, "Next: ").contains("lead"));
    assert_eq!(f.bytes(), before);
    f.advance("lead", "scoped", "scope");
    fs::write(f.root.join(".soulmate/artifacts/scope.md"), "drift").unwrap();
    let before = f.bytes();
    let next = f.call(&["run", "next", &f.ledger, "--text"]);
    assert!(!next.status.success());
    assert!(follow_hint(&f, "Inspect: ").contains("submissions"));
    assert_eq!(f.bytes(), before);
    fs::write(
        f.root.join(".soulmate/artifacts/scope.md"),
        "Private scope body\n",
    )
    .unwrap();
    f.advance("worker", "blocked", "blocked");
    let before = f.bytes();
    assert!(follow_hint(&f, "Inspect: ").contains("blocked"));
    assert_eq!(f.bytes(), before);
}

#[cfg(unix)]
#[test]
fn public_checked_work_demo_runs_with_the_built_binary() {
    let output = Command::new("sh")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/demo-checked-work.sh"
        ))
        .env("SOULMATE_BIN", env!("CARGO_BIN_EXE_soulmate"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    for line in [
        "Attempt 1: host check failed; acceptance refused.",
        "Attempt 2: host check passed; separately reviewed and accepted.",
        "Earlier worker artifact retained; new process reconstructed the assignment.",
    ] {
        assert!(text.contains(line), "{text}");
    }
}

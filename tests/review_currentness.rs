#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{fs, path::PathBuf, process::Output};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::temp("review-currentness");
        support::git_topology::repository(&root);
        let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .args(["init", "--mode", "portable", "--root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        fs::write(root.join("marker"), b"pass\n").unwrap();
        Self { root }
    }

    fn call(&self, args: &[&str]) -> Output {
        support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn artifact(&self, name: &str) -> &'static str {
        let path = format!(".exitbind/artifacts/{name}.md");
        fs::write(self.root.join(&path), format!("{name}\n")).unwrap();
        Box::leak(path.into_boxed_str())
    }
}

#[test]
fn stale_review_is_hidden_after_input_check_rerun_and_artifact_loss() {
    let fixture = Fixture::new();
    let ledger = ".exitbind/runs/review-currentness.jsonl";
    fixture.json(&[
        "run",
        "start",
        "change",
        "--goal",
        "review currentness",
        "--ledger",
        ledger,
        "--check-command",
        "test -f marker",
        "--proof-origin",
        "synthetic",
        "--config",
        "exitbind.json",
    ]);
    fixture.json(&[
        "run",
        "submit",
        "lead",
        ledger,
        "--outcome",
        "scoped",
        "--artifact",
        fixture.artifact("scope"),
        "--artifact-root",
        "state",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    let worker = fixture.json(&[
        "run",
        "submit",
        "worker",
        ledger,
        "--outcome",
        "completed",
        "--artifact",
        fixture.artifact("worker"),
        "--artifact-root",
        "state",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    let target = worker["event"]["eventSha256"].as_str().unwrap();
    fixture.json(&[
        "run",
        "record-check",
        ledger,
        "--target",
        target,
        "--check-command",
        "test -f marker",
        "--exit-code",
        "0",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    fixture.json(&[
        "run",
        "submit",
        "reviewer",
        ledger,
        "--outcome",
        "approved",
        "--artifact",
        fixture.artifact("reviewer"),
        "--artifact-root",
        "state",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    assert_eq!(
        fixture.json(&[
            "run",
            "status",
            ledger,
            "--json",
            "--config",
            "exitbind.json"
        ])["review"]["status"],
        "approved"
    );

    let reviewer_path = fixture.root.join(".exitbind/artifacts/reviewer.md");
    let reviewer_bytes = fs::read(&reviewer_path).unwrap();
    fs::remove_file(&reviewer_path).unwrap();
    let stale = fixture.json(&[
        "run",
        "status",
        ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(stale["review"]["status"], "stale");
    let next = fixture.json(&["run", "next", ledger, "--config", "exitbind.json"]);
    assert_eq!(next["progress"]["components"]["review"]["completed"], 0);
    assert_eq!(next["progress"]["components"]["review"]["earned"], 0);
    let refused = fixture.call(&[
        "run",
        "submit",
        "lead",
        ledger,
        "--outcome",
        "accepted",
        "--artifact",
        fixture.artifact("acceptance-before-restore"),
        "--artifact-root",
        "state",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("artifact drift detected"));

    fs::write(&reviewer_path, reviewer_bytes).unwrap();
    let recovered = fixture.json(&[
        "run",
        "status",
        ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(recovered["review"]["status"], "approved");
    let next = fixture.json(&["run", "next", ledger, "--config", "exitbind.json"]);
    assert_eq!(next["progress"]["components"]["review"]["completed"], 1);
    assert_eq!(next["progress"]["components"]["review"]["earned"], 20);

    fixture.json(&[
        "run",
        "record-check",
        ledger,
        "--target",
        target,
        "--check-command",
        "test -f marker",
        "--exit-code",
        "0",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    let rerun = fixture.json(&[
        "run",
        "status",
        ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(rerun["checks"]["status"], "passed");
    assert_eq!(rerun["review"]["status"], "stale");

    fs::write(fixture.root.join("marker"), b"changed\n").unwrap();
    let stale = fixture.json(&[
        "run",
        "status",
        ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(stale["review"]["status"], "stale");

    fixture.json(&[
        "run",
        "record-check",
        ledger,
        "--target",
        target,
        "--check-command",
        "test -f marker",
        "--exit-code",
        "0",
        "--config",
        "exitbind.json",
        "--json",
    ]);
    fs::remove_file(fixture.root.join(".exitbind/artifacts/reviewer.md")).unwrap();
    let rerun = fixture.json(&[
        "run",
        "status",
        ledger,
        "--json",
        "--config",
        "exitbind.json",
    ]);
    assert_eq!(rerun["checks"]["status"], "passed");
    assert_eq!(rerun["review"]["status"], "stale");

    let human = fixture.call(&["run", "status", ledger, "--config", "exitbind.json"]);
    assert!(human.status.success(), "{}", text(&human));
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(
        human.contains("Reviewer outcome: reviewer (reviewer) stage 3 pending"),
        "{human}"
    );
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

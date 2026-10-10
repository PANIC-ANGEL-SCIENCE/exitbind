// Current Exitbind and preserved historical-reader contracts run in the default suite.
use serde_json::{json, Value};
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CHECK: &str = "exitbind check --config verification.json";

fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let output = invoke_exitbind(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(output.status.success(), "{}", text(&output));
    root
}

fn exitbind_project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let output = invoke_exitbind(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(output.status.success(), "{}", text(&output));
    root
}

fn invoke_exitbind(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("exitbind binary should start")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; output: {}", text(output)))
}

fn assert_operational_receipt_error(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{}", text(output));
    let value = json_output(output);
    assert!(value["error"].as_str().is_some(), "{value}");
    assert!(value["outcome"].is_null(), "{value}");
    assert!(value["reason"].is_null(), "{value}");
}

fn state_artifact(root: &Path, name: &str, content: &str) -> String {
    let path = root.join(".exitbind/artifacts").join(name);
    fs::write(path, content).expect("artifact should be written");
    format!(".exitbind/artifacts/{name}")
}

fn configure_workers(root: &Path, workers: &[&str]) {
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let base = config["agents"]["worker"].clone();
    for worker in workers.iter().copied().filter(|worker| *worker != "worker") {
        let mut agent = base.clone();
        agent["profile"] = json!(format!("exitbind/agents/{worker}.md"));
        agent["purpose"] = json!(format!("Complete bounded work for {worker}."));
        config["agents"][worker] = agent;
        fs::write(
            root.join(format!("exitbind/agents/{worker}.md")),
            format!("# {worker}\n\nComplete bounded work.\n"),
        )
        .unwrap();
    }
    config["workflows"]["change"]["workers"] = json!(workers);
    fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

fn submit(root: &Path, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Value {
    let output = invoke_exitbind(
        root,
        &[
            "run",
            "submit",
            agent,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    json_output(&output)
}

fn checked_start(root: &Path, ledger: &str) {
    let output = invoke_exitbind(
        root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "receipt test",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            "synthetic",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
}

fn record_check(root: &Path, ledger: &str, target: &str) {
    let output = invoke_exitbind(
        root,
        &[
            "run",
            "record-check",
            ledger,
            "--target",
            target,
            "--check-command",
            CHECK,
            "--exit-code",
            "0",
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
}

fn worker_target(submission: &Value) -> String {
    submission["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn accepted_v5(root: &Path, ledger: &str, workers: &[&str]) -> String {
    configure_workers(root, workers);
    checked_start(root, ledger);
    let lead = state_artifact(root, "receipt-lead.md", "scope\n");
    submit(root, "lead", ledger, "scoped", &lead);
    let mut targets = Vec::new();
    for worker in workers {
        let artifact = state_artifact(root, &format!("receipt-{worker}.md"), "worker\n");
        let result = submit(root, worker, ledger, "completed", &artifact);
        targets.push(worker_target(&result));
    }
    for target in targets {
        record_check(root, ledger, &target);
    }
    let reviewer = state_artifact(root, "receipt-reviewer.md", "review\n");
    submit(root, "reviewer", ledger, "approved", &reviewer);
    let acceptance = state_artifact(root, "receipt-acceptance.md", "accepted\n");
    submit(root, "lead", ledger, "accepted", &acceptance);
    let output = invoke_exitbind(
        root,
        &[
            "receipt",
            ledger,
            "--json",
            "--output",
            ".exitbind/receipts/exit.json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    ".exitbind/receipts/exit.json".to_owned()
}

#[test]
fn v5_acceptance_requires_review_and_receipt_verification_fails_closed() {
    let root = project("value-receipt-review");
    let ledger = ".exitbind/runs/review.jsonl";
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["workflows"]["change"]["reviewers"] = json!([]);
    fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    checked_start(&root, ledger);
    let lead = state_artifact(&root, "missing-review-lead.md", "scope\n");
    submit(&root, "lead", ledger, "scoped", &lead);
    let worker = state_artifact(&root, "missing-review-worker.md", "worker\n");
    let worker_event = submit(&root, "worker", ledger, "completed", &worker);
    record_check(&root, ledger, &worker_target(&worker_event));
    let acceptance = state_artifact(&root, "missing-review-acceptance.md", "accepted\n");
    let refused = invoke_exitbind(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "accepted",
            "--artifact",
            &acceptance,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("canonical acceptance requires reviewer approval"),
        "{}",
        text(&refused)
    );

    let valid_root = project("value-receipt-valid");
    let receipt = accepted_v5(&valid_root, ".exitbind/runs/accepted.jsonl", &["worker"]);
    let valid = invoke_exitbind(
        &valid_root,
        &["verify", &receipt, "--json", "--config", "exitbind.json"],
    );
    assert!(valid.status.success(), "{}", text(&valid));
    let valid_value = json_output(&valid);
    assert_eq!(valid_value["valid"], true);
    assert_eq!(valid_value["outcome"], "READY");

    let receipt_path = valid_root.join(&receipt);
    let original: Value = serde_json::from_slice(&fs::read(receipt_path).unwrap()).unwrap();
    for (name, review) in [
        ("missing-review.json", Value::Null),
        (
            "fabricated-review.json",
            json!({"eventSha256": "0".repeat(64), "artifactSha256": "1".repeat(64)}),
        ),
    ] {
        let mut mutated = original.clone();
        if review.is_null() {
            mutated.as_object_mut().unwrap().remove("review");
        } else {
            mutated["review"] = review;
        }
        let path = format!(".exitbind/receipts/{name}");
        fs::write(
            valid_root.join(&path),
            serde_json::to_vec_pretty(&mutated).unwrap(),
        )
        .unwrap();
        let rejected =
            invoke_exitbind(&valid_root, &["verify", &path, "--config", "exitbind.json"]);
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert!(text(&rejected).contains("receipt review binding changed"));
    }
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(valid_root).unwrap();
}

#[test]
fn v5_receipt_binds_all_current_worker_artifacts() {
    let root = project("value-receipt-artifacts");
    let receipt = accepted_v5(&root, ".exitbind/runs/multi.jsonl", &["worker", "worker2"]);
    let value: Value = serde_json::from_slice(&fs::read(root.join(receipt)).unwrap()).unwrap();
    assert_eq!(value["outcome"], "READY");
    assert_eq!(value["reason"]["code"], "accepted");
    let artifacts = value["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 4);
    for expected in [
        ".exitbind/artifacts/receipt-worker.md",
        ".exitbind/artifacts/receipt-worker2.md",
        ".exitbind/artifacts/receipt-reviewer.md",
        ".exitbind/artifacts/receipt-acceptance.md",
    ] {
        assert!(artifacts
            .iter()
            .any(|artifact| artifact["path"] == expected));
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn operational_receipt_failures_do_not_emit_lifecycle_labels() {
    let root = project("value-receipt-operational-errors");
    for ledger in [
        ".exitbind/runs/absent.jsonl",
        ".exitbind/runs/malformed.jsonl",
        ".exitbind/runs/unloadable.jsonl",
    ] {
        let path = root.join(ledger);
        if ledger.contains("malformed") {
            fs::write(&path, b"not-json\n").unwrap();
        } else if ledger.contains("unloadable") {
            fs::create_dir(&path).unwrap();
        }
        let output = invoke_exitbind(
            &root,
            &["receipt", ledger, "--json", "--config", "exitbind.json"],
        );
        assert_operational_receipt_error(&output);
        if path.is_dir() {
            fs::remove_dir(path).unwrap();
        } else if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn exitbind_verify_operational_receipt_failures_do_not_emit_lifecycle_labels() {
    let root = exitbind_project("value-receipt-exitbind-operational-errors");
    for receipt in [
        ".exitbind/receipts/absent.json",
        ".exitbind/receipts/malformed.json",
        ".exitbind/receipts/unloadable.json",
    ] {
        let path = root.join(receipt);
        if receipt.contains("malformed") {
            fs::write(&path, b"not-json\n").unwrap();
        } else if receipt.contains("unloadable") {
            fs::create_dir(&path).unwrap();
        }
        let output = invoke_exitbind(&root, &["verify", receipt, "--config", "exitbind.json"]);
        assert_operational_receipt_error(&output);
        if path.is_dir() {
            fs::remove_dir(path).unwrap();
        } else if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }

    fs::remove_file(root.join("exitbind.json")).unwrap();
    let output = invoke_exitbind(
        &root,
        &[
            "verify",
            ".exitbind/receipts/absent.json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_operational_receipt_error(&output);
    fs::remove_dir_all(root).unwrap();
}

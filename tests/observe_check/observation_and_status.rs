use super::*;

#[test]
fn observes_frozen_command_in_product_root_and_records_provenance() {
    let root = support::temp("observe-check");
    support::git_topology::repository(&root);
    let config = root.join("exitbind.json");
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    let ledger = ".exitbind/runs/observed.jsonl";
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "observe",
            "--ledger",
            ledger,
            "--check-command",
            "printf child-stdout; printf child-stderr >&2; printf observed > .exitbind/artifacts/observed.txt; pwd > .exitbind/artifacts/observed.cwd",
            "--config",
            "exitbind.json",
        ],
    );
    fs::write(root.join(".exitbind/artifacts/worker.md"), "worker\n").unwrap();
    run(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "scoped",
            "--artifact",
            ".exitbind/artifacts/worker.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    let worker = run(
        &root,
        &[
            "run",
            "submit",
            "worker",
            ledger,
            "--outcome",
            "completed",
            "--artifact",
            ".exitbind/artifacts/worker.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    let target = worker["event"]["eventSha256"].as_str().unwrap();
    let observed_output = call(
        &root,
        &[
            "run",
            "observe-check",
            ledger,
            "--target",
            target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(
        observed_output.status.success(),
        "{}",
        text(&observed_output)
    );
    let observed = json(&observed_output);
    assert_eq!(observed["event"]["version"], 8);
    assert_eq!(observed["event"]["acquisition"], "observed");
    assert_eq!(observed["event"]["result"]["kind"], "exit");
    assert_eq!(observed["event"]["result"]["code"], 0);
    assert_eq!(observed["checks"]["targets"][0]["acquisition"], "observed");
    assert_eq!(observed["checks"]["targets"][0]["result"]["code"], 0);
    assert!(!String::from_utf8_lossy(&observed_output.stdout).starts_with("child-stdout"));
    for (stream, expected) in [("stdout", b"child-stdout"), ("stderr", b"child-stderr")] {
        let artifact = &observed["event"][stream];
        assert_eq!(artifact["root"], "state");
        let bytes = fs::read(root.join(artifact["path"].as_str().unwrap())).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(artifact["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(artifact["bytes"], bytes.len());
        assert!(!String::from_utf8_lossy(&observed_output.stderr)
            .contains(std::str::from_utf8(expected).unwrap()));
    }
    let human = call(
        &root,
        &["run", "status", ledger, "--config", "exitbind.json"],
    );
    assert!(human.status.success(), "{}", text(&human));
    assert!(text(&human).contains("Locally observed check: passed"));
    assert!(text(&human).contains("acquisition=observed"));
    assert!(text(&human).contains("kind\":\"exit\""));
    let report = run(
        &root,
        &[
            "run",
            "report",
            ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(report["groups"]["local_report"]["durationMsReported"], 0);
    assert_eq!(
        report["groups"]["local_report"]["durationMsReportedCount"],
        0
    );
    assert_eq!(
        fs::read_to_string(root.join(".exitbind/artifacts/observed.txt")).unwrap(),
        "observed"
    );
    assert_eq!(
        fs::read_to_string(root.join(".exitbind/artifacts/observed.cwd"))
            .unwrap()
            .trim(),
        root.canonicalize().unwrap().to_str().unwrap()
    );

    let before = fs::read(root.join(ledger)).unwrap();
    let rejected = call(
        &root,
        &[
            "run",
            "observe-check",
            ledger,
            "--target",
            "not-a-target",
            "--config",
            config.to_str().unwrap(),
        ],
    );
    assert!(!rejected.status.success());
    assert_eq!(fs::read(root.join(ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn records_signal_without_fabricating_exit_code() {
    let root = support::temp("observe-signal");
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    let ledger = ".exitbind/runs/signal.jsonl";
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "observe",
            "--ledger",
            ledger,
            "--check-command",
            "kill -TERM $$",
            "--config",
            "exitbind.json",
        ],
    );
    fs::write(root.join(".exitbind/artifacts/worker.md"), "worker\n").unwrap();
    run(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "scoped",
            "--artifact",
            ".exitbind/artifacts/worker.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    let worker = run(
        &root,
        &[
            "run",
            "submit",
            "worker",
            ledger,
            "--outcome",
            "completed",
            "--artifact",
            ".exitbind/artifacts/worker.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    let target = worker["event"]["eventSha256"].as_str().unwrap();
    let observed = run(
        &root,
        &[
            "run",
            "observe-check",
            ledger,
            "--target",
            target,
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(observed["event"]["result"]["kind"], "signal");
    assert_eq!(observed["event"]["result"]["signal"], 15);
    assert!(observed["event"]["result"].get("code").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nonzero_observation_is_recorded_and_acceptance_is_refused() {
    let (root, ledger, target) = checked_worker("observe-nonzero", "exit 7");
    let observed = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(observed.status.success(), "{}", text(&observed));
    let value = json(&observed);
    assert_eq!(value["event"]["result"]["code"], 7);

    let reviewer = call(
        &root,
        &[
            "run",
            "submit",
            "reviewer",
            &ledger,
            "--outcome",
            "approved",
            "--artifact",
            ".exitbind/artifacts/observe-nonzero.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(
        !reviewer.status.success(),
        "reviewer approval bypassed failed check: {}",
        text(&reviewer)
    );
    let pending = run(
        &root,
        &[
            "run",
            "status",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    let next = run(
        &root,
        &[
            "run",
            "next",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(next["assignments"][0]["role"], "lead");
    assert_eq!(next["currentStage"], pending["stage"]);
    assert_eq!(pending["checks"]["status"], "blocked");
    assert_eq!(pending["checks"]["targets"][0]["status"], "failed");
    assert_eq!(pending["review"]["status"], "absent");
    assert_eq!(pending["acceptance"]["status"], "absent");
    let pending_human = call(
        &root,
        &["run", "status", &ledger, "--config", "exitbind.json"],
    );
    assert!(pending_human.status.success(), "{}", text(&pending_human));
    assert!(text(&pending_human).contains("Lead decision: pending"));
    let acceptance = call(
        &root,
        &[
            "run",
            "submit",
            "lead",
            &ledger,
            "--outcome",
            "accepted",
            "--artifact",
            ".exitbind/artifacts/observe-nonzero.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!acceptance.status.success(), "{}", text(&acceptance));
    assert!(text(&acceptance).contains("acceptance refused"));
    let status = run(
        &root,
        &[
            "run",
            "status",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(status["status"], "running");
    assert_eq!(status["acceptance"]["status"], "absent");
    let human = call(
        &root,
        &["run", "status", &ledger, "--config", "exitbind.json"],
    );
    assert!(human.status.success(), "{}", text(&human));
    let human_text = text(&human);
    assert!(human_text.contains("acquisition=observed"));
    assert!(!human_text.contains("acquisition=reported"));
    let inspected = run(
        &root,
        &[
            "run",
            "inspect",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    let protection = inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["action"] == "protect")
        .and_then(|event| event["eventSha256"].as_str())
        .unwrap();
    let explanation = call(
        &root,
        &[
            "run",
            "explain",
            &ledger,
            "--event",
            protection,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(explanation.status.success(), "{}", text(&explanation));
    let explanation_text = text(&explanation);
    assert!(explanation_text.contains("acquisition=observed"));
    assert!(!explanation_text.contains("acquisition=reported"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn passing_observation_does_not_auto_review_or_accept() {
    let (root, ledger, target) = checked_worker("observe-zero", "exit 0");
    let observed = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(observed.status.success(), "{}", text(&observed));
    let status = run(
        &root,
        &[
            "run",
            "status",
            &ledger,
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(status["checks"]["status"], "passed");
    assert_eq!(status["review"]["status"], "absent");
    assert_eq!(status["acceptance"]["status"], "absent");
    let human = call(
        &root,
        &["run", "status", &ledger, "--config", "exitbind.json"],
    );
    let human = text(&human);
    assert!(human.contains("Locally observed check: passed"));
    assert!(human.contains("Reviewer outcome: reviewer (reviewer) stage 3 pending"));
    assert!(human.contains("Lead decision: pending"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn human_status_distinguishes_reported_observed_and_mixed_acquisition() {
    let root = support::temp("observe-mixed");
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    configure_workers(&root, &["worker", "worker_two"]);
    let ledger = ".exitbind/runs/mixed.jsonl";
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "mixed",
            "--ledger",
            ledger,
            "--check-command",
            "exit 0",
            "--config",
            "exitbind.json",
        ],
    );
    fs::write(root.join(".exitbind/artifacts/mixed.md"), "mixed\n").unwrap();
    run(
        &root,
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "scoped",
            "--artifact",
            ".exitbind/artifacts/mixed.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    let mut targets = Vec::new();
    for worker in ["worker", "worker_two"] {
        let submitted = run(
            &root,
            &[
                "run",
                "submit",
                worker,
                ledger,
                "--outcome",
                "completed",
                "--artifact",
                ".exitbind/artifacts/mixed.md",
                "--artifact-root",
                "state",
                "--config",
                "exitbind.json",
            ],
        );
        targets.push(
            submitted["event"]["eventSha256"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    run(
        &root,
        &[
            "run",
            "observe-check",
            ledger,
            "--target",
            &targets[1],
            "--config",
            "exitbind.json",
        ],
    );
    let observed_with_missing = call(
        &root,
        &["run", "status", ledger, "--config", "exitbind.json"],
    );
    let observed_with_missing = text(&observed_with_missing);
    assert!(observed_with_missing.contains("Locally observed check: not observed"));
    assert!(observed_with_missing.contains("local observe-check is available"));
    assert!(observed_with_missing.contains("acquisition=observed"));
    assert!(observed_with_missing.contains("reported exit=missing"));

    let explained_with_missing = call(
        &root,
        &["run", "explain", ledger, "--config", "exitbind.json"],
    );
    assert!(
        explained_with_missing.status.success(),
        "{}",
        text(&explained_with_missing)
    );
    let explained_with_missing = text(&explained_with_missing);
    assert!(explained_with_missing.contains("Locally observed check: not observed"));
    assert!(explained_with_missing.contains("local observe-check is available"));
    assert!(explained_with_missing.contains("acquisition=observed"));
    assert!(explained_with_missing.contains("reported exit=missing"));

    run(
        &root,
        &[
            "run",
            "record-check",
            ledger,
            "--target",
            &targets[0],
            "--check-command",
            "exit 0",
            "--exit-code",
            "0",
            "--config",
            "exitbind.json",
        ],
    );
    let human = call(
        &root,
        &["run", "status", ledger, "--config", "exitbind.json"],
    );
    let human = text(&human);
    assert!(human.contains("Configured check: passed (mixed acquisition; see targets)"));
    assert!(human.contains("acquisition=reported"));
    assert!(human.contains("acquisition=observed"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn advanced_help_documents_the_positive_observe_timeout() {
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["help", "advanced"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let help = text(&output);
    assert!(help.contains("[--timeout-ms MS]"));
    assert!(help.contains("1,800,000 ms (30 minute) timeout"));
    assert!(help.contains("--timeout-ms must be positive"));
}

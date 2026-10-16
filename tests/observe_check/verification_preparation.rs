use super::*;

#[test]
#[cfg(target_os = "linux")]
fn selected_preparation_suppresses_product_bytecode_and_owns_generic_scratch() {
    let root = support::temp("observe-verification-preparation");
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    fs::write(root.join("cache_probe.py"), "VALUE = 'ready'\n").unwrap();

    let ordinary = std::process::Command::new("python3")
        .current_dir(&root)
        .env_remove("PYTHONDONTWRITEBYTECODE")
        .env_remove("PYTHONPYCACHEPREFIX")
        .args(["-c", "import cache_probe; print(cache_probe.VALUE)"])
        .output()
        .unwrap();
    assert!(ordinary.status.success(), "{}", text(&ordinary));
    assert!(
        root.join("__pycache__").exists(),
        "ordinary import did not create the control bytecode cache"
    );
    fs::remove_dir_all(root.join("__pycache__")).unwrap();

    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let ledger = ".exitbind/runs/prepared.jsonl";
    run(&root, &["run", "start", "change", "--goal", "prepared check", "--ledger", ledger,
        "--check-command", "python3 -c 'import cache_probe, os, pathlib; assert os.environ[\"PYTHONDONTWRITEBYTECODE\"] == \"1\"; p=pathlib.Path(os.environ[\"EXITBIND_VERIFICATION_SCRATCH\"]); assert os.environ[\"TMPDIR\"] == str(p); (p/\"scratch.txt\").write_text(\"owned\"); print(cache_probe.VALUE)'",
        "--review-policy", "required", "--config", "exitbind.json"]);
    let events = fs::read_to_string(root.join(ledger)).unwrap();
    let start_event: Value = serde_json::from_str(events.lines().next().unwrap()).unwrap();
    assert_eq!(start_event["version"], 8);
    assert_eq!(start_event["verificationPreparationProtocol"], 1);
    assert!(start_event["verificationCheckerSources"].is_array());
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
    assert_eq!(observed["event"]["result"]["code"], 0);
    assert!(
        !root.join("__pycache__").exists(),
        "selected verification wrote Python bytecode into product inputs"
    );
    let operation = observed["event"]["observationEventSha256"]
        .as_str()
        .unwrap();
    let paths = fs::read_dir(root.join(".exitbind/verification"))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert!(!paths.is_empty());
    let scratches = walk_scratch(&root.join(".exitbind/verification"));
    assert!(
        scratches
            .iter()
            .any(|path| path.join("scratch.txt").is_file()),
        "checker did not receive its owned scratch path; observation={operation}"
    );
}

#[test]
fn selected_checker_source_drift_is_refused_before_observation_admission() {
    let root = support::temp("observe-verification-checker-drift");
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    let checker = root.with_extension("external-checker.py");
    fs::write(&checker, "print('EXECUTED_FIRST')\n").unwrap();
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let ledger = ".exitbind/runs/checker-drift.jsonl";
    let checker_arg = checker.to_str().unwrap();
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "checker drift",
            "--ledger",
            ledger,
            "--check-command",
            &format!("python3 {checker_arg}"),
            "--review-policy",
            "required",
            "--config",
            "exitbind.json",
        ],
    );
    let start: Value = serde_json::from_str(
        fs::read_to_string(root.join(ledger))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        start["verificationCheckerSources"]
            .as_array()
            .unwrap()
            .len(),
        1
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
    fs::write(&checker, "print('EXECUTED_CHANGED')\n").unwrap();
    let result = call(
        &root,
        &[
            "run",
            "observe-check",
            ledger,
            "--target",
            worker["event"]["eventSha256"].as_str().unwrap(),
            "--config",
            "exitbind.json",
        ],
    );
    assert!(
        !result.status.success(),
        "changed source was executed: {}",
        text(&result)
    );
    let events = fs::read_to_string(root.join(ledger)).unwrap();
    assert!(
        !events.contains("check_observation\""),
        "checker drift should be refused before admission"
    );
    assert!(
        !text(&result).contains("EXECUTED_CHANGED"),
        "changed checker unexpectedly ran"
    );
    fs::remove_file(checker).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
fn selected_external_checker_run(
    label: &str,
) -> (
    std::path::PathBuf,
    String,
    std::path::PathBuf,
    std::path::PathBuf,
    String,
) {
    use std::os::unix::fs::symlink;
    let root = support::temp(label);
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    let name = root.file_name().unwrap().to_string_lossy();
    let external = root.parent().unwrap().join(format!("{name}-external.py"));
    let argument = root
        .parent()
        .unwrap()
        .join(format!("{name}-checker-link.py"));
    fs::write(&external, "print('EXECUTED_EXTERNAL')\n").unwrap();
    symlink(&external, &argument).unwrap();
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let ledger = ".exitbind/runs/checker-source-loss.jsonl".to_owned();
    let command = format!("python3 {}", argument.display());
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "checker source is bound",
            "--ledger",
            &ledger,
            "--check-command",
            &command,
            "--review-policy",
            "required",
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
            &ledger,
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
            &ledger,
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
    (
        root,
        ledger,
        external,
        argument,
        worker["event"]["eventSha256"].as_str().unwrap().to_owned(),
    )
}

#[cfg(unix)]
#[test]
fn selected_missing_or_managed_retargeted_checker_refuses_before_admission() {
    use std::os::unix::fs::symlink;

    let (root, ledger, external, argument, target) =
        selected_external_checker_run("observe-verification-checker-missing");
    fs::remove_file(external).unwrap();
    let before = fs::read(root.join(&ledger)).unwrap();
    let missing = call(
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
    assert!(!missing.status.success(), "{}", text(&missing));
    assert!(text(&missing).contains("verification checker source is unavailable"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    assert!(!fs::read_to_string(root.join(&ledger))
        .unwrap()
        .contains("check_observation\""));
    fs::remove_file(argument).unwrap();
    fs::remove_dir_all(root).unwrap();

    let (root, ledger, external, argument, target) =
        selected_external_checker_run("observe-verification-checker-managed-retarget");
    let managed = root.join(".exitbind/managed-checker.py");
    fs::write(&managed, "print('EXECUTED_MANAGED')\n").unwrap();
    fs::remove_file(&argument).unwrap();
    symlink(&managed, &argument).unwrap();
    let before = fs::read(root.join(&ledger)).unwrap();
    let retargeted = call(
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
    assert!(!retargeted.status.success(), "{}", text(&retargeted));
    assert!(text(&retargeted).contains("verification checker source changed"));
    assert!(!text(&retargeted).contains("EXECUTED_MANAGED"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    fs::remove_file(argument).unwrap();
    fs::remove_file(external).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn selected_configuration_drift_refuses_before_checker_effect_or_admission() {
    let (root, ledger, external, argument, target) =
        selected_external_checker_run("observe-verification-config-drift");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]
        .as_object_mut()
        .unwrap()
        .remove("verificationPreparation");
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let before = fs::read(root.join(&ledger)).unwrap();
    let refused = call(
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
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("frozen current configuration"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    assert!(!text(&refused).contains("EXECUTED_EXTERNAL"));
    fs::remove_file(argument).unwrap();
    fs::remove_file(external).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn selected_start_marker_is_refused_by_installed_older_reader_before_effects() {
    let Ok(old_reader) = std::env::var("EXITBIND_PREPARATION_OLD_READER") else {
        eprintln!("skipped: set EXITBIND_PREPARATION_OLD_READER to the installed older reader");
        return;
    };
    let (root, ledger, external, argument, target) =
        selected_external_checker_run("observe-verification-old-reader");
    let old_config_path = root.join("old-reader.json");
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
    config["project"]
        .as_object_mut()
        .unwrap()
        .remove("verificationPreparation");
    fs::write(
        &old_config_path,
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let before = fs::read(root.join(&ledger)).unwrap();
    let output = std::process::Command::new(old_reader)
        .current_dir(&root)
        .args([
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "old-reader.json",
        ])
        .output()
        .unwrap();
    let output_text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{output_text}");
    assert!(
        output_text.contains("verificationPreparationProtocol")
            || output_text.contains("verificationCheckerSources"),
        "old reader did not identify the selected start marker: {output_text}"
    );
    assert!(!output_text.contains("EXECUTED_EXTERNAL"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    fs::remove_file(argument).unwrap();
    fs::remove_file(external).unwrap();
    fs::remove_file(old_config_path).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn selected_verification_retains_source_mutation_without_credit() {
    let root = support::temp("observe-verification-source-mutation");
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    fs::write(root.join("source.txt"), "before\n").unwrap();
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let ledger = ".exitbind/runs/source-mutation.jsonl";
    let command = "python3 -c 'from pathlib import Path; import sys; Path(\"source.txt\").write_text(\"after\\n\"); print(\"retained stdout\"); print(\"retained stderr\", file=sys.stderr)'";
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "source mutation",
            "--ledger",
            ledger,
            "--check-command",
            command,
            "--review-policy",
            "required",
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
    let observed = run(
        &root,
        &[
            "run",
            "observe-check",
            ledger,
            "--target",
            worker["event"]["eventSha256"].as_str().unwrap(),
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(
        observed["observationFailure"]["action"],
        "check_observation_failed"
    );
    let facts = &observed["observationFailure"]["observation"]["facts"];
    assert_eq!(
        facts["diagnostic"],
        "observation binding changed before commit; no check was recorded"
    );
    assert_eq!(facts["process"]["code"], 0);
    assert_eq!(facts["process"]["kind"], "exit");
    assert_eq!(facts["capture"], "complete");
    assert_eq!(facts["storage"], "not_committed");
    assert_eq!(observed["checkRecorded"], false);
    let partials = facts["partialCaptures"].as_array().unwrap();
    assert_eq!(partials.len(), 2);
    for (partial, expected) in partials
        .iter()
        .zip([b"retained stdout\n" as &[u8], b"retained stderr\n" as &[u8]])
    {
        assert_eq!(partial["completeness"], "partial");
        assert_eq!(
            fs::read(root.join(partial["path"].as_str().unwrap())).unwrap(),
            expected
        );
    }
    assert_eq!(
        fs::read_to_string(root.join("source.txt")).unwrap(),
        "after\n"
    );
    let events = fs::read_to_string(root.join(ledger)).unwrap();
    assert!(events
        .lines()
        .any(|line| line.contains("\"action\":\"check_observation_failed\"")));
    assert!(!events
        .lines()
        .any(|line| line.contains("\"action\":\"check\"")));
    let status = run(
        &root,
        &[
            "run",
            "status",
            ledger,
            "--config",
            "exitbind.json",
            "--json",
        ],
    );
    assert_eq!(status["checks"]["status"], "not_observed");
    assert_eq!(status["checks"]["missingCount"], 1);
    assert_eq!(status["checks"]["targets"][0]["status"], "missing");
    assert_eq!(
        status["evidence"]["checkCount"], 0,
        "changed-input execution is failure evidence, not check credit"
    );
    assert_eq!(status["acceptance"]["status"], "absent");
    fs::remove_dir_all(root).unwrap();
}

fn walk_scratch(root: &Path) -> Vec<std::path::PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(path) = pending.pop() {
        let entries = fs::read_dir(&path)
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        if entries
            .iter()
            .any(|entry| entry.file_name() == "scratch.txt")
        {
            found.push(path);
        } else {
            pending.extend(
                entries
                    .into_iter()
                    .filter(|entry| entry.file_type().unwrap().is_dir())
                    .map(|entry| entry.path()),
            );
        }
    }
    found
}

#[cfg(unix)]
#[test]
fn selected_preparation_rejects_product_symlink_alias_before_checker_launch() {
    use std::os::unix::fs::symlink;
    let root = support::temp("observe-verification-alias");
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = serde_json::json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    symlink(".exitbind/verification", root.join("output-alias")).unwrap();
    let ledger = ".exitbind/runs/alias.jsonl";
    run(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "alias refusal",
            "--ledger",
            ledger,
            "--check-command",
            "touch should-not-launch",
            "--review-policy",
            "required",
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
    let failure = run(
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
    assert_eq!(
        failure["observationFailure"]["action"],
        "check_observation_failed"
    );
    assert_eq!(
        failure["observationFailure"]["observation"]["facts"]["processStarted"],
        false
    );
    assert!(serde_json::to_string(&failure)
        .unwrap()
        .contains("verification scratch aliases a product input symlink"));
    assert!(
        !root.join("should-not-launch").exists(),
        "alias refusal happened after checker launch"
    );
}

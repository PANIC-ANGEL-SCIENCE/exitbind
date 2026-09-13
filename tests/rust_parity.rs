use serde_json::Value;
mod support;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output},
};

fn project() -> PathBuf {
    let path = support::temp("rust-test");
    let output = invoke(&["init", "--root", path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    path
}

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .args(arguments)
        .output()
        .unwrap()
}

fn config(path: &std::path::Path) -> String {
    path.join("soulmate.json").to_string_lossy().into_owned()
}

#[test]
fn version_is_the_rust_release() {
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn entrypoint_help_and_version_forms_are_compatible() {
    for arguments in [&[][..], &["help"][..], &["--help"][..]] {
        let output = invoke(arguments);
        assert!(
            output.status.success(),
            "arguments {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("Usage: soulmate"),
            "arguments {arguments:?}"
        );
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("Core: init, brief, work, check"));
        assert!(help.contains("init prepares portable project setup"));
        assert!(help.contains("the host runs project tests"));
        assert!(help.contains("soulmate benchmark"));
        assert!(help.contains("soulmate help advanced"));
        assert!(!help.contains("Do the next change"));
        assert!(!help.contains("--event-id"));
        assert!(output.stderr.is_empty(), "arguments {arguments:?}");
    }

    let advanced = invoke(&["help", "advanced"]);
    assert!(advanced.status.success());
    let advanced_help = String::from_utf8_lossy(&advanced.stdout);
    assert!(advanced_help.contains("Do the next change"));
    assert!(advanced_help.contains("When work fails or changes"));
    assert!(advanced_help.contains("Optional surfaces"));
    assert!(advanced_help.contains("--event-id"));
    assert!(advanced_help.contains("--text"));
    assert!(advanced_help.contains("Advanced commands: bind, doctor"));

    for arguments in [&["version"][..], &["--version"][..]] {
        let output = invoke(arguments);
        assert!(
            output.status.success(),
            "arguments {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            env!("CARGO_PKG_VERSION")
        );
    }
}

#[test]
fn hook_protocol_is_stable() {
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .arg("hook-protocol")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "soulmate-hook-v1"
    );
}

#[test]
fn rust_inspects_frozen_v1_and_v2_run_ledgers() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (fixture, version) in [
        ("tests/fixtures/v0.0.8-run.jsonl", 1),
        ("tests/fixtures/v0.1.x-run.jsonl", 1),
        ("tests/fixtures/v0.2.x-run.jsonl", 1),
        ("tests/fixtures/v0.7.x-run-v2.jsonl", 2),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
            .current_dir(manifest)
            .args([
                "run",
                "inspect",
                fixture,
                "--config",
                "examples/soulmate.json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{fixture}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["valid"], true, "{fixture}");
        assert_eq!(body["events"][0]["version"], version, "{fixture}");
    }
}

#[test]
fn rust_inspects_a_golden_v008_memory_ledger() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .current_dir(manifest)
        .args([
            "memory",
            "inspect",
            "tests/fixtures/v0.0.8-memory.jsonl",
            "--json",
            "--config",
            "examples/soulmate.json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["valid"], true);
    assert_eq!(body["items"][0]["state"], "accepted");
    assert_eq!(body["events"].as_array().unwrap().len(), 3);
}

#[test]
fn unchanged_configuration_resumes() {
    let root = project();
    let cfg = config(&root);
    let started = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(started.status.success());
    let resumed = invoke(&["run", "next", ".soulmate/run.jsonl", "--config", &cfg]);
    assert!(resumed.status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn drift_is_machine_readable_and_does_not_disclose_goal() {
    let root = project();
    let cfg = config(&root);
    let private_goal = "private-goal-must-not-appear";
    assert!(invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        private_goal,
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &cfg
    ])
    .status
    .success());
    fs::OpenOptions::new()
        .append(true)
        .open(&cfg)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let drift = invoke(&[
        "run",
        "next",
        ".soulmate/run.jsonl",
        "--json",
        "--config",
        &cfg,
    ]);
    assert!(!drift.status.success());
    let body: Value = serde_json::from_slice(&drift.stdout).unwrap();
    assert_eq!(body["classification"], "config_drift");
    assert_eq!(body["expectedConfigSha256"].as_str().unwrap().len(), 64);
    assert_eq!(body["currentConfigSha256"].as_str().unwrap().len(), 64);
    assert!(!String::from_utf8_lossy(&drift.stdout).contains(private_goal));
    assert!(!String::from_utf8_lossy(&drift.stderr).contains(private_goal));
    let inspected = invoke(&["run", "inspect", ".soulmate/run.jsonl", "--config", &cfg]);
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn supersession_preserves_and_seals_predecessor() {
    let root = project();
    let cfg = config(&root);
    let old = root.join(".soulmate/run.jsonl");
    assert!(invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "old",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &cfg
    ])
    .status
    .success());
    let before = fs::read(&old).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&cfg)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    assert!(invoke(&[
        "run",
        "supersede",
        "./.soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "new",
        "--ledger",
        ".soulmate/resume.jsonl",
        "--config",
        &cfg
    ])
    .status
    .success());
    assert_eq!(fs::read(&old).unwrap(), before);
    let blocked = invoke(&[
        "run",
        "submit",
        "lead",
        ".soulmate/../.soulmate/run.jsonl",
        "--outcome",
        "scoped",
        "--artifact",
        "soulmate.json",
        "--config",
        &cfg,
    ]);
    assert!(!blocked.status.success());
    assert_eq!(fs::read(&old).unwrap(), before);
    let retried = invoke(&[
        "run",
        "supersede",
        ".soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "new",
        "--ledger",
        ".soulmate/resume.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(retried.status.success());
    let competing = invoke(&[
        "run",
        "supersede",
        ".soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "competing",
        "--ledger",
        ".soulmate/other.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(!competing.status.success());
    assert!(!root.join(".soulmate/other.jsonl").exists());
    fs::remove_file(&old).unwrap();
    let orphaned = invoke(&["run", "inspect", ".soulmate/resume.jsonl", "--config", &cfg]);
    assert!(!orphaned.status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn supersession_conflict_does_not_orphan_a_predecessor_claim() {
    let root = project();
    let cfg = config(&root);
    assert!(invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "old",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &cfg,
    ])
    .status
    .success());
    let successor = root.join(".soulmate/resume.jsonl");
    fs::write(&successor, "conflicting successor\n").unwrap();

    let conflict = invoke(&[
        "run",
        "supersede",
        ".soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "new",
        "--ledger",
        ".soulmate/resume.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(!conflict.status.success());
    assert!(!root.join(".soulmate/run.jsonl.supersede").exists());

    fs::remove_file(successor).unwrap();
    let retry = invoke(&[
        "run",
        "supersede",
        ".soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "new",
        "--ledger",
        ".soulmate/resume.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(
        retry.status.success(),
        "{}{}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn blocked_terminal_run_can_be_superseded_once() {
    let root = project();
    let cfg = config(&root);
    let old = root.join(".soulmate/run.jsonl");
    assert!(invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "blocked predecessor",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &cfg,
    ])
    .status
    .success());
    assert!(invoke(&[
        "run",
        "submit",
        "lead",
        ".soulmate/run.jsonl",
        "--outcome",
        "blocked",
        "--artifact",
        "soulmate.json",
        "--config",
        &cfg,
    ])
    .status
    .success());
    let terminal = fs::read(&old).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&cfg)
        .unwrap()
        .write_all(b"\n")
        .unwrap();

    let successor = invoke(&[
        "run",
        "supersede",
        ".soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "bounded continuation",
        "--ledger",
        ".soulmate/resume.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(
        successor.status.success(),
        "{}{}",
        String::from_utf8_lossy(&successor.stdout),
        String::from_utf8_lossy(&successor.stderr)
    );
    assert_eq!(fs::read(&old).unwrap(), terminal);
    assert!(root.join(".soulmate/run.jsonl.supersede").is_file());
    assert!(
        invoke(&["run", "inspect", ".soulmate/resume.jsonl", "--config", &cfg,])
            .status
            .success()
    );

    let competing = invoke(&[
        "run",
        "supersede",
        ".soulmate/run.jsonl",
        "--workflow",
        "change",
        "--goal",
        "competing continuation",
        "--ledger",
        ".soulmate/other.jsonl",
        "--config",
        &cfg,
    ]);
    assert!(!competing.status.success());
    assert!(!root.join(".soulmate/other.jsonl").exists());
    assert_eq!(fs::read(&old).unwrap(), terminal);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn accepted_and_rejected_runs_remain_final() {
    for terminal_outcome in ["accepted", "rejected"] {
        let root = project();
        let cfg = config(&root);
        assert!(invoke(&[
            "run",
            "start",
            "change",
            "--goal",
            "terminal predecessor",
            "--ledger",
            ".soulmate/run.jsonl",
            "--config",
            &cfg,
        ])
        .status
        .success());
        for (agent, outcome) in [
            ("lead", "scoped"),
            ("worker", "completed"),
            ("reviewer", "approved"),
            ("lead", terminal_outcome),
        ] {
            assert!(invoke(&[
                "run",
                "submit",
                agent,
                ".soulmate/run.jsonl",
                "--outcome",
                outcome,
                "--artifact",
                "soulmate.json",
                "--config",
                &cfg,
            ])
            .status
            .success());
        }

        let refused = invoke(&[
            "run",
            "supersede",
            ".soulmate/run.jsonl",
            "--workflow",
            "change",
            "--goal",
            "must refuse",
            "--ledger",
            ".soulmate/resume.jsonl",
            "--config",
            &cfg,
        ]);
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr)
            .contains("only a running or blocked run can be superseded"));
        assert!(!root.join(".soulmate/resume.jsonl").exists());
        assert!(!root.join(".soulmate/run.jsonl.supersede").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn receipt_round_trip_uses_the_rust_binary() {
    let root = project();
    let cfg = config(&root);
    let receipt = root.join("receipt.json").to_string_lossy().into_owned();
    let planned = invoke(&[
        "plan",
        "change",
        "--goal",
        "bounded",
        "--receipt",
        &receipt,
        "--config",
        &cfg,
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let receipt_value: Value =
        serde_json::from_str(&fs::read_to_string(&receipt).unwrap()).unwrap();
    assert_eq!(receipt_value["version"], 1);
    assert_eq!(receipt_value["producer"]["name"], "soulmate");
    assert_eq!(
        receipt_value["producer"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    let verified = invoke(&["verify", &receipt, "--config", &cfg]);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let mut frozen_v03 = receipt_value;
    frozen_v03["producer"]["version"] = serde_json::json!("0.3.0");
    fs::write(
        &receipt,
        format!("{}\n", serde_json::to_string_pretty(&frozen_v03).unwrap()),
    )
    .unwrap();
    let verified = invoke(&["verify", &receipt, "--config", &cfg]);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn memory_lifecycle_keeps_review_and_acceptance_separate() {
    let root = project();
    let cfg = config(&root);
    let mut value: Value = serde_json::from_slice(&fs::read(&cfg).unwrap()).unwrap();
    value["agents"]["worker"]["memoryWrite"] = serde_json::json!(["invariants"]);
    value["agents"]["worker"]["memoryReview"] = serde_json::json!(["invariants"]);
    value["agents"]["lead"]["memoryPromote"] = serde_json::json!(["invariants"]);
    fs::write(
        &cfg,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
    fs::write(root.join("memory.md"), "synthetic invariant\n").unwrap();
    let ledger = ".soulmate/memory.jsonl";
    assert!(invoke(&[
        "memory",
        "propose",
        "worker",
        "memory.md",
        "--scope",
        "invariants",
        "--ledger",
        ledger,
        "--config",
        &cfg
    ])
    .status
    .success());
    assert!(
        invoke(&["memory", "review", "worker", ledger, "--config", &cfg])
            .status
            .success()
    );
    assert!(
        invoke(&["memory", "promote", "lead", ledger, "--config", &cfg])
            .status
            .success()
    );
    let inspected = invoke(&["memory", "inspect", ledger, "--json", "--config", &cfg]);
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let body: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(body["items"][0]["state"], "accepted");
    assert_eq!(body["events"].as_array().unwrap().len(), 3);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hooks_require_an_explicit_host_selection() {
    let root = project();
    let missing = invoke(&["hooks", "plan", "--root", root.to_str().unwrap()]);
    assert!(!missing.status.success());
    let planned = invoke(&[
        "hooks",
        "plan",
        "--hosts",
        "codex,claude",
        "--root",
        root.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let body: Value = serde_json::from_slice(&planned.stdout).unwrap();
    assert_eq!(body["hosts"].as_array().unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn profile_audit_is_standalone_and_doctor_fails_required_checks() {
    let root = project();
    let source = root.join("portable.md");
    fs::write(&source, "Portable bounded role.\n").unwrap();
    let missing = root.join("missing.json").to_string_lossy().into_owned();
    let audited = invoke(&[
        "profile",
        "audit",
        source.to_str().unwrap(),
        "--json",
        "--config",
        &missing,
    ]);
    assert!(audited.status.success());
    let doctor = invoke(&["doctor", "--config", &missing]);
    assert!(!doctor.status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn doctor_reports_npx_managed_dotagents_without_invoking_it() {
    let root = project();
    let home = root.join("home");
    let bin = root.join("bin");
    fs::create_dir_all(home.join(".agents")).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(home.join(".agents/agents.toml"), "").unwrap();
    fs::write(bin.join("npx"), "").unwrap();
    let config = root.join("soulmate.json").to_string_lossy().into_owned();
    let doctor = Command::new(env!("CARGO_BIN_EXE_soulmate"))
        .env("HOME", &home)
        .env("PATH", &bin)
        .args(["doctor", "--config", &config])
        .output()
        .unwrap();
    assert!(doctor.status.success());
    assert!(String::from_utf8_lossy(&doctor.stdout)
        .contains("npx launcher and agents.toml observed; package not invoked; host-specific caches and active-session reload were not inspected"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn run_locks_fail_closed_and_recover_only_verifiable_stale_pids() {
    use std::os::unix::fs::PermissionsExt;

    let active_root = project();
    let active_cfg = config(&active_root);
    let active_lock = active_root.join(".soulmate/run.jsonl.lock");
    fs::write(
        active_lock,
        format!(
            "{{\"pid\":{},\"createdAt\":\"2026-08-29T00:00:00.000Z\"}}",
            std::process::id()
        ),
    )
    .unwrap();
    let active = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &active_cfg,
    ]);
    assert!(!active.status.success());
    assert!(!active_root.join(".soulmate/run.jsonl").exists());
    fs::remove_dir_all(active_root).unwrap();

    let stale_root = project();
    let stale_cfg = config(&stale_root);
    let stale_lock = stale_root.join(".soulmate/run.jsonl.lock");
    fs::write(
        &stale_lock,
        "{\"pid\":4000000,\"createdAt\":\"2026-08-29T00:00:00.000Z\"}",
    )
    .unwrap();
    let recovered = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &stale_cfg,
    ]);
    assert!(recovered.status.success());
    assert!(!stale_lock.exists());
    fs::remove_dir_all(stale_root).unwrap();

    let denied_root = project();
    let denied_cfg = config(&denied_root);
    let denied_lock = denied_root.join(".soulmate/run.jsonl.lock");
    fs::write(&denied_lock, "not-readable").unwrap();
    fs::set_permissions(&denied_lock, fs::Permissions::from_mode(0o000)).unwrap();
    let denied = invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "bounded",
        "--ledger",
        ".soulmate/run.jsonl",
        "--config",
        &denied_cfg,
    ]);
    assert!(!denied.status.success());
    assert!(!denied_root.join(".soulmate/run.jsonl").exists());
    fs::set_permissions(&denied_lock, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_dir_all(denied_root).unwrap();
}

#[test]
fn malformed_and_truncated_golden_ledgers_are_rejected() {
    let root = project();
    let cfg = config(&root);
    let source = fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.0.8-run.jsonl"),
    )
    .unwrap();
    let ledger = root.join(".soulmate/broken.jsonl");
    fs::write(ledger, &source[..source.len() / 2]).unwrap();
    let inspected = invoke(&["run", "inspect", ".soulmate/broken.jsonl", "--config", &cfg]);
    assert!(!inspected.status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_external_inputs_fail_cleanly_as_json() {
    fn assert_json_error(output: Output) {
        assert!(!output.status.success());
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!combined.contains("panicked at"), "{combined}");
        let value: Value = serde_json::from_slice(&output.stdout).expect("JSON error output");
        assert!(value["error"].as_str().is_some(), "{value}");
    }

    let malformed_root = project();
    let malformed_config = malformed_root.join("malformed.json");
    fs::write(&malformed_config, b"{not-json").unwrap();
    let malformed_config = malformed_config.to_str().unwrap();
    for arguments in [
        vec!["check", "--json", "--config", malformed_config],
        vec![
            "brief",
            "worker",
            "--task",
            "x",
            "--json",
            "--config",
            malformed_config,
        ],
        vec![
            "plan",
            "change",
            "--goal",
            "x",
            "--json",
            "--config",
            malformed_config,
        ],
        vec![
            "verify",
            "receipt.json",
            "--json",
            "--config",
            malformed_config,
        ],
        vec!["profile", "worker", "--json", "--config", malformed_config],
        vec![
            "memory",
            "resolve",
            "worker",
            "--json",
            "--config",
            malformed_config,
        ],
        vec![
            "run",
            "inspect",
            ".soulmate/run.jsonl",
            "--json",
            "--config",
            malformed_config,
        ],
    ] {
        assert_json_error(invoke(&arguments));
    }
    fs::remove_dir_all(malformed_root).unwrap();

    let root = project();
    let cfg = config(&root);
    fs::write(root.join("boundary.json"), b"{bad-boundary").unwrap();
    assert_json_error(invoke(&[
        "run",
        "start",
        "change",
        "--goal",
        "x",
        "--ledger",
        ".soulmate/run.jsonl",
        "--boundary",
        "boundary.json",
        "--json",
        "--config",
        &cfg,
    ]));

    fs::write(root.join(".soulmate/run.jsonl"), b"{bad-ledger\n").unwrap();
    assert_json_error(invoke(&[
        "run",
        "inspect",
        ".soulmate/run.jsonl",
        "--json",
        "--config",
        &cfg,
    ]));
    fs::write(root.join(".soulmate/memory.jsonl"), b"{bad-ledger\n").unwrap();
    assert_json_error(invoke(&[
        "memory",
        "inspect",
        ".soulmate/memory.jsonl",
        "--json",
        "--config",
        &cfg,
    ]));
    fs::remove_dir_all(root).unwrap();
}

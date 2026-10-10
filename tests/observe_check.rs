// Current Exitbind and preserved historical-reader contracts run in the default suite.
mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn call(root: &Path, args: &[&str]) -> Output {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("soulmate should start")
}

fn call_exitbind(root: &Path, args: &[&str]) -> Output {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("exitbind should start")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; output: {}", text(output)))
}

fn run(root: &Path, args: &[&str]) -> Value {
    let output = call(root, args);
    assert!(output.status.success(), "{}", text(&output));
    json(&output)
}

fn checked_worker(label: &str, command: &str) -> (std::path::PathBuf, String, String) {
    let root = support::temp(label);
    support::git_topology::repository(&root);
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{}", text(&init));
    let ledger = format!(".exitbind/runs/{label}.jsonl");
    let output = call(
        &root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "observe",
            "--ledger",
            &ledger,
            "--check-command",
            command,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let artifact = format!(".exitbind/artifacts/{label}.md");
    fs::write(root.join(&artifact), "worker\n").unwrap();
    let output = call(
        &root,
        &[
            "run",
            "submit",
            "lead",
            &ledger,
            "--outcome",
            "scoped",
            "--artifact",
            &artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    let worker_output = call(
        &root,
        &[
            "run",
            "submit",
            "worker",
            &ledger,
            "--outcome",
            "completed",
            "--artifact",
            &artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(worker_output.status.success(), "{}", text(&worker_output));
    let worker = json(&worker_output);
    let target = worker["event"]["eventSha256"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (root, ledger, target)
}

fn checked_exitbind_worker(label: &str, command: &str) -> (std::path::PathBuf, String, String) {
    let root = support::temp(label);
    support::git_topology::repository(&root);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["init", "--mode", "portable", "--root", "."])
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", text(&init));
    let ledger = format!(".exitbind/runs/{label}.jsonl");
    let started = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "run",
            "start",
            "change",
            "--goal",
            "observe input drift",
            "--ledger",
            &ledger,
            "--check-command",
            command,
            "--proof-origin",
            "local_report",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(started.status.success(), "{}", text(&started));
    let artifact = format!(".exitbind/artifacts/{label}.md");
    fs::write(root.join(&artifact), "worker\n").unwrap();
    let lead = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "run",
            "submit",
            "lead",
            &ledger,
            "--outcome",
            "scoped",
            "--artifact",
            &artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(lead.status.success(), "{}", text(&lead));
    let worker = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "run",
            "submit",
            "worker",
            &ledger,
            "--outcome",
            "completed",
            "--artifact",
            &artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(worker.status.success(), "{}", text(&worker));
    let target = json(&worker)["event"]["eventSha256"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (root, ledger, target)
}

fn wait_for_path(path: &Path) {
    let started = Instant::now();
    while !path.exists() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "timed out waiting for {}", path.display());
}

fn configure_workers(root: &Path, workers: &[&str]) {
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let base = config["agents"]["worker"].clone();
    for worker in workers.iter().copied().filter(|worker| *worker != "worker") {
        let mut agent = base.clone();
        agent["profile"] = serde_json::json!(format!("exitbind/agents/{worker}.md"));
        agent["purpose"] = serde_json::json!(format!("Complete bounded work for {worker}."));
        config["agents"][worker] = agent;
        fs::write(
            root.join(format!("exitbind/agents/{worker}.md")),
            format!("# {worker}\n\nComplete bounded work.\n"),
        )
        .unwrap();
    }
    config["workflows"]["change"]["workers"] = serde_json::json!(workers);
    fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

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

#[cfg(unix)]
#[test]
fn timeout_cleans_descendants_appends_nothing_and_allows_recovery() {
    #[cfg(target_os = "linux")]
    if std::env::var("EXITBIND_LEGACY_OBSERVE_REAPER").as_deref() != Ok("1") {
        let captures = support::temp("legacy-observe-reaper");
        let mut child = support::git_topology::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "timeout_cleans_descendants_appends_nothing_and_allows_recovery",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("EXITBIND_LEGACY_OBSERVE_REAPER", "1")
            .stdout(fs::File::create(captures.join("stdout")).unwrap())
            .stderr(fs::File::create(captures.join("stderr")).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(25);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("legacy fixture deadline; captures {}", captures.display());
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(
            status.success(),
            "legacy fixture failed; captures {}: {}",
            captures.display(),
            fs::read_to_string(captures.join("stderr")).unwrap()
        );
        fs::remove_dir_all(captures).unwrap();
        return;
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let command = "if [ -f allow-check ]; then exit 0; fi; trap '' TERM; sh -c 'trap \"\" TERM; echo $$ > child.pid; exec sleep 30' & wait";
    let (root, ledger, target) = checked_worker("observe-timeout", command);
    let before = fs::read(root.join(&ledger)).unwrap();
    #[cfg(target_os = "linux")]
    let reaper = {
        let pid_file = root.join("child.pid");
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !pid_file.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(2));
            }
            let pid: i32 = fs::read_to_string(pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            while Instant::now() < deadline {
                let mut status = 0;
                if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == pid {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            for _ in 0..100 {
                let mut status = 0;
                if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == pid {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
            panic!("owned legacy descendant not reaped");
        })
    };

    let started = Instant::now();
    let timed_out = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--timeout-ms",
            "100",
            "--config",
            "exitbind.json",
        ],
    );
    #[cfg(target_os = "linux")]
    reaper.join().unwrap();
    assert!(!timed_out.status.success(), "{}", text(&timed_out));
    assert!(text(&timed_out).contains("timed out after 100 ms"));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);

    let child_pid: i32 = fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(!process_exists(child_pid), "timed-out descendant survived");
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
    assert_eq!(status["checks"]["status"], "not_observed");
    assert_eq!(status["acceptance"]["status"], "absent");
    let human = call(
        &root,
        &["run", "status", &ledger, "--config", "exitbind.json"],
    );
    assert!(text(&human).contains("local observe-check is available"));

    fs::write(root.join("allow-check"), "ready\n").unwrap();
    let recovered = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--timeout-ms",
            "1000",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(recovered.status.success(), "{}", text(&recovered));
    assert_eq!(json(&recovered)["event"]["result"]["code"], 0);

    for timeout in ["0", "invalid"] {
        let rejected = call(
            &root,
            &[
                "run",
                "observe-check",
                &ledger,
                "--target",
                &target,
                "--timeout-ms",
                timeout,
                "--config",
                "exitbind.json",
            ],
        );
        assert!(!rejected.status.success());
        assert!(text(&rejected).contains("--timeout-ms must be a positive integer"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn concurrent_mutation_is_not_locked_out_and_stale_observation_is_refused() {
    let command = "printf started > observe-started; while [ ! -f observe-continue ]; do sleep 0.01; done; printf observed > observe-effect.txt; exit 7";
    let (root, ledger, target) = checked_worker("observe-race", command);
    let observation = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--timeout-ms",
            "2000",
            "--config",
            "exitbind.json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&root.join("observe-started"));

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
            ".exitbind/artifacts/observe-race.md",
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(reviewer.status.success(), "{}", text(&reviewer));
    fs::write(root.join("observe-continue"), "continue\n").unwrap();
    let observed = observation.wait_with_output().unwrap();
    assert!(!observed.status.success());
    assert!(root.join("observe-effect.txt").is_file());
    let observed_text = text(&observed);
    assert!(observed_text.contains("ledger changed while observing"));
    assert!(
        observed_text.contains("this check result was not recorded"),
        "{observed_text}"
    );
    assert!(
        !observed_text.contains("no mutation was made"),
        "{observed_text}"
    );
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
    assert!(inspected["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["action"] != "check"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn launch_failure_and_caller_overrides_do_not_append() {
    let (root, ledger, target) = checked_worker("observe-launch", "printf launched > launched.txt");
    let before = fs::read(root.join(&ledger)).unwrap();
    let bin = root.join("no-sh");
    fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/usr/bin/git", bin.join("git")).unwrap();
    let failed = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .env("PATH", &bin)
        .args([
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(text(&failed).contains("could not be launched"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    assert!(!root.join("launched.txt").exists());

    for extra in [
        vec!["--check-command", "exit 0"],
        vec!["--exit-code", "0"],
        vec!["--duration-ms", "0"],
        vec!["--env", "X=1"],
        vec!["--cwd", "."],
    ] {
        let mut args = vec![
            "run",
            "observe-check",
            &ledger,
            "--target",
            &target,
            "--config",
            "exitbind.json",
        ];
        args.extend(extra);
        let rejected = call(&root, &args);
        assert!(!rejected.status.success(), "{}", text(&rejected));
        assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
        assert!(!root.join("launched.txt").exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_target_after_supersede_and_post_execution_drift_do_not_append() {
    let (root, ledger, target) = checked_worker("observe-stale", "printf launched > launched.txt");
    let successor = ".exitbind/runs/observe-successor.jsonl";
    let supersede = call(
        &root,
        &[
            "run",
            "supersede",
            &ledger,
            "--workflow",
            "change",
            "--goal",
            "new",
            "--ledger",
            successor,
            "--check-command",
            "printf launched > launched.txt",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(supersede.status.success(), "{}", text(&supersede));
    let rejected = call(
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
    assert!(!rejected.status.success());
    assert!(!root.join("launched.txt").exists());

    let (root, ledger, target) = checked_worker("observe-drift", "printf changed > exitbind.json");
    let drifted = call(
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
    assert!(drifted.status.success(), "{}", text(&drifted));
    let observed: Value = serde_json::from_slice(&drifted.stdout).unwrap();
    assert_eq!(observed["event"]["result"]["code"], 0);
    assert_eq!(observed["warnings"][0]["classification"], "input_drift");
    fs::remove_dir_all(root).unwrap();

    let (root, ledger, target) = checked_worker(
        "observe-artifact-drift",
        "printf changed > .exitbind/artifacts/observe-artifact-drift.md",
    );
    let before = fs::read(root.join(&ledger)).unwrap();
    let drifted = call(
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
    assert!(!drifted.status.success());
    assert!(text(&drifted).contains("artifact"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn input_drift_during_observed_check_keeps_recorded_identity_and_result() {
    let (root, ledger, target) = checked_exitbind_worker(
        "observe-input-drift",
        "printf changed > observed-input.txt; sleep 0.05; exit 7",
    );
    let before = call_exitbind(
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
    assert!(before.status.success(), "{}", text(&before));
    let expected_inputs = json(&before)["inputsSha256"].clone();
    assert!(expected_inputs.is_string());
    let observed = call_exitbind(
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
    let observed = json(&observed);
    assert_eq!(observed["event"]["result"]["code"], 7);
    assert_eq!(observed["event"]["inputsSha256"], expected_inputs);
    assert_eq!(observed["warnings"][0]["classification"], "input_drift");
    assert_eq!(
        observed["warnings"][0]["expectedInputsSha256"],
        expected_inputs
    );
    assert_ne!(
        observed["warnings"][0]["currentInputsSha256"],
        expected_inputs
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reviewer_rework_makes_old_target_stale_before_observation_launch() {
    let (root, ledger, old_target) = checked_worker(
        "observe-reviewer-rework",
        "printf launched > reviewer-rework-launched.txt",
    );
    let rework_artifact = ".exitbind/artifacts/observe-reviewer-rework-request.md";
    fs::write(root.join(rework_artifact), "please rework\n").unwrap();
    let reviewer = call(
        &root,
        &[
            "run",
            "submit",
            "reviewer",
            &ledger,
            "--outcome",
            "rework",
            "--artifact",
            rework_artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(reviewer.status.success(), "{}", text(&reviewer));
    let fresh_artifact = ".exitbind/artifacts/observe-reviewer-rework-fresh.md";
    fs::write(root.join(fresh_artifact), "fresh completion\n").unwrap();
    let worker = call(
        &root,
        &[
            "run",
            "submit",
            "worker",
            &ledger,
            "--outcome",
            "completed",
            "--artifact",
            fresh_artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(worker.status.success(), "{}", text(&worker));
    let before = fs::read(root.join(&ledger)).unwrap();
    let rejected = call(
        &root,
        &[
            "run",
            "observe-check",
            &ledger,
            "--target",
            &old_target,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!rejected.status.success(), "{}", text(&rejected));
    assert!(text(&rejected).contains("current worker completion"));
    assert_eq!(fs::read(root.join(&ledger)).unwrap(), before);
    assert!(!root.join("reviewer-rework-launched.txt").exists());
    fs::remove_dir_all(root).unwrap();
}

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

fn checked_worker(label: &str, command: &str) -> (support::TempRoot, String, String) {
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

fn checked_exitbind_worker(label: &str, command: &str) -> (support::TempRoot, String, String) {
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

#[cfg(unix)]
fn wait_for_positive_pid(path: &Path, deadline: Instant) -> std::io::Result<i32> {
    loop {
        match fs::read_to_string(path) {
            Ok(contents) if !contents.trim().is_empty() => {
                return contents
                    .trim()
                    .parse::<i32>()
                    .ok()
                    .filter(|pid| *pid > 0)
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid child PID")
                    });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child PID was not published before the deadline",
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[path = "observe_check/observation_and_status.rs"]
mod observation_and_status;
#[path = "observe_check/pid_readiness.rs"]
mod pid_readiness;
#[path = "observe_check/timeouts_and_drift.rs"]
mod timeouts_and_drift;
#[path = "observe_check/verification_preparation.rs"]
mod verification_preparation;

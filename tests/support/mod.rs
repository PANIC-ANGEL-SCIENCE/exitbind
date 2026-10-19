use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256};

#[allow(dead_code)]
pub mod git_topology;

pub fn temp(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "exitbind-{label}-{}-{sequence}",
            std::process::id()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return std::fs::canonicalize(path).expect("canonicalize test directory"),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("create test directory: {error}"),
        }
    }
}

/// Rewrite a Work ledger's start event as a legacy (unmarked) start event and
/// recompute its hash chain. A Work handle is derived from the ledger file name
/// and assignment handles do not hash the start event, so both stay valid.
#[allow(dead_code)]
pub fn unmark_work(root: &Path, work: &str) {
    let token = work.strip_prefix("smw_").expect("Work locator");
    unmark_ledger(root, &format!(".exitbind/runs/work-{token}.jsonl"));
}

/// Same rewrite for a ledger that is not a Work locator, such as `run start`.
#[allow(dead_code)]
pub fn unmark_ledger(root: &Path, ledger: &str) {
    let path = root.join(ledger);
    let text = std::fs::read_to_string(&path).expect("read ledger");
    let mut events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("ledger event"))
        .collect::<Vec<_>>();
    for field in [
        "guidanceContinuityProtocol",
        "guidanceConfigSha256",
        "verificationInputContract",
    ] {
        events[0]
            .as_object_mut()
            .expect("ledger start event")
            .remove(field);
    }
    let mut previous = Value::Null;
    let mut lines = Vec::with_capacity(events.len());
    for event in &mut events {
        event["previousEventSha256"] = previous.clone();
        let mut unsigned = event.clone();
        unsigned
            .as_object_mut()
            .expect("ledger event")
            .remove("eventSha256");
        let event_hash = format!("{:x}", Sha256::digest(canonical(&unsigned).as_bytes()));
        event["eventSha256"] = Value::String(event_hash.clone());
        previous = Value::String(event_hash);
        lines.push(serde_json::to_string(event).expect("serialize ledger event"));
    }
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("rewrite ledger");
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON key serializes"),
                        canonical(&object[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => {
            format!(
                "[{}]",
                values.iter().map(canonical).collect::<Vec<_>>().join(",")
            )
        }
        _ => serde_json::to_string(value).expect("JSON value serializes"),
    }
}

/// Resolve a compact Work response's emitted read-only `work next --full` route.
/// The returned value is the expanded current action, never a replay of the
/// response's preceding mutation.
#[allow(dead_code)]
pub fn resolved_work_next(
    root: &Path,
    work: &str,
    response: &Value,
    expected_executable: &str,
) -> Value {
    if response["truncated"] != true {
        return response["next"].clone();
    }

    let command = response["fullCommand"]
        .as_array()
        .expect("a compact response supplies its fullCommand")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("fullCommand arguments are strings")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(command.len(), 7, "unexpected fullCommand: {command:?}");
    assert_eq!(
        std::fs::canonicalize(&command[0]).unwrap(),
        std::fs::canonicalize(expected_executable).unwrap(),
        "fullCommand must use the current test executable"
    );
    assert_eq!(command[1], "work");
    assert_eq!(command[2], "next");
    assert_eq!(command[3], work);
    assert_eq!(command[4], "--full");
    assert_eq!(command[5], "--config");
    assert_eq!(
        std::fs::canonicalize(&command[6]).unwrap(),
        std::fs::canonicalize(root.join("exitbind.json")).unwrap(),
        "fullCommand must use the fixture's current configuration"
    );

    let expanded = Command::new(&command[0])
        .current_dir(root)
        .args(&command[1..])
        .output()
        .expect("run the emitted read-only fullCommand");
    assert!(
        expanded.status.success(),
        "fullCommand failed: stdout={} stderr={}",
        String::from_utf8_lossy(&expanded.stdout),
        String::from_utf8_lossy(&expanded.stderr)
    );
    let expanded: Value = serde_json::from_slice(&expanded.stdout)
        .expect("fullCommand returns its complete JSON response");
    assert_ne!(expanded["truncated"], true);
    assert_eq!(expanded["work"], work);
    expanded["next"].clone()
}

/// Reconstruct an exact current Work argv from a bounded command descriptor.
/// The caller retains responsibility for checking command-specific operands
/// and executing the returned argv through its fixture's original executor.
#[allow(dead_code)]
pub fn reconstruct_work_command(
    descriptor: &Value,
    expected_executable: &Path,
    expected_config: &Path,
    invocation_cwd: &Path,
) -> Vec<String> {
    let same_executable = descriptor["sameExecutableRequired"]
        .as_bool()
        .expect("command descriptor states its executable requirement");
    let same_config = descriptor["sameConfigRequired"]
        .as_bool()
        .expect("command descriptor states its config requirement");
    let mut argv = descriptor["argv"]
        .as_array()
        .expect("command descriptor argv is an array")
        .iter()
        .map(|argument| {
            argument
                .as_str()
                .expect("command arguments are strings")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(!argv.is_empty(), "command descriptor argv is not empty");
    assert_eq!(
        argv.iter()
            .filter(|argument| argument.as_str() == "--config")
            .count(),
        1,
        "command descriptor has exactly one config boundary"
    );

    let expected_executable =
        std::fs::canonicalize(expected_executable).expect("canonical expected Work executable");
    if same_executable {
        assert_eq!(argv.first().map(String::as_str), Some("work"));
        argv.insert(
            0,
            expected_executable
                .to_str()
                .expect("Work executable path is UTF-8")
                .to_owned(),
        );
    } else {
        let path = std::path::Path::new(argv.first().unwrap());
        assert!(
            path.is_absolute(),
            "command descriptor cannot use PATH lookup"
        );
        assert_eq!(
            std::fs::canonicalize(path).expect("canonical command executable"),
            expected_executable,
            "command descriptor uses the current Work executable"
        );
    }
    assert_eq!(argv.get(1).map(String::as_str), Some("work"));

    let expected_config =
        std::fs::canonicalize(expected_config).expect("canonical expected Work config");
    if same_config {
        assert_eq!(argv.last().map(String::as_str), Some("--config"));
        argv.push(
            expected_config
                .to_str()
                .expect("Work config path is UTF-8")
                .to_owned(),
        );
    } else {
        assert!(argv.len() >= 3, "complete config descriptor has a suffix");
        assert_eq!(argv[argv.len() - 2], "--config");
        let route_config = std::path::Path::new(argv.last().unwrap());
        let route_config = if route_config.is_absolute() {
            route_config.to_path_buf()
        } else {
            invocation_cwd.join(route_config)
        };
        assert_eq!(
            std::fs::canonicalize(route_config).expect("canonical descriptor config"),
            expected_config,
            "command descriptor uses the current Work config"
        );
    }
    argv
}

/// Run a command under a pseudo-terminal on both GNU and BSD `script`.
///
/// GNU accepts `script -qec COMMAND /dev/null`; macOS uses `script -q
/// /dev/null sh -c COMMAND`. Keeping the platform spelling here preserves
/// the TTY-dependent presentation checks without skipping them on macOS.
#[allow(dead_code)]
pub fn pty(command: &str) -> Command {
    let mut process = Command::new("script");
    if cfg!(target_os = "macos") {
        process.args(["-q", "/dev/null", "sh", "-c", command]);
    } else {
        process.args(["-qec", command, "/dev/null"]);
    }
    process
}

/// Place an executable at `target` without racing another thread's fork.
///
/// Copying onto a path that a concurrently forked child inherited as an open
/// descriptor makes the following `exec` fail with `ETXTBSY`. Writing beside it
/// and renaming gives the path a fresh inode, so the exec cannot see a
/// half-written or still-held file.
#[allow(dead_code)]
pub fn place_executable(source: &Path, target: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let staging = target.with_file_name(format!(
        ".{}.staging-{}",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("binary"),
        std::process::id()
    ));
    std::fs::copy(source, &staging).expect("stage test binary");
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
        .expect("mark test binary executable");
    std::fs::rename(&staging, target).expect("place test binary");
}

/// Run a command that may have just been written to disk.
///
/// A concurrently forked child can still hold the write descriptor for the
/// binary's inode for the moment before it execs, and Linux answers `exec` on
/// such a file with `ETXTBSY`. That is a property of the harness, not of the
/// product, so wait briefly for the descriptor to go away instead.
#[allow(dead_code)]
pub fn run(command: &mut std::process::Command) -> std::process::Output {
    const TEXT_FILE_BUSY: i32 = 26;
    for _ in 0..200 {
        match command.output() {
            Ok(output) => return output,
            Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("run test command: {error}"),
        }
    }
    panic!("test binary stayed busy for two seconds")
}

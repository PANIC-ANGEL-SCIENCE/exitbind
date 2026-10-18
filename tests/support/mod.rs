use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

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

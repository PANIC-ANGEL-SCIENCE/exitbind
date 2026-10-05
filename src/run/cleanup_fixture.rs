//! Test-only ownership and bounded transport for the isolated reaping fixture.
use super::super::capture::process_group_exists;
use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use std::{
    fs::File,
    io::Read,
    process::{ExitStatus, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "linux")]
const CAPTURE_LIMIT: u64 = 64 * 1024;

#[cfg(target_os = "linux")]
pub(super) struct Captured {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
}

#[cfg(target_os = "linux")]
pub(super) fn isolated(test: &str, mode: &str) -> Result<Captured, String> {
    isolated_with_budget(test, mode, Duration::from_secs(5))
}

#[cfg(target_os = "linux")]
pub(super) fn isolated_with_budget(
    test: &str,
    mode: &str,
    budget: Duration,
) -> Result<Captured, String> {
    let root = std::env::temp_dir().join(format!(
        "exitbind-reaper-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).map_err(|error| format!("fixture directory: {error}"))?;
    let result = supervise(test, mode, &root, budget);
    match result {
        Ok(captured) => {
            let _ = fs::remove_dir_all(root);
            Ok(captured)
        }
        Err(error) => Err(format!(
            "{error}; fixture capture retained at {}",
            root.display()
        )),
    }
}

#[cfg(target_os = "linux")]
fn supervise(test: &str, mode: &str, root: &Path, budget: Duration) -> Result<Captured, String> {
    let stdout = root.join("stdout");
    let stderr = root.join("stderr");
    let started = Instant::now();
    // Files make completion independent of inherited descendant pipe handles.
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(["--exact", test, "--nocapture"])
        .env("EXITBIND_CLEANUP_REAPER_FIXTURE", mode)
        .env("EXITBIND_CLEANUP_FIXTURE_ROOT", root)
        .stdout(Stdio::from(
            File::create(&stdout).map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(
            File::create(&stderr).map_err(|e| e.to_string())?,
        ))
        .process_group(0)
        .spawn()
        .map_err(|error| format!("launch isolated reaper fixture: {error}"))?;
    let mut failure = None;
    let status = loop {
        // A delayed status poll cannot turn a deadline failure into success.
        if started.elapsed() >= budget {
            failure = Some(format!(
                "isolated fixture status deadline exceeded ({budget:?})"
            ));
            break stop_controller(&mut child, root)?;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if started.elapsed() >= budget {
                    failure = Some(format!(
                        "isolated fixture status deadline exceeded ({budget:?})"
                    ));
                }
                break status;
            }
            Ok(None) => {}
            Err(error) => {
                failure = Some(format!("isolated fixture status: {error}"));
                break stop_controller(&mut child, root)?;
            }
        }
        if [&stdout, &stderr].iter().any(|path| {
            fs::metadata(path)
                .map(|m| m.len() > CAPTURE_LIMIT)
                .unwrap_or(true)
        }) {
            failure = Some("isolated fixture capture exceeded its bound".into());
        }
        if failure.is_some() {
            break stop_controller(&mut child, root)?;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let cleanup = cleanup_registered(root);
    let captured = Captured {
        status,
        stdout: read_capture(&stdout)?,
        stderr: read_capture(&stderr)?,
        elapsed: started.elapsed(),
    };
    if let Some(failure) = failure {
        return Err(format!(
            "{failure}; status={status}; cleanup={cleanup:?}; stdout={}; stderr={}",
            captured.stdout, captured.stderr
        ));
    }
    cleanup?;
    Ok(captured)
}

#[cfg(target_os = "linux")]
fn read_capture(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(CAPTURE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > CAPTURE_LIMIT {
        return Err("isolated fixture capture exceeded its bound".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_os = "linux")]
fn stop_controller(child: &mut Child, root: &Path) -> Result<ExitStatus, String> {
    // Stop registered descendants first, allowing the subreaper to reap them.
    let cleanup = cleanup_registered(root);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(600) {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(2));
    }
    let group = i32::try_from(child.id()).map_err(|e| e.to_string())?;
    signal(group, libc::SIGTERM);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(250) {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(2));
    }
    signal(group, libc::SIGKILL);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(250) {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(2));
    }
    Err(format!("isolated fixture controller did not exit after forced termination; descendant cleanup={cleanup:?}"))
}

#[cfg(target_os = "linux")]
fn cleanup_registered(root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("group") {
            continue;
        }
        let group = fs::read_to_string(&path)
            .map_err(|e| e.to_string())?
            .parse::<i32>()
            .map_err(|e| e.to_string())?;
        if !process_group_exists(group).map_err(|e| e.to_string())? {
            continue;
        }
        // The separate ownership acknowledgement remains valid even when the
        // readiness record under test is missing, partial or malformed.
        let pid = readiness(&path.with_extension("owner"), group, Duration::ZERO)?;
        signal(group, libc::SIGTERM);
        thread::sleep(Duration::from_millis(250));
        signal(group, libc::SIGKILL);
        let started = Instant::now();
        while process_group_exists(group).map_err(|e| e.to_string())? {
            if started.elapsed() >= Duration::from_millis(350) {
                return Err(format!("owned fixture group {group} remained after termination (descendant {pid}); live/zombie state unresolved"));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

pub(super) struct OwnedGroup {
    pub child: Child,
    pub group: i32,
    pub marker: PathBuf,
    pub owner: PathBuf,
    registration: PathBuf,
}

impl OwnedGroup {
    pub fn launch(mode: &str, reap: bool) -> Self {
        let root = std::env::var_os("EXITBIND_CLEANUP_FIXTURE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let marker = root.join(format!(
            "exitbind-cleanup-{}-{reap}.ready",
            std::process::id()
        ));
        let owner = marker.with_extension("owner");
        let child = Command::new("sh")
            .args(["-c", "sh -c 'trap : TERM HUP; printf \"%s\\n\" $$ > \"$2\"; case \"$3\" in missing) ;; partial) printf \"%s\" $$ > \"$1\" ;; malformed) printf \"invalid\\n\" > \"$1\" ;; *) printf \"%s\\n\" $$ > \"$1\" ;; esac; while :; do :; done' fixture \"$1\" \"$2\" \"$3\" & exit 0", "fixture"])
            .arg(&marker).arg(&owner).arg(mode).process_group(0).spawn()
            .expect("cleanup fixture should launch");
        let group = i32::try_from(child.id()).expect("POSIX fixture group");
        let guard = Self {
            child,
            group,
            registration: marker.with_extension("group"),
            marker,
            owner,
        };
        fs::write(&guard.registration, group.to_string()).expect("register owned fixture group");
        guard
    }
}

impl Drop for OwnedGroup {
    fn drop(&mut self) {
        let started = Instant::now();
        signal(self.group, libc::SIGTERM);
        loop {
            // Never steal children from another fixture or parallel test.
            while unsafe { libc::waitpid(-self.group, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
            if matches!(process_group_exists(self.group), Ok(false)) {
                break;
            }
            if started.elapsed() >= Duration::from_millis(250) {
                signal(self.group, libc::SIGKILL);
            }
            if started.elapsed() >= Duration::from_millis(600) {
                eprintln!(
                    "owned fixture cleanup failed: group {} remains live/zombie/unknown",
                    self.group
                );
                return; // Leave its registration for the supervisor to diagnose.
            }
            thread::sleep(Duration::from_millis(2));
        }
        for path in [&self.marker, &self.owner, &self.registration] {
            let _ = fs::remove_file(path);
        }
    }
}

fn signal(group: i32, signal: i32) {
    unsafe { libc::kill(-group, signal) };
}

pub(super) fn readiness(path: &Path, group: i32, budget: Duration) -> Result<i32, String> {
    let started = Instant::now();
    loop {
        match fs::read_to_string(path) {
            Ok(record) if record.ends_with('\n') => {
                let pid = record
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| format!("malformed descendant readiness record: {record:?}"))?;
                if pid <= 0 || unsafe { libc::getpgid(pid) } != group {
                    return Err(format!("descendant readiness does not name a live member of owned group {group}: {pid}"));
                }
                return Ok(pid);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("descendant readiness: {error}")),
        }
        if started.elapsed() >= budget {
            return Err(format!(
                "descendant readiness deadline: missing or partial record for owned group {group}"
            ));
        }
        thread::sleep(Duration::from_millis(1));
    }
}

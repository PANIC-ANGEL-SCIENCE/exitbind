//! Typed observed-command execution and bounded capture ownership.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(all(unix, any(not(target_os = "linux"), test)))]
use std::process::Child;
use std::process::{ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const OBSERVE_POLL_MS: u64 = 10;
const OBSERVE_TERMINATION_GRACE_MS: u64 = 250;
const MAX_CAPTURE_COMBINED_BYTES: usize = 16 * 1_048_576;

#[derive(Clone)]
pub(crate) struct ObservationRequest {
    pub(crate) command: String,
    pub(crate) working_dir: PathBuf,
    pub(crate) artifact_dir: PathBuf,
    pub(crate) state_root: PathBuf,
    pub(crate) deadline: Instant,
    pub(crate) timeout_ms: u64,
    pub(crate) operation_id: String,
    pub(crate) capture: CapturePolicy,
    pub(crate) retain_partial: bool,
    /// Optional exclusively owned proof for an interrupted pre-return.
    pub(crate) durable_proof: Option<PathBuf>,
    pub(crate) preparation: Option<super::verification_preparation::RuntimePreparation>,
}

#[derive(Clone, Copy)]
pub(crate) struct CapturePolicy {
    pub(crate) per_stream_bytes: u64,
    pub(crate) combined_bytes: u64,
}

impl CapturePolicy {
    pub(crate) const FINITE: Self = Self {
        per_stream_bytes: crate::run_value::MAX_CAPTURE_BYTES,
        combined_bytes: MAX_CAPTURE_COMBINED_BYTES as u64,
    };
}

#[path = "capture_error.rs"]
mod error;
pub(crate) use error::{CaptureDisposition, ExecutionOutcome, ObservationError};

pub(crate) fn prelaunch_failure_facts(message: String) -> Value {
    ObservationError::new(
        message,
        ExecutionOutcome {
            status: None,
            duration_ms: 0,
            capture: CaptureDisposition::Disabled,
            group_ended: true,
            process_started: false,
        },
    )
    .facts()
}

struct OwnedPaths(Vec<PathBuf>);

impl OwnedPaths {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn add(&mut self, path: PathBuf) {
        self.0.push(path);
    }

    fn cleanup(&self) -> Vec<String> {
        self.0
            .iter()
            .filter_map(|path| match fs::remove_file(path) {
                Ok(()) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => Some(format!("{}: {error}", path.display())),
            })
            .collect()
    }

    fn remaining(&self) -> Vec<PathBuf> {
        self.0
            .iter()
            .filter(|path| path.exists())
            .cloned()
            .collect()
    }
}

fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

pub(crate) struct CapturedCheck {
    pub(crate) status: ExitStatus,
    pub(crate) duration_ms: u64,
    pub(crate) group_ended: bool,
    pub(crate) stdout_temp: Option<PathBuf>,
    pub(crate) stderr_temp: Option<PathBuf>,
    pub(crate) stdout_final: Option<PathBuf>,
    pub(crate) stderr_final: Option<PathBuf>,
    pub(crate) stdout_artifact: Option<Value>,
    pub(crate) stderr_artifact: Option<Value>,
}

impl CapturedCheck {
    pub(crate) fn without_logs(outcome: ExecutionOutcome) -> Self {
        let status = outcome
            .status
            .expect("successful execution always has an exit status");
        Self {
            status,
            duration_ms: outcome.duration_ms,
            group_ended: outcome.group_ended,
            stdout_temp: None,
            stderr_temp: None,
            stdout_final: None,
            stderr_final: None,
            stdout_artifact: None,
            stderr_artifact: None,
        }
    }
}

#[path = "capture_controlled.rs"]
mod controlled;
#[path = "capture_execution.rs"]
mod execution;
pub(crate) use execution::{run_observed_command, run_observed_command_with_control};

pub(crate) fn capture_observed_command(
    request: &ObservationRequest,
) -> Result<CapturedCheck, ObservationError> {
    capture_observed_command_with_control(request, || Ok(()))
}

pub(crate) fn capture_observed_command_with_control(
    request: &ObservationRequest,
    mut control: impl FnMut() -> Result<(), String>,
) -> Result<CapturedCheck, ObservationError> {
    control().map_err(|message| {
        ObservationError::new(
            format!("check control held before launch: {message}"),
            ExecutionOutcome {
                status: None,
                duration_ms: 0,
                capture: CaptureDisposition::Failed,
                group_ended: true,
                process_started: false,
            },
        )
    })?;
    crate::project::managed_files::ensure_state_directory(
        &request.state_root,
        &request.artifact_dir,
    )
    .map_err(|error| {
        ObservationError::new(
            error,
            ExecutionOutcome {
                status: None,
                duration_ms: 0,
                capture: CaptureDisposition::Failed,
                group_ended: true,
                process_started: false,
            },
        )
    })?;
    let log_id = &request.operation_id;
    let stdout_final = request
        .artifact_dir
        .join(format!("check-log-{log_id}-stdout.raw"));
    let stderr_final = request
        .artifact_dir
        .join(format!("check-log-{log_id}-stderr.raw"));
    let nonce = format!("{}-{}", std::process::id(), timestamp_nanos());
    let stdout_temp = request
        .artifact_dir
        .join(format!(".check-log-{log_id}-{nonce}-stdout.tmp"));
    let stderr_temp = request
        .artifact_dir
        .join(format!(".check-log-{log_id}-{nonce}-stderr.tmp"));
    let (outcome, owned) =
        controlled::capture_to_files(request, &stdout_temp, &stderr_temp, &mut control)?;
    let (stdout_artifact, stdout_bytes) = artifact_value(
        &request.state_root,
        &stdout_final,
        &stdout_temp,
        &owned,
        &outcome,
    )?;
    let (stderr_artifact, stderr_bytes) = artifact_value(
        &request.state_root,
        &stderr_final,
        &stderr_temp,
        &owned,
        &outcome,
    )?;
    if stdout_bytes.saturating_add(stderr_bytes) > request.capture.combined_bytes {
        return Err(ObservationError::new(
            format!(
                "combined check capture exceeded the {}-byte limit",
                request.capture.combined_bytes
            ),
            ExecutionOutcome {
                capture: CaptureDisposition::Failed,
                ..outcome
            },
        )
        .with_cleanup(&owned, owned.cleanup()));
    }
    Ok(CapturedCheck {
        status: outcome
            .status
            .expect("capture completed with process status"),
        duration_ms: outcome.duration_ms,
        group_ended: outcome.group_ended,
        stdout_temp: Some(stdout_temp),
        stderr_temp: Some(stderr_temp),
        stdout_final: Some(stdout_final),
        stderr_final: Some(stderr_final),
        stdout_artifact: Some(stdout_artifact),
        stderr_artifact: Some(stderr_artifact),
    })
}

fn capture_failure(
    message: String,
    started: Instant,
    status: Option<ExitStatus>,
    owned: &OwnedPaths,
    mut cleanup_errors: Vec<String>,
) -> ObservationError {
    cleanup_errors.extend(owned.cleanup());
    ObservationError::new(
        message,
        ExecutionOutcome {
            status,
            duration_ms: elapsed_ms(started),
            capture: CaptureDisposition::Failed,
            group_ended: status.is_some() && cleanup_errors.is_empty(),
            process_started: true,
        },
    )
    .with_cleanup(owned, cleanup_errors)
}

fn drain_capture_stream<R: Read>(
    mut reader: R,
    mut file: File,
    stream: &str,
    limit: u64,
) -> Result<u64, String> {
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("{stream} capture read failed: {error}"))?;
        if count == 0 {
            file.flush()
                .map_err(|error| format!("{stream} capture flush failed: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("{stream} capture sync failed: {error}"))?;
            return Ok(total);
        }
        if total.saturating_add(count as u64) > limit {
            return Err(format!(
                "{stream} capture exceeded the {}-byte stream limit",
                crate::run_value::MAX_CAPTURE_BYTES
            ));
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("{stream} capture write failed: {error}"))?;
        total += count as u64;
    }
}

fn artifact_value(
    state_root: &Path,
    final_path: &Path,
    temp_path: &Path,
    owned: &OwnedPaths,
    outcome: &ExecutionOutcome,
) -> Result<(Value, u64), ObservationError> {
    let result = (|| {
        let mut file = File::open(temp_path).map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            bytes = bytes
                .checked_add(count as u64)
                .ok_or_else(|| "captured log byte count overflowed".to_owned())?;
        }
        Ok::<_, String>((
            json!({
                "root": "state",
                "path": crate::config::rel(state_root, final_path).map_err(|error| error.to_string())?,
                "sha256": format!("{:x}", hasher.finalize()),
                "bytes": bytes,
            }),
            bytes,
        ))
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            Err(ObservationError::new(error, outcome.clone()).with_cleanup(owned, owned.cleanup()))
        }
    }
}

pub(crate) fn install_capture(
    temp: &std::path::Path,
    final_path: &std::path::Path,
) -> Result<bool, String> {
    match fs::symlink_metadata(final_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("captured log artifact already exists with invalid type".into());
            }
            if !same_file_contents(final_path, temp).map_err(|error| error.to_string())? {
                return Err("captured log artifact already exists with different bytes".into());
            }
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(temp, final_path).map_err(|error| error.to_string())?;
            Ok(true)
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn cleanup_capture(capture: &CapturedCheck) -> Result<(), String> {
    let mut errors = Vec::new();
    for path in [&capture.stdout_temp, &capture.stderr_temp]
        .into_iter()
        .flatten()
    {
        if let Err(error) = remove_owned(path) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub(crate) fn rollback_installed(
    capture: &CapturedCheck,
    installed_stdout: bool,
    installed_stderr: bool,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for (installed, path) in [
        (installed_stdout, capture.stdout_final.as_ref()),
        (installed_stderr, capture.stderr_final.as_ref()),
    ] {
        if installed {
            if let Some(path) = path {
                if let Err(error) = remove_owned(path) {
                    errors.push(error);
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn remove_owned(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn same_file_contents(left: &std::path::Path, right: &std::path::Path) -> io::Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }
    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut left_buffer = [0u8; 64 * 1024];
    let mut right_buffer = [0u8; 64 * 1024];
    loop {
        let left_count = left.read(&mut left_buffer)?;
        let right_count = right.read(&mut right_buffer)?;
        if left_count != right_count {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
        if left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
    }
}

#[cfg(all(unix, any(not(target_os = "linux"), test)))]
pub(crate) fn terminate_process_group(child: &mut Child) -> Result<(), String> {
    let process_group = match i32::try_from(child.id()) {
        Ok(process_group) => Some(process_group),
        Err(_) => None,
    };
    let mut failures = Vec::new();
    if process_group.is_none() {
        failures.push("check process identifier is outside the POSIX range".to_owned());
    }
    if let Some(process_group) = process_group {
        if let Err(error) = signal_process_group(process_group, libc::SIGTERM) {
            failures.push(format!("TERM cleanup failed: {error}"));
        }
    }

    let cleanup_started = Instant::now();
    let grace_started = Instant::now();
    let grace = Duration::from_millis(OBSERVE_TERMINATION_GRACE_MS);
    let cleanup_deadline = cleanup_started + grace.saturating_add(grace);
    let mut group_present = process_group.is_some();
    let mut leader_reaped = false;
    while grace_started.elapsed() < grace && Instant::now() < cleanup_deadline {
        match child.try_wait() {
            Ok(Some(_)) => leader_reaped = true,
            Ok(None) => {}
            Err(error) => failures.push(format!("check process could not be inspected: {error}")),
        }
        if let Some(process_group) = process_group {
            match process_group_exists(process_group) {
                Ok(false) => {
                    group_present = false;
                    break;
                }
                Ok(true) => {}
                Err(error) => failures.push(error),
            }
        }
        let remaining = grace.saturating_sub(grace_started.elapsed());
        if !remaining.is_zero() {
            thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
        }
    }
    if group_present {
        if let Some(process_group) = process_group {
            if let Err(error) = signal_process_group(process_group, libc::SIGKILL) {
                failures.push(format!("KILL cleanup failed: {error}"));
            }
        }
    }

    while !leader_reaped && Instant::now() < cleanup_deadline {
        match child.try_wait() {
            Ok(Some(_)) => leader_reaped = true,
            Ok(None) => {}
            Err(error) => {
                failures.push(format!("check process could not be reaped: {error}"));
                break;
            }
        }
        let remaining = cleanup_deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
        }
    }
    if !leader_reaped {
        failures.push("check process could not be reaped before cleanup deadline".to_owned());
    }

    while group_present && Instant::now() < cleanup_deadline {
        if let Some(process_group) = process_group {
            match process_group_exists(process_group) {
                Ok(false) => group_present = false,
                Ok(true) => {}
                Err(error) => failures.push(error),
            }
        } else {
            break;
        }
        if group_present {
            let remaining = cleanup_deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
            }
        }
    }
    if group_present {
        failures.push("check process group remained after forced termination".into());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        failures.sort();
        failures.dedup();
        Err(failures.join("; "))
    }
}

#[cfg(all(unix, any(not(target_os = "linux"), test)))]
fn signal_process_group(process_group: i32, signal: i32) -> Result<(), String> {
    if unsafe { libc::kill(-process_group, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!(
            "check process group could not be terminated: {error}"
        ))
    }
}

#[cfg(unix)]
pub(crate) fn process_group_exists(process_group: i32) -> Result<bool, String> {
    if unsafe { libc::kill(-process_group, 0) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(format!(
            "check process group could not be inspected: {error}"
        )),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub(crate) fn observed_result(status: &std::process::ExitStatus) -> Result<Value, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Ok(json!({"kind": "signal", "signal": signal}));
        }
    }
    status
        .code()
        .map(|code| json!({"kind": "exit", "code": code as u64}))
        .ok_or_else(|| "check command ended indeterminately; no mutation was made".into())
}

pub(super) fn retain_complete_failure(
    request: &ObservationRequest,
    capture: &CapturedCheck,
) -> Vec<Value> {
    let owned = OwnedPaths(
        [
            capture
                .stdout_temp
                .clone()
                .filter(|p| p.exists())
                .or_else(|| capture.stdout_final.clone()),
            capture
                .stderr_temp
                .clone()
                .filter(|p| p.exists())
                .or_else(|| capture.stderr_final.clone()),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
    error::retain_partial(request, &owned)
}

fn capture_prelaunch_failure(
    message: String,
    started: Instant,
    status: Option<ExitStatus>,
    owned: &OwnedPaths,
    errors: Vec<String>,
) -> ObservationError {
    let mut failure = capture_failure(message, started, status, owned, errors);
    failure.outcome.group_ended = true;
    failure.outcome.process_started = false;
    failure
}

//! Typed observed-command execution and bounded capture ownership.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
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

#[derive(Clone, Debug)]
pub(crate) enum CaptureDisposition {
    Disabled,
    Complete {
        stdout_bytes: u64,
        stderr_bytes: u64,
    },
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) struct ExecutionOutcome {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) duration_ms: u64,
    pub(crate) capture: CaptureDisposition,
}

pub(crate) struct ObservationError {
    message: String,
    outcome: ExecutionOutcome,
    cleanup_errors: Vec<String>,
    remaining_paths: Vec<PathBuf>,
}

impl ObservationError {
    fn new(message: impl Into<String>, outcome: ExecutionOutcome) -> Self {
        Self {
            message: message.into(),
            outcome,
            cleanup_errors: Vec::new(),
            remaining_paths: Vec::new(),
        }
    }

    fn with_cleanup(
        mut self,
        owned: &OwnedPaths,
        cleanup_errors: impl IntoIterator<Item = String>,
    ) -> Self {
        self.cleanup_errors.extend(cleanup_errors);
        self.remaining_paths = owned.remaining();
        self
    }
}

impl fmt::Display for ObservationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = self
            .outcome
            .status
            .as_ref()
            .map(|status| format!("{:?}", status))
            .unwrap_or_else(|| "unavailable".into());
        let capture = match &self.outcome.capture {
            CaptureDisposition::Disabled => "disabled".to_owned(),
            CaptureDisposition::Failed => "failed".to_owned(),
            CaptureDisposition::Complete {
                stdout_bytes,
                stderr_bytes,
            } => format!("complete(stdout={stdout_bytes}, stderr={stderr_bytes})"),
        };
        write!(
            output,
            "{} (status={status}, durationMs={}, capture={capture})",
            self.message, self.outcome.duration_ms
        )?;
        if !self.cleanup_errors.is_empty() {
            write!(
                output,
                "; cleanup failed: {}",
                self.cleanup_errors.join("; ")
            )?;
        }
        if !self.remaining_paths.is_empty() {
            let paths = self
                .remaining_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            write!(output, "; owned paths remain: {}", paths.join(", "))?;
        }
        Ok(())
    }
}

impl fmt::Debug for ObservationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, output)
    }
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
            stdout_temp: None,
            stderr_temp: None,
            stdout_final: None,
            stderr_final: None,
            stdout_artifact: None,
            stderr_artifact: None,
        }
    }
}

pub(crate) fn run_observed_command(
    request: &ObservationRequest,
) -> Result<ExecutionOutcome, ObservationError> {
    #[cfg(not(unix))]
    {
        let _ = request;
        return Err(ObservationError::new(
            "observe-check requires a POSIX host",
            ExecutionOutcome {
                status: None,
                duration_ms: 0,
                capture: CaptureDisposition::Disabled,
            },
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let child_stderr = child_stderr_stdio().map_err(|error| {
            ObservationError::new(
                error,
                ExecutionOutcome {
                    status: None,
                    duration_ms: 0,
                    capture: CaptureDisposition::Disabled,
                },
            )
        })?;
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&request.command)
            .arg("exitbind-observe-check")
            .current_dir(&request.working_dir)
            .stdout(child_stderr)
            .stderr(Stdio::inherit())
            .process_group(0);
        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            ObservationError::new(
                format!("check command could not be launched: {error}"),
                ExecutionOutcome {
                    status: None,
                    duration_ms: 0,
                    capture: CaptureDisposition::Disabled,
                },
            )
        })?;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(ExecutionOutcome {
                        status: Some(status),
                        duration_ms: elapsed_ms(started),
                        capture: CaptureDisposition::Disabled,
                    })
                }
                Ok(None) if Instant::now() >= request.deadline => {
                    let cleanup = terminate_process_group(&mut child);
                    let mut error = ObservationError::new(
                        format!(
                            "check observation timed out after {} ms",
                            request.timeout_ms
                        ),
                        ExecutionOutcome {
                            status: None,
                            duration_ms: elapsed_ms(started),
                            capture: CaptureDisposition::Disabled,
                        },
                    );
                    if let Err(cleanup) = cleanup {
                        error.cleanup_errors.push(cleanup);
                    }
                    return Err(error);
                }
                Ok(None) => {
                    let remaining = request.deadline.saturating_duration_since(Instant::now());
                    thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
                }
                Err(error) => {
                    let cleanup = terminate_process_group(&mut child);
                    let mut result = ObservationError::new(
                        format!("check command could not be observed: {error}"),
                        ExecutionOutcome {
                            status: None,
                            duration_ms: elapsed_ms(started),
                            capture: CaptureDisposition::Disabled,
                        },
                    );
                    if let Err(cleanup) = cleanup {
                        result.cleanup_errors.push(cleanup);
                    }
                    return Err(result);
                }
            }
        }
    }
}

fn child_stderr_stdio() -> Result<Stdio, String> {
    #[cfg(unix)]
    {
        use std::os::unix::io::{FromRawFd, RawFd};
        let fd: RawFd = unsafe { libc::dup(libc::STDERR_FILENO) };
        if fd < 0 {
            return Err(format!(
                "check command stderr could not be connected: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(unsafe { Stdio::from(std::fs::File::from_raw_fd(fd)) })
    }
    #[cfg(not(unix))]
    {
        Err("observe-check requires a POSIX host".into())
    }
}

pub(crate) fn capture_observed_command(
    request: &ObservationRequest,
) -> Result<CapturedCheck, ObservationError> {
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
    let (outcome, owned) = capture_to_files(request, &stdout_temp, &stderr_temp)?;
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
        stdout_temp: Some(stdout_temp),
        stderr_temp: Some(stderr_temp),
        stdout_final: Some(stdout_final),
        stderr_final: Some(stderr_final),
        stdout_artifact: Some(stdout_artifact),
        stderr_artifact: Some(stderr_artifact),
    })
}

fn capture_to_files(
    request: &ObservationRequest,
    stdout_temp: &Path,
    stderr_temp: &Path,
) -> Result<(ExecutionOutcome, OwnedPaths), ObservationError> {
    #[cfg(not(unix))]
    {
        let _ = (request, stdout_temp, stderr_temp);
        return Err(ObservationError::new(
            "observe-check requires a POSIX host",
            ExecutionOutcome {
                status: None,
                duration_ms: 0,
                capture: CaptureDisposition::Failed,
            },
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let started = Instant::now();
        let mut owned = OwnedPaths::new();
        let stdout_file =
            crate::project::managed_files::open_state_file(stdout_temp).map_err(|error| {
                capture_failure(
                    format!("stdout capture could not be created: {error}"),
                    started,
                    None,
                    &owned,
                    Vec::new(),
                )
            })?;
        owned.add(stdout_temp.to_owned());
        let stderr_file = match crate::project::managed_files::open_state_file(stderr_temp) {
            Ok(file) => file,
            Err(error) => {
                return Err(capture_failure(
                    format!("stderr capture could not be created: {error}"),
                    started,
                    None,
                    &owned,
                    Vec::new(),
                ));
            }
        };
        owned.add(stderr_temp.to_owned());
        let per_stream_bytes = request.capture.per_stream_bytes;
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&request.command)
            .arg("exitbind-observe-check")
            .current_dir(&request.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Err(capture_failure(
                    format!("check command could not be launched: {error}"),
                    started,
                    None,
                    &owned,
                    Vec::new(),
                ))
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let mut cleanup = Vec::new();
                if let Err(error) = terminate_process_group(&mut child) {
                    cleanup.push(error);
                }
                return Err(capture_failure(
                    "check stdout pipe was not available".into(),
                    started,
                    None,
                    &owned,
                    cleanup,
                ));
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                let mut cleanup = Vec::new();
                if let Err(error) = terminate_process_group(&mut child) {
                    cleanup.push(error);
                }
                return Err(capture_failure(
                    "check stderr pipe was not available".into(),
                    started,
                    None,
                    &owned,
                    cleanup,
                ));
            }
        };
        let (sender, receiver) = mpsc::channel();
        let stdout_thread = {
            let sender = sender.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    drain_capture_stream(stdout, stdout_file, "stdout", per_stream_bytes)
                }))
                .map_err(|_| "stdout capture reader panicked".to_owned())
                .and_then(|result| result);
                let _ = sender.send(("stdout", result));
            })
        };
        let stderr_thread = {
            let sender = sender.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    drain_capture_stream(stderr, stderr_file, "stderr", per_stream_bytes)
                }))
                .map_err(|_| "stderr capture reader panicked".to_owned())
                .and_then(|result| result);
                let _ = sender.send(("stderr", result));
            })
        };
        drop(sender);
        let mut stream_results = Vec::new();
        let mut stream_errors = Vec::new();
        let mut status = None;
        let mut timed_out = false;
        loop {
            while let Ok((stream, result)) = receiver.try_recv() {
                if let Err(error) = &result {
                    stream_errors.push(error.clone());
                }
                stream_results.push((stream, result));
            }
            if !stream_errors.is_empty() || (status.is_some() && stream_results.len() == 2) {
                break;
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(exit_status)) => status = Some(exit_status),
                    Ok(None) => {}
                    Err(error) => {
                        stream_errors.push(format!("check command could not be observed: {error}"));
                        break;
                    }
                }
            }
            if stream_results.len() == 2 {
                if status.is_some() {
                    break;
                }
                if Instant::now() >= request.deadline {
                    timed_out = true;
                    break;
                }
                let remaining = request.deadline.saturating_duration_since(Instant::now());
                thread::sleep(remaining.min(Duration::from_millis(OBSERVE_POLL_MS)));
                continue;
            }
            if Instant::now() >= request.deadline {
                timed_out = true;
                break;
            }
            let remaining = request.deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(Duration::from_millis(OBSERVE_POLL_MS));
            match receiver.recv_timeout(wait) {
                Ok((stream, result)) => {
                    if let Err(error) = &result {
                        stream_errors.push(error.clone());
                    }
                    stream_results.push((stream, result));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stream_errors.push(format!(
                        "capture reader stopped unexpectedly ({} stream result(s))",
                        stream_results.len()
                    ));
                }
            }
        }

        let process_cleanup = if timed_out || !stream_errors.is_empty() {
            // A failed reader can leave a descendant holding the other pipe
            // even after the shell leader has exited; always inspect and
            // terminate the process group on this path.
            terminate_process_group(&mut child)
        } else {
            Ok(())
        };
        let mut status_observation_error = None;
        if status.is_none() {
            match child.try_wait() {
                Ok(observed) => status = observed,
                Err(error) => {
                    status_observation_error = Some(format!(
                        "check process status could not be observed after cleanup: {error}"
                    ));
                }
            }
        }
        while stream_results.len() < 2 {
            match receiver.recv_timeout(Duration::from_millis(OBSERVE_TERMINATION_GRACE_MS)) {
                Ok((stream, result)) => {
                    if let Err(error) = &result {
                        stream_errors.push(error.clone());
                    }
                    stream_results.push((stream, result));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    stream_errors.push("capture readers did not finish after cleanup".into());
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stream_errors.push(format!(
                        "capture reader stopped unexpectedly ({} stream result(s))",
                        stream_results.len()
                    ));
                    break;
                }
            }
        }
        if stream_results.len() == 2 {
            let stdout_panicked = stdout_thread.join().is_err();
            let stderr_panicked = stderr_thread.join().is_err();
            if stdout_panicked || stderr_panicked {
                stream_errors.push("capture reader panicked".into());
            }
        } else {
            // A reader which did not report by the bounded cleanup deadline is
            // deliberately detached here: joining it could turn a failed
            // capture into an unbounded wait. Its pathname is removed below.
            drop(stdout_thread);
            drop(stderr_thread);
        }
        let mut cleanup_errors = Vec::new();
        if let Some(error) = status_observation_error {
            cleanup_errors.push(error);
        }
        if let Err(error) = process_cleanup {
            cleanup_errors.push(format!("check process cleanup failed: {error}"));
        }
        if timed_out {
            stream_errors.push(format!(
                "check observation timed out after {} ms",
                request.timeout_ms
            ));
        }
        if !stream_errors.is_empty() {
            return Err(capture_failure(
                stream_errors.join("; "),
                started,
                status,
                &owned,
                cleanup_errors,
            ));
        }
        let status = status.ok_or_else(|| {
            capture_failure(
                "check command ended without a status".into(),
                started,
                None,
                &owned,
                cleanup_errors.clone(),
            )
        })?;
        let mut stdout_bytes = None;
        let mut stderr_bytes = None;
        for (stream, result) in stream_results {
            if let Ok(bytes) = result {
                match stream {
                    "stdout" => stdout_bytes = Some(bytes),
                    "stderr" => stderr_bytes = Some(bytes),
                    _ => {}
                }
            }
        }
        Ok((
            ExecutionOutcome {
                status: Some(status),
                duration_ms: elapsed_ms(started),
                capture: CaptureDisposition::Complete {
                    stdout_bytes: stdout_bytes.unwrap_or_default(),
                    stderr_bytes: stderr_bytes.unwrap_or_default(),
                },
            },
            owned,
        ))
    }
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

#[cfg(unix)]
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

#[cfg(unix)]
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

//! Uncaptured historical execution; current observed logs use capture.rs.
use super::*;

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
                group_ended: false,
                process_started: false,
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
                    group_ended: false,
                    process_started: false,
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
                    group_ended: false,
                    process_started: false,
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
                        group_ended: i32::try_from(child.id())
                            .ok()
                            .and_then(|pid| process_group_exists(pid).ok())
                            == Some(false),
                        process_started: true,
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
                            status: child.try_wait().ok().flatten(),
                            duration_ms: elapsed_ms(started),
                            capture: CaptureDisposition::Disabled,
                            group_ended: cleanup.is_ok(),
                            process_started: true,
                        },
                    );
                    error.deadline_exceeded = true;
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
                            status: child.try_wait().ok().flatten(),
                            duration_ms: elapsed_ms(started),
                            capture: CaptureDisposition::Disabled,
                            group_ended: cleanup.is_ok(),
                            process_started: true,
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

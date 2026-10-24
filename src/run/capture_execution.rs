//! Uncaptured historical execution; current observed logs use capture.rs.
use super::*;

pub(crate) fn run_observed_command(
    request: &ObservationRequest,
) -> Result<ExecutionOutcome, ObservationError> {
    run_observed_command_with_control(request, || Ok(()))
}

pub(crate) fn run_observed_command_with_control(
    request: &ObservationRequest,
    mut control: impl FnMut() -> Result<(), String>,
) -> Result<ExecutionOutcome, ObservationError> {
    control().map_err(|message| {
        ObservationError::new(
            format!("check control held before launch: {message}"),
            ExecutionOutcome {
                status: None,
                duration_ms: 0,
                capture: CaptureDisposition::Disabled,
                group_ended: true,
                process_started: false,
            },
        )
    })?;
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
        let started = Instant::now();
        control().map_err(|message| {
            ObservationError::new(
                format!("check control held before launch: {message}"),
                ExecutionOutcome {
                    status: None,
                    duration_ms: elapsed_ms(started),
                    capture: CaptureDisposition::Disabled,
                    group_ended: true,
                    process_started: false,
                },
            )
        })?;
        let mut child = crate::run::capture_supervisor::ObservedChild::spawn(
            request,
            child_stderr,
            Stdio::inherit(),
        )
        .map_err(|error| {
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
            if let Err(message) = control() {
                let cleanup = child.terminate();
                let mut failure = ObservationError::new(
                    format!("check control held: {message}"),
                    ExecutionOutcome {
                        status: child.status(),
                        duration_ms: elapsed_ms(started),
                        capture: CaptureDisposition::Disabled,
                        group_ended: child.ended(),
                        process_started: child.process_started(),
                    },
                )
                .with_owned_group(child.owned_group());
                if let Err(cleanup) = cleanup {
                    failure.cleanup_errors.push(cleanup);
                }
                return Err(failure);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if child.timed_out() || child.cancelled() {
                        let mut failure = ObservationError::new(
                            if child.timed_out() {
                                format!(
                                    "check observation timed out after {} ms",
                                    request.timeout_ms
                                )
                            } else {
                                "check observation cancelled".into()
                            },
                            ExecutionOutcome {
                                status: Some(status),
                                duration_ms: elapsed_ms(started),
                                capture: CaptureDisposition::Disabled,
                                group_ended: child.ended(),
                                process_started: true,
                            },
                        )
                        .with_owned_group(child.owned_group());
                        failure.deadline_exceeded = child.timed_out();
                        return Err(failure);
                    }
                    return Ok(ExecutionOutcome {
                        status: Some(status),
                        duration_ms: elapsed_ms(started),
                        capture: CaptureDisposition::Disabled,
                        group_ended: child.ended(),
                        process_started: true,
                    });
                }
                Ok(None) if Instant::now() >= request.deadline => {
                    let cleanup = child.terminate();
                    let mut error = ObservationError::new(
                        format!(
                            "check observation timed out after {} ms",
                            request.timeout_ms
                        ),
                        ExecutionOutcome {
                            status: child.status(),
                            duration_ms: elapsed_ms(started),
                            capture: CaptureDisposition::Disabled,
                            group_ended: child.ended() && cleanup.is_ok(),
                            process_started: child.process_started(),
                        },
                    )
                    .with_owned_group(child.owned_group());
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
                    let cleanup = child.terminate();
                    let mut result = ObservationError::new(
                        format!("check command could not be observed: {error}"),
                        ExecutionOutcome {
                            status: child.status(),
                            duration_ms: elapsed_ms(started),
                            capture: CaptureDisposition::Disabled,
                            group_ended: child.ended() && cleanup.is_ok(),
                            process_started: child.process_started(),
                        },
                    )
                    .with_owned_group(child.owned_group());
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

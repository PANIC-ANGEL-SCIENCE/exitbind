//! Owned captured-child polling and bounded cleanup, including external holds.
use super::*;

pub(super) fn capture_to_files(
    request: &ObservationRequest,
    stdout_temp: &Path,
    stderr_temp: &Path,
    control: &mut impl FnMut() -> Result<(), String>,
) -> Result<(ExecutionOutcome, OwnedPaths), ObservationError> {
    #[cfg(not(unix))]
    {
        let _ = (request, stdout_temp, stderr_temp, control);
        return Err(ObservationError::new(
            "observe-check requires a POSIX host",
            ExecutionOutcome {
                status: None,
                duration_ms: 0,
                capture: CaptureDisposition::Failed,
                group_ended: false,
                process_started: false,
            },
        ));
    }
    #[cfg(unix)]
    {
        let started = Instant::now();
        let mut owned = OwnedPaths::new();
        let stdout_file =
            crate::project::managed_files::open_state_file(stdout_temp).map_err(|error| {
                capture_prelaunch_failure(
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
                return Err(capture_prelaunch_failure(
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
        control().map_err(|message| {
            capture_prelaunch_failure(
                format!("check control held before launch: {message}"),
                started,
                None,
                &owned,
                Vec::new(),
            )
        })?;
        let mut child = match crate::run::capture_supervisor::ObservedChild::spawn(
            request,
            Stdio::piped(),
            Stdio::piped(),
        ) {
            Ok(child) => child,
            Err(error) => {
                return Err(capture_prelaunch_failure(
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
                if let Err(error) = child.terminate() {
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
                if let Err(error) = child.terminate() {
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
            if let Err(error) = control() {
                stream_errors.push(format!("check control held: {error}"));
                break;
            }
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
            child.terminate()
        } else {
            Ok(())
        };
        let mut status_observation_error = None;
        if status.is_none() && child.process_started() {
            match child.try_wait() {
                Ok(observed) => status = observed,
                Err(error) => {
                    status_observation_error = Some(format!(
                        "check process status could not be observed after cleanup: {error}"
                    ));
                }
            }
            if status.is_none() {
                status = child.status();
            }
        }
        timed_out |= child.timed_out();
        if child.cancelled() {
            stream_errors.push("check observation cancelled".into());
        }
        while stream_results.len() < 2 {
            if let Err(error) = control() {
                let message = format!("check control held: {error}");
                if !stream_errors.contains(&message) {
                    stream_errors.push(message);
                }
            }
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
            let partial_captures = if request.retain_partial && stream_results.len() == 2 {
                error::retain_partial(request, &owned)
            } else {
                Vec::new()
            };
            let mut failure = capture_failure(
                stream_errors.join("; "),
                started,
                status,
                &owned,
                cleanup_errors,
            );
            failure.deadline_exceeded = timed_out;
            // An escaped descendant may hold a pipe after the owned group has
            // ended. Detached readers leave that effect uninspectably live.
            failure.capture_readers_ended = stream_results.len() == 2;
            failure.outcome.group_ended = child.ended();
            failure.outcome.process_started = child.process_started();
            failure.partial_captures = partial_captures;
            return Err(failure);
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
                process_started: true,
                group_ended: child.ended(),
                capture: CaptureDisposition::Complete {
                    stdout_bytes: stdout_bytes.unwrap_or_default(),
                    stderr_bytes: stderr_bytes.unwrap_or_default(),
                },
            },
            owned,
        ))
    }
}

#[cfg(all(test, unix))]
#[path = "capture_controlled_tests.rs"]
mod tests;

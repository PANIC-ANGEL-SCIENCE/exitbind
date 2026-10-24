//! Bounded process lifetime for a native Codex invocation.

use super::{ProcessIdentity, ProcessLiveness, RunError};
use serde_json::Value;
use std::io::Read;
use std::process::{Child, ExitStatus};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub(super) const POLL: Duration = Duration::from_millis(10);
pub(super) const TERM_GRACE: Duration = Duration::from_millis(250);
pub(super) const REAP_GRACE: Duration = Duration::from_secs(2);

pub(super) fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        process_group: i32::try_from(pid).ok(),
        start_time_ticks: start_time_ticks(pid),
    }
}

/// The process recording a check observation. It is this process, and it owns
/// no group marker: the admission records the PID and its start marker so a
/// later command can tell the observer ended without trusting PID reuse.
pub(super) fn observer_identity() -> ProcessIdentity {
    let pid = std::process::id();
    ProcessIdentity {
        pid,
        process_group: None,
        start_time_ticks: start_time_ticks(pid),
    }
}

/// PID-only liveness for a process whose group this one did not create. A
/// live PID with a different start marker was reused; a missing PID is ended
/// only when its start marker was observed, and an unverifiable one is never
/// ended.
pub(super) fn observer_liveness(identity: &ProcessIdentity) -> ProcessLiveness {
    #[cfg(unix)]
    {
        let pid = match libc::pid_t::try_from(identity.pid) {
            Ok(pid) if pid > 0 => pid,
            _ => return ProcessLiveness::Uncertain,
        };
        if unsafe { libc::kill(pid, 0) } == 0 {
            return match (identity.start_time_ticks, start_time_ticks(identity.pid)) {
                (Some(expected), Some(actual)) if expected == actual => ProcessLiveness::Alive,
                (Some(_), Some(_)) => ProcessLiveness::Ended,
                _ => ProcessLiveness::Uncertain,
            };
        }
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return if identity.start_time_ticks.is_some() {
                ProcessLiveness::Ended
            } else {
                ProcessLiveness::Uncertain
            };
        }
        ProcessLiveness::Uncertain
    }
    #[cfg(not(unix))]
    {
        let _ = identity;
        ProcessLiveness::Uncertain
    }
}

pub(super) fn liveness(identity: &ProcessIdentity) -> ProcessLiveness {
    #[cfg(unix)]
    {
        let pid = match libc::pid_t::try_from(identity.pid) {
            Ok(pid) if pid > 0 => pid,
            _ => return ProcessLiveness::Uncertain,
        };
        let result = unsafe { libc::kill(pid, 0) };
        if result == 0 {
            let state = match (identity.start_time_ticks, start_time_ticks(identity.pid)) {
                (Some(expected), Some(actual)) if expected == actual => ProcessLiveness::Alive,
                (Some(_), Some(_)) => ProcessLiveness::Ended,
                _ => ProcessLiveness::Uncertain,
            };
            return if state == ProcessLiveness::Ended {
                match group_liveness(identity) {
                    ProcessLiveness::Ended => ProcessLiveness::Ended,
                    ProcessLiveness::Alive | ProcessLiveness::Uncertain => {
                        ProcessLiveness::Uncertain
                    }
                }
            } else {
                state
            };
        }
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return match group_liveness(identity) {
                ProcessLiveness::Ended => ProcessLiveness::Ended,
                ProcessLiveness::Alive | ProcessLiveness::Uncertain => ProcessLiveness::Uncertain,
            };
        }
        ProcessLiveness::Uncertain
    }
    #[cfg(not(unix))]
    {
        let _ = identity;
        ProcessLiveness::Uncertain
    }
}

#[cfg(unix)]
pub(super) fn group_liveness(identity: &ProcessIdentity) -> ProcessLiveness {
    let Some(group) = identity.process_group.filter(|group| *group > 0) else {
        return ProcessLiveness::Uncertain;
    };
    if unsafe { libc::kill(-group, 0) } == 0 {
        return ProcessLiveness::Alive;
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        ProcessLiveness::Ended
    } else {
        ProcessLiveness::Uncertain
    }
}

#[cfg(not(unix))]
pub(super) fn group_liveness(_: &ProcessIdentity) -> ProcessLiveness {
    ProcessLiveness::Uncertain
}

#[cfg(target_os = "linux")]
fn start_time_ticks(pid: u32) -> Option<u64> {
    let source = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = source.rsplit_once(") ")?;
    fields.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
fn start_time_ticks(pid: u32) -> Option<u64> {
    // proc_pidinfo is supplied by the host's libproc, not a provider process.
    #[link(name = "proc")]
    extern "C" {}
    let native_pid = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0)?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    let observed = unsafe {
        libc::proc_pidinfo(
            native_pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if observed != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    macos_start_marker(
        pid,
        info.pbi_pid,
        info.pbi_start_tvsec,
        info.pbi_start_tvusec,
    )
}

// This is an opaque host-specific marker in the existing journal field. A
// missing, malformed or overflowing observation never becomes live identity.
#[cfg(any(test, target_os = "macos"))]
fn macos_start_marker(pid: u32, observed_pid: u32, seconds: u64, micros: u64) -> Option<u64> {
    if pid == 0 || observed_pid != pid || micros >= 1_000_000 {
        return None;
    }
    seconds
        .checked_mul(1_000_000)?
        .checked_add(micros)
        .filter(|marker| *marker > 0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn start_time_ticks(_: u32) -> Option<u64> {
    None
}

pub(super) fn capture_with_lines<R: Read>(
    mut reader: R,
    limit: usize,
    lines: mpsc::Sender<Vec<u8>>,
) -> super::Captured {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut line = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut oversized = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let retained = if bytes.len() < limit {
                    let keep = read.min(limit - bytes.len());
                    bytes.extend_from_slice(&buffer[..keep]);
                    if keep < read {
                        oversized = true;
                    }
                    &buffer[..keep]
                } else {
                    oversized = true;
                    &buffer[..0]
                };
                for byte in retained {
                    if line.len() <= limit {
                        line.push(*byte);
                    }
                    if *byte == b'\n' {
                        if line.len() <= limit {
                            let _ = lines.send(std::mem::take(&mut line));
                        } else {
                            line.clear();
                        }
                    }
                }
            }
            Err(_) => {
                oversized = true;
                break;
            }
        }
    }
    if !line.is_empty() && line.len() <= limit {
        let _ = lines.send(line);
    }
    super::Captured { bytes, oversized }
}

pub(super) fn drain_stream_lines(
    lines: &mpsc::Receiver<Vec<u8>>,
    want_final: bool,
    observer: &mut Option<&mut super::StreamObserver<'_>>,
) -> Result<(), RunError> {
    while let Ok(line) = lines.try_recv() {
        notify_stream_observer(&line, want_final, observer)?;
    }
    Ok(())
}

pub(super) fn drain_stream_lines_until_closed(
    lines: &mpsc::Receiver<Vec<u8>>,
    want_final: bool,
    observer: &mut Option<&mut super::StreamObserver<'_>>,
    grace: Duration,
) -> Result<(), RunError> {
    loop {
        match lines.recv_timeout(grace) {
            Ok(line) => notify_stream_observer(&line, want_final, observer)?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(RunError::InvalidRequestDetail(
                    "Codex stdout event reader did not finish after process exit".into(),
                ))
            }
        }
    }
}

fn notify_stream_observer(
    line: &[u8],
    want_final: bool,
    observer: &mut Option<&mut super::StreamObserver<'_>>,
) -> Result<(), RunError> {
    let Some(update) = early_stream_update(line, want_final) else {
        return Ok(());
    };
    if let Some(observer) = observer.as_mut() {
        observer(&update).map_err(RunError::Observer)?;
    }
    Ok(())
}

fn early_stream_update(line: &[u8], want_final: bool) -> Option<super::StreamObservation> {
    let event: Value = serde_json::from_slice(line).ok()?;
    let object = event.as_object()?;
    match object.get("type").and_then(Value::as_str) {
        Some("thread.started") => {
            let id = object.get("thread_id").and_then(Value::as_str)?;
            super::validate_id(Some(id), "thread ID").ok()?;
            Some(super::StreamObservation {
                thread_id: Some(id.to_owned()),
                final_result: None,
            })
        }
        Some("item.completed") if want_final => {
            let item = object.get("item")?.as_object()?;
            if item.get("type").and_then(Value::as_str) != Some("agent_message") {
                return None;
            }
            let text = item.get("text").and_then(Value::as_str)?;
            if text.len() > super::MAX_FINAL_BYTES {
                return None;
            }
            let final_result = serde_json::from_str(text).ok()?;
            Some(super::StreamObservation {
                thread_id: None,
                final_result: Some(final_result),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<(ExitStatus, bool), RunError> {
    wait_with_timeout_poll(child, timeout, || Ok(()))
}

pub(super) fn wait_with_timeout_poll<F>(
    child: &mut Child,
    timeout: Duration,
    mut poll: F,
) -> Result<(ExitStatus, bool), RunError>
where
    F: FnMut() -> Result<(), RunError>,
{
    let started = Instant::now();
    let owned = identity(child.id());
    loop {
        if let Err(error) = poll() {
            return Err(super::control::interrupt(child, &owned, error));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status, false)),
            Ok(None) if started.elapsed() >= timeout => break,
            Ok(None) => thread::sleep(POLL),
            Err(error) => {
                super::control::cleanup(child, &owned);
                return Err(RunError::Launch(error));
            }
        }
    }
    terminate_group(child);
    let deadline = Instant::now() + REAP_GRACE;
    loop {
        if let Err(error) = poll() {
            return Err(super::control::interrupt(child, &owned, error));
        }
        match child.try_wait().map_err(RunError::Launch)? {
            Some(status) => return Ok((status, true)),
            None if Instant::now() >= deadline => {
                return Err(RunError::InvalidRequest(
                    "Codex process did not exit after timeout cleanup",
                ));
            }
            None => thread::sleep(POLL),
        }
    }
}

pub(super) fn terminate_group(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(group) = i32::try_from(child.id()) {
            // command_line starts Codex in its own process group. Signal the
            // entire group, including tools still holding its output pipes.
            unsafe { libc::kill(-group, libc::SIGTERM) };
            thread::sleep(TERM_GRACE);
            unsafe { libc::kill(-group, libc::SIGKILL) };
        } else {
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    #[test]
    fn incremental_updates_match_authoritative_thread_and_final_fields() {
        let thread =
            early_stream_update(br#"{"type":"thread.started","thread_id":"thread-1"}"#, true)
                .unwrap();
        assert_eq!(thread.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(thread.final_result, None);

        let final_result = early_stream_update(
            br#"{"type":"item.completed","item":{"type":"agent_message","text":"{\"ok\":true}"}}"#,
            true,
        )
        .unwrap();
        assert_eq!(final_result.thread_id, None);
        assert_eq!(
            final_result.final_result,
            Some(serde_json::json!({"ok": true}))
        );
        assert!(early_stream_update(
            br#"{"type":"item.completed","item":{"type":"agent_message","text":"{\"ok\":true}"}}"#,
            false,
        )
        .is_none());
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn identity_classification_is_conservative_and_pid_reuse_safe() {
        let current = identity(std::process::id());
        assert_eq!(liveness(&current), ProcessLiveness::Alive);

        let mut mismatched = current.clone();
        mismatched.start_time_ticks = Some(0);
        assert_eq!(liveness(&mismatched), ProcessLiveness::Ended);

        let mut unverifiable = current;
        unverifiable.start_time_ticks = None;
        assert_eq!(liveness(&unverifiable), ProcessLiveness::Uncertain);
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn observer_liveness_uses_only_the_recorded_start_marker() {
        let observer = observer_identity();
        assert_eq!(observer.process_group, None);
        assert_eq!(
            observer_liveness(&observer),
            ProcessLiveness::Alive,
            "the recording process is alive"
        );

        let mut reused = observer.clone();
        reused.start_time_ticks = Some(0);
        assert_eq!(observer_liveness(&reused), ProcessLiveness::Ended);

        // A vanished observer without a start marker cannot be distinguished
        // from a reused PID, so it is never closed.
        let mut ended = Command::new("sh").arg("-c").arg("exit 0").spawn().unwrap();
        let ended_pid = ended.id();
        ended.wait().unwrap();
        let vanished = ProcessIdentity {
            pid: ended_pid,
            process_group: None,
            start_time_ticks: None,
        };
        assert_eq!(observer_liveness(&vanished), ProcessLiveness::Uncertain);
        let vanished_with_marker = ProcessIdentity {
            start_time_ticks: Some(7),
            ..vanished
        };
        assert_eq!(
            observer_liveness(&vanished_with_marker),
            ProcessLiveness::Ended
        );
    }

    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn unverifiable_process_start_remains_uncertain() {
        assert_eq!(
            liveness(&identity(std::process::id())),
            ProcessLiveness::Uncertain
        );
    }

    #[test]
    fn macos_start_marker_requires_exact_pid_and_valid_bounded_timestamp() {
        assert_eq!(macos_start_marker(42, 42, 123, 456), Some(123_000_456));
        assert_eq!(macos_start_marker(42, 43, 123, 456), None);
        assert_eq!(macos_start_marker(0, 0, 123, 456), None);
        assert_eq!(macos_start_marker(42, 42, 123, 1_000_000), None);
        assert_eq!(macos_start_marker(42, 42, u64::MAX, 0), None);
        assert_eq!(macos_start_marker(42, 42, 0, 0), None);
        assert_ne!(
            macos_start_marker(42, 42, 123, 456),
            macos_start_marker(42, 42, 123, 457)
        );
    }

    #[test]
    fn timeout_closes_a_grandchild_held_output_pipe() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("sleep 10 & wait")
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = sender.send(stdout.read_to_end(&mut bytes));
        });
        let started = Instant::now();
        let (_, timed_out) = wait_with_timeout(&mut child, Duration::from_millis(50)).unwrap();
        assert!(timed_out);
        let closed = receiver.recv_timeout(Duration::from_secs(2));
        if closed.is_err() {
            terminate_group(&mut child);
        }
        assert!(
            closed.is_ok(),
            "a child process retained the Codex output pipe"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

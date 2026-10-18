//! Owned, bounded subprocess output for read-only discovery operations.
//!
//! The child gets a private process group. The caller polls nonblocking pipes
//! and child status on one thread, so inherited pipe writers cannot hold a
//! reader join open after the direct child exits. Timeout, output overflow, or
//! SIGINT terminates only that owned process group and reaps the direct child.
//! This deliberately small API can also serve other read-only providers: pass
//! the command, separate stream caps and an absolute deadline; callers retain
//! responsibility for mapping these reasons into their own product status.

use std::io;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct BoundedOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct BoundedProcessError {
    pub(crate) reason: &'static str,
    pub(crate) detail: String,
    pub(crate) kind: Option<io::ErrorKind>,
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard};

    static CANCELLED: AtomicBool = AtomicBool::new(false);
    #[cfg(test)]
    pub(super) static CHILD_ACTIVE: AtomicBool = AtomicBool::new(false);
    #[cfg(test)]
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static INTERRUPT_LEASE: Mutex<()> = Mutex::new(());
    const POLL: Duration = Duration::from_millis(5);
    const TERMINATE_GRACE: Duration = Duration::from_millis(75);
    const CLEANUP_LIMIT: Duration = Duration::from_millis(450);
    const READ_CHUNK: usize = 16 * 1024;

    extern "C" fn record_interrupt(_: libc::c_int) {
        CANCELLED.store(true, Ordering::Relaxed);
    }

    fn cancelled() -> bool {
        CANCELLED.load(Ordering::Relaxed)
    }

    struct InterruptGuard {
        previous: libc::sigaction,
        _lease: MutexGuard<'static, ()>,
    }

    impl InterruptGuard {
        fn install(deadline: Instant) -> Result<Self, BoundedProcessError> {
            let lease = loop {
                match INTERRUPT_LEASE.try_lock() {
                    Ok(lease) => break lease,
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        return Err(failure(
                            "interrupt_handler_unavailable",
                            "bounded process interrupt lease is poisoned",
                        ));
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {
                        if Instant::now() >= deadline {
                            return Err(failure(
                                "deadline_exceeded",
                                "bounded command deadline expired waiting for interrupt ownership",
                            ));
                        }
                        std::thread::sleep(POLL);
                    }
                }
            };
            CANCELLED.store(false, Ordering::Relaxed);
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = record_interrupt as usize;
            unsafe { libc::sigemptyset(&mut action.sa_mask) };
            let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
            if unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous) } != 0 {
                return Err(failure(
                    "interrupt_handler_unavailable",
                    io::Error::last_os_error().to_string(),
                ));
            }
            #[cfg(test)]
            ACTIVE.store(true, Ordering::Release);
            Ok(Self {
                previous,
                _lease: lease,
            })
        }
    }

    impl Drop for InterruptGuard {
        fn drop(&mut self) {
            unsafe { libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut()) };
            CANCELLED.store(false, Ordering::Relaxed);
            #[cfg(test)]
            ACTIVE.store(false, Ordering::Release);
        }
    }

    #[cfg(test)]
    struct ChildActivityGuard;

    #[cfg(test)]
    impl Drop for ChildActivityGuard {
        fn drop(&mut self) {
            CHILD_ACTIVE.store(false, Ordering::Release);
        }
    }

    pub(crate) fn run(
        command: &mut Command,
        stdout_limit: usize,
        stderr_limit: usize,
        deadline: Instant,
    ) -> Result<BoundedOutput, BoundedProcessError> {
        let interrupt = InterruptGuard::install(deadline)?;
        if cancelled() {
            return Err(failure("cancelled", "operation interrupted before launch"));
        }
        if Instant::now() >= deadline {
            return Err(failure(
                "deadline_exceeded",
                "bounded command deadline expired before launch",
            ));
        }
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|error| BoundedProcessError {
            reason: "launch_failed",
            detail: format!("bounded command could not start: {error}"),
            kind: Some(error.kind()),
        })?;
        #[cfg(test)]
        let _child_activity = {
            CHILD_ACTIVE.store(true, Ordering::Release);
            ChildActivityGuard
        };
        // Unix process IDs and process-group IDs share the positive pid_t
        // range; the private group was established by `process_group(0)`.
        let group = child.id() as i32;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            cleanup_error(
                &mut child,
                group,
                "capture_unavailable",
                "bounded stdout pipe was not created",
            )
        })?;
        let mut stderr = child.stderr.take().ok_or_else(|| {
            cleanup_error(
                &mut child,
                group,
                "capture_unavailable",
                "bounded stderr pipe was not created",
            )
        })?;
        if let Err(error) =
            set_nonblocking(stdout.as_raw_fd()).and_then(|_| set_nonblocking(stderr.as_raw_fd()))
        {
            return Err(cleanup_error(
                &mut child,
                group,
                "capture_unavailable",
                error.to_string(),
            ));
        }

        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut stdout_eof = false;
        let mut stderr_eof = false;
        let mut status = None;
        loop {
            if cancelled() {
                let error = cleanup_error(
                    &mut child,
                    group,
                    "cancelled",
                    "bounded command cancelled by SIGINT",
                );
                drop(interrupt);
                return Err(error);
            }
            if Instant::now() >= deadline {
                let error = cleanup_error(
                    &mut child,
                    group,
                    "deadline_exceeded",
                    "bounded command deadline exceeded",
                );
                drop(interrupt);
                return Err(error);
            }
            if let Err(reason) = collect(
                &mut stdout,
                &mut out,
                stdout_limit,
                &mut stdout_eof,
                "stdout_limit_exceeded",
            ) {
                let error = cleanup_error(&mut child, group, reason, "bounded stdout failed");
                drop(interrupt);
                return Err(error);
            }
            if let Err(reason) = collect(
                &mut stderr,
                &mut err,
                stderr_limit,
                &mut stderr_eof,
                "stderr_limit_exceeded",
            ) {
                let error = cleanup_error(&mut child, group, reason, "bounded stderr failed");
                drop(interrupt);
                return Err(error);
            }
            if status.is_none() {
                status = match child.try_wait() {
                    Ok(status) => status,
                    Err(error) => {
                        return Err(cleanup_error(
                            &mut child,
                            group,
                            "wait_failed",
                            format!("bounded child status failed: {error}"),
                        ));
                    }
                };
            }
            let group_exists = match process_group_exists(group) {
                Ok(exists) => exists,
                Err(error) => {
                    return Err(cleanup_error(
                        &mut child,
                        group,
                        "process_group_unverified",
                        error.to_string(),
                    ));
                }
            };
            if status.is_some() && stdout_eof && stderr_eof && !group_exists {
                drop(interrupt);
                return Ok(BoundedOutput {
                    status: status.expect("status was checked"),
                    stdout: out,
                    stderr: err,
                });
            }
            std::thread::sleep(POLL);
        }
    }

    fn collect<R: Read>(
        stream: &mut R,
        bytes: &mut Vec<u8>,
        limit: usize,
        eof: &mut bool,
        overflow_reason: &'static str,
    ) -> Result<(), &'static str> {
        if *eof {
            return Ok(());
        }
        let mut buffer = [0u8; READ_CHUNK];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    *eof = true;
                    return Ok(());
                }
                Ok(count) => {
                    let remaining = limit.saturating_add(1).saturating_sub(bytes.len());
                    bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                    if bytes.len() > limit {
                        return Err(overflow_reason);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("pipe_read_failed"),
            }
        }
    }

    fn set_nonblocking(fd: libc::c_int) -> io::Result<()> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn process_group_exists(group: i32) -> io::Result<bool> {
        if unsafe { libc::kill(-group, 0) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else if error.raw_os_error() == Some(libc::EPERM) {
            Ok(true)
        } else {
            Err(error)
        }
    }

    fn cleanup(child: &mut Child, group: i32) -> bool {
        let _ = unsafe { libc::kill(-group, libc::SIGTERM) };
        let grace_deadline = Instant::now() + TERMINATE_GRACE;
        while Instant::now() < grace_deadline {
            let _ = child.try_wait();
            if process_group_exists(group).ok() == Some(false) {
                if child.try_wait().ok().flatten().is_some() {
                    return true;
                }
            }
            std::thread::sleep(POLL);
        }
        let _ = unsafe { libc::kill(-group, libc::SIGKILL) };
        let cleanup_deadline = Instant::now() + CLEANUP_LIMIT;
        loop {
            let leader_reaped = child.try_wait().ok().flatten().is_some();
            let group_ended = process_group_exists(group).ok() == Some(false);
            if leader_reaped && group_ended {
                return true;
            }
            if Instant::now() >= cleanup_deadline {
                return false;
            }
            std::thread::sleep(POLL);
        }
    }

    fn cleanup_error(
        child: &mut Child,
        group: i32,
        reason: &'static str,
        detail: impl Into<String>,
    ) -> BoundedProcessError {
        let detail = detail.into();
        if cleanup(child, group) {
            failure(reason, detail)
        } else {
            failure(
                "cleanup_incomplete",
                format!("{detail}; owned child group did not end within cleanup bound"),
            )
        }
    }

    fn failure(reason: &'static str, detail: impl Into<String>) -> BoundedProcessError {
        BoundedProcessError {
            reason,
            detail: detail.into(),
            kind: None,
        }
    }
}

#[cfg(unix)]
pub(crate) use unix::run;

#[cfg(not(unix))]
pub(crate) fn run(
    _command: &mut Command,
    _stdout_limit: usize,
    _stderr_limit: usize,
    _deadline: Instant,
) -> Result<BoundedOutput, BoundedProcessError> {
    Err(BoundedProcessError {
        reason: "bounded_process_unsupported",
        detail: "bounded subprocess ownership is unavailable on this platform".into(),
        kind: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[cfg(unix)]
    static SERIAL: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[cfg(unix)]
    #[test]
    fn waits_for_inherited_pipe_writer_until_deadline_then_cleans_group() {
        let _serial = SERIAL.lock().unwrap();
        let started = Instant::now();
        let error = run(
            &mut shell("sleep 5 & exit 0"),
            1024,
            1024,
            started + Duration::from_millis(100),
        )
        .expect_err("descendant keeps inherited pipes open");
        assert_eq!(error.reason, "deadline_exceeded");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn bounds_both_output_streams() {
        let _serial = SERIAL.lock().unwrap();
        for (script, expected) in [
            ("while :; do printf 12345678; done", "stdout_limit_exceeded"),
            (
                "(while :; do printf x; done) & exit 0",
                "stdout_limit_exceeded",
            ),
            (
                "while :; do printf 12345678 >&2; done",
                "stderr_limit_exceeded",
            ),
        ] {
            let error = run(
                &mut shell(script),
                32,
                32,
                Instant::now() + Duration::from_secs(1),
            )
            .expect_err("stream must exceed its cap");
            assert_eq!(error.reason, expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn successful_output_is_complete_and_status_is_owned() {
        let _serial = SERIAL.lock().unwrap();
        let output = run(
            &mut shell("printf ok; printf note >&2"),
            8,
            8,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("small process output completes");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"ok");
        assert_eq!(output.stderr, b"note");
    }

    #[cfg(unix)]
    #[test]
    fn sigint_cancels_only_owned_group_and_restores_previous_handler() {
        let _serial = SERIAL.lock().unwrap();
        let mut before = unsafe { std::mem::zeroed::<libc::sigaction>() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut before) },
            0
        );
        let sender = std::thread::spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(1);
            while !unix::CHILD_ACTIVE.load(std::sync::atomic::Ordering::Acquire)
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(unix::CHILD_ACTIVE.load(std::sync::atomic::Ordering::Acquire));
            unsafe { libc::kill(libc::getpid(), libc::SIGINT) }
        });
        let error = run(
            &mut shell("sleep 5 & wait"),
            1024,
            1024,
            Instant::now() + Duration::from_secs(2),
        )
        .expect_err("SIGINT cancels the owned process group");
        assert_eq!(sender.join().unwrap(), 0);
        assert_eq!(error.reason, "cancelled", "{error:?}");
        let mut after = unsafe { std::mem::zeroed::<libc::sigaction>() };
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut after) },
            0
        );
        assert_eq!(before.sa_sigaction, after.sa_sigaction);
        assert_eq!(before.sa_flags, after.sa_flags);
    }

    #[cfg(unix)]
    #[test]
    fn launch_failure_has_no_unowned_child_to_reap() {
        let _serial = SERIAL.lock().unwrap();
        let mut command = Command::new("/no/such/owned-command");
        let error = run(
            &mut command,
            32,
            32,
            Instant::now() + Duration::from_secs(1),
        )
        .expect_err("missing executable is explicit");
        assert_eq!(error.reason, "launch_failed");
        assert_eq!(error.kind, Some(io::ErrorKind::NotFound));
    }
}

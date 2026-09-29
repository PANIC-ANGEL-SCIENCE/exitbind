//! Bounded process lifetime for a native Codex invocation.

use super::RunError;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);
const TERM_GRACE: Duration = Duration::from_millis(250);
const REAP_GRACE: Duration = Duration::from_secs(2);

pub(super) fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<(ExitStatus, bool), RunError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status, false)),
            Ok(None) if started.elapsed() >= timeout => break,
            Ok(None) => thread::sleep(POLL),
            Err(error) => {
                terminate_group(child);
                return Err(RunError::Launch(error));
            }
        }
    }
    terminate_group(child);
    let deadline = Instant::now() + REAP_GRACE;
    loop {
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

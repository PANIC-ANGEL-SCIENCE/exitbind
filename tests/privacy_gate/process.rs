//! Bounded communication with a child owned by this privacy check.

use std::process::Command;
use std::time::Duration;

#[cfg(unix)]
pub(super) fn run(
    mut command: Command,
    input: &[u8],
    output_limit: usize,
    timeout: Duration,
) -> Option<Vec<u8>> {
    use std::io::{ErrorKind, Read, Write};
    use std::net::Shutdown;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;
    use std::time::Instant;

    let (mut input_parent, input_child) = UnixStream::pair().ok()?;
    let (mut output_parent, output_child) = UnixStream::pair().ok()?;
    input_parent.set_nonblocking(true).ok()?;
    output_parent.set_nonblocking(true).ok()?;
    let input_fd: OwnedFd = input_child.into();
    let output_fd: OwnedFd = output_child.into();
    let deadline = Instant::now().checked_add(timeout)?;
    let mut child = command
        .stdin(Stdio::from(input_fd))
        .stdout(Stdio::from(output_fd))
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // The builder retains its Stdio handles after spawn. Close those parent
    // copies so EOF reflects the owned child's actual output lifetime.
    drop(command);
    let mut sent = 0;
    let mut input_closed = false;
    let mut output_closed = false;
    let mut result = Vec::new();
    let mut status = None;
    let mut buffer = [0_u8; 8192];
    let outcome = loop {
        let mut advanced = false;
        if !input_closed {
            if sent == input.len() {
                if input_parent.shutdown(Shutdown::Write).is_err() {
                    break None;
                }
                input_closed = true;
                advanced = true;
            } else {
                match input_parent.write(&input[sent..]) {
                    Ok(0) => break None,
                    Ok(count) => {
                        sent += count;
                        advanced = true;
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => break None,
                }
            }
        }
        if !output_closed {
            match output_parent.read(&mut buffer) {
                Ok(0) => {
                    output_closed = true;
                    advanced = true;
                }
                Ok(count) => {
                    if count > output_limit.saturating_sub(result.len()) {
                        break None;
                    }
                    result.extend_from_slice(&buffer[..count]);
                    advanced = true;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(_) => break None,
            }
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => break None,
            }
        }
        if let Some(exit) = status {
            if !exit.success() {
                break None;
            }
            if input_closed && output_closed {
                break Some(result);
            }
        }
        if Instant::now() >= deadline {
            break None;
        }
        if !advanced {
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    outcome
}

#[cfg(not(unix))]
pub(super) fn run(
    _command: Command,
    _input: &[u8],
    _output_limit: usize,
    _timeout: Duration,
) -> Option<Vec<u8>> {
    // Unsupported pipe semantics retain the original privacy finding.
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::run;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn privacy_owned_child_drains_both_directions_and_observes_eof() {
        let input = vec![b'x'; 128 * 1024];
        let output = run(
            Command::new("/bin/cat"),
            &input,
            input.len(),
            Duration::from_secs(3),
        )
        .expect("bounded child should observe EOF");
        assert_eq!(output, input);
    }

    #[test]
    fn privacy_owned_child_output_over_limit_is_not_accepted() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 12345678"]);
        assert!(run(command, b"", 4, Duration::from_secs(3)).is_none());
    }
}

//! Fresh-exec capsule transport; no shared caller becomes a subreaper.
use super::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};

#[path = "linux/runtime.rs"]
mod runtime;
const MAGIC: &[u8; 8] = b"EXOBv1\0\0";
const ENTRY: &str = "--exitbind-observer-capsule-v1";
// magic (8) + group (4) + leader start marker (8) + status (4) + flags (4).
// The first twenty bytes are flushed as soon as the checker exists, so the
// group and its start marker stay re-readable when the proof cannot finish.
const PROOF_BYTES: usize = 28;
const DURABLE_MAGIC: &[u8; 8] = b"EXPRv1\0\0";
const DURABLE_HEADER_BYTES: usize = 8 + 64;

pub(super) struct Proof {
    reader: File,
    bytes: Vec<u8>,
    eof: bool,
    valid: bool,
}

impl Proof {
    pub(super) fn poll(&mut self) -> io::Result<()> {
        let mut buffer = [0u8; PROOF_BYTES + 1];
        loop {
            match self.reader.read(&mut buffer) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(());
                }
                Ok(count) => {
                    self.bytes.extend_from_slice(&buffer[..count]);
                    if self.bytes.len() > PROOF_BYTES {
                        return Err(io::Error::other("observer proof exceeds its fixed frame"));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// The checker's owned process group, recorded when it was launched. It
    /// stays readable when the capsule could not finish its proof, so a later
    /// read can observe the group itself; a missing or zero group is None.
    pub(super) fn owned_group(&self) -> Option<i32> {
        (self.bytes.len() >= 12 && self.bytes.get(..8) == Some(MAGIC))
            .then(|| i32::from_le_bytes(self.bytes[8..12].try_into().unwrap()))
            .filter(|group| *group > 0)
    }

    /// The owned group leader's start marker, recorded with the group. Zero
    /// (or a frame without it) means no marker is known, never that the group
    /// ended.
    pub(super) fn owned_leader_start(&self) -> Option<u64> {
        (self.bytes.len() >= 20 && self.bytes.get(..8) == Some(MAGIC))
            .then(|| u64::from_le_bytes(self.bytes[12..20].try_into().unwrap()))
            .filter(|ticks| *ticks > 0)
    }

    pub(super) fn status(&self) -> Option<ExitStatus> {
        (self.bytes.len() >= 24
            && self.bytes.get(..8) == Some(MAGIC)
            && i32::from_le_bytes(self.bytes[8..12].try_into().unwrap()) > 0)
            .then(|| {
                ExitStatus::from_raw(i32::from_le_bytes(self.bytes[20..24].try_into().unwrap()))
            })
    }

    pub(super) fn finish(&mut self, capsule: ExitStatus) -> io::Result<()> {
        self.poll()?;
        self.valid = capsule.success() && self.eof && valid_frame(&self.bytes);
        if self.valid {
            Ok(())
        } else {
            Err(io::Error::other(
                "observer capsule ended without a complete valid proof",
            ))
        }
    }

    pub(super) fn ended(&self) -> bool {
        if self.launch_failed() {
            return true;
        }
        if !self.valid || self.bytes[24] & 1 == 0 {
            return false;
        }
        let group = i32::from_le_bytes(self.bytes[8..12].try_into().unwrap());
        process_group_exists(group).ok() == Some(false)
    }

    pub(super) fn launch_failed(&self) -> bool {
        self.valid && self.bytes[24] == 8
    }

    pub(super) fn cancelled(&self) -> bool {
        self.valid && self.bytes[24] & 4 != 0
    }

    pub(super) fn timed_out(&self) -> bool {
        self.valid && self.bytes[24] & 2 != 0
    }
}

fn valid_frame(bytes: &[u8]) -> bool {
    if bytes.len() != PROOF_BYTES || bytes.get(..8) != Some(MAGIC) || bytes[25..] != [0, 0, 0] {
        return false;
    }
    if bytes[8..24] == [0; 16] && bytes[24] == 8 {
        return true;
    }
    let group = i32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let status = ExitStatus::from_raw(i32::from_le_bytes(bytes[20..24].try_into().unwrap()));
    group > 0 && (status.code().is_some() || status.signal().is_some()) && bytes[24] <= 7
}

/// A complete, synced capsule frame says its owned children were drained.
/// It does not prove the recorder's exit status or grant passing-check credit.
pub(super) fn durable_evidence(
    path: &std::path::Path,
    attempt_id: &str,
) -> Result<serde_json::Value, String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let metadata =
        std::fs::symlink_metadata(path).map_err(|e| format!("observer proof unavailable: {e}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err("observer proof is not a private regular file".into());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take((DURABLE_HEADER_BYTES + PROOF_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if attempt_id.len() != 64
        || bytes.len() != DURABLE_HEADER_BYTES + PROOF_BYTES
        || bytes.get(..8) != Some(DURABLE_MAGIC)
        || bytes.get(8..DURABLE_HEADER_BYTES) != Some(attempt_id.as_bytes())
    {
        return Err("observer proof does not bind the admitted attempt".into());
    }
    let frame = &bytes[DURABLE_HEADER_BYTES..];
    if !valid_frame(frame) || frame[24] & 1 == 0 {
        return Err("observer proof is incomplete; execution remains unknown".into());
    }
    let group = i32::from_le_bytes(frame[8..12].try_into().unwrap());
    let start = u64::from_le_bytes(frame[12..20].try_into().unwrap());
    if start == 0 || process_group_exists(group).map_err(|e| e.to_string())? {
        return Err("observer group identity is unavailable or still present".into());
    }
    Ok(
        serde_json::json!({"proofSha256":crate::evidence::hash::bytes(&bytes),
        "ownedGroup":group,"ownedLeaderStartTicks":start,"ownedChildrenDrained":true,
        "nativeExit":"unknown","checkCredit":false}),
    )
}

pub(super) fn spawn(
    request: &ObservationRequest,
    stdout: Stdio,
    stderr: Stdio,
) -> io::Result<ObservedChild> {
    let mut disposition = unsafe { std::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut disposition) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if disposition.sa_sigaction == libc::SIG_IGN || disposition.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(io::Error::other(
            "observer requires waitable capsule children",
        ));
    }
    let (read, write): (File, OwnedFd) = if let Some(path) = &request.durable_proof {
        if request.operation_id.len() != 64
            || !request
                .operation_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(io::Error::other(
                "durable observer attempt identity is invalid",
            ));
        }
        let mut write = crate::project::managed_files::open_state_file(path)?;
        write.write_all(DURABLE_MAGIC)?;
        write.write_all(request.operation_id.as_bytes())?;
        write.sync_all()?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        let mut read = File::open(path)?;
        read.seek(SeekFrom::Start(DURABLE_HEADER_BYTES as u64))?;
        (read, write.into())
    } else {
        let mut fds = [-1; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        if unsafe { libc::fcntl(read.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        (File::from(read), write)
    };
    let fd = write.as_raw_fd();
    #[cfg(test)]
    let test_streams = test_streams(fd)?;
    let remaining = request.deadline.saturating_duration_since(Instant::now());
    let mut command = capsule_command(
        fd,
        remaining.as_millis().min(u64::MAX as u128) as u64,
        &request.command,
    )?;
    #[cfg(test)]
    command.env(
        "EXITBIND_TEST_CAPSULE_STREAMS",
        format!(
            "{},{}",
            test_streams.0.as_raw_fd(),
            test_streams.1.as_raw_fd()
        ),
    );
    command
        .current_dir(&request.working_dir)
        .envs(
            request
                .preparation
                .as_ref()
                .map(|value| value.environment())
                .unwrap_or_default(),
        )
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0);
    unsafe {
        #[cfg(test)]
        {
            let out = test_streams.0.as_raw_fd();
            let err = test_streams.1.as_raw_fd();
            let null = test_streams.2.as_raw_fd();
            command.pre_exec(move || {
                // The harness writes only to /dev/null. The one entry test
                // restores the dedicated checker streams before capsule code.
                for (from, to) in [(1, out), (2, err), (null, 1), (null, 2)] {
                    if libc::dup2(from, to) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        // Only this owned write end survives the capsule exec. All proof FDs
        // become CLOEXEC again before any checker executable is launched.
        command.pre_exec(move || {
            if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let child = command.spawn()?;
    drop(write);
    Ok(ObservedChild {
        child,
        proof: Proof {
            reader: read,
            bytes: Vec::new(),
            eof: false,
            valid: false,
        },
    })
}

#[cfg(test)]
fn test_streams(fd: i32) -> io::Result<(OwnedFd, OwnedFd, File)> {
    let clone = || {
        let copied = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
        if copied < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(copied) })
        }
    };
    Ok((clone()?, clone()?, File::open("/dev/null")?))
}

#[cfg(test)]
pub(super) fn test_entry() {
    let Ok(args) = std::env::var("EXITBIND_TEST_CAPSULE_ARGS") else {
        return;
    };
    let (fd, timeout, shell): (i32, u64, String) = serde_json::from_str(&args).unwrap();
    let streams = std::env::var("EXITBIND_TEST_CAPSULE_STREAMS").unwrap();
    for (source, target) in streams.split(',').zip([1, 2]) {
        let source: i32 = source.parse().unwrap();
        assert_eq!(unsafe { libc::dup2(source, target) }, target);
        assert_eq!(unsafe { libc::close(source) }, 0);
    }
    match std::env::var("EXITBIND_TEST_CAPSULE_PRIOR").as_deref() {
        Ok("subreaper") => {
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
                0
            );
        }
        Ok(mode @ ("ignore" | "no_cld_wait")) => {
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = if mode == "ignore" {
                libc::SIG_IGN
            } else {
                libc::SIG_DFL
            };
            action.sa_flags = if mode == "no_cld_wait" {
                libc::SA_NOCLDWAIT
            } else {
                0
            };
            assert_eq!(
                unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) },
                0
            );
        }
        _ => {}
    }
    std::process::exit(runtime::run(fd, timeout, &shell));
}

#[cfg(test)]
pub(super) fn test_cleanup() -> bool {
    runtime::fixture_cleanup()
}

fn capsule_command(fd: i32, timeout: u64, shell: &str) -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    #[cfg(not(test))]
    command.args([ENTRY, &fd.to_string(), &timeout.to_string(), shell]);
    #[cfg(test)]
    command
        .args([
            "--exact",
            "run::capture_supervisor::tests::capsule_entry",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(
            "EXITBIND_TEST_CAPSULE_ARGS",
            serde_json::to_string(&(fd, timeout, shell)).unwrap(),
        );
    Ok(command)
}

pub(crate) fn entry() -> Option<i32> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().map_or(true, |arg| arg != ENTRY) {
        return None;
    }
    let parsed = (|| {
        if args.len() != 4 {
            return None;
        }
        Some((
            args[1].to_str()?.parse::<i32>().ok()?,
            args[2].to_str()?.parse::<u64>().ok()?,
            args[3].to_str()?.to_owned(),
        ))
    })();
    Some(match parsed {
        Some((fd, timeout, shell)) if fd > 2 => runtime::run(fd, timeout, &shell),
        _ => 1,
    })
}

/// A successful waitid establishes an unreaped current child. With capsule-
/// local default SIGCHLD and no competing waiter its PID cannot be reused
/// between this check and kill. A candidate list never grants authority.
pub(super) fn signal_owned(pid: i32, signal: i32) -> io::Result<bool> {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid as u32,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ECHILD) {
            return Ok(false);
        }
        return Err(e);
    }
    // Already-dead owned children require reaping, never another signal.
    if unsafe { info.si_pid() } != 0 {
        return Ok(false);
    }
    if unsafe { libc::kill(pid, signal) } == 0 {
        return Ok(true);
    }
    Err(io::Error::last_os_error())
}

#[cfg(test)]
#[test]
fn proof_rejects_truncation_duplicates_invalid_status_and_trailing_bytes() {
    let mut frame = MAGIC.to_vec();
    frame.extend_from_slice(&1i32.to_le_bytes());
    frame.extend_from_slice(&99u64.to_le_bytes());
    frame.extend_from_slice(&0i32.to_le_bytes());
    frame.extend_from_slice(&[1, 0, 0, 0]);
    let make = |bytes| Proof {
        reader: File::open("/dev/null").unwrap(),
        bytes,
        eof: false,
        valid: false,
    };
    assert_eq!(frame.len(), PROOF_BYTES);
    assert_eq!(make(frame.clone()).owned_group(), Some(1));
    assert_eq!(make(frame.clone()).owned_leader_start(), Some(99));
    assert!(make(frame.clone()).finish(ExitStatus::from_raw(0)).is_ok());
    let mut cases = vec![
        frame[..27].to_vec(),
        [frame.clone(), frame.clone()].concat(),
    ];
    for (offset, value) in [(0, b'X'), (24, 8), (25, 1), (20, 127)] {
        let mut invalid = frame.clone();
        invalid[offset] = value;
        cases.push(invalid);
    }
    for bytes in cases {
        let mut proof = make(bytes);
        assert!(proof.finish(ExitStatus::from_raw(0)).is_err());
        assert!(!proof.ended());
    }
    assert!(make(frame).finish(ExitStatus::from_raw(9)).is_err());
    let mut no_launch = MAGIC.to_vec();
    no_launch.extend_from_slice(&[0; 16]);
    assert!(make(no_launch.clone()).status().is_none());
    no_launch.extend_from_slice(&[8, 0, 0, 0]);
    let mut proof = make(no_launch.clone());
    assert!(proof.finish(ExitStatus::from_raw(0)).is_ok());
    assert!(proof.launch_failed() && proof.ended());
    assert!(proof.status().is_none());
    for flags in [0, 1, 2, 4, 9, 10, 12, 15] {
        let mut invalid = no_launch.clone();
        invalid[24] = flags;
        assert!(make(invalid).finish(ExitStatus::from_raw(0)).is_err());
    }
}

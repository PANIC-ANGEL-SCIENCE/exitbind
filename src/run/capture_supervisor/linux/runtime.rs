//! Single child-tree owner, entered only by a fresh exclusive process.
use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
static CANCELLED: AtomicBool = AtomicBool::new(false);
#[path = "inventory.rs"]
mod inventory;
const POLL: Duration = Duration::from_millis(5);
const DRAIN_BATCH: usize = 4096;
// Leave scheduling/transport room in the parent's total 500 ms cleanup bound.
const CLEANUP_GRACE: Duration = Duration::from_millis(200);

extern "C" fn cancel(_: i32) {
    CANCELLED.store(true, Ordering::Relaxed);
}

struct Lease {
    subreaper: i32,
    sigchld: libc::sigaction,
    active: bool,
}
impl Lease {
    fn acquire() -> io::Result<Self> {
        let mut lease = Self {
            subreaper: 0,
            sigchld: unsafe { std::mem::zeroed() },
            active: false,
        };
        if fault("acquire")
            || unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &mut lease.subreaper, 0, 0, 0) }
                != 0
        {
            return Err(io::Error::other("observer subreaper acquisition failed"));
        }
        if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut lease.sigchld) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        lease.active = true;
        // This exclusive exec owns its disposition before creating any child.
        // Inherited SIG_IGN / SA_NOCLDWAIT cannot silently erase child identity.
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = libc::SIG_DFL;
        if unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        action.sa_sigaction = cancel as usize;
        if unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(lease)
    }
    fn finish(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let restored =
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, self.subreaper, 0, 0, 0) } == 0;
        let signal_restored =
            unsafe { libc::sigaction(libc::SIGCHLD, &self.sigchld, std::ptr::null_mut()) } == 0;
        let mut actual_reaper = -1;
        let mut actual_signal = unsafe { std::mem::zeroed::<libc::sigaction>() };
        let verified =
            unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &mut actual_reaper, 0, 0, 0) } == 0
                && unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut actual_signal) }
                    == 0
                && actual_reaper == self.subreaper
                && actual_signal.sa_sigaction == self.sigchld.sa_sigaction
                && actual_signal.sa_flags & libc::SA_NOCLDWAIT
                    == self.sigchld.sa_flags & libc::SA_NOCLDWAIT;
        if restored && signal_restored && verified && !fault("restore") {
            self.active = false;
            Ok(())
        } else {
            Err(io::Error::other("observer ownership restoration failed"))
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub(super) fn run(fd: i32, timeout: u64, shell: &str) -> i32 {
    // Captured streams belong exclusively to the checker. Transport failures
    // are diagnosed by the caller, never appended to the checker's raw stderr.
    if execute(fd, timeout, shell).is_ok() {
        0
    } else {
        1
    }
}

fn execute(fd: i32, timeout: u64, shell: &str) -> io::Result<()> {
    // Refuse descriptors which could redirect proof onto a captured stream.
    if fd <= 2 || unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::other("invalid observer proof descriptor"));
    }
    let mut proof = unsafe { File::from_raw_fd(fd) };
    let mut lease = match Lease::acquire() {
        Ok(lease) => lease,
        Err(_) => return no_launch(&mut proof),
    };
    let child = Command::new("sh")
        .args(["-c", shell, "exitbind-observe-check"])
        .process_group(0)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(_) => {
            let _ = lease.finish();
            return no_launch(&mut proof);
        }
    };
    // Record the owned group and its leader's start marker as soon as the
    // checker exists, so a capsule that cannot finish its proof still leaves a
    // re-readable identity behind. The status half of the frame follows when
    // the leader is reaped.
    proof.write_all(MAGIC)?;
    proof.write_all(&(child.id() as i32).to_le_bytes())?;
    proof.write_all(&start_marker(child.id()).unwrap_or(0).to_le_bytes())?;
    flush_proof(&mut proof)?;
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let mut status = None;
    let mut cleanup_at = None;
    let mut complete = false;
    let mut failed = false;
    let mut cleanup_error = None;
    let mut timed_out = false;
    let mut cancelled = false;
    let mut empty_once = false;
    loop {
        let now = Instant::now();
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(s)) => {
                    status = Some(s);
                    proof.write_all(&s.into_raw().to_le_bytes())?;
                    flush_proof(&mut proof)?;
                }
                Ok(None) => {}
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if cleanup_at.is_none() && (CANCELLED.load(Ordering::Relaxed) || now >= deadline) {
            timed_out = now >= deadline;
            cancelled = CANCELLED.load(Ordering::Relaxed);
            cleanup_at = Some(now);
        }
        if status.is_none() && !failed {
            let end = cleanup_at
                .map(|started| started + CLEANUP_GRACE + CLEANUP_GRACE)
                .unwrap_or(deadline)
                .min(Instant::now() + POLL);
            if let Err(error) = reap_exited_adoptees(child.id() as i32, end) {
                failed = true;
                cleanup_error = Some(error.to_string());
                cleanup_at.get_or_insert(now);
            }
        }
        if let Some(started) = cleanup_at {
            let signal = if now.duration_since(started) < CLEANUP_GRACE {
                libc::SIGTERM
            } else {
                libc::SIGKILL
            };
            if let Err(e) = signal_children(
                child.id() as i32,
                signal,
                started + CLEANUP_GRACE + CLEANUP_GRACE,
            ) {
                failed = true;
                if cleanup_error.is_none() {
                    cleanup_error = Some(e.to_string());
                }
            }
            if now.duration_since(started) >= CLEANUP_GRACE + CLEANUP_GRACE {
                break;
            }
        }
        if status.is_some() {
            match drain() {
                Ok(true) if empty_once => {
                    complete = true;
                    break;
                }
                Ok(true) => empty_once = true,
                Ok(false) => empty_once = false,
                Err(_) => {
                    failed = true;
                    break;
                }
            }
            // Preserve ownership until natural ECHILD or actual cancellation.
        }
        std::thread::sleep(POLL);
    }
    if status.is_none() {
        // No generic reap is permitted until std::Child owns leader status.
        if let Some(s) = child.try_wait()? {
            proof.write_all(&s.into_raw().to_le_bytes())?;
        } else {
            return Err(io::Error::other(format!(
                "observer could not own exact leader status; cleanup: {}",
                cleanup_error.as_deref().unwrap_or("no signal error")
            )));
        }
    }
    let restored = lease.finish().is_ok();
    let flags = u8::from(complete && !failed && restored)
        | (u8::from(timed_out) << 1)
        | (u8::from(cancelled) << 2);
    if !fault("proof") {
        proof.write_all(&[flags, 0, 0, 0])?;
        flush_proof(&mut proof)?;
    }
    Ok(())
}

fn no_launch(proof: &mut File) -> io::Result<()> {
    proof.write_all(MAGIC)?;
    proof.write_all(&[0; 16])?;
    proof.write_all(&[8, 0, 0, 0])?;
    flush_proof(proof)
}

fn flush_proof(proof: &mut File) -> io::Result<()> {
    proof.flush()?;
    if proof.metadata()?.is_file() {
        proof.sync_all()?;
    }
    Ok(())
}

/// The checker leader's start marker in host clock ticks, when readable right
/// after spawn. Zero means no marker is known; the reader filters it.
fn start_marker(pid: u32) -> Option<u64> {
    let stat = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
    let text = std::str::from_utf8(&stat).ok()?;
    let (_, fields) = text.rsplit_once(") ")?;
    fields
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
        .filter(|ticks| *ticks > 0)
}

/// Reap only waitable adoptees while the checker leader still has a Child
/// owner. Peeking first prevents a concurrent leader exit from being reaped
/// by a generic wait, which would lose the checker's exact status.
fn reap_exited_adoptees(leader: i32, deadline: Instant) -> io::Result<()> {
    if fault("active-reap") {
        return Err(io::Error::other("observer active child drain failed"));
    }
    for _ in 0..DRAIN_BATCH {
        if Instant::now() >= deadline {
            // Yield to deadline/cancellation handling; this proves no cleanup.
            return Ok(());
        }
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        let result = unsafe {
            libc::waitid(
                libc::P_ALL,
                0,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        let pid = unsafe { info.si_pid() };
        if pid == 0 || pid == leader {
            return Ok(());
        }
        if pid < 0 {
            return Err(io::Error::other(
                "observer waitable child identity is invalid",
            ));
        }
        let waited = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        if waited == pid {
            continue;
        }
        if waited < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        return Err(io::Error::other("observer waitable child was not reaped"));
    }
    Err(io::Error::other(
        "observer active child drain exceeded its bounded batch",
    ))
}

fn drain() -> io::Result<bool> {
    // A finite batch prevents a hostile churn loop from defeating deadlines.
    for _ in 0..DRAIN_BATCH {
        let result = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if result > 0 {
            continue;
        }
        if result == 0 {
            return Ok(false);
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ECHILD) {
            return Ok(true);
        }
        if e.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(e);
    }
    Err(io::Error::other(
        "observer child drain exceeded its bounded batch",
    ))
}

fn signal_children(leader: i32, signal: i32, deadline: Instant) -> io::Result<()> {
    signal_owned(leader, signal)?;
    for pid in inventory::candidates(deadline)? {
        if Instant::now() >= deadline {
            return Err(io::Error::other("observer signal deadline exceeded"));
        }
        signal_owned(pid, signal)?;
    }
    Ok(())
}

fn fault(name: &str) -> bool {
    #[cfg(test)]
    {
        std::env::var("EXITBIND_TEST_CAPSULE_FAULT").as_deref() == Ok(name)
    }
    #[cfg(not(test))]
    {
        let _ = name;
        false
    }
}

#[cfg(test)]
pub(super) fn fixture_cleanup() -> bool {
    // Used only after an exclusive fixture's ordinary child owners have gone
    // out of scope. Never invoked by production or a parallel host process.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if let Ok(candidates) = inventory::candidates(deadline) {
            for pid in candidates {
                let _ = signal_owned(pid, libc::SIGKILL);
            }
        }
        if drain().ok() == Some(true) {
            return true;
        }
        std::thread::sleep(POLL);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn waitable_leader_keeps_exact_child_status() {
        const TEST: &str = "run::capture_supervisor::linux::runtime::tests::waitable_leader_keeps_exact_child_status";
        if super::super::super::tests::isolated(TEST, "leader-peek-race") {
            return;
        }
        let _owner = super::super::super::tests::FixtureOwner::new();
        for (script, code, signal) in [
            ("exit 37", Some(37), None),
            ("kill -TERM $$", None, Some(libc::SIGTERM)),
        ] {
            let mut leader = Command::new("sh")
                .args(["-c", script])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        leader.id(),
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                assert_eq!(result, 0, "{}", io::Error::last_os_error());
                if unsafe { info.si_pid() } == leader.id() as i32 {
                    break;
                }
                assert!(Instant::now() < deadline, "leader did not become waitable");
                std::thread::sleep(Duration::from_millis(2));
            }
            // Force the race: the previous try_wait could have returned None,
            // but the leader is now waitable before the adoptee scan.
            reap_exited_adoptees(leader.id() as i32, deadline).unwrap();
            let status = leader.wait().expect("the Child still owns leader status");
            assert_eq!(status.code(), code);
            assert_eq!(status.signal(), signal);
        }
    }
}

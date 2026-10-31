use super::*;
use crate::run::capture::{CapturePolicy, ObservationRequest};
use std::io::Read;
use std::path::PathBuf;

#[test]
#[ignore = "exclusive capsule entry, launched by the observer tests"]
fn capsule_entry() {
    linux::test_entry();
}

fn request(command: &str, timeout: u64) -> ObservationRequest {
    ObservationRequest {
        command: command.into(),
        working_dir: std::env::current_dir().unwrap(),
        artifact_dir: PathBuf::new(),
        state_root: PathBuf::new(),
        deadline: Instant::now() + Duration::from_millis(timeout),
        timeout_ms: timeout,
        operation_id: "capsule-test".into(),
        capture: CapturePolicy::FINITE,
        retain_partial: false,
        durable_proof: None,
        preparation: None,
    }
}

fn observe(command: &str, timeout: u64) -> (ExitStatus, bool, bool, Vec<u8>, Vec<u8>) {
    let mut child =
        ObservedChild::spawn(&request(command, timeout), Stdio::piped(), Stdio::piped()).unwrap();
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let read = |mut stream: Box<dyn Read + Send>| {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream.read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        });
        receiver
    };
    let out = read(Box::new(out));
    let err = read(Box::new(err));
    let deadline = Instant::now() + Duration::from_millis(timeout + 1000);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => panic!("capsule proof/status failure: {e}"),
        }
        assert!(Instant::now() < deadline, "capsule test exceeded bound");
        std::thread::sleep(Duration::from_millis(5));
    };
    let read = |receiver: std::sync::mpsc::Receiver<io::Result<Vec<u8>>>| {
        receiver
            .recv_timeout(Duration::from_millis(250))
            .expect("bounded fixture stream completion")
            .unwrap()
    };
    (
        status,
        child.ended(),
        child.timed_out(),
        read(out),
        read(err),
    )
}

#[test]
fn orphan_generations_preserve_exact_nonzero_signal_and_streams() {
    use std::os::unix::process::ExitStatusExt;
    for (tail, code, signal) in [
        ("exit 0", Some(0), None),
        ("exit 101", Some(101), None),
        ("kill -TERM $$", None, Some(libc::SIGTERM)),
    ] {
        let command = format!(
            "( (sleep 0.02) & exit 0 ) & printf 'raw\\000out'; printf 'raw\\377err' >&2; {tail}"
        );
        let (status, ended, timed, out, err) = observe(&command, 1000);
        assert_eq!(status.code(), code);
        assert_eq!(status.signal(), signal);
        assert!(ended);
        assert!(!timed);
        assert_eq!(out, b"raw\0out");
        assert_eq!(err, b"raw\xfferr");
    }
}

#[test]
fn stubborn_and_escaped_children_are_reaped_after_timeout_without_success() {
    const TEST: &str = "run::capture_supervisor::tests::stubborn_and_escaped_children_are_reaped_after_timeout_without_success";
    if isolated(TEST, "timeout") {
        return;
    }
    let _owner = FixtureOwner::new();
    for command in [
        "trap '' TERM; sh -c 'trap \"\" TERM; exec sleep 30' & wait",
        "python3 -c 'import os,time; os.setsid(); time.sleep(30)' & wait",
    ] {
        let started = Instant::now();
        let (status, ended, timed, _, _) = observe(command, 100);
        assert!(!status.success());
        assert!(ended);
        assert!(timed);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn natural_background_completion_is_reaped_without_signals() {
    const TEST: &str =
        "run::capture_supervisor::tests::natural_background_completion_is_reaped_without_signals";
    if isolated(TEST, "survivor") {
        return;
    }
    let _owner = FixtureOwner::new();
    let started = Instant::now();
    let (status, ended, timed, out, err) =
        observe("sleep 0.4 >/dev/null 2>&1 & printf '%s' $!; exit 0", 2000);
    assert!(status.success());
    assert!(ended);
    assert!(!timed);
    assert!(err.is_empty());
    let pid: i32 = std::str::from_utf8(&out).unwrap().parse().unwrap();
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "natural child not reaped"
    );
    assert!(started.elapsed() >= Duration::from_millis(350));
    assert!(started.elapsed() < Duration::from_secs(1));
}

fn isolated(test: &str, mode: &str) -> bool {
    if std::env::var("EXITBIND_SUPERVISOR_TEST_MODE").as_deref() == Ok(mode) {
        return false;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env("EXITBIND_SUPERVISOR_TEST_MODE", mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("isolated supervisor fixture exceeded deadline: {mode}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{mode}: stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[test]
fn prelaunch_is_known_while_missing_and_restoration_proof_remain_unknown() {
    const TEST: &str = "run::capture_supervisor::tests::prelaunch_is_known_while_missing_and_restoration_proof_remain_unknown";
    let mode = std::env::var("EXITBIND_SUPERVISOR_TEST_MODE").unwrap_or_default();
    if !["acquire", "restore", "proof"].contains(&mode.as_str()) {
        for mode in ["acquire", "restore", "proof"] {
            assert!(isolated(TEST, mode));
        }
        return;
    }
    let _owner = FixtureOwner::new();
    std::env::set_var("EXITBIND_TEST_CAPSULE_FAULT", &mode);
    let mut child = ObservedChild::spawn(
        &request("printf checker-error >&2; exit 23", 1000),
        Stdio::null(),
        Stdio::piped(),
    )
    .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                assert_eq!(mode, "restore");
                assert_eq!(status.code(), Some(23));
                break;
            }
            Err(_) => {
                assert_ne!(mode, "restore");
                break;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(child.ended(), mode == "acquire");
    assert_eq!(child.process_started(), mode != "acquire");
    if mode != "acquire" {
        assert_eq!(child.status().unwrap().code(), Some(23));
    }
    let mut bytes = Vec::new();
    stderr.read_to_end(&mut bytes).unwrap();
    assert_eq!(
        bytes,
        if mode == "acquire" {
            b"" as &[u8]
        } else {
            b"checker-error"
        }
    );
}

#[test]
fn stale_candidate_and_externally_reaped_capsule_never_grant_signal_authority() {
    for _ in 0..32 {
        let mut external = Command::new("sh").args(["-c", "exit 47"]).spawn().unwrap();
        assert_eq!(external.wait().unwrap().code(), Some(47));
        assert!(!linux::signal_owned(external.id() as i32, libc::SIGKILL).unwrap());
    }
    let mut child =
        ObservedChild::spawn(&request("exit 0", 1000), Stdio::null(), Stdio::null()).unwrap();
    assert_eq!(
        unsafe { libc::waitpid(child.id() as i32, std::ptr::null_mut(), 0) },
        child.id() as i32
    );
    assert!(!linux::signal_owned(child.id() as i32, libc::SIGTERM).unwrap());
    assert!(child.try_wait().is_err());
    assert!(!child.ended());
}

#[test]
fn proof_descriptor_does_not_reach_checker_and_stdout_cannot_forge_it() {
    let (status, ended, _, out, err) = observe("python3 -c 'import os,json; fd=json.loads(os.environ[\"EXITBIND_TEST_CAPSULE_ARGS\"])[0];\ntry: os.fstat(fd)\nexcept OSError: print(\"closed\")\nelse: raise RuntimeError(\"proof inherited\")'; printf EXOBv1", 1000);
    assert!(status.success() && ended);
    assert_eq!(out, b"closed\nEXOBv1");
    assert!(err.is_empty());
}

/// The proof identity is written and flushed when the checker exists, so a
/// read while the capsule still runs must first take the bytes already queued
/// on the pipe; reading the buffer alone would find nothing.
#[cfg(target_os = "linux")]
#[test]
fn a_running_capsule_reveals_its_owned_group_from_the_proof_frame() {
    let mut child =
        ObservedChild::spawn(&request("sleep 30", 5000), Stdio::null(), Stdio::null()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let owned = loop {
        if let Some(owned) = child.owned_group() {
            break owned;
        }
        assert!(
            Instant::now() < deadline,
            "a running capsule revealed no owned group"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(owned.group > 0);
    assert_ne!(owned.group, child.id() as i32);
    assert!(owned.start_time_ticks.is_some());
    assert_eq!(unsafe { libc::kill(-owned.group, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) => {}
        }
        assert!(
            Instant::now() < deadline,
            "the capsule did not end after its group was killed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn capsule_owns_waitable_disposition_and_restores_prior_subreaper_state() {
    const TEST: &str = "run::capture_supervisor::tests::capsule_owns_waitable_disposition_and_restores_prior_subreaper_state";
    let mode = std::env::var("EXITBIND_SUPERVISOR_TEST_MODE").unwrap_or_default();
    if !["subreaper", "ignore", "no_cld_wait"].contains(&mode.as_str()) {
        for mode in ["subreaper", "ignore", "no_cld_wait"] {
            assert!(isolated(TEST, mode));
        }
        return;
    }
    let _owner = FixtureOwner::new();
    std::env::set_var("EXITBIND_TEST_CAPSULE_PRIOR", mode);
    let (status, ended, _, _, _) = observe("(sleep 0.01) & exit 31", 1000);
    assert_eq!(status.code(), Some(31));
    assert!(ended);
}

#[test]
fn parent_automatic_reap_refuses_before_capsule_launch() {
    const TEST: &str =
        "run::capture_supervisor::tests::parent_automatic_reap_refuses_before_capsule_launch";
    let mode = std::env::var("EXITBIND_SUPERVISOR_TEST_MODE").unwrap_or_default();
    if !["parent_ignore", "parent_no_cld_wait"].contains(&mode.as_str()) {
        for mode in ["parent_ignore", "parent_no_cld_wait"] {
            assert!(isolated(TEST, mode));
        }
        return;
    }
    let mut prior = unsafe { std::mem::zeroed::<libc::sigaction>() };
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = if mode == "parent_ignore" {
        libc::SIG_IGN
    } else {
        libc::SIG_DFL
    };
    action.sa_flags = if mode == "parent_no_cld_wait" {
        libc::SA_NOCLDWAIT
    } else {
        0
    };
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGCHLD, &action, &mut prior) },
        0
    );
    let refused =
        ObservedChild::spawn(&request("exit 0", 1000), Stdio::null(), Stdio::null()).is_err();
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGCHLD, &prior, std::ptr::null_mut()) },
        0
    );
    assert!(refused);
}

#[test]
fn forced_fallback_cleans_owned_tree_and_inventory_error_stays_unknown() {
    const TEST: &str = "run::capture_supervisor::tests::forced_fallback_cleans_owned_tree_and_inventory_error_stays_unknown";
    let mode = std::env::var("EXITBIND_SUPERVISOR_TEST_MODE").unwrap_or_default();
    if !["fallback", "inventory"].contains(&mode.as_str()) {
        for mode in ["fallback", "inventory"] {
            assert!(isolated(TEST, mode));
        }
        return;
    }
    let _owner = FixtureOwner::new();
    std::env::set_var("EXITBIND_TEST_CAPSULE_FAULT", &mode);
    let (status, ended, timed, _, _) = observe(
        if mode == "fallback" {
            "trap '' TERM; sh -c 'trap \"\" TERM; exec sleep 2' & wait"
        } else {
            "trap '' TERM; exec sleep 2"
        },
        100,
    );
    assert!(!status.success());
    assert!(timed);
    assert_eq!(ended, mode == "fallback");
    // The negative uses an exec'd sole leader: no unowned orphan is left by
    // deliberately making descendant inventory unavailable.
}

#[test]
fn capsule_death_is_unknown_and_exclusive_fixture_reaps_survivors() {
    const TEST: &str = "run::capture_supervisor::tests::capsule_death_is_unknown_and_exclusive_fixture_reaps_survivors";
    if isolated(TEST, "capsule-death") {
        return;
    }
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let mut child = ObservedChild::spawn(
        &request("printf r; sleep 0.05", 1000),
        Stdio::piped(),
        Stdio::null(),
    )
    .unwrap();
    let mut ready = [0u8; 1];
    child
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut ready)
        .unwrap();
    assert_eq!(&ready, b"r");
    assert!(linux::signal_owned(child.id() as i32, libc::SIGKILL).unwrap());
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Err(_) => break,
            Ok(None) => {}
            Ok(Some(_)) => panic!("dead capsule supplied valid proof"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!child.ended());
    loop {
        let waited = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if waited == -1 {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(libc::ECHILD)
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn unrelated_child_and_concurrent_observations_keep_wait_ownership() {
    let mut unrelated = Command::new("sh")
        .args(["-c", "sleep 0.05; exit 73"])
        .spawn()
        .unwrap();
    let first = std::thread::spawn(|| observe("(sleep 0.01) & exit 0", 1000));
    let second = std::thread::spawn(|| observe("(sleep 0.01) & exit 19", 1000));
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert!(first.0.success() && first.1);
    assert_eq!(second.0.code(), Some(19));
    assert!(second.1);
    assert_eq!(unrelated.wait().unwrap().code(), Some(73));
}

#[test]
fn early_reader_failure_cancels_owned_tree_without_claiming_deadline_expiry() {
    const TEST: &str = "run::capture_supervisor::tests::early_reader_failure_cancels_owned_tree_without_claiming_deadline_expiry";
    if isolated(TEST, "reader-failure") {
        return;
    }
    let _owner = FixtureOwner::new();
    let root = crate::test_root::temp("capsule-reader");
    let mut req = request("printf oversized; exec sleep 2", 30_000);
    req.state_root = root.to_path_buf();
    req.artifact_dir = root.join(".exitbind/artifacts");
    req.capture = CapturePolicy {
        per_stream_bytes: 1,
        combined_bytes: 2,
    };
    let started = Instant::now();
    let failure = crate::run::capture::capture_observed_command(&req)
        .err()
        .expect("reader limit must fail");
    let facts = failure.facts();
    assert_eq!(facts["deadlineExceeded"], false);
    assert_eq!(facts["termination"], "ended");
    assert!(failure.to_string().contains("stream limit"));
    assert!(started.elapsed() < Duration::from_secs(2));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unsolicited_capsule_cancellation_cannot_become_a_passing_check() {
    const TEST: &str = "run::capture_supervisor::tests::unsolicited_capsule_cancellation_cannot_become_a_passing_check";
    if isolated(TEST, "unsolicited-cancel") {
        return;
    }
    let _owner = FixtureOwner::new();
    let root = crate::test_root::temp("capsule-cancel");
    let mut req = request("kill -TERM $PPID; exit 0", 30_000);
    req.state_root = root.to_path_buf();
    req.artifact_dir = root.join(".exitbind/artifacts");
    let captured = crate::run::capture::capture_observed_command(&req)
        .err()
        .expect("cancel cannot pass");
    let uncaptured =
        crate::run::capture::run_observed_command(&req).expect_err("uncaptured cancel cannot pass");
    for failure in [captured, uncaptured] {
        let facts = failure.facts();
        assert_eq!(facts["deadlineExceeded"], false);
        assert_eq!(facts["process"]["code"], 0);
        assert_eq!(facts["termination"], "ended");
        assert!(failure.to_string().contains("cancelled"));
    }
    std::fs::remove_dir_all(root).unwrap();
}

struct FixtureOwner;
impl FixtureOwner {
    fn new() -> Self {
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        Self
    }
}
impl Drop for FixtureOwner {
    fn drop(&mut self) {
        if !linux::test_cleanup() {
            if std::thread::panicking() {
                eprintln!("exclusive supervisor fixture cleanup unresolved");
            } else {
                panic!("exclusive supervisor fixture cleanup unresolved");
            }
        }
    }
}

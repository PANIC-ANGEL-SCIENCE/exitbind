//! Reaping belongs to an isolated fixture, not the parallel test process.
use super::capture::{process_group_exists, terminate_process_group};
#[path = "cleanup_fixture.rs"]
mod fixture;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

pub(super) fn external_reap_fixture() {
    #[cfg(target_os = "linux")]
    {
        const HARNESS: &str = "EXITBIND_CLEANUP_REAPER_FIXTURE";
        if std::env::var(HARNESS).as_deref() != Ok("normal") {
            let output = fixture::isolated(
                "run::tests::cleanup_reports_reap_failure_after_still_killing_the_group",
                "normal",
            )
            .expect("bounded isolated reaper fixture");
            assert!(
                output.status.success(),
                "isolated reaper fixture failed: {}; stdout={}; stderr={}",
                output.status,
                output.stdout,
                output.stderr
            );
            eprint!("{}", output.stderr);
            return;
        }
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0,
            "fixture could not own descendant reaping: {}",
            std::io::Error::last_os_error()
        );
        // Retain the distinction: termination alone leaves a zombie/group until
        // its owner reaps it. The subsequent owned-reaper case keeps all of the
        // original cleanup assertions, including the external leader-reap error.
        run_fixture(false);
    }
    run_fixture(true);
}

fn run_fixture(reap: bool) {
    let mut owned = fixture::OwnedGroup::launch("normal", reap);
    let group = owned.group;
    let readiness = fixture::readiness(&owned.marker, group, Duration::from_secs(1));
    let ready = readiness.is_ok();
    let descendant = readiness.expect("descendant readiness");
    let waited = unsafe { libc::waitpid(group, std::ptr::null_mut(), 0) };
    let group_before = process_group_exists(group).unwrap_or(false);
    eprintln!("cleanup fixture: reaper={} group={group} descendant={descendant} owned_reaping={reap} before={}", std::process::id(), descendant_state(descendant));
    #[cfg(target_os = "linux")]
    let reaper = reap.then(|| {
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                let result = unsafe { libc::waitpid(-group, std::ptr::null_mut(), libc::WNOHANG) };
                if result > 0 {
                    return result;
                }
                if Instant::now() >= deadline {
                    panic!("owned reaper deadline: {}", descendant_state(descendant));
                }
                thread::sleep(Duration::from_millis(1));
            }
        })
    });
    let started = Instant::now();
    let failure =
        terminate_process_group(&mut owned.child).expect_err("external leader reap is an error");
    let elapsed = started.elapsed();
    let group_after = process_group_exists(group);
    let after = descendant_state(descendant);
    eprintln!("cleanup fixture: after={after} group_present={group_after:?} elapsed={elapsed:?} cleanup={failure}");
    #[cfg(target_os = "linux")]
    if let Some(reaper) = reaper {
        assert_eq!(reaper.join().expect("owned reaper result"), descendant);
    } else {
        assert!(
            matches!(group_after, Ok(true)),
            "unreaped group must remain visible: {group_after:?}"
        );
        assert!(
            after.starts_with("zombie-only/unreaped"),
            "termination did not establish zombie-only state: {after}"
        );
        assert!(failure.contains("process group remained"), "{failure}");
        assert_eq!(
            unsafe { libc::waitpid(descendant, std::ptr::null_mut(), 0) },
            descendant
        );
        assert!(
            matches!(process_group_exists(group), Ok(false)),
            "owned final reap did not remove group"
        );
    }
    assert_eq!(waited, group);
    assert!(ready, "cleanup fixture did not become ready");
    assert!(
        group_before,
        "cleanup fixture process group was not present"
    );
    if reap {
        assert!(
            matches!(group_after, Ok(false)),
            "cleanup left the process group behind: {group_after:?}; descendant={after}"
        );
    }
    assert!(
        elapsed >= Duration::from_millis(250),
        "cleanup skipped its bounded TERM grace"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "cleanup exceeded its bounded budget"
    );
    assert!(failure.contains("could not be reaped"), "{failure}");
}

#[cfg(target_os = "linux")]
#[test]
fn readiness_failure_and_panic_are_visible_bounded_and_reaped() {
    const TEST: &str =
        "run::cleanup_tests::readiness_failure_and_panic_are_visible_bounded_and_reaped";
    if let Ok(mode) = std::env::var("EXITBIND_CLEANUP_REAPER_FIXTURE") {
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        let owned = fixture::OwnedGroup::launch(&mode, true);
        let pid = fixture::readiness(&owned.owner, owned.group, Duration::from_secs(1))
            .expect("independent acknowledgement of live owned descendant");
        eprintln!(
            "negative fixture: mode={mode} group={} descendant={pid}",
            owned.group
        );
        if mode == "panic" {
            panic!("injected failure after descendant ownership");
        }
        fixture::readiness(&owned.marker, owned.group, Duration::from_millis(100))
            .expect("negative fixture readiness must remain a visible failure");
        panic!("negative readiness unexpectedly succeeded");
    }
    for mode in ["missing", "partial", "malformed", "panic"] {
        let output =
            fixture::isolated(TEST, mode).expect("negative fixture transport and owned cleanup");
        assert!(!output.status.success(), "{mode} failure was swallowed");
        assert!(
            output.elapsed < Duration::from_secs(2),
            "{mode} exceeded bounded failure budget: {:?}",
            output.elapsed
        );
        assert!(
            output.stderr.contains("negative fixture: mode="),
            "{mode} did not reach an owned live descendant: {}",
            output.stderr
        );
        let expected = match mode {
            "panic" => "injected failure after descendant ownership",
            "malformed" => "malformed descendant readiness record",
            _ => "descendant readiness deadline: missing or partial record",
        };
        assert!(
            output.stderr.contains(expected),
            "{mode} failure diagnostic missing: {}",
            output.stderr
        );
        let acknowledgement = output
            .stderr
            .lines()
            .find(|line| line.starts_with("negative fixture: mode="))
            .expect("owned descendant acknowledgement");
        let number = |prefix| {
            acknowledgement
                .split_whitespace()
                .find_map(|field| field.strip_prefix(prefix))
                .expect("acknowledged fixture identity")
                .parse::<i32>()
                .expect("acknowledged numeric identity")
        };
        let group = number("group=");
        let descendant = number("descendant=");
        assert!(
            matches!(process_group_exists(group), Ok(false)),
            "{mode} left an owned group live/zombie/unknown"
        );
        assert_eq!(
            descendant_state(descendant),
            "fully reaped",
            "{mode} left an owned descendant live/zombie/unknown"
        );
        eprintln!(
            "negative fixture {mode}: status={} elapsed={:?}; owned group absent",
            output.status, output.elapsed
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn isolated_deadline_failure_survives_later_successful_child_exit() {
    const TEST: &str =
        "run::cleanup_tests::isolated_deadline_failure_survives_later_successful_child_exit";
    if std::env::var("EXITBIND_CLEANUP_REAPER_FIXTURE").as_deref() == Ok("late-success") {
        thread::sleep(Duration::from_millis(150));
        return;
    }
    let started = Instant::now();
    let failure = fixture::isolated_with_budget(TEST, "late-success", Duration::from_millis(50))
        .err()
        .expect("the deadline must remain failure after the child exits successfully");
    assert!(failure.contains("status deadline exceeded"), "{failure}");
    assert!(failure.contains("status=exit status: 0"), "{failure}");
    assert!(started.elapsed() < Duration::from_secs(2), "{failure}");
}

fn descendant_state(pid: i32) -> String {
    #[cfg(target_os = "linux")]
    {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => {
                let Some((_, fields)) = stat.rsplit_once(") ") else {
                    return "unknown: malformed descendant stat".into();
                };
                let fields: Vec<_> = fields.split_whitespace().collect();
                match fields.as_slice() {
                    [state, parent, group, ..] => format!(
                        "{} pid={pid} parent={parent} group={group}",
                        if *state == "Z" {
                            "zombie-only/unreaped"
                        } else {
                            "live"
                        }
                    ),
                    _ => "unknown: incomplete descendant stat".into(),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "fully reaped".into(),
            Err(error) => format!("unknown: descendant state unavailable: {error}"),
        }
    }
    #[cfg(not(target_os = "linux"))]
    format!("unknown: /proc descendant diagnostics unavailable for pid={pid}; operating-system reaper owns orphan")
}

//! Eligibility at native admission and bounded observation of owned cleanup.
use super::{process, Observation, ProcessIdentity, ProcessLiveness, Request, RunError};
use serde_json::{json, Value};
use std::process::Child;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub(super) type Eligibility<'a> = dyn FnMut() -> Result<(), String> + 'a;

pub(crate) fn run_with_control<P, S, C>(
    request: &Request,
    mut process_observer: P,
    mut stream_observer: S,
    mut control: C,
) -> Result<Observation, RunError>
where
    P: FnMut(&ProcessIdentity) -> Result<(), String>,
    S: FnMut(&super::StreamObservation) -> Result<(), String>,
    C: FnMut() -> Result<(), String>,
{
    super::run_inner(
        request,
        Some(&mut process_observer),
        Some(&mut stream_observer),
        Some(&mut control),
    )
}

impl RunError {
    /// Only allowlisted process facts are suitable for durable journals. The
    /// callback's reason, provider text and request material are excluded.
    pub(crate) fn interruption_facts(&self) -> Option<Value> {
        match self {
            Self::ControlledInterruption { cleanup, .. } => Some(cleanup.clone()),
            _ => None,
        }
    }
}

pub(super) fn poll(control: &mut Option<&mut Eligibility<'_>>) -> Result<(), RunError> {
    if let Some(control) = control.as_mut() {
        control().map_err(|reason| RunError::ControlledInterruption {
            reason,
            cleanup: json!({"version":1,"spawned":false,"terminationRequested":false,
                "actualProcessOutcome":null,"ownedRemainingLiveness":"not_started",
                "wholeGroupEnded":null,"bounded":true}),
        })?;
    }
    Ok(())
}

pub(super) fn interrupt(
    child: &mut Child,
    owned: &ProcessIdentity,
    mut error: RunError,
) -> RunError {
    let facts = cleanup(child, owned);
    if let RunError::ControlledInterruption { cleanup, .. } = &mut error {
        *cleanup = facts;
    }
    error
}

fn alive_label(liveness: ProcessLiveness) -> &'static str {
    match liveness {
        ProcessLiveness::Alive => "alive",
        ProcessLiveness::Ended => "ended",
        ProcessLiveness::Uncertain => "uncertain",
    }
}

/// The direct Child handle establishes ownership while it remains unreaped.
/// Once its root has already been reaped, signal no possibly reused identity;
/// report remaining group liveness conservatively instead.
pub(super) fn cleanup(child: &mut Child, owned: &ProcessIdentity) -> Value {
    let mut uncertainty = Vec::new();
    let mut requested = false;
    let mut signals = Vec::new();
    let (mut status, live_owned) = match child.try_wait() {
        Ok(Some(status)) => (Some(status), false),
        Ok(None) => (None, true),
        Err(_) => {
            uncertainty.push("root_wait_failed");
            (None, false)
        }
    };
    // A successful live-child observation keeps its PID unavailable for reuse
    // until this function reaps it. Never signal from a stale saved PID alone.
    if live_owned && child.id() == owned.pid {
        requested = true;
        #[cfg(unix)]
        if let Some(group) = owned
            .process_group
            .filter(|group| *group > 0 && u32::try_from(*group) == Ok(owned.pid))
        {
            for signal in [libc::SIGTERM, libc::SIGKILL] {
                let sent = unsafe { libc::kill(-group, signal) } == 0;
                signals.push(json!({"signal":signal,"delivered":sent}));
                if signal == libc::SIGTERM {
                    thread::sleep(process::TERM_GRACE);
                }
            }
        } else {
            signals.push(json!({"scope":"owned_child","delivered":child.kill().is_ok()}));
        }
        #[cfg(not(unix))]
        signals.push(json!({"scope":"owned_child","delivered":child.kill().is_ok()}));
        let deadline = Instant::now() + process::REAP_GRACE;
        loop {
            match child.try_wait() {
                Ok(Some(observed)) => {
                    status = Some(observed);
                    break;
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(process::POLL),
                Ok(None) => {
                    uncertainty.push("root_reap_unconfirmed");
                    break;
                }
                Err(_) => {
                    uncertainty.push("root_wait_failed");
                    break;
                }
            }
        }
    } else if status.is_none() {
        uncertainty.push("owned_identity_not_current");
    }
    let remaining = process::group_liveness(owned);
    if remaining != ProcessLiveness::Ended {
        uncertainty.push("owned_group_not_observed_ended");
    }
    json!({"version":1,"spawned":true,"processIdentity":{"pid":owned.pid,
        "processGroup":owned.process_group,"startTimeTicks":owned.start_time_ticks},
        "terminationRequested":requested,"signals":signals,"rootReaped":status.is_some(),
        "actualProcessOutcome":status.map(|status| {
            let outcome=super::process_outcome(status,false);
            json!({"code":outcome.code,"signal":outcome.signal,"success":outcome.success,"timedOut":null})
        }),"ownedRemainingLiveness":alive_label(remaining),
        "wholeGroupEnded":if remaining==ProcessLiveness::Ended {Some(true)} else {None},
        "uncertainty":uncertainty,"bounded":true})
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(script: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = loop {
                let path = std::env::temp_dir().join(format!(
                    "exitbind-native-control-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => break fs::canonicalize(path).unwrap(),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("create control fixture: {error}"),
                }
            };
            let executable = root.join("native.sh");
            fs::write(&executable, format!("#!/bin/sh\n{script}\n")).unwrap();
            fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
            Self(root)
        }
        fn request(&self) -> Request {
            Request {
                executable: self.0.join("native.sh"),
                cwd: self.0.clone(),
                prompt: String::new(),
                model: None,
                effort: None,
                sandbox: None,
                output_schema: None,
                resume_thread_id: None,
                persist_session: false,
                timeout: Duration::from_secs(5),
                controlled_tool: None,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn initial_control_hold_never_spawns_or_persists_callback_reason() {
        let fixture = Fixture::new("printf started > spawned");
        let mut observed = false;
        let error = run_with_control(
            &fixture.request(),
            |_| {
                observed = true;
                Ok(())
            },
            |_| Ok(()),
            || Err("private-owner-detail".into()),
        )
        .unwrap_err();
        assert!(!observed);
        assert!(!fixture.0.join("spawned").exists());
        let facts = error.interruption_facts().unwrap();
        assert_eq!(facts["spawned"], false);
        assert_eq!(facts["ownedRemainingLiveness"], "not_started");
        assert!(facts["actualProcessOutcome"].is_null());
        assert!(!facts.to_string().contains("private-owner-detail"));
    }

    #[test]
    fn silent_child_hold_prevents_late_effect_and_leaves_unrelated_child_alive() {
        let fixture = Fixture::new("printf started > started\nsleep 1\nprintf late > late-effect");
        let mut unrelated = Command::new("sleep").arg("5").spawn().unwrap();
        let began = Instant::now();
        let mut process_count = 0;
        let mut stream_count = 0;
        let mut polls = 0;
        let error = run_with_control(
            &fixture.request(),
            |_| {
                process_count += 1;
                Ok(())
            },
            |_| {
                stream_count += 1;
                Ok(())
            },
            || {
                polls += 1;
                if fixture.0.join("started").exists()
                    && began.elapsed() >= Duration::from_millis(100)
                {
                    Err("owner stopped".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        let facts = error.interruption_facts().unwrap();
        assert_eq!(process_count, 1);
        assert_eq!(
            stream_count, 0,
            "eligibility must not depend on provider output"
        );
        assert!(polls > 2);
        assert_eq!(facts["spawned"], true);
        assert_eq!(facts["terminationRequested"], true);
        assert_eq!(facts["rootReaped"], true);
        assert!(facts["actualProcessOutcome"].is_object());
        assert!(began.elapsed() < Duration::from_secs(3));
        let unrelated_alive = unrelated.try_wait().unwrap().is_none();
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
        assert!(
            unrelated_alive,
            "control cleanup must affect only the owned group"
        );
        thread::sleep(Duration::from_millis(1100));
        assert!(!fixture.0.join("late-effect").exists());
    }

    #[test]
    fn ordinary_observers_and_eligible_control_keep_existing_observations() {
        let fixture = Fixture::new("printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"controlled-thread\"}' '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}'");
        let mut ordinary_processes = 0;
        let ordinary = super::super::run_with_observers(
            &fixture.request(),
            |_| {
                ordinary_processes += 1;
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();
        let mut controlled_processes = 0;
        let mut controlled_streams = 0;
        let controlled = run_with_control(
            &fixture.request(),
            |_| {
                controlled_processes += 1;
                Ok(())
            },
            |_| {
                controlled_streams += 1;
                Ok(())
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(ordinary_processes, 1);
        assert_eq!(controlled_processes, 1);
        assert_eq!(controlled_streams, 1);
        assert_eq!(ordinary.thread_id, controlled.thread_id);
        assert_eq!(ordinary.usage, controlled.usage);
        assert_eq!(ordinary.process, controlled.process);
    }

    #[test]
    fn process_observer_failure_keeps_ordinary_error_semantics() {
        let fixture = Fixture::new("sleep 1\nprintf late > late-effect");
        let began = Instant::now();
        let error = super::super::run_with_observers(
            &fixture.request(),
            |_| Err("observer failed".into()),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(error, RunError::Observer(_)));
        assert!(error.interruption_facts().is_none());
        assert!(began.elapsed() < Duration::from_secs(3));
        thread::sleep(Duration::from_millis(1100));
        assert!(!fixture.0.join("late-effect").exists());
    }

    #[test]
    fn reaped_root_success_does_not_establish_whole_group_termination() {
        let fixture = Fixture::new("");
        let mut child = Command::new("sh")
            .args(["-c", "sleep 1 &"])
            .current_dir(&fixture.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let owned = process::identity(child.id());
        assert!(child.wait().unwrap().success());
        let facts = cleanup(&mut child, &owned);
        assert_eq!(facts["rootReaped"], true);
        assert_eq!(facts["actualProcessOutcome"]["success"], true);
        assert_eq!(facts["terminationRequested"], false);
        assert_ne!(facts["ownedRemainingLiveness"], "ended");
        assert!(facts["wholeGroupEnded"].is_null());
        thread::sleep(Duration::from_millis(1100));
    }
}

pub(super) fn receive<T>(
    receiver: &mpsc::Receiver<T>,
    grace: Duration,
    child: &mut Child,
    owned: &ProcessIdentity,
    eligibility: &mut Option<&mut Eligibility<'_>>,
    label: &'static str,
) -> Result<T, RunError> {
    let deadline = Instant::now() + grace;
    loop {
        if let Err(error) = poll(eligibility) {
            return Err(interrupt(child, owned, error));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining.min(process::POLL)) {
            Ok(value) => return Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(_) => {
                cleanup(child, owned);
                return Err(RunError::InvalidRequestDetail(format!(
                    "Codex {label} did not finish after process exit"
                )));
            }
        }
    }
}

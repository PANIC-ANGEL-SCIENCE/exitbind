use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "exitbind-controlled-capture-{}-{}",
            std::process::id(),
            timestamp_nanos(),
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn request(&self, command: &str, identity: &str) -> ObservationRequest {
        ObservationRequest {
            command: command.into(),
            working_dir: self.0.clone(),
            artifact_dir: self.0.join(".exitbind/artifacts"),
            state_root: self.0.clone(),
            deadline: Instant::now() + Duration::from_secs(10),
            timeout_ms: 10_000,
            operation_id: identity.into(),
            capture: CapturePolicy::FINITE,
            retain_partial: true,
            preparation: None,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn initial_hold_never_launches_or_creates_capture_artifacts() {
    let fixture = Fixture::new();
    let request = fixture.request("printf forbidden > launched", "initial-hold");
    let captured = capture_observed_command_with_control(&request, || Err("owner stopped".into()))
        .err()
        .expect("initial hold must refuse capture");
    let uncaptured = run_observed_command_with_control(&request, || Err("owner stopped".into()))
        .expect_err("initial hold must refuse an uncaptured child");
    for failure in [captured, uncaptured] {
        let facts = failure.facts();
        assert_eq!(facts["processStarted"], false);
        assert_eq!(facts["groupEnded"], true);
        assert_eq!(facts["termination"], "ended");
        assert_eq!(facts["process"], Value::Null);
        assert_eq!(facts["deadlineExceeded"], false);
        assert!(failure.to_string().contains("owner stopped"));
    }
    assert!(!fixture.0.join("launched").exists());
    assert!(!request.artifact_dir.exists());
}

#[test]
fn last_prelaunch_check_refuses_after_capture_setup_without_a_child() {
    let fixture = Fixture::new();
    let request = fixture.request("printf forbidden > launched", "pre-spawn-hold");
    let mut calls = 0;
    let failure = capture_observed_command_with_control(&request, || {
        calls += 1;
        if calls == 1 {
            Ok(())
        } else {
            Err("stopped before spawn".into())
        }
    })
    .err()
    .expect("second control check must run before actual spawn");
    assert_eq!(calls, 2);
    assert_eq!(failure.facts()["processStarted"], false);
    assert_eq!(failure.facts()["termination"], "ended");
    assert!(!fixture.0.join("launched").exists());
    assert_eq!(fs::read_dir(&request.artifact_dir).unwrap().count(), 0);
}

#[test]
fn silent_checker_hold_interrupts_only_its_owned_child_and_preserves_facts() {
    for logged in [true, false] {
        let fixture = Fixture::new();
        let request = fixture.request(
            "printf ready > checker-started; exec sleep 30",
            "silent-stop",
        );
        let began = Instant::now();
        let mut polls = 0;
        let mut control = || {
            polls += 1;
            if fixture.0.join("checker-started").exists() {
                Err("owner stopped while silent".into())
            } else {
                Ok(())
            }
        };
        let failure = if logged {
            capture_observed_command_with_control(&request, &mut control)
                .err()
                .expect("silent captured checker must stop")
        } else {
            run_observed_command_with_control(&request, &mut control)
                .expect_err("silent uncaptured checker must stop")
        };
        let facts = failure.facts();
        assert!(polls > 2, "control must continue after prelaunch");
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "hold waited for the command deadline"
        );
        assert_eq!(facts["processStarted"], true);
        assert_eq!(facts["groupEnded"], true);
        assert_eq!(facts["captureReadersEnded"], true);
        assert_eq!(facts["termination"], "ended");
        assert_eq!(facts["deadlineExceeded"], false);
        assert_ne!(facts["process"], Value::Null);
        assert!(failure.to_string().contains("owner stopped while silent"));
        if logged {
            assert_eq!(facts["captureAvailability"], "partial");
            let retained = facts["partialCaptures"].as_array().unwrap();
            assert_eq!(retained.len(), 2);
            for artifact in retained {
                assert!(fixture.0.join(artifact["path"].as_str().unwrap()).is_file());
            }
        }
    }
}

#[test]
fn ordinary_and_noop_controlled_capture_keep_exact_nonzero_status_and_bytes() {
    let fixture = Fixture::new();
    let command = "printf 'out\\000bytes'; printf error >&2; exit 7";
    let ordinary = capture_observed_command(&fixture.request(command, "ordinary")).unwrap();
    let mut calls = 0;
    let controlled =
        capture_observed_command_with_control(&fixture.request(command, "controlled"), || {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert!(calls >= 3);
    for capture in [&ordinary, &controlled] {
        assert_eq!(capture.status.code(), Some(7));
        assert!(capture.group_ended);
        assert_eq!(
            fs::read(capture.stdout_temp.as_ref().unwrap()).unwrap(),
            b"out\0bytes"
        );
        assert_eq!(
            fs::read(capture.stderr_temp.as_ref().unwrap()).unwrap(),
            b"error"
        );
        cleanup_capture(capture).unwrap();
    }
    let ordinary = run_observed_command(&fixture.request("exit 7", "ordinary-unlogged")).unwrap();
    let controlled = run_observed_command_with_control(
        &fixture.request("exit 7", "controlled-unlogged"),
        || Ok(()),
    )
    .unwrap();
    for outcome in [ordinary, controlled] {
        assert_eq!(outcome.status.unwrap().code(), Some(7));
        assert!(outcome.group_ended && outcome.process_started);
    }
}

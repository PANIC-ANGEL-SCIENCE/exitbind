//! Optional local Goal display keeps its original single-assembly behavior.
use super::JourneyObservation;
use crate::session_goal;

#[test]
fn unavailable_ambient_history_stays_absent_without_close_authority() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_LOCAL_GOAL_HISTORY";
    if std::env::var_os(CHILD).is_none() {
        let outer = crate::test_root::temp("local-goal-history-child");
        let store = outer.join("sessions");
        crate::project::portability::create_private_directory(&store).unwrap();
        // raw-spawn: the native selector and Goal store belong to this fixture.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        for key in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
            "CLAUDE_ENV_FILE",
            "XDG_STATE_HOME",
        ] {
            child.env_remove(key);
        }
        let output = child
            .args([
                "session_goal::progress::local_history_tests::unavailable_ambient_history_stays_absent_without_close_authority",
                "--exact",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .env("CODEX_THREAD_ID", "fixture-local-goal-history")
            .env("EXITBIND_SESSION_GOALS_DIR", &store)
            .env("TMPDIR", &*outer)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }

    let root = crate::test_root::temp("local-goal-history");
    let config =
        crate::project::onboarding::init_with_options(crate::project::onboarding::InitOptions {
            product_root: root.to_str().unwrap(),
            coffee: false,
            skip_skills: true,
            mode: Some("portable"),
            project_id: None,
            control_root: None,
            state_root: None,
        })
        .unwrap();
    let loaded = crate::config::load(config.to_str()).unwrap();
    let work = "local-goal-history-fixture";
    let record = session_goal::incorporate(
        &loaded.state_root,
        &loaded,
        work,
        "A separately bound local Goal",
        Some("Complete the local result"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("findings,blockers,decisions,externalActions"),
        false,
    )
    .unwrap();
    let scope = session_goal::native::scope(&loaded, true).unwrap();
    scope.require_owner().unwrap();
    let relative = ".exitbind/session-goal.jsonl";
    let local_history = std::fs::read(loaded.state_root.join(relative)).unwrap();
    let ambient_history = scope.root.join(relative);
    std::fs::write(&ambient_history, b"invalid disposable Goal history\n").unwrap();

    let mut observation = JourneyObservation::for_work(work, true);
    assert!(observation.progress(Some(&loaded), &record).is_none());
    observation.status().unwrap();
    let retained = observation
        .observed
        .as_ref()
        .expect("initial absence retained");
    assert!(!retained.blocks_close_route(&loaded, &scope.root, &record));
    assert!(!observation.used_blocked_close_route());

    // The fresh canonical reader now sees a journey, so the next assertion
    // distinguishes a retained absence from a silently repeated lookup.
    std::fs::write(ambient_history, local_history).unwrap();
    assert!(session_goal::journey::progress_for_record(Some(&loaded), &record).is_some());
    assert!(observation.progress(Some(&loaded), &record).is_none());
    observation.status().unwrap();
    assert!(!observation.used_blocked_close_route());

    let mut changed_record = record.clone();
    changed_record["goalId"] = serde_json::json!("another-goal");
    assert!(observation
        .progress(Some(&loaded), &changed_record)
        .is_none());
    assert!(observation.status().is_err());
    assert!(observation.finish(&loaded).is_err());
}

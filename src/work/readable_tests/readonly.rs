// Tests read-only native scope and its journey witness.
// The fixture rechecks the retained witness; see `src/work/readable_tests.rs`.
//! Read-only native scope keeps the observed witness and its final recheck.
use super::*;
use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};

struct RestorePermissions<'a> {
    path: &'a std::path::Path,
    original: std::fs::Permissions,
}

impl RestorePermissions<'_> {
    fn restore(&self) -> std::io::Result<()> {
        std::fs::set_permissions(self.path, self.original.clone())
    }
}

impl Drop for RestorePermissions<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("could not restore fixture store permissions: {error}");
        }
    }
}

#[test]
fn readonly_scope_retains_and_rechecks_the_shared_journey_witness() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_READONLY_JOURNEY_WITNESS";
    if std::env::var_os(CHILD).is_none() {
        for host in ["codex", "claude"] {
            let outer = crate::test_root::temp("readonly-journey-child");
            let store = outer.join("sessions");
            crate::project::portability::create_private_directory(&store).unwrap();
            // raw-spawn: the selector and Goal store are owned by this fixture.
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
            let selector = if host == "codex" {
                "CODEX_THREAD_ID"
            } else {
                "CLAUDE_CODE_SESSION_ID"
            };
            let output = child.args([
                "work::readable::tests::readonly::readonly_scope_retains_and_rechecks_the_shared_journey_witness",
                "--exact", "--test-threads=1", "--nocapture",
            ]).env(CHILD, host).env(selector, "fixture-readonly-journey")
                .env("EXITBIND_SESSION_GOALS_DIR", &store).env("TMPDIR", &*outer)
                .output().unwrap();
            assert!(
                output.status.success(),
                "{host}: child exited {}; stderr:\n{}\nstdout:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            );
            if String::from_utf8_lossy(&output.stderr).contains("COVERAGE UNAVAILABLE") {
                eprintln!("{host}: {}", String::from_utf8_lossy(&output.stderr));
            }
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        }
        return;
    }

    let (_root, loaded) = configured("readonly-journey-witness");
    let scope = crate::session_goal::native::scope(&loaded, true).unwrap();
    scope.require_owner().unwrap();
    revise_goal(&loaded, &scope.root, "readonly-goal", "Original", true).unwrap();
    let ancestor = begin(&loaded, "true", true);
    scope_current_lead(&loaded, &ancestor);
    let successor = crate::work::supersede::supersede(&loaded, &ancestor, "Fixture successor")
        .unwrap()["work"]
        .as_str()
        .unwrap()
        .to_owned();
    scope_current_lead(&loaded, &successor);
    let current = crate::work::resolve(&loaded, &successor).unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &current).unwrap();
    let next = crate::work::next_from(&loaded, &successor, &snapshot).unwrap();
    let expected =
        crate::session_goal::progress_for_work(&loaded, &successor, &next["progress"]).unwrap();

    let store = scope.root.parent().unwrap();
    let permissions = RestorePermissions {
        path: store,
        original: std::fs::metadata(store).unwrap().permissions(),
    };
    std::fs::set_permissions(store, std::fs::Permissions::from_mode(0o500)).unwrap();
    let path = std::ffi::CString::new(store.as_os_str().as_bytes()).unwrap();
    if unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0 {
        eprintln!("COVERAGE UNAVAILABLE: current credentials can write a mode-0500 fixture");
        permissions.restore().unwrap();
        return;
    }
    let mut observation = crate::session_goal::WorkGoalObservation::acquire(&loaded, &successor);
    let actual = observation.progress(&next["progress"]).unwrap();
    assert_eq!(actual, expected);
    assert!(observation.has_journey_observation());
    assert!(observation.used_blocked_close_route());
    observation.finish().unwrap();

    let mut stale = crate::session_goal::WorkGoalObservation::acquire(&loaded, &successor);
    stale.progress(&next["progress"]).unwrap();
    assert!(stale.has_journey_observation());
    permissions.restore().unwrap();
    let changed = revise_goal(&loaded, &scope.root, "readonly-goal", "Changed", true);
    std::fs::set_permissions(store, std::fs::Permissions::from_mode(0o500)).unwrap();
    changed.unwrap();
    assert!(
        stale.finish().is_err(),
        "the final witness must refuse the changed Goal"
    );
    permissions.restore().unwrap();
}

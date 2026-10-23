//! Optional native Goal absence is distinct from damaged or expected context.
mod support;

use serde_json::Value;
use std::{fs, path::Path};

fn invoke(root: &Path, args: &[&str], session: Option<&str>) -> Value {
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    if let Some(session) = session {
        command.env("CODEX_THREAD_ID", session);
    }
    let output = command
        .current_dir(root)
        .env("EXITBIND_SESSION_GOALS_DIR", root.join("sessions"))
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn project(label: &str) -> support::TempRoot {
    let root = support::temp(label);
    support::git_topology::repository(&root);
    fs::write(
        root.join("README.md"),
        "A governed task without a session Goal\n",
    )
    .unwrap();
    invoke(
        &root,
        &[
            "setup",
            "--mode",
            "portable",
            "--root",
            ".",
            "--worker-observe",
            "README.md",
            "--worker-write",
            "none",
            "--worker-commands",
            "true",
            "--json",
            "--apply",
        ],
        None,
    );
    root
}

#[test]
fn genuine_optional_absence_does_not_emit_a_failure_banner() {
    for session in [None, Some("unincorporated-session")] {
        let root = project("optional-goal-absence");
        let started = invoke(
            &root,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Deliver the scoped result",
                "--check-command",
                "true",
                "--review-policy",
                "required",
                "--detail",
            ],
            session,
        );
        let progress = &started["sections"]["tasks"];
        assert_eq!(progress["overall"], "not_requested");
        assert_eq!(
            progress["decomposition"]["reason"],
            "native_session_goal_not_requested"
        );
        assert!(!progress["systemText"]
            .as_str()
            .unwrap()
            .contains("UNAVAILABLE"));
        assert_eq!(progress["resultReadiness"]["state"], "IN_PROGRESS");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn malformed_native_selector_remains_diagnosable() {
    let root = project("optional-goal-invalid-selector");
    let started = invoke(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Deliver the scoped result",
            "--check-command",
            "true",
            "--review-policy",
            "required",
            "--detail",
        ],
        Some(" invalid-session"),
    );
    let progress = &started["sections"]["tasks"];
    assert_eq!(progress["overall"], "unavailable");
    assert_eq!(
        progress["decomposition"]["reason"],
        "native_session_goal_context_invalid"
    );
    fs::remove_dir_all(root).unwrap();
}

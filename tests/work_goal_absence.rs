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

#[cfg(unix)]
#[test]
fn local_work_view_survives_unreadable_ambient_session_history() {
    let root = project("local-goal-ambient-history");
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
            "--json",
        ],
        None,
    );
    let work = started["work"].as_str().unwrap();
    invoke(
        &root,
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            work,
            "--goal",
            "The local goal",
            "--obligation",
            "Complete the scoped result",
            "--json",
        ],
        None,
    );
    let session = Some("local-goal-ambient-history");
    invoke(
        &root,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "separate-native-goal",
            "--goal",
            "A separate ambient native Goal",
            "--obligation",
            "Retain the native Goal",
            "--json",
        ],
        session,
    );
    let before = invoke(&root, &["work", "detail", work, "--json"], session);
    assert_eq!(before["sections"]["tasks"]["goal"], "The local goal");
    assert!(before["sections"]["tasks"]
        .get("journeyCompletion")
        .is_none());
    let before_next = invoke(&root, &["work", "next", work, "--json"], session);

    // Display storage can coexist with the native session's hashed directory.
    fs::create_dir_all(root.join("sessions/display")).unwrap();
    let sessions: Vec<_> = fs::read_dir(root.join("sessions"))
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let key = name.to_str()?.strip_prefix("session-")?;
            if key.len() != 64
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                || !entry.file_type().unwrap().is_dir()
            {
                return None;
            }
            let path = entry.path();
            let owner: Value =
                serde_json::from_slice(&fs::read(path.join("owner.json")).ok()?).ok()?;
            (owner["kind"] == "native_session_goal_owner"
                && owner["host"] == "codex"
                && owner["sessionKey"] == key)
                .then_some(path)
        })
        .collect();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    let history = sessions[0].join(".exitbind/session-goal.jsonl");
    fs::write(&history, b"invalid disposable session goal history\n").unwrap();

    let after = invoke(&root, &["work", "detail", work, "--json"], session);
    assert_eq!(after["valid"], true);
    assert_eq!(after["complete"], true);
    assert_eq!(after["sections"]["tasks"], before["sections"]["tasks"]);
    let after_next = invoke(&root, &["work", "next", work, "--json"], session);
    assert_eq!(
        after_next["next"]["assignment"],
        before_next["next"]["assignment"]
    );
    assert_eq!(
        fs::read(history).unwrap(),
        b"invalid disposable session goal history\n"
    );
    fs::remove_dir_all(root).unwrap();
}

//! A readable native Goal store must not acquire write authority.
use crate::fixture::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

struct RestoreMode {
    path: PathBuf,
    mode: fs::Permissions,
}

impl Drop for RestoreMode {
    fn drop(&mut self) {
        fs::set_permissions(&self.path, self.mode.clone()).expect("restore fixture permissions");
    }
}

fn readonly(path: &Path) -> Option<RestoreMode> {
    let restore = RestoreMode {
        path: path.to_path_buf(),
        mode: fs::metadata(path).unwrap().permissions(),
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o500)).unwrap();
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    if unsafe { libc::access(name.as_ptr(), libc::W_OK) } == 0 {
        eprintln!("COVERAGE UNAVAILABLE: current credentials can write a mode-0500 fixture");
        return None;
    }
    Some(restore)
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                files.insert(relative, None);
                visit(root, &path, files);
            } else {
                files.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn value(project: &Project, host: &str, session: &str, args: &[&str]) -> Value {
    project.value_in_session(host, session, &project.session_store, args)
}

fn refused(output: &Output, expected: &str) {
    assert!(!output.status.success(), "{output:?}");
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(message.contains(expected), "{message}");
}

fn pending_work(project: &Project, host: &str, session: &str) -> String {
    let start = value(
        project,
        host,
        session,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Complete the scoped task",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    );
    let work = start["work"].as_str().unwrap();
    let lead = start["next"]["assignment"].as_str().unwrap();
    value(
        project,
        host,
        session,
        &["work", "return", work, lead, "--outcome", "scoped"],
    );
    work.to_owned()
}

fn project_with_store(label: &str) -> (Project, super::support::TempRoot) {
    let mut project = Project::new(label);
    let store = super::support::temp("readonly-native-store");
    fs::set_permissions(&*store, fs::Permissions::from_mode(0o700)).unwrap();
    project.session_store = store.path().to_owned();
    (project, store)
}

fn incorporate(project: &Project, host: &str, session: &str) {
    value(
        project,
        host,
        session,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "native-goal",
            "--goal",
            "Complete the wider task",
            "--obligation",
            "Complete the task",
        ],
    );
}

#[test]
fn readonly_pending_work_and_adopted_goal_keep_their_exact_views() {
    for host in ["codex", "claude"] {
        let (project, store) = project_with_store("readonly-context");
        let standalone = pending_work(&project, host, "no-goal");
        let before = value(
            &project,
            host,
            "no-goal",
            &["work", "detail", &standalone, "--json"],
        );
        assert_eq!(before["sections"]["tasks"]["overall"], "not_requested");
        let before_store = tree(&store);
        let Some(mode) = readonly(&store) else { return };
        let after = value(
            &project,
            host,
            "no-goal",
            &["work", "detail", &standalone, "--json"],
        );
        assert_eq!(after["binding"], before["binding"], "{host}");
        assert_eq!(after["actionForms"], before["actionForms"], "{host}");
        assert_eq!(after["sections"], before["sections"], "{host}");
        assert_eq!(after["recipient"], before["recipient"], "{host}");
        assert_eq!(
            after["recipientContext"], before["recipientContext"],
            "{host}"
        );
        assert_eq!(tree(&store), before_store, "{host}");
        drop(mode);

        incorporate(&project, host, "parent");
        let work = pending_work(&project, host, "parent");
        let key = digest_value(&json!({"host":host,"identity":"parent"}));
        value(
            &project,
            host,
            "adopter",
            &["goal", "adopt", "--session-key", &key],
        );
        let before = value(
            &project,
            host,
            "adopter",
            &["work", "detail", &work, "--json"],
        );
        assert_eq!(before["valid"], true, "{host}");
        assert_eq!(before["complete"], true, "{host}");
        assert_ne!(
            before["sections"]["tasks"]["overall"], "unavailable",
            "{host}"
        );
        assert!(
            before["sections"]["tasks"]["journeyCompletion"].is_object(),
            "{host}"
        );
        let status = value(&project, host, "adopter", &["goal", "status", "--json"]);
        let before_store = tree(&store);
        let Some(mode) = readonly(&store) else { return };
        let after = value(
            &project,
            host,
            "adopter",
            &["work", "detail", &work, "--json"],
        );
        assert_eq!(after["binding"], before["binding"], "{host}");
        assert_eq!(after["actionForms"], before["actionForms"], "{host}");
        assert_eq!(after["sections"], before["sections"], "{host}");
        assert_eq!(after["recipient"], before["recipient"], "{host}");
        assert_eq!(
            after["recipientContext"], before["recipientContext"],
            "{host}"
        );
        let readonly_status = value(&project, host, "adopter", &["goal", "status", "--json"]);
        assert_eq!(readonly_status["goalId"], status["goalId"], "{host}");
        assert_eq!(
            readonly_status["goalProgress"], status["goalProgress"],
            "{host}"
        );
        assert_eq!(tree(&store), before_store, "{host}");
        drop(mode);
    }
}

fn usage(project: &Project, host: &str, session: &str) -> Output {
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&project.root)
        .args([
            "goal",
            "usage",
            "--goal-id",
            "native-goal",
            "--apply",
            "--config",
        ])
        .arg(project.root.join("exitbind.json"))
        .env("EXITBIND_SESSION_GOALS_DIR", &project.session_store)
        .env(
            if host == "codex" {
                "CODEX_THREAD_ID"
            } else {
                "CLAUDE_CODE_SESSION_ID"
            },
            session,
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let input = br#"{"id":"readonly-usage","source":"host_reported","scope":"setup","phase":"setup","status":"observed","semantics":"delta","lifetime":"invocation","values":{"inputTokens":4}}"#;
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

#[test]
fn readonly_base_refuses_goal_mutations_before_any_persistence() {
    for host in ["codex", "claude"] {
        let (project, store) = project_with_store("readonly-refusal");
        fs::write(project.root.join("request.txt"), b"Complete the task\n").unwrap();
        incorporate(&project, host, "parent");
        let _work = pending_work(&project, host, "parent");
        let key = digest_value(&json!({"host":host,"identity":"parent"}));
        let descendant = native_session_root(&store, host, "parent").join(".exitbind");
        let name = CString::new(descendant.as_os_str().as_bytes()).unwrap();
        assert_eq!(
            unsafe { libc::access(name.as_ptr(), libc::W_OK) },
            0,
            "{host}"
        );
        let before_store = tree(&store);
        let before_project = tree(&project.root);
        let Some(mode) = readonly(&store) else { return };
        for args in [
            vec![
                "goal",
                "require",
                "--goal-id",
                "native-goal",
                "--requirement",
                "R1",
                "--obligation",
                "Complete the task",
                "--artifact",
                "request.txt",
            ],
            vec![
                "goal",
                "incorporate",
                "--goal-id",
                "native-goal",
                "--goal",
                "Changed",
                "--obligation",
                "Complete the task",
            ],
        ] {
            let result = project.call_in_session(host, "parent", &store, &args);
            refused(&result, "read-only");
        }
        let result = usage(&project, host, "parent");
        refused(&result, "read-only");
        let result = project.call_in_session(
            host,
            "adopter",
            &store,
            &["goal", "adopt", "--session-key", &key],
        );
        refused(&result, "read-only");
        assert_eq!(tree(&store), before_store, "{host}");
        assert_eq!(tree(&project.root), before_project, "{host}");
        drop(mode);
        let required = project.call_in_session(
            host,
            "parent",
            &store,
            &[
                "goal",
                "require",
                "--goal-id",
                "native-goal",
                "--requirement",
                "R1",
                "--obligation",
                "Complete the task",
                "--artifact",
                "request.txt",
            ],
        );
        assert!(required.status.success(), "{host}: {required:?}");
        let recorded = usage(&project, host, "parent");
        assert!(recorded.status.success(), "{host}: {recorded:?}");
        let adopted = project.call_in_session(
            host,
            "adopter",
            &store,
            &["goal", "adopt", "--session-key", &key],
        );
        assert!(adopted.status.success(), "{host}: {adopted:?}");
    }
}

#[test]
fn readonly_ready_goal_does_not_offer_or_execute_close() {
    for host in ["codex", "claude"] {
        let (project, store) = project_with_store("readonly-ready");
        value(
            &project,
            host,
            "ready",
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "ready",
                "--goal",
                "A ready direct Goal",
                "--none-applicable",
                "obligations,findings,blockers,decisions,externalActions",
            ],
        );
        let before = value(&project, host, "ready", &["goal", "status", "--json"]);
        assert!(
            before["goalProgress"]["nextAction"].is_object(),
            "{host}: {before}"
        );
        let before_store = tree(&store);
        let Some(mode) = readonly(&store) else { return };
        let after = value(&project, host, "ready", &["goal", "status", "--json"]);
        assert_eq!(after["goalId"], "ready", "{host}");
        assert!(
            after["goalProgress"]["nextAction"].is_null(),
            "{host}: {after}"
        );
        let close = project.call_in_session(
            host,
            "ready",
            &store,
            &["goal", "close", "--goal-id", "ready", "--direct", "--json"],
        );
        refused(&close, "read-only");
        assert_eq!(tree(&store), before_store, "{host}");
        drop(mode);
    }
}

#[test]
fn readonly_foreign_source_status_does_not_offer_an_unavailable_preview() {
    for host in ["codex", "claude"] {
        let (owner, store) = project_with_store("readonly-recovery-owner");
        let source = Project::new("readonly-recovery-source");
        incorporate(&owner, host, "parent");
        let started = source.value_in_session(
            host,
            "parent",
            &store,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "A foreign source contribution",
                "--check-command",
                "true",
                "--review-policy",
                "required",
            ],
        );
        assert!(started["work"].is_string(), "{host}: {started}");
        let before_store = tree(&store);
        let before_owner = tree(&owner.root);
        let before_source = tree(&source.root);
        let before = value(&owner, host, "parent", &["goal", "status", "--json"]);
        assert!(
            before["goalProgress"]["journeyCompletion"]["recoveryForm"].is_object(),
            "{host}: {before}"
        );
        assert!(before["goalProgress"]["nextAction"].is_null(), "{host}");
        assert_eq!(tree(&store), before_store, "{host}");
        assert_eq!(tree(&owner.root), before_owner, "{host}");
        assert_eq!(tree(&source.root), before_source, "{host}");

        let Some(mode) = readonly(&store) else { return };
        let after = value(&owner, host, "parent", &["goal", "status", "--json"]);
        assert_eq!(after["goalId"], before["goalId"], "{host}");
        assert!(
            after["goalProgress"]["journeyCompletion"]["recoveryForm"].is_null(),
            "{host}: {after}"
        );
        assert!(after["goalProgress"]["nextAction"].is_null(), "{host}");
        let source_config = source.root.join("exitbind.json");
        let preview = owner.call_in_session(
            host,
            "parent",
            &store,
            &[
                "goal",
                "recover",
                "--source-config",
                source_config.to_str().unwrap(),
                "--request-id",
                "readonly-preview",
                "--reason",
                "Inspect the foreign source",
                "--json",
            ],
        );
        refused(&preview, "read-only");
        assert_eq!(tree(&store), before_store, "{host}");
        assert_eq!(tree(&owner.root), before_owner, "{host}");
        assert_eq!(tree(&source.root), before_source, "{host}");
        drop(mode);
    }
}

#[test]
fn readonly_resolution_keeps_invalid_contexts_and_foreign_owners_distinct() {
    use std::os::unix::fs::symlink;

    for host in ["codex", "claude"] {
        let (project, store) = project_with_store("readonly-invalid");
        incorporate(&project, host, "parent");
        let work = pending_work(&project, host, "parent");
        let foreign = Project::new("readonly-foreign");
        let denied = foreign.call_in_session(
            host,
            "parent",
            &store,
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "other",
                "--goal",
                "Foreign change",
                "--obligation",
                "Foreign task",
            ],
        );
        refused(&denied, "owned by another configured project");
        let Some(mode) = readonly(&store) else { return };
        let foreign_status =
            foreign.value_in_session(host, "parent", &store, &["goal", "status", "--json"]);
        assert_eq!(foreign_status["contextOnly"], true, "{host}");
        assert_eq!(foreign_status["authority"], "none", "{host}");

        let conflict = project.call_with_native_selectors(
            &[
                ("CODEX_THREAD_ID", "one"),
                ("CLAUDE_CODE_SESSION_ID", "two"),
            ],
            &["goal", "status", "--json"],
        );
        refused(&conflict, "conflicting native");

        let root = native_session_root(&store, host, "parent");
        let owner = root.join("owner.json");
        let owner_bytes = fs::read(&owner).unwrap();
        fs::write(&owner, b"{}\n").unwrap();
        let invalid = value(
            &project,
            host,
            "parent",
            &["work", "detail", &work, "--json"],
        );
        assert_eq!(
            invalid["sections"]["tasks"]["decomposition"]["reason"],
            "native_session_goal_context_invalid",
            "{host}"
        );
        let status = project.call_in_session(host, "parent", &store, &["goal", "status", "--json"]);
        assert!(!status.status.success(), "{host}: {status:?}");
        fs::write(&owner, owner_bytes).unwrap();

        fs::set_permissions(&*store, fs::Permissions::from_mode(0o555)).unwrap();
        let unsafe_mode =
            project.call_in_session(host, "parent", &store, &["goal", "status", "--json"]);
        refused(&unsafe_mode, "private");
        fs::set_permissions(&*store, fs::Permissions::from_mode(0o500)).unwrap();
        drop(mode);

        let saved = root.with_extension("saved");
        fs::rename(&root, &saved).unwrap();
        symlink(&saved, &root).unwrap();
        let Some(mode) = readonly(&store) else { return };
        let unsafe_link =
            project.call_in_session(host, "parent", &store, &["goal", "status", "--json"]);
        refused(&unsafe_link, "symlink");
        drop(mode);
        fs::remove_file(&root).unwrap();
        fs::rename(saved, root).unwrap();
    }
}

//! The native Stop hook observes only its own current Goal and preserves the
//! host's bounded stop protocol and existing Goal journal.
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    path::PathBuf,
    process::{Output, Stdio},
};

const SESSION: &str = "r62-stop-owner";
const QUESTION: &str = "Which destination should I use?";

struct Fixture {
    _base: super::support::TempRoot,
    product: PathBuf,
    other: PathBuf,
    store: PathBuf,
    host: &'static str,
}

impl Fixture {
    fn new(host: &'static str, name: &str) -> Self {
        let base = super::support::temp(name);
        let product = base.join("product");
        let other = base.join("other");
        let store = base.join("session-goals");
        for root in [&product, &other] {
            fs::create_dir(root).unwrap();
            super::support::git_topology::repository(root);
            let init = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
                .args(["init", "--mode", "portable", "--root"])
                .arg(root)
                .output()
                .unwrap();
            assert!(init.status.success(), "{init:?}");
        }
        Self {
            _base: base,
            product,
            other,
            store,
            host,
        }
    }

    fn run(&self, host: &str, selector: &str, cwd: &Path, args: &[&str], input: &[u8]) -> Output {
        let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
        command
            .current_dir(cwd)
            .args(args)
            .env("EXITBIND_SESSION_GOALS_DIR", &self.store)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if selector == "conflicting-selectors" {
            command
                .env("CODEX_THREAD_ID", "different-codex-thread")
                .env("CODEX_SESSION_ID", "different-codex-session")
                .env("CLAUDE_CODE_SESSION_ID", "different-claude-session")
                .env("EXITBIND_NATIVE_SESSION_ID", "different-export");
        } else if !selector.is_empty() {
            command.env(
                if host == "codex" {
                    "CODEX_THREAD_ID"
                } else {
                    "CLAUDE_CODE_SESSION_ID"
                },
                selector,
            );
        }
        let mut child = command.spawn().unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }

    fn goal(&self, args: &[&str]) -> Value {
        let output = self.run(self.host, SESSION, &self.product, args, b"");
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn hook(&self, host: &str, selector: &str, cwd: &Path, payload: Value) -> Option<Value> {
        let output = self.run(
            host,
            selector,
            cwd,
            &["hook-run", "--host", host],
            payload.to_string().as_bytes(),
        );
        assert!(output.status.success(), "{payload}: {output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        if output.stdout.is_empty() {
            None
        } else {
            Some(serde_json::from_slice(&output.stdout).unwrap())
        }
    }

    fn stop(&self, message: &str) -> Value {
        json!({"hook_event_name":"Stop", "cwd":self.product, "session_id":SESSION,
            "stop_hook_active":false, "last_assistant_message":message})
    }

    fn assert_blocked(&self, payload: Value) {
        assert_eq!(
            self.hook(self.host, SESSION, &self.product, payload),
            Some(json!({
                "decision":"block", "reason":"The session Goal is open. Continue the authorized work."
            }))
        );
    }
}

fn files_under(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    if !root.exists() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((path.clone(), fs::read(path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn stop_is_scoped_to_the_open_goal_and_exact_declared_owner_wait() {
    for host in ["codex", "claude"] {
        let fixture = Fixture::new(host, &format!("r62-stop-open-{host}"));
        let ordinary = fixture.stop("A side answer.");
        assert_eq!(
            fixture.hook(host, SESSION, &fixture.product, ordinary.clone()),
            None
        );
        assert!(!fixture.store.exists(), "a Goal was created on Stop");

        fixture.goal(&[
            "goal",
            "incorporate",
            "--goal-id",
            "r62-stop",
            "--goal",
            "Finish the full request",
        ]);
        let before = files_under(&fixture.store);
        fixture.assert_blocked(ordinary.clone());
        fixture.assert_blocked(fixture.stop("Why stop now?"));
        let mut active = ordinary.clone();
        active["stop_hook_active"] = json!(true);
        assert_eq!(fixture.hook(host, SESSION, &fixture.product, active), None);
        let mut missing_flag = ordinary.clone();
        missing_flag
            .as_object_mut()
            .unwrap()
            .remove("stop_hook_active");
        assert_eq!(
            fixture.hook(host, SESSION, &fixture.product, missing_flag),
            None
        );
        let mut child = ordinary.clone();
        child["agent_id"] = json!("worker-child");
        child["agent_type"] = json!("worker");
        assert_eq!(fixture.hook(host, SESSION, &fixture.product, child), None);
        let mut other_session = ordinary.clone();
        other_session["session_id"] = json!("another-session");
        assert_eq!(
            fixture.hook(host, SESSION, &fixture.product, other_session),
            None
        );
        for selector in ["", "another-session", "conflicting-selectors"] {
            assert_eq!(
                fixture.hook(host, selector, &fixture.product, ordinary.clone()),
                Some(json!({
                    "decision":"block", "reason":"The session Goal is open. Continue the authorized work."
                })),
                "Stop identity must come from its registered host and payload"
            );
        }
        let other_host = if host == "codex" { "claude" } else { "codex" };
        assert_eq!(
            fixture.hook(other_host, SESSION, &fixture.product, ordinary.clone()),
            None
        );
        let mut foreign = ordinary.clone();
        foreign["cwd"] = json!(fixture.other);
        assert_eq!(fixture.hook(host, SESSION, &fixture.other, foreign), None);
        let mut malformed = ordinary.clone();
        malformed["session_id"] = json!(" invalid identity ");
        assert_eq!(
            fixture.hook(host, SESSION, &fixture.product, malformed),
            None
        );
        assert_eq!(
            fixture.hook("unknown", SESSION, &fixture.product, ordinary.clone()),
            None
        );
        let without_host = fixture.run(
            host,
            SESSION,
            &fixture.product,
            &["hook-run"],
            ordinary.to_string().as_bytes(),
        );
        assert!(without_host.status.success());
        assert!(without_host.stdout.is_empty());
        assert_eq!(
            files_under(&fixture.store),
            before,
            "Stop mutated the Goal store"
        );

        fixture.goal(&[
            "goal",
            "incorporate",
            "--goal-id",
            "r62-stop",
            "--goal",
            "Finish the full request",
            "--decision",
            QUESTION,
        ]);
        let before_wait = files_under(&fixture.store);
        let exact = fixture.stop(&format!(" \n{QUESTION} \n"));
        assert_eq!(fixture.hook(host, SESSION, &fixture.product, exact), None);
        fixture.assert_blocked(fixture.stop("A side answer."));
        fixture.assert_blocked(fixture.stop(&format!("{QUESTION} I will continue.")));
        fixture.assert_blocked(fixture.stop("Why stop now?"));
        assert_eq!(
            files_under(&fixture.store),
            before_wait,
            "Stop mutated the Goal store"
        );
    }
}

#[test]
fn stop_allows_a_closed_goal_for_each_host() {
    for host in ["codex", "claude"] {
        let fixture = Fixture::new(host, &format!("r62-stop-closed-{host}"));
        fixture.goal(&[
            "goal",
            "incorporate",
            "--goal-id",
            "closed",
            "--goal",
            "Finish directly",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ]);
        fixture.goal(&["goal", "close", "--goal-id", "closed", "--direct", "--json"]);
        let before = files_under(&fixture.store);
        assert_eq!(
            fixture.hook(host, SESSION, &fixture.product, fixture.stop("Done.")),
            None
        );
        assert_eq!(files_under(&fixture.store), before);
    }
}

#[cfg(unix)]
#[test]
fn stop_can_observe_existing_private_read_only_goal_state() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("codex", "r62-stop-read-only");
    fixture.goal(&[
        "goal",
        "incorporate",
        "--goal-id",
        "read-only",
        "--goal",
        "Finish the full request",
    ]);
    let root = fs::read_dir(&fixture.store)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let state = root.join(".exitbind");
    let before = files_under(&fixture.store);
    for path in [&fixture.store, &root, &state] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o500)).unwrap();
    }
    let response = fixture.hook(
        "codex",
        SESSION,
        &fixture.product,
        fixture.stop("A side answer."),
    );
    for path in [&fixture.store, &root, &state] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert_eq!(response.unwrap()["decision"], "block");
    assert_eq!(files_under(&fixture.store), before);
}

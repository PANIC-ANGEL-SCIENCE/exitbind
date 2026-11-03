//! R54 follow-up (native UX): after each ledger command the native session's
//! display file carries the same Goal revision as `goal status --json`; the
//! Claude status line reads only that file; a closed Goal stays closed on the
//! screen after the tree moves; `setup --apply` installs the status line.
use super::lesson_carry::{repository, Root};
use super::support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Output, Stdio},
    time::Instant,
};

const CARD: &str = "# R9 task\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 1 | Add the parser | The parser rejects empty input. |\n| 2 | Add the report | The report shows the count. |\n";
const STATUS_COMMAND: &str = "command -v exitbind >/dev/null 2>&1 && exitbind statusline || true";

struct Session {
    _owned: Root,
    root: PathBuf,
    store: PathBuf,
    home: PathBuf,
    id: String,
    selector: &'static str,
    extra: Vec<(&'static str, PathBuf)>,
}

impl Session {
    fn new(label: &str) -> Self {
        let owned = repository(label);
        let root = owned.0.to_path_buf();
        let home = root.with_extension("home");
        fs::create_dir_all(&home).unwrap();
        Self {
            _owned: owned,
            store: root.with_extension("session-goals"),
            home,
            id: format!("r54-native-display-{label}"),
            root,
            selector: "CLAUDE_CODE_SESSION_ID",
            extra: Vec::new(),
        }
    }

    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut command = support::git_topology::command(args[0]);
        command
            .current_dir(&self.root)
            .args(&args[1..])
            .env(self.selector, &self.id)
            .envs(self.extra.iter().map(|(name, value)| (*name, value)))
            .env("EXITBIND_SESSION_GOALS_DIR", &self.store)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env("HOME", &self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        // A command may refuse and exit before reading stdin; its output is still asserted.
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        }
        child.wait_with_output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.call(args, b"");
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn display_path(&self) -> PathBuf {
        let files = fs::read_dir(self.store.join("display"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1, "one display file per session: {files:?}");
        files[0].clone()
    }

    fn display(&self) -> Value {
        serde_json::from_slice(&fs::read(self.display_path()).unwrap()).unwrap()
    }

    fn status_line(&self) -> (String, u128) {
        let payload = json!({"session_id": self.id, "cwd": self.root, "model": {"id":"m"}});
        let started = Instant::now();
        let output = self.call(
            &[env!("CARGO_BIN_EXE_exitbind"), "statusline"],
            payload.to_string().as_bytes(),
        );
        let elapsed = started.elapsed().as_micros();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        (String::from_utf8(output.stdout).unwrap(), elapsed)
    }

    fn config(&self) -> String {
        fs::canonicalize(self.root.join("exitbind.json"))
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }
}

fn setup(session: &Session) -> Value {
    fs::create_dir_all(session.root.join("work/r9")).unwrap();
    fs::create_dir_all(session.root.join("scripts")).unwrap();
    fs::write(session.root.join("work/r9/00_TASK.md"), CARD).unwrap();
    fs::write(
        session.root.join("scripts/check.sh"),
        "test -s work/r9/00_TASK.md\n",
    )
    .unwrap();
    let root = session.root.to_str().unwrap();
    session.json(&[
        env!("CARGO_BIN_EXE_exitbind"),
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root,
        "--scope",
        "worker",
        "--observe",
        "scripts,work",
        "--write",
        "scripts",
        "--commands",
        "sh scripts/check.sh",
        "--check-command",
        "sh scripts/check.sh",
        "--goal",
        "Ship the R9 parser and report",
        "--review-policy",
        "required",
        "--hosts",
        "claude",
        "--skip-skills",
        "--task-card",
        "work/r9/00_TASK.md",
        "--apply",
    ])
}

fn p95(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[(samples.len() * 95).div_ceil(100) - 1]
}

#[test]
fn display_file_follows_every_ledger_command_and_feeds_the_status_line() {
    let session = Session::new("native-display-ledger");
    let report = setup(&session);
    assert_eq!(report["statusLine"]["status"], "installed", "{report}");
    let settings: Value =
        serde_json::from_slice(&fs::read(session.root.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["statusLine"]["command"], STATUS_COMMAND);
    // Without any Goal the status line prints nothing and exits 0.
    assert_eq!(session.status_line().0, "");

    let config = session.config();
    for command in report["next"]["argv"].as_array().unwrap() {
        let argv = command
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect::<Vec<_>>();
        session.json(&argv);
        let status = session.json(&[
            env!("CARGO_BIN_EXE_exitbind"),
            "goal",
            "status",
            "--json",
            "--config",
            &config,
        ]);
        let display = session.display();
        assert_eq!(display["revision"], status["revision"], "after {argv:?}");
        assert_eq!(display["goal"], status["goal"]);
    }
    let display = session.display();
    assert_eq!(display["state"], "open");
    assert_eq!(display["total"], 2);
    assert_eq!(
        display["requirements"][0]["doneWhen"],
        "The parser rejects empty input."
    );
    assert!(
        display["currentWork"]
            .as_str()
            .is_some_and(|w| w.starts_with("smw_")),
        "{display}"
    );
    assert!(
        display["lessonCount"].is_null(),
        "no lessons configured: {display}"
    );

    let (line, _) = session.status_line();
    assert_eq!(
        line.trim_end(),
        "◎ Ship the R9 parser and report · S0/2 met · next S1: The parser rejects empty input."
    );
    let samples = (0..100)
        .map(|_| session.status_line().1)
        .collect::<Vec<_>>();
    let p95 = p95(samples);
    eprintln!("statusline p95 over 100 runs: {p95} us");
    assert!(p95 < 50_000, "status line p95 {p95} us");

    // SessionStart retains progress only for the same session Goal event.
    let env_file = session.home.join("claude-env");
    fs::write(&env_file, "").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&env_file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut hook_session = Session {
        extra: vec![("CLAUDE_ENV_FILE", env_file)],
        ..session
    };
    let payload = json!({"hook_event_name":"SessionStart","source":"resume",
        "session_id":hook_session.id,"cwd":hook_session.root});
    let hook = hook_session.call(
        &[
            env!("CARGO_BIN_EXE_exitbind"),
            "hook-run",
            "--host",
            "claude",
        ],
        payload.to_string().as_bytes(),
    );
    assert!(hook.status.success(), "{hook:?}");
    assert_eq!(hook_session.status_line().0.trim_end(), line.trim_end());
    let kept = hook_session.display();
    for field in ["requirements", "met", "total", "nextAction", "lessonCount"] {
        assert_eq!(kept[field], display[field], "{field}");
    }

    // A different event cannot retain progress, even when the Goal ID agrees.
    let mut stale = kept.clone();
    stale["eventSha256"] = json!("0".repeat(64));
    fs::write(hook_session.display_path(), stale.to_string()).unwrap();
    let hook = hook_session.call(
        &[
            env!("CARGO_BIN_EXE_exitbind"),
            "hook-run",
            "--host",
            "claude",
        ],
        payload.to_string().as_bytes(),
    );
    assert!(hook.status.success(), "{hook:?}");
    let refreshed = hook_session.display();
    assert_eq!(refreshed["eventSha256"], display["eventSha256"]);
    for field in [
        "requirements",
        "met",
        "total",
        "nextAction",
        "lessonCount",
        "currentWork",
        "closingRoute",
    ] {
        assert!(refreshed.get(field).is_none(), "stale {field}: {refreshed}");
    }
    assert_eq!(
        hook_session.status_line().0.trim_end(),
        "◎ Ship the R9 parser and report"
    );

    // An explicitly selected project journal display is not touched.
    let mut project = kept.clone();
    project["journal"] = json!("project");
    let project_bytes = project.to_string();
    fs::write(hook_session.display_path(), &project_bytes).unwrap();
    let hook = hook_session.call(
        &[
            env!("CARGO_BIN_EXE_exitbind"),
            "hook-run",
            "--host",
            "claude",
        ],
        payload.to_string().as_bytes(),
    );
    assert!(hook.status.success(), "{hook:?}");
    assert_eq!(
        fs::read_to_string(hook_session.display_path()).unwrap(),
        project_bytes
    );

    // A missing display is recreated from metadata only.
    fs::remove_dir_all(hook_session.store.join("display")).unwrap();
    assert_eq!(hook_session.status_line().0, "");
    let hook = hook_session.call(
        &[
            env!("CARGO_BIN_EXE_exitbind"),
            "hook-run",
            "--host",
            "claude",
        ],
        payload.to_string().as_bytes(),
    );
    assert!(hook.status.success(), "{hook:?}");
    assert_eq!(
        hook_session.status_line().0.trim_end(),
        "◎ Ship the R9 parser and report"
    );
    assert_eq!(hook_session.display()["revision"], display["revision"]);
    hook_session.extra.clear();
    let session = hook_session;

    // Re-running setup keeps the installed line unchanged.
    assert_eq!(setup(&session)["statusLine"]["status"], "unchanged");
}

#[test]
fn a_closed_goal_reads_closed_after_the_tree_moves() {
    let session = Session::new("native-display-closed");
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--skip-skills", "--root"])
        .arg(&session.root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let config = session.config();
    let exe = env!("CARGO_BIN_EXE_exitbind");
    session.json(&[
        exe,
        "goal",
        "incorporate",
        "--goal-id",
        "g1",
        "--goal",
        "Close the display test goal",
        "--none-applicable",
        "obligations,findings,blockers,decisions,externalActions",
        "--json",
        "--config",
        &config,
    ]);
    assert_eq!(session.display()["state"], "open");
    let started = Instant::now();
    session.json(&[
        exe,
        "goal",
        "close",
        "--direct",
        "--goal-id",
        "g1",
        "--json",
        "--config",
        &config,
    ]);
    eprintln!("goal close incl. display refresh: {:?}", started.elapsed());
    let closed = session.display();
    assert_eq!(closed["state"], "closed");
    // The tree moves after closure; a later ledger command rewrites the file.
    fs::write(session.root.join("moved.txt"), "after closure\n").unwrap();
    session.json(&[
        exe,
        "goal",
        "usage",
        "--goal-id",
        "g1",
        "--json",
        "--config",
        &config,
    ]);
    let after = session.display();
    assert_eq!(after["state"], "closed");
    let status = session.json(&[exe, "goal", "status", "--json", "--config", &config]);
    assert_eq!(after["revision"], status["revision"]);
    assert_eq!(
        session.status_line().0.trim_end(),
        "◎ Close the display test goal · closed"
    );

    // A fresh SessionStart restores the recorded closed state without
    // re-evaluating its old Work, inputs or lesson history.
    fs::remove_dir_all(session.store.join("display")).unwrap();
    let env_file = session.home.join("claude-env");
    fs::write(&env_file, "").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&env_file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let session = Session {
        extra: vec![("CLAUDE_ENV_FILE", env_file)],
        ..session
    };
    let payload = json!({"hook_event_name":"SessionStart","source":"resume",
        "session_id":session.id,"cwd":session.root});
    let hook = session.call(
        &[exe, "hook-run", "--host", "claude"],
        payload.to_string().as_bytes(),
    );
    assert!(hook.status.success(), "{hook:?}");
    let metadata = session.display();
    assert_eq!(metadata["state"], "closed");
    assert_eq!(metadata["revision"], closed["revision"]);
    assert_eq!(metadata["eventSha256"], closed["eventSha256"]);
    assert!(metadata.get("requirements").is_none());
    assert_eq!(
        session.status_line().0.trim_end(),
        "◎ Close the display test goal · closed"
    );
}

#[test]
fn claude_session_start_refuses_over_limit_history_without_replacing_display() {
    let session = Session::new("native-display-recovery-limits");
    let output = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--skip-skills", "--root"])
        .arg(&session.root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let config = session.config();
    let exe = env!("CARGO_BIN_EXE_exitbind");
    session.json(&[
        exe,
        "goal",
        "incorporate",
        "--goal-id",
        "g",
        "--goal",
        "Keep the Goal",
        "--config",
        &config,
        "--json",
    ]);
    let display_path = session.display_path();
    let previous_display = fs::read(&display_path).unwrap();
    let journal = fs::read_dir(&session.store)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(".exitbind/session-goal.jsonl"))
        .find(|path| path.is_file())
        .expect("the native Goal journal");
    let valid = fs::read(&journal).unwrap();
    let env_file = session.home.join("claude-env");
    fs::write(&env_file, "").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&env_file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let session = Session {
        extra: vec![("CLAUDE_ENV_FILE", env_file)],
        ..session
    };
    let payload = json!({"hook_event_name":"SessionStart","source":"resume",
        "session_id":session.id,"cwd":session.root});
    for (bytes, expected) in [(false, "4096 physical records"), (true, "byte bound")] {
        if bytes {
            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&journal)
                .unwrap()
                .set_len(64 * 1024 * 1024 + 1)
                .unwrap();
        } else {
            fs::write(&journal, "{}\n".repeat(4097)).unwrap();
        }
        let hook = session.call(
            &[exe, "hook-run", "--host", "claude"],
            payload.to_string().as_bytes(),
        );
        assert!(hook.status.success(), "{hook:?}");
        let result: Value = serde_json::from_slice(&hook.stdout).unwrap();
        let context = result["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(
            context.contains("session recovery is unresolved"),
            "{context}"
        );
        assert!(context.contains(expected), "{context}");
        assert_eq!(fs::read(&display_path).unwrap(), previous_display);
    }
    fs::write(journal, valid).unwrap();
}

#[test]
fn setup_keeps_a_status_line_the_human_already_has() {
    let session = Session::new("native-display-foreign");
    fs::create_dir_all(session.home.join(".claude")).unwrap();
    fs::write(
        session.home.join(".claude/settings.json"),
        r#"{"statusLine":{"type":"command","command":"my-line"}}"#,
    )
    .unwrap();
    let report = setup(&session);
    assert_eq!(report["statusLine"]["status"], "kept", "{report}");
    assert!(
        !session.root.join(".claude/settings.json").exists()
            || !fs::read_to_string(session.root.join(".claude/settings.json"))
                .unwrap()
                .contains("statusline")
    );
}

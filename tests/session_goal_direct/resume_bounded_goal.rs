//! Resume must bound Goal projection as well as its initial Work scan.
use crate::fixture::Project;
use serde_json::{json, Value};
use std::{fs, io::Write, process::Stdio};

#[cfg(target_os = "linux")]
struct OpenWatch(std::os::fd::RawFd);

#[cfg(target_os = "linux")]
impl OpenWatch {
    fn new(path: &std::path::Path) -> Self {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // The owned descriptor is closed in Drop; the watched path belongs to
        // this isolated fixture and IN_OPEN observes reads without reading it.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        assert!(fd >= 0, "{}", std::io::Error::last_os_error());
        let result = unsafe { libc::inotify_add_watch(fd, path.as_ptr(), libc::IN_OPEN) };
        assert!(result >= 0, "{}", std::io::Error::last_os_error());
        Self(fd)
    }

    fn assert_unopened(&self) {
        let mut events = [0u8; 1024];
        let count = unsafe { libc::read(self.0, events.as_mut_ptr().cast(), events.len()) };
        assert!(
            count == -1 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock,
            "resume opened the oversized tested input after refusing it; event bytes={count}"
        );
    }
}

#[cfg(target_os = "linux")]
impl Drop for OpenWatch {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn in_session(project: &Project, session: &str, args: &[&str]) -> Value {
    project.value_in_session("codex", session, &project.session_store, args)
}

fn begin(project: &Project, session: &str) -> String {
    fs::write(project.root.join("input"), b"original").unwrap();
    in_session(
        project,
        session,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "resume-budget",
            "--goal",
            "Keep the direct task current",
            "--obligation",
            "Direct task",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ],
    );
    in_session(
        project,
        session,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Controlled Work",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    )["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn direct(project: &Project, session: &str) {
    in_session(
        project,
        session,
        &[
            "goal",
            "incorporate",
            "--direct",
            "--goal-id",
            "resume-budget",
            "--goal",
            "Keep the direct task current",
            "--obligation",
            "Direct task",
        ],
    );
}

fn block(project: &Project, session: &str, work: &str) {
    let detail = in_session(project, session, &["work", "detail", work, "--json"]);
    let choice = detail["actionForms"]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["label"] == "blocked")
        .unwrap();
    let argv = choice["command"]["argv"].as_array().unwrap();
    let mut command = super::support::git_topology::command(argv[0].as_str().unwrap());
    command.current_dir(&project.root);
    for arg in &argv[1..] {
        let arg = arg.as_str().unwrap();
        command.arg(if arg == "<REASON>" {
            "controlled blocked Work"
        } else {
            arg
        });
    }
    for name in [
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "EXITBIND_NATIVE_SESSION_ID",
    ] {
        command.env_remove(name);
    }
    command
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", &project.session_store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture blocked\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_direct_state(resumed: &Value, expected: &str) {
    assert_eq!(
        resumed["presentation"]["goalProgress"]["categoryDispositions"]["obligations"]["items"][0]
            ["directEvidence"]["state"],
        expected,
        "{resumed}"
    );
    assert!(resumed["presentation"]["terminal"].is_null());
}

#[test]
fn blocked_terminal_goal_projection_obeys_resume_input_limit() {
    let project = Project::new("resume-terminal-goal-budget");
    let session = "resume-terminal-goal-budget";
    let work = begin(&project, session);
    block(&project, session, &work);
    direct(&project, session);

    let current = in_session(&project, session, &["work", "resume", "--json"]);
    assert_eq!(current["status"], "none");
    assert_direct_state(&current, "current");
    fs::write(project.root.join("input"), b"changed").unwrap();
    let changed = in_session(&project, session, &["work", "resume", "--json"]);
    assert_direct_state(&changed, "stale");

    fs::File::create(project.root.join("large"))
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    #[cfg(target_os = "linux")]
    let watch = OpenWatch::new(&project.root.join("large"));
    let limited = in_session(&project, session, &["work", "resume", "--json"]);
    assert_eq!(limited["status"], "unresolved", "{limited}");
    assert_eq!(limited["reason"]["code"], "discovery_limited");
    assert_eq!(
        limited["discovery"]["limitedReason"],
        "fingerprint_byte_limit_exceeded"
    );
    assert_eq!(limited["discovery"]["complete"], false);
    assert!(limited["presentation"]["terminal"].is_null());
    #[cfg(target_os = "linux")]
    watch.assert_unopened();

    let key = crate::fixture::digest_value(&json!({"host":"codex","identity":session}));
    let display: Value = serde_json::from_slice(
        &fs::read(
            project
                .session_store
                .join("display")
                .join(format!("{key}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(display["goalId"], "resume-budget");
    assert_eq!(display["journal"], "session");
    assert_eq!(display["state"], "open");
    assert!(display.get("requirements").is_none(), "{display}");
    assert!(display.get("met").is_none(), "{display}");
}

#[test]
fn active_work_keeps_currentness_and_association_with_resume_budget() {
    let project = Project::new("resume-active-goal-budget");
    let session = "resume-active-goal-budget";
    let work = begin(&project, session);
    direct(&project, session);

    let current = in_session(&project, session, &["work", "resume", "--json"]);
    assert_eq!(current["status"], "resumed");
    assert_eq!(current["work"], work);
    if current["truncated"] == true {
        assert!(current["fullCommand"].is_array());
    }
    let current = in_session(&project, session, &["work", "resume", "--json", "--full"]);
    assert_direct_state(&current, "current");
    assert_eq!(
        current["presentation"]["goalProgress"]["workAssociation"]["kind"],
        "journey_member"
    );
    fs::write(project.root.join("input"), b"changed").unwrap();
    let changed = in_session(&project, session, &["work", "resume", "--json", "--full"]);
    assert_eq!(changed["work"], work);
    assert_direct_state(&changed, "stale");

    fs::File::create(project.root.join("large"))
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let limited = in_session(&project, session, &["work", "resume", "--json"]);
    assert_eq!(limited["status"], "unresolved");
    assert_eq!(limited["reason"]["code"], "discovery_limited");
    assert_eq!(
        limited["discovery"]["limitedReason"],
        "fingerprint_byte_limit_exceeded"
    );
    assert!(limited["presentation"]["terminal"].is_null());
}

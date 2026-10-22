//! R55 F5b (FR55-10): a lens for a same-host child needs no cross-host
//! protocol. A Work begun in a native session, with no `work record`,
//! `work continuation` or `work bind`: `work next` prints the
//! `work child prepare … --agent-type sonic --perspectives …` route, it runs
//! unedited, and SubagentStart presents the profile and both lens ids with
//! hashes. Another session neither sees the route nor may prepare a child.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

const SESSION: &str = "f5b-lead-session";
const OTHER: &str = "f5b-other-session";
const CHILD: &str = "f5b-sonic-child";
const PROFILE: &str = "your.soulmate/agents/sonic/profile.md";
const PROFILE_BODY: &str = "Sonic implements one bounded change.\n";
const LENSES: [(&str, &str); 2] = [
    (
        "agent-surface-text",
        "Derive printed facts from the record, not from directory layout.\n",
    ),
    (
        "release-trust-boundary",
        "Name which side of the release path is trusted.\n",
    ),
];
const RESULT: &str = "Implemented; evidence attached.\n";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Fixture {
    base: PathBuf,
    product: PathBuf,
    store: PathBuf,
    config: String,
    work: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// `argv[0]` is the executable; `session` is the native session selector.
fn run(argv: &[String], cwd: &Path, store: &Path, session: Option<&str>, input: &[u8]) -> Output {
    let mut command = super::support::git_topology::command(&argv[0]);
    command
        .current_dir(cwd)
        .args(&argv[1..])
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(session) = session {
        command.env("CLAUDE_CODE_SESSION_ID", session);
    }
    let mut child = command.spawn().unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = super::support::temp(name);
        let product = base.join("product");
        let store = base.join("session-goals");
        fs::create_dir(&product).unwrap();
        super::support::git_topology::repository(&product);
        let mut fixture = Self {
            base,
            product,
            store,
            config: String::new(),
            work: String::new(),
        };
        let root = fixture.product.to_str().unwrap().to_owned();
        let init = fixture.call(
            None,
            &["init", "--mode", "portable", "--root", &root],
            b"",
            false,
        );
        assert!(init.status.success(), "{}", text(&init));
        let config_path = fixture.product.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        config["agents"]["worker"]["profile"] = json!(PROFILE);
        config["agents"]["worker"]["nativeName"] = json!("sonic");
        fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let mut files = vec![(PROFILE.to_owned(), PROFILE_BODY)];
        for (id, body) in LENSES {
            files.push((format!("your.soulmate/lenses/{id}.md"), body));
        }
        for (path, body) in files {
            let path = fixture.product.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        fixture.config = fs::canonicalize(&config_path)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        fixture.value(
            Some(SESSION),
            &[
                "goal",
                "incorporate",
                "--goal-id",
                "f5b-goal",
                "--goal",
                "A same-host lens needs no cross-host protocol",
                "--obligation",
                "Present the lens to the same-host child",
            ],
        );
        let begun = fixture.value(
            Some(SESSION),
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "Spawn sonic with its lenses from the Lead session",
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--review-policy",
                "required",
            ],
        );
        fixture.work = begun["work"].as_str().unwrap().to_owned();
        let lead = begun["next"]["assignment"].as_str().unwrap().to_owned();
        let work = fixture.work.clone();
        let returned = fixture.call(
            None,
            &["work", "return", &work, &lead, "--outcome", "scoped"],
            b"scope",
            true,
        );
        assert!(returned.status.success(), "{}", text(&returned));
        fixture
    }

    fn call(&self, session: Option<&str>, args: &[&str], input: &[u8], config: bool) -> Output {
        let mut argv = vec![env!("CARGO_BIN_EXE_exitbind").to_owned()];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        if config {
            argv.extend(["--config".to_owned(), self.config.clone()]);
        }
        run(&argv, &self.product, &self.store, session, input)
    }

    fn value(&self, session: Option<&str>, args: &[&str]) -> Value {
        let output = self.call(session, args, b"", true);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn next(&self, session: Option<&str>) -> Value {
        self.value(session, &["work", "next", &self.work])["next"].clone()
    }

    fn hook(&self, payload: Value) -> Option<String> {
        let argv = [env!("CARGO_BIN_EXE_exitbind").to_owned(), "hook-run".into()];
        let mut payload = payload;
        payload["cwd"] = json!(self.product);
        let output = run(
            &argv,
            &self.product,
            &self.store,
            None,
            payload.to_string().as_bytes(),
        );
        assert!(output.status.success(), "{payload}: {}", text(&output));
        if output.stdout.is_empty() {
            return None;
        }
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        Some(
            value["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    }

    /// The private child intent records, if any were written.
    fn intents(&self) -> Option<Vec<Value>> {
        let path = self.product.join(".exitbind/child-intents.json");
        let bytes = fs::read(path).ok()?;
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        Some(value["intents"].as_array().unwrap().clone())
    }

    /// The cross-host protocol was never used for this Work.
    fn assert_no_continuation(&self) {
        let read = self.call(None, &["work", "continuation", &self.work], b"", true);
        assert!(!read.status.success(), "{}", text(&read));
        assert!(
            text(&read).contains("continuation has not been initialized"),
            "{}",
            text(&read)
        );
    }
}

fn argv(route: &Value) -> Vec<String> {
    route["command"]
        .as_array()
        .unwrap_or_else(|| panic!("no command: {route}"))
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_same_host_child_receives_its_lenses_through_the_printed_route_alone() {
    let fixture = Fixture::new("f5b-same-host-lens");
    fixture.assert_no_continuation();
    let next = fixture.next(Some(SESSION));
    let route = &next["prepareChild"];
    assert_eq!(route["sameConfigRequired"], false, "{route}");
    assert_eq!(route["sameExecutableRequired"], false, "{route}");
    assert_eq!(
        route["perspectives"],
        json!(["agent-surface-text", "release-trust-boundary"])
    );
    let argv = argv(route);
    assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"));
    assert_eq!(argv[1..4], ["work", "child", "prepare"]);
    for word in ["--agent-type", "sonic", "--perspectives", "--config"] {
        assert!(argv.iter().any(|item| item == word), "{argv:?}");
    }
    assert!(!argv.iter().any(|item| item == "--context"), "{argv:?}");
    assert!(
        !argv.iter().any(|item| item.starts_with('<')),
        "the route carries no placeholder: {argv:?}"
    );

    // Run exactly as printed, in the Lead session.
    let prepared = run(&argv, &fixture.product, &fixture.store, Some(SESSION), b"");
    assert!(prepared.status.success(), "{}", text(&prepared));
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["effect"], "prepared", "{prepared}");
    assert_eq!(
        prepared["selected"]["nativeTaskName"], "sonic",
        "{prepared}"
    );
    fixture.assert_no_continuation();

    let context = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .expect("SubagentStart presents context");
    assert!(context.contains("Native task name: sonic"), "{context}");
    let profile = format!("Profile SHA-256: {}", sha(PROFILE_BODY.as_bytes()));
    assert!(context.contains(&profile), "{context}");
    for (id, body) in LENSES {
        let lens = format!(
            "Perspective {id} (source SHA-256 {}, presented SHA-256 ",
            sha(body.as_bytes())
        );
        assert!(context.contains(&lens), "{context}");
    }

    let stopped = fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
    );
    assert_eq!(stopped, None);
    let intents = fixture.intents().unwrap();
    assert_eq!(intents.len(), 1, "{intents:?}");
    assert_eq!(intents[0]["state"], "recorded", "{intents:?}");
    assert_eq!(intents[0]["nativeChild"], CHILD);
    assert_eq!(intents[0]["resultSha256"], sha(RESULT.as_bytes()));
    fixture.assert_no_continuation();
}

#[test]
fn a_session_that_does_not_hold_the_work_gets_no_route_and_prepares_nothing() {
    let fixture = Fixture::new("f5b-other-session");
    let next = fixture.next(Some(SESSION));
    let argv = argv(&next["prepareChild"]);
    assert!(fixture.next(Some(OTHER)).get("prepareChild").is_none());
    assert!(fixture.next(None).get("prepareChild").is_none());

    // The same printed route from another session, or from no session.
    let other = run(&argv, &fixture.product, &fixture.store, Some(OTHER), b"");
    assert!(!other.status.success(), "{}", text(&other));
    assert!(
        text(&other).contains("this native session does not hold the Work"),
        "{}",
        text(&other)
    );
    let none = run(&argv, &fixture.product, &fixture.store, None, b"");
    assert!(!none.status.success(), "{}", text(&none));
    assert!(
        text(&none).contains("uses the --context token from work continuation"),
        "{}",
        text(&none)
    );
    // A --context token keeps the cross-host route and its refusal.
    let mut forged = argv.clone();
    forged.extend(["--context".to_owned(), format!("{}:1:0:00", fixture.work)]);
    let forged = run(
        &forged,
        &fixture.product,
        &fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(!forged.status.success(), "{}", text(&forged));
    assert!(
        text(&forged).contains("work context token is invalid"),
        "{}",
        text(&forged)
    );
    assert!(fixture.intents().map_or(true, |intents| intents.is_empty()));

    // A child the other session starts claims nothing and is shown nothing
    // beyond its profile.
    let prepared = run(&argv, &fixture.product, &fixture.store, Some(SESSION), b"");
    assert!(prepared.status.success(), "{}", text(&prepared));
    let context = fixture
        .hook(json!({"hook_event_name":"SubagentStart","session_id":OTHER,
            "agent_id":CHILD,"agent_type":"sonic"}))
        .unwrap_or_default();
    assert!(!context.contains("Perspective "), "{context}");
    assert_eq!(fixture.intents().unwrap()[0]["state"], "prepared");
}

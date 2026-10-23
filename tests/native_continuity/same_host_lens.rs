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
    base: super::support::TempRoot,
    product: PathBuf,
    store: PathBuf,
    config: String,
    work: String,
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

impl Fixture {
    /// Prepare `sonic` through the printed route in the Lead session.
    fn prepare_sonic(&self) {
        let argv = argv(&self.next(Some(SESSION))["prepareChild"]);
        let prepared = run(&argv, &self.product, &self.store, Some(SESSION), b"");
        assert!(prepared.status.success(), "{}", text(&prepared));
    }

    /// The Work's native Goal context companion.
    fn companion(&self) -> PathBuf {
        let token = self.work.strip_prefix("smw_").unwrap();
        self.product
            .join(format!(".exitbind/runs/work-{token}.session-goal.json"))
    }
}

/// Review D1: a same-host binding that stops verifying between prepare and
/// stop is a changed binding: the child is recorded `failed` with its reason
/// and its final message kept, never silently left `claimed`; at start it is
/// `abandoned` with the reason. `work link-session` from another session
/// cannot rebind the Work (it refuses and changes nothing), so the binding is
/// broken by removing the Work's companion, which every same-host
/// verification reads.
#[test]
fn a_same_host_binding_that_stops_verifying_fails_the_child_and_keeps_its_message() {
    let fixture = Fixture::new("f5b-binding-changed-stop");
    fixture.prepare_sonic();
    let started = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(
        started.contains("Perspective agent-surface-text"),
        "{started}"
    );
    let linked = fixture.call(
        Some(OTHER),
        &["work", "link-session", &fixture.work],
        b"",
        true,
    );
    assert!(!linked.status.success(), "{}", text(&linked));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "claimed");

    fs::remove_file(fixture.companion()).unwrap();
    let stopped = fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
    );
    assert_eq!(stopped, None);
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(intent["state"], "failed", "{intent}");
    let failure = intent["failure"].as_str().unwrap();
    assert!(
        failure.starts_with("the receiving binding changed before the child finished: "),
        "{failure}"
    );
    assert_eq!(intent["pendingResult"], RESULT, "{intent}");
    assert_eq!(intent["resultSha256"], sha(RESULT.as_bytes()));

    let fixture = Fixture::new("f5b-binding-changed-start");
    fixture.prepare_sonic();
    fs::remove_file(fixture.companion()).unwrap();
    let started = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(
        started.contains("the receiving binding changed before a child started: "),
        "{started}"
    );
    assert!(!started.contains("Perspective "), "{started}");
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(intent["state"], "abandoned", "{intent}");
    assert!(intent["failure"]
        .as_str()
        .unwrap()
        .starts_with("the receiving binding changed before a child started: "));
}

/// Review D2: a Work begun in the Lead session by a project that does not own
/// the session's native Goal gets no route, and preparing refuses naming the
/// owner, though the session holds the Work's context.
#[test]
fn a_work_whose_native_goal_another_project_owns_gets_no_route_and_prepares_nothing() {
    let fixture = Fixture::new("f5b-foreign-owner");
    let other = fixture.base.join("other");
    fs::create_dir(&other).unwrap();
    super::support::git_topology::repository(&other);
    let exe = env!("CARGO_BIN_EXE_exitbind").to_owned();
    let call = |session: Option<&str>, args: &[&str]| {
        let mut argv = vec![exe.clone()];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        run(&argv, &other, &fixture.store, session, b"")
    };
    let root = other.to_str().unwrap();
    let init = call(None, &["init", "--mode", "portable", "--root", root]);
    assert!(init.status.success(), "{}", text(&init));
    let config = other.join("exitbind.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["agents"]["worker"]["profile"] = json!(PROFILE);
    value["agents"]["worker"]["nativeName"] = json!("sonic");
    fs::write(&config, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    for (path, body) in [
        (PROFILE.to_owned(), PROFILE_BODY),
        (
            format!("your.soulmate/lenses/{}.md", LENSES[0].0),
            LENSES[0].1,
        ),
    ] {
        fs::create_dir_all(other.join(&path).parent().unwrap()).unwrap();
        fs::write(other.join(path), body).unwrap();
    }
    let config = fs::canonicalize(config).unwrap();
    let config = config.to_str().unwrap();
    let begun = call(
        Some(SESSION),
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "A Work of a project that does not own the session Goal",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--config",
            config,
        ],
    );
    assert!(begun.status.success(), "{}", text(&begun));
    let begun: Value = serde_json::from_slice(&begun.stdout).unwrap();
    let work = begun["work"].as_str().unwrap();
    let lead = begun["next"]["assignment"].as_str().unwrap();
    let scoped = call(
        None,
        &[
            "work",
            "return",
            work,
            lead,
            "--outcome",
            "scoped",
            "--config",
            config,
        ],
    );
    assert!(scoped.status.success(), "{}", text(&scoped));
    let token = work.strip_prefix("smw_").unwrap();
    assert!(
        other
            .join(format!(".exitbind/runs/work-{token}.session-goal.json"))
            .is_file(),
        "the Lead session holds this Work's native Goal context"
    );

    let next = call(Some(SESSION), &["work", "next", work, "--config", config]);
    assert!(next.status.success(), "{}", text(&next));
    let next: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_eq!(next["current"]["action"], "spawn", "{next}");
    assert!(next["next"].get("prepareChild").is_none(), "{next}");
    let assignment = next["next"]["assignment"].as_str().unwrap();
    let prepared = call(
        Some(SESSION),
        &[
            "work",
            "child",
            "prepare",
            work,
            assignment,
            "--agent-type",
            "sonic",
            "--config",
            config,
        ],
    );
    assert!(!prepared.status.success(), "{}", text(&prepared));
    assert!(
        text(&prepared).contains("owned by another configured project"),
        "{}",
        text(&prepared)
    );
    assert!(!other.join(".exitbind/child-intents.json").exists());
}

/// Review D3: a lens the perspective limits refuse is left out of the printed
/// route with why, so the route still runs unedited.
#[test]
fn an_oversized_lens_is_omitted_with_its_reason_and_the_route_still_runs() {
    let fixture = Fixture::new("f5b-oversized-lens");
    let big = "your.soulmate/lenses/oversized.md";
    fs::write(fixture.product.join(big), "x".repeat(4097)).unwrap();
    let route = fixture.next(Some(SESSION))["prepareChild"].clone();
    assert_eq!(
        route["perspectives"],
        json!(["agent-surface-text", "release-trust-boundary"]),
        "{route}"
    );
    let omitted = route["perspectivesOmitted"].as_array().unwrap();
    assert_eq!(omitted.len(), 1, "{route}");
    assert!(
        omitted[0].as_str().unwrap().starts_with(big)
            && omitted[0].as_str().unwrap().contains("4096 bytes"),
        "{route}"
    );
    let argv = argv(&route);
    assert!(
        !argv.iter().any(|word| word.contains("oversized.md")),
        "{argv:?}"
    );
    let prepared = run(&argv, &fixture.product, &fixture.store, Some(SESSION), b"");
    assert!(prepared.status.success(), "{}", text(&prepared));
}

impl Fixture {
    /// `work child recover WORK INTENT` with `extra`, from `session`.
    fn recover(&self, session: Option<&str>, intent: &str, extra: &[&str]) -> Output {
        let mut args = vec!["work", "child", "recover", self.work.as_str(), intent];
        args.extend(extra);
        self.call(session, &args, b"", true)
    }
}

/// Review F5b R2: a failed same-host child's retained final message is
/// reattached without a cross-host context. Recover without `--context`
/// refuses another session and no session and changes nothing; with
/// `--context` it keeps the cross-host route and its refusal; from the Lead
/// session that holds the Work again it records the hash and state, as a
/// same-host stop does, with no continuation record.
#[test]
fn a_failed_same_host_child_is_recovered_from_the_session_that_holds_the_work() {
    let fixture = Fixture::new("f5b-same-host-recover");
    fixture.prepare_sonic();
    fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let companion = fs::read(fixture.companion()).unwrap();
    fs::remove_file(fixture.companion()).unwrap();
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
    );
    let failed = fixture.intents().unwrap()[0].clone();
    assert_eq!(failed["state"], "failed", "{failed}");
    assert_eq!(failed["pendingResult"], RESULT, "{failed}");
    let intent = failed["id"].as_str().unwrap().to_owned();
    fs::write(fixture.companion(), companion).unwrap();

    let other = fixture.recover(Some(OTHER), &intent, &[]);
    assert!(!other.status.success(), "{}", text(&other));
    assert!(
        text(&other).contains("this native session does not hold the Work"),
        "{}",
        text(&other)
    );
    let none = fixture.recover(None, &intent, &[]);
    assert!(!none.status.success(), "{}", text(&none));
    assert!(
        text(&none).contains("uses the --context token from work continuation"),
        "{}",
        text(&none)
    );
    let token = format!("{}:1:0:00", fixture.work);
    let context = fixture.recover(Some(SESSION), &intent, &["--context", &token]);
    assert!(!context.status.success(), "{}", text(&context));
    assert!(
        text(&context).contains("work context token is invalid"),
        "{}",
        text(&context)
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        failed,
        "refusals change nothing"
    );

    let recovered = fixture.recover(Some(SESSION), &intent, &[]);
    assert!(recovered.status.success(), "{}", text(&recovered));
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["effect"], "recorded", "{recovered}");
    assert_eq!(recovered["intent"], intent.as_str());
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(intent["state"], "recorded", "{intent}");
    assert_eq!(intent["nativeChild"], CHILD);
    assert_eq!(intent["resultSha256"], sha(RESULT.as_bytes()));
    assert!(intent.get("pendingResult").is_none(), "{intent}");
    assert!(intent.get("failure").is_none(), "{intent}");
    fixture.assert_no_continuation();
    let again = fixture.recover(Some(SESSION), intent["id"].as_str().unwrap(), &[]);
    assert!(
        text(&again).contains("this child has no retained result to recover"),
        "{}",
        text(&again)
    );
}

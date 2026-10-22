//! R55 F5a: every hook event the dispatcher handles resolves the project the
//! session's native Goal belongs to, or says which project it used. The Lead
//! session's `cwd` is outside the product root (R55: the Lead's home
//! directory, whose `exitbind.json` names no `sonic`); the native Goal is owned by the product
//! project. The event list is read from the dispatcher's own declaration, so
//! a new event fails here until its row exists.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

const SESSION: &str = "f5a-lead-session";
const UNOWNED: &str = "f5a-session-without-goal";
const CHILD: &str = "f5a-sonic-child";
const PROFILE: &str = "your.soulmate/agents/sonic/profile.md";
const PROFILE_BODY: &str = "Sonic implements one bounded change.\n";
const LENS: &str = "lenses/agent-surface-text.md";
const LENS_BODY: &str = "Derive printed facts from the record, not from directory layout.\n";
const RESULT: &str = "Implemented; evidence attached.\n";
const PLAN: &str = "# Plan: F5a\n\n## Steps\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 1 | Resolve the owner | Every event uses the owner. |\n";
const OWNER_FROM: &str = "(native Goal owner).";
const CWD_FROM: &str = "(working directory).";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The events `hook-run` handles, read from the dispatcher's declaration.
/// The declared array length is checked by the compiler, so a reader that
/// dropped or invented a name fails the count.
fn dispatcher_events(source: &str) -> Vec<String> {
    let declaration = source
        .split("const EVENTS: [&str; ")
        .nth(1)
        .expect("the dispatcher declares its event list");
    let (count, rest) = declaration.split_once("] = [").expect("array form");
    let body = rest.split("];").next().expect("closed list");
    let names = body
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .flat_map(|line| line.split(','))
        .map(|name| name.trim().trim_matches('"').to_owned())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), count.trim().parse::<usize>().unwrap());
    names
}

#[test]
fn the_event_list_reader_reads_a_differently_written_declaration() {
    let source = "const OTHER: [&str; 1] = [\"Decoy\"];\nconst EVENTS: [&str; 3] = [\"First\", // one\n    \"Second\",\"Third\" ,\n];\nconst LATER: [&str; 1] = [\"Late\"];\n";
    assert_eq!(dispatcher_events(source), ["First", "Second", "Third"]);
    let real =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/host/runtime.rs"))
            .unwrap();
    let events = dispatcher_events(&real);
    assert!(
        events.iter().any(|event| event == "SubagentStart"),
        "{events:?}"
    );
}

struct Fixture {
    base: PathBuf,
    product: PathBuf,
    decoy: PathBuf,
    store: PathBuf,
    config: String,
    work: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn exitbind(
    cwd: &Path,
    store: &Path,
    session: Option<&str>,
    args: &[&str],
    input: &[u8],
) -> Output {
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(cwd)
        .args(args)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(session) = session {
        command.env("CLAUDE_CODE_SESSION_ID", session);
    }
    let mut child = command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
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

fn files_under(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

impl Fixture {
    fn new() -> Self {
        let base = super::support::temp("f5a-goal-owner");
        let product = base.join("product");
        let decoy = base.join("home");
        let store = base.join("session-goals");
        fs::create_dir(&product).unwrap();
        fs::create_dir(&decoy).unwrap();
        super::support::git_topology::repository(&product);
        for root in [&product, &decoy] {
            let init = exitbind(
                &base,
                &store,
                None,
                &[
                    "init",
                    "--mode",
                    "portable",
                    "--root",
                    root.to_str().unwrap(),
                ],
                b"",
            );
            assert!(init.status.success(), "{}", text(&init));
        }
        let config_path = product.join("exitbind.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        config["agents"]["worker"]["profile"] = json!(PROFILE);
        config["agents"]["worker"]["nativeName"] = json!("sonic");
        fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        for (path, body) in [(PROFILE, PROFILE_BODY), (LENS, LENS_BODY)] {
            fs::create_dir_all(product.join(path).parent().unwrap()).unwrap();
            fs::write(product.join(path), body).unwrap();
        }
        let config = fs::canonicalize(&config_path)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let mut fixture = Self {
            base,
            product,
            decoy,
            store,
            config,
            work: String::new(),
        };
        fixture.lead(&[
            "goal",
            "incorporate",
            "--goal-id",
            "f5a-goal",
            "--goal",
            "Every hook event resolves the Goal's project",
            "--obligation",
            "Present the named agent from outside the product root",
        ]);
        let begun = fixture.lead(&[
            "work",
            "begin",
            "change",
            "--goal",
            "Spawn sonic from a Lead session outside the product root",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "owner:Resolve the owner project.",
            "--preservation-check-command",
            "true",
        ]);
        fixture.work = begun["work"].as_str().unwrap().to_owned();
        let lead = begun["next"]["assignment"].as_str().unwrap().to_owned();
        let returned = fixture.project(
            &[
                "work",
                "return",
                &fixture.work,
                &lead,
                "--outcome",
                "scoped",
            ],
            b"scope",
        );
        assert!(returned.status.success(), "{}", text(&returned));
        fixture.prepare_sonic();
        fixture
    }

    /// A Lead command in the native session.
    fn lead(&self, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.extend(["--config", self.config.as_str()]);
        let output = exitbind(&self.product, &self.store, Some(SESSION), &args, b"");
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// A project command without session selectors.
    fn project(&self, args: &[&str], input: &[u8]) -> Output {
        let mut args = args.to_vec();
        args.extend(["--config", self.config.as_str()]);
        exitbind(&self.product, &self.store, None, &args, input)
    }

    fn value(&self, args: &[&str]) -> Value {
        let output = self.project(args, b"");
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// `sonic` prepared with one lens for the worker assignment, received by
    /// the Lead session.
    fn prepare_sonic(&self) {
        let init = json!({"action":"init","sourceRef":"owner:request",
            "sourceText":"Resolve the owner project.",
            "requirements":[{"id":"owner","text":"Resolve the owner project."}]});
        let recorded = self.project(&["work", "record", &self.work], init.to_string().as_bytes());
        assert!(recorded.status.success(), "{}", text(&recorded));
        let token = |fixture: &Self| {
            fixture.value(&["work", "continuation", &fixture.work])["mutationContext"]["token"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let context = token(self);
        self.value(&[
            "work",
            "bind",
            &self.work,
            "--context",
            &context,
            "--host",
            "claude",
            "--session",
            SESSION,
            "--host-version",
            "fixture",
        ]);
        let context = token(self);
        let next = self.value(&["work", "next", &self.work, "--full"]);
        assert_eq!(next["next"]["packet"]["nativeTaskName"], "sonic", "{next}");
        let assignment = next["next"]["assignment"].as_str().unwrap();
        let perspectives = json!([{"id":"agent-surface-text","path":LENS}]).to_string();
        let prepared = self.value(&[
            "work",
            "child",
            "prepare",
            &self.work,
            assignment,
            "--context",
            &context,
            "--perspectives",
            &perspectives,
            "--agent-type",
            "sonic",
        ]);
        assert_eq!(prepared["effect"], "prepared", "{prepared}");
    }

    /// One hook event; its context, or `None` when it printed nothing.
    fn hook(&self, cwd: &Path, session: Option<&str>, payload: Value) -> Option<String> {
        self.hook_as(cwd, session, session.unwrap_or(UNOWNED), payload)
    }

    /// `selector`: the session in the hook process environment, if any;
    /// `reported`: the payload `session_id`.
    fn hook_as(
        &self,
        cwd: &Path,
        selector: Option<&str>,
        reported: &str,
        mut payload: Value,
    ) -> Option<String> {
        payload["cwd"] = json!(cwd);
        payload["session_id"] = json!(reported);
        let output = exitbind(
            cwd,
            &self.store,
            selector,
            &["hook-run"],
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

    fn fact(&self, from: &str) -> String {
        format!("Configuration used: {} {from}", self.config)
    }

    /// SessionStart names the exact `--config` path in its routes and says
    /// which configuration that is.
    fn session_names(&self, context: &str, config: &str, from: &str) {
        let line = format!("Configuration used: the --config path above {from}");
        assert!(context.contains(&line), "{context}");
        assert!(context.contains(&format!("--config {config}")), "{context}");
    }

    fn decoy_config(&self) -> String {
        let decoy = fs::canonicalize(self.decoy.join("exitbind.json")).unwrap();
        decoy.to_str().unwrap().to_owned()
    }

    fn children(&self) -> Vec<Value> {
        self.value(&["work", "continuation", &self.work])["children"]
            .as_array()
            .unwrap()
            .clone()
    }
}

fn payload(event: &str) -> Value {
    match event {
        "SessionStart" => json!({"hook_event_name":event,"source":"startup"}),
        "SubagentStart" => json!({"hook_event_name":event,"agent_id":CHILD,"agent_type":"sonic"}),
        "SubagentStop" => json!({"hook_event_name":event,"agent_id":CHILD,"agent_type":"sonic",
            "last_assistant_message":RESULT}),
        "PostToolUse" => json!({"hook_event_name":event,"tool_name":"ExitPlanMode","tool_input":{},
            "tool_response":{"isAgent":false,"plan":PLAN,"filePath":"/plans/f5a-owner.md"}}),
        _ => json!({"hook_event_name":event}),
    }
}

/// Every dispatcher event, `cwd` outside the product root, native Goal owned
/// by the product project: each event uses the owner project and says so.
#[test]
fn every_hook_event_resolves_the_project_that_owns_the_native_goal() {
    let fixture = Fixture::new();
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/host/runtime.rs"))
            .unwrap();
    let events = dispatcher_events(&source);
    let decoy_before = files_under(&fixture.decoy);
    let owner = fixture.fact(OWNER_FROM);
    let profile_sha = format!("Profile SHA-256: {}", sha(PROFILE_BODY.as_bytes()));
    let lens = format!(
        "Perspective agent-surface-text (source SHA-256 {}, presented SHA-256 ",
        sha(LENS_BODY.as_bytes())
    );
    for event in &events {
        let printed = fixture.hook(&fixture.decoy, Some(SESSION), payload(event));
        match event.as_str() {
            "SessionStart" => {
                for context in [
                    printed.unwrap(),
                    // Claude Code's channel after compaction.
                    fixture
                        .hook(
                            &fixture.decoy,
                            Some(SESSION),
                            json!({"hook_event_name":event,"source":"compact"}),
                        )
                        .unwrap(),
                ] {
                    fixture.session_names(&context, &fixture.config, OWNER_FROM);
                    assert!(
                        context.contains(&format!("current Work {}", fixture.work)),
                        "{context}"
                    );
                    assert!(
                        !context.contains("exact configuration is unresolved"),
                        "{context}"
                    );
                    assert!(
                        !context.contains(fixture.decoy.to_str().unwrap()),
                        "{context}"
                    );
                }
            }
            "SubagentStart" => {
                let context = printed.unwrap();
                assert!(context.contains("Native task name: sonic"), "{context}");
                assert!(context.contains(&profile_sha), "{context}");
                assert!(context.contains(&lens), "{context}");
                assert!(context.contains(&owner), "{context}");
            }
            "SubagentStop" => {
                assert_eq!(printed, None);
                let children = fixture.children();
                assert_eq!(children.len(), 1, "{children:?}");
                assert_eq!(children[0]["nativeChild"], CHILD);
                assert_eq!(children[0]["resultText"], RESULT);
                assert_eq!(children[0]["origin"]["session"], SESSION);
            }
            // Compaction stdout is not a context channel (Codex contract,
            // tests/session_goal_direct/native_context_recovery.rs).
            "PreCompact" | "PostCompact" => assert_eq!(printed, None),
            "PostToolUse" => {
                let line = printed.unwrap();
                assert!(
                    line.contains(&format!("--root {} ", fixture.product.display())),
                    "{line}"
                );
                assert!(line.contains(&owner), "{line}");
                assert!(fixture
                    .product
                    .join(".exitbind/plan-cards/f5a-owner/TASK.md")
                    .is_file());
            }
            other => panic!("hook event {other} has no row in the F5a table"),
        }
    }
    assert_eq!(
        files_under(&fixture.decoy),
        decoy_before,
        "the cwd project is never used"
    );
}

/// The same table with no native Goal: the cwd project is used and named.
#[test]
fn without_a_native_goal_every_hook_event_uses_and_names_the_cwd_project() {
    let fixture = Fixture::new();
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/host/runtime.rs"))
            .unwrap();
    let cwd = fixture.fact(CWD_FROM);
    let profile_sha = format!("Profile SHA-256: {}", sha(PROFILE_BODY.as_bytes()));
    for event in &dispatcher_events(&source) {
        let printed = fixture.hook(&fixture.product, None, payload(event));
        match event.as_str() {
            "SessionStart" => {
                let context = printed.unwrap();
                fixture.session_names(&context, &fixture.config, CWD_FROM);
                assert!(!context.contains(OWNER_FROM), "{context}");
                // Outside the product root, the cwd project used is named too.
                let decoy = fixture.hook(&fixture.decoy, None, payload(event)).unwrap();
                fixture.session_names(&decoy, &fixture.decoy_config(), CWD_FROM);
            }
            "SubagentStart" => {
                let context = printed.unwrap();
                assert!(context.contains(&profile_sha), "{context}");
                assert!(context.contains(&cwd), "{context}");
            }
            "SubagentStop" => {
                assert_eq!(printed, None);
                assert!(
                    fixture.children().is_empty(),
                    "an unprepared child is not recorded"
                );
            }
            "PreCompact" | "PostCompact" => assert_eq!(printed, None),
            "PostToolUse" => {
                let line = printed.unwrap();
                assert!(line.contains(&cwd), "{line}");
            }
            other => panic!("hook event {other} has no row in the F5a table"),
        }
    }
}

impl Fixture {
    /// A sealed, valid recovery hint for the Lead session whose owner fields
    /// are the true Goal owner's but whose location names the decoy (cwd)
    /// configuration: a candidate offered by path alone.
    fn forge_foreign_hint(&self) {
        let key = sha(
            serde_json::to_string(&json!({"host":"claude","identity":SESSION}))
                .unwrap()
                .as_bytes(),
        );
        let root = self.store.join(format!("session-{key}"));
        let owner: Value =
            serde_json::from_slice(&fs::read(root.join("owner.json")).unwrap()).unwrap();
        let decoy = self.decoy_config();
        let mut hint = json!({
            "kind":"exitbind_session_recovery_hint","protocol":2,"host":"claude",
            "sessionKey":key,"ownerConfigPath":decoy,
            "ownerConfigSha256":sha(&fs::read(&decoy).unwrap()),
            "ownerProjectId":owner["ownerProjectId"],
            "ownerIdentitySha256":owner["ownerIdentitySha256"],
            "goalId":"f5a-goal","goalRevision":1,"goalEventSha256":sha(b"any event"),
            "currentStep":"Inspect the Goal","currentWork":{"state":"not_selected","work":null},
            "latestCorrections":[],"evidenceRefs":[],"workRefs":[],"pendingOperation":null,
            "guidanceRefs":[],"lastCheckpoint":null,"sha256":"",
        });
        // serde_json objects are key-sorted: this is the product's canonical form.
        hint["sha256"] = json!(sha(serde_json::to_string(&hint).unwrap().as_bytes()));
        let path = root.join("recovery-hint.json");
        fs::write(&path, serde_json::to_vec(&hint).unwrap()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        // The hint is readable: `session recovery show` returns it.
        let shown = exitbind(
            &self.product,
            &self.store,
            Some(SESSION),
            &["session", "recovery", "show", "--json"],
            b"",
        );
        let shown: Value = serde_json::from_slice(&shown.stdout).unwrap_or_default();
        assert_eq!(
            shown["savedHint"]["ownerConfigPath"],
            json!(decoy),
            "the forged hint must be a valid candidate: {shown}"
        );
    }
}

/// A hint naming another project's configuration under the owner's identity
/// fields is a path, not an identity: the decoy is never adopted, the owner
/// project (from its journey member) is used, and the child is finalized
/// there. Dropping the identity check makes the decoy the first candidate.
#[test]
fn a_configuration_offered_by_path_alone_is_never_adopted() {
    let fixture = Fixture::new();
    fixture.forge_foreign_hint();
    let decoy_before = files_under(&fixture.decoy);
    let decoy = fixture.decoy_config();
    let session = fixture
        .hook(&fixture.decoy, Some(SESSION), payload("SessionStart"))
        .unwrap();
    fixture.session_names(&session, &fixture.config, OWNER_FROM);
    assert!(!session.contains(&format!("--config {decoy}")), "{session}");
    let child = fixture
        .hook(&fixture.decoy, Some(SESSION), payload("SubagentStart"))
        .unwrap();
    assert!(child.contains(&fixture.fact(OWNER_FROM)), "{child}");
    assert!(!child.contains(&decoy), "{child}");
    assert_eq!(
        fixture.hook(&fixture.decoy, Some(SESSION), payload("SubagentStop")),
        None
    );
    assert_eq!(fixture.children()[0]["nativeChild"], CHILD);
    assert_eq!(
        files_under(&fixture.decoy),
        decoy_before,
        "nothing reaches the decoy"
    );
}

/// The likely live hook process: no session selector in the environment,
/// the Claude payload `session_id` owns the Goal.
#[test]
fn a_payload_only_claude_session_resolves_its_goal_owner() {
    let fixture = Fixture::new();
    let decoy_before = files_under(&fixture.decoy);
    let session = fixture
        .hook_as(&fixture.decoy, None, SESSION, payload("SessionStart"))
        .unwrap();
    fixture.session_names(&session, &fixture.config, OWNER_FROM);
    let child = fixture
        .hook_as(&fixture.decoy, None, SESSION, payload("SubagentStart"))
        .unwrap();
    assert!(
        child.contains(&format!(
            "Profile SHA-256: {}",
            sha(PROFILE_BODY.as_bytes())
        )),
        "{child}"
    );
    assert!(
        child.contains(&format!(
            "Perspective agent-surface-text (source SHA-256 {}",
            sha(LENS_BODY.as_bytes())
        )),
        "{child}"
    );
    assert!(child.contains(&fixture.fact(OWNER_FROM)), "{child}");
    assert_eq!(
        fixture.hook_as(&fixture.decoy, None, SESSION, payload("SubagentStop")),
        None
    );
    let children = fixture.children();
    assert_eq!(children.len(), 1, "{children:?}");
    assert_eq!(children[0]["nativeChild"], CHILD);
    assert_eq!(files_under(&fixture.decoy), decoy_before);
}

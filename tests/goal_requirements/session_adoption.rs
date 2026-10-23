//! R55 F14 (FR55-11): a new native session of the same project adopts that
//! project's open Goal journal by one explicit, emitted act, never by a
//! guess. Session A incorporates the Goal and begins a Work; session B reads
//! UNAVAILABLE with the route, runs it unedited, and continues A's Work to
//! the Goal's close; session C adopts after B. A foreign project, a closed
//! Goal and a second open journal get no route, and the act refuses them
//! with nothing written. Every renderer of the native UNAVAILABLE reasons is
//! read from the source and must go through the one route builder.
use super::*;

const SESSION_A: &str = "f14-session-a";
const SESSION_B: &str = "f14-session-b";
const SESSION_C: &str = "f14-session-c";
const LINE: &str = "Adopt Goal import, open in an earlier session of this project: ";

fn argv(route: &Value) -> Vec<String> {
    route["command"]
        .as_array()
        .unwrap_or_else(|| panic!("an adoption route: {route}"))
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// The printed one-line form of an argv, as the route builder quotes it.
fn shell_line(argv: &[String]) -> String {
    argv.iter()
        .map(|word| {
            if !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=<>@%+,".contains(&byte))
            {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// SessionStart hook context for the current session, launched in the
/// project root.
pub(super) fn session_start(p: &Project) -> String {
    let (identity, store) = p.native_session.as_ref().unwrap();
    let payload = json!({"hook_event_name":"SessionStart","source":"startup",
        "session_id":identity,"cwd":p.root});
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&p.root)
        .arg("hook-run")
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("CLAUDE_ENV_FILE")
        .env_remove("XDG_STATE_HOME")
        .env("CODEX_THREAD_ID", identity)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_new_session_adopts_the_projects_open_goal_by_the_emitted_route_and_finishes_it() {
    let mut p = Project::new("f14-adoption").with_native_session(SESSION_A);
    p.setup();
    let work = p.begin("R1,R2,R3", "sh check.sh all");
    let incorporated = p.status();
    let revision = incorporated["revision"].as_u64().unwrap();

    // Session B: every UNAVAILABLE surface carries the one full route.
    p.set_native_session(SESSION_B);
    let unavailable = p.status();
    assert_eq!(unavailable["contextOnly"], true, "{unavailable}");
    let route = unavailable["goalProgress"]["adoptRoute"].clone();
    let command = argv(&route);
    assert_eq!(route["goalId"], "import");
    assert_eq!(route["sameConfigRequired"], false);
    assert!(command.contains(&"adopt".to_owned()), "{command:?}");
    let line = format!("{LINE}{}", shell_line(&command));
    let system = unavailable["goalProgress"]["systemText"].as_str().unwrap();
    assert!(system.starts_with("Goal: UNAVAILABLE"), "{system}");
    assert!(system.ends_with(&line), "{system}");
    let next = p.value(&["work", "next", &work, "--json"], None);
    let progress = &next["presentation"]["goalProgress"];
    assert_eq!(
        progress["decomposition"]["reason"], "native_session_goal_context_invalid",
        "{next}"
    );
    assert_eq!(progress["adoptRoute"], route, "{next}");
    let full = p.value(&["work", "next", &work, "--json", "--full"], None);
    let full = &full["presentation"]["goalProgress"];
    assert_eq!(full["adoptRoute"], route, "{full}");
    assert!(
        full["systemText"].as_str().unwrap().contains(&line),
        "{full}"
    );
    let resume = p.call(&["work", "resume"], None);
    assert!(resume.status.success(), "{resume:?}");
    let key = &command[command
        .iter()
        .position(|word| word == "--session-key")
        .unwrap()
        + 1];
    assert!(
        String::from_utf8_lossy(&resume.stdout).contains(key.as_str()),
        "{resume:?}"
    );
    let hook = session_start(&p);
    assert!(hook.contains(&line), "{hook}");

    // A different project running the same route is refused byte-unchanged.
    let foreign = Project::new("f14-foreign");
    let mut other = command.clone();
    let config = other.iter().position(|word| word == "--config").unwrap() + 1;
    other[config] = foreign
        .root
        .join("exitbind.json")
        .to_str()
        .unwrap()
        .to_owned();
    let (store_before, foreign_before) = (p.native_store_snapshot(), foreign.run_entries());
    let refused = p.call_exact_argv(&other);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        stderr(&refused).contains("belongs to another project")
            && stderr(&refused).contains("nothing was written"),
        "{refused:?}"
    );
    assert_eq!(p.native_store_snapshot(), store_before);
    assert_eq!(foreign.run_entries(), foreign_before);

    // B runs the route unedited.
    let adopted = p.call_exact_argv(&command);
    assert!(adopted.status.success(), "{adopted:?}");
    let adopted: Value = serde_json::from_slice(&adopted.stdout).unwrap();
    assert_eq!(adopted["effect"], "recorded", "{adopted}");
    let status = p.status();
    assert_eq!(status["goalId"], "import", "{status}");
    assert_eq!(status["revision"], revision + 1, "{status}");
    assert!(status["goalProgress"].get("adoptRoute").is_none());
    let next = p.value(&["work", "next", &work, "--json"], None);
    assert_ne!(
        next["presentation"]["goalProgress"]["overall"], "unavailable",
        "{next}"
    );
    assert!(!session_start(&p).contains(LINE));

    // The old session still resolves the one journal (nothing revoked).
    p.set_native_session(SESSION_A);
    assert_eq!(p.status()["revision"], revision + 1);

    // Session C adopts after B; its hook then reads the ordinary resume text.
    p.set_native_session(SESSION_C);
    let hook = session_start(&p);
    assert!(hook.contains(LINE), "{hook}");
    let route = p.status()["goalProgress"]["adoptRoute"].clone();
    let adopted = p.call_exact_argv(&argv(&route));
    assert!(adopted.status.success(), "{adopted:?}");
    let hook = session_start(&p);
    assert!(!hook.contains(LINE), "{hook}");
    assert!(!hook.contains("owner record is absent"), "{hook}");
    assert!(hook.contains("import"), "{hook}");
    assert_eq!(p.status()["revision"], revision + 2);

    // B continues A's Work and finishes the Goal: assign and close.
    p.set_native_session(SESSION_B);
    p.drive(&work, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &work,
        ],
        None,
    );
    let close = p.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        None,
    );
    assert!(close.status.success(), "{close:?}");
    assert_eq!(p.status()["closure"]["closed"], true);

    // A closed Goal produces no route, and the act refuses it.
    p.set_native_session("f14-session-d");
    let closed = p.status();
    assert!(
        closed["goalProgress"].get("adoptRoute").is_none(),
        "{closed}"
    );
    let before = p.native_store_snapshot();
    let refused = p.call(
        &[
            "goal",
            "adopt",
            "--session-key",
            &command[command.iter().position(|w| w == "--session-key").unwrap() + 1],
        ],
        None,
    );
    assert!(
        stderr(&refused).contains("Goal import is closed"),
        "{refused:?}"
    );
    assert_eq!(p.native_store_snapshot(), before);
}

#[test]
fn two_open_journals_of_one_project_produce_no_route_and_the_act_refuses() {
    let mut p = Project::new("f14-two-open").with_native_session(SESSION_A);
    p.setup();
    p.begin("R1,R2,R3", "sh check.sh all");
    p.set_native_session(SESSION_C);
    p.setup();
    p.begin("R1,R2,R3", "sh check.sh all");
    let key = {
        p.set_native_session(SESSION_A);
        p.status()["journeyMembers"][0]["sessionKey"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    p.set_native_session(SESSION_B);
    let status = p.status();
    assert!(
        status["goalProgress"].get("adoptRoute").is_none(),
        "{status}"
    );
    assert!(!status["goalProgress"]["systemText"]
        .as_str()
        .unwrap()
        .contains("Adopt Goal"));
    assert!(!session_start(&p).contains("Adopt Goal"));
    let before = p.native_store_snapshot();
    let refused = p.call(&["goal", "adopt", "--session-key", &key], None);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(
        stderr(&refused).contains("reaches 2 open Goal journals"),
        "{refused:?}"
    );
    assert_eq!(p.native_store_snapshot(), before);
}

/// Every renderer of the native UNAVAILABLE reasons, read from `src/`: the
/// reason codes are spelled once, in the route builder, and every reason
/// value is passed to that builder, which attaches the route.
#[test]
fn every_native_unavailable_renderer_goes_through_the_adoption_route_builder() {
    const BUILDER: &str = "src/session_goal/adoption.rs";
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    let mut renderers = Vec::new();
    for path in &files {
        let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
        let source = fs::read_to_string(path).unwrap();
        for code in [
            "\"native_session_goal_unavailable\"",
            "\"native_session_goal_context_invalid\"",
            "native Goal owner record is absent",
        ] {
            assert!(
                relative == BUILDER || !source.contains(code),
                "{relative} renders {code} outside the adoption route builder"
            );
        }
        if relative == BUILDER {
            continue;
        }
        let calls = source.matches("adoption::unavailable(").count();
        let reasons = source.matches("Reason::Unavailable").count()
            + source.matches("Reason::ContextInvalid").count();
        assert_eq!(
            calls, reasons,
            "{relative}: every native reason is passed to adoption::unavailable"
        );
        for _ in 0..calls {
            renderers.push(relative.to_owned());
        }
        if source.contains("adoption::hook_text(") {
            renderers.push(relative.to_owned());
        }
    }
    renderers.sort();
    assert_eq!(
        renderers,
        [
            "src/host/session_recovery/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
        ],
        "a new renderer joins this inventory"
    );
    let builder = fs::read_to_string(root.join(BUILDER)).unwrap();
    let body = builder
        .split("pub(crate) fn unavailable(")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(body.contains("route(loaded"), "{body}");
    assert!(body.contains("\"adoptRoute\""), "{body}");
    let hook = builder.split("pub(crate) fn hook_text(").nth(1).unwrap();
    assert!(hook.split("\n}\n").next().unwrap().contains("route.line"));
}

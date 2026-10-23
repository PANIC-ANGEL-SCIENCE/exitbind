//! R55 F14 (FR55-11): a new native session of the same project adopts that
//! project's open Goal journal by one explicit, emitted act, never by a
//! guess. Session A incorporates the Goal and begins a Work; session B reads
//! UNAVAILABLE with the route, runs it unedited, and continues A's Work to
//! the Goal's close; session C adopts after B. A foreign project, a closed
//! Goal and a second open journal get no route, and the act refuses them
//! with nothing written. Every renderer of the native UNAVAILABLE reasons is
//! read from the source and must go through the one route builder.
use super::rust_source::{blank, code_words, literals, production};
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

/// Calls of `adoption::NAME` in code, in any spacing (`adoption :: NAME (`).
fn adoption_calls(source: &str, name: &str) -> usize {
    let code = blank(source);
    code_words(source, name)
        .into_iter()
        .filter(|at| {
            let before: String = code[..*at].chars().filter(|c| !c.is_whitespace()).collect();
            before.ends_with("adoption::") && code[at + name.len()..].trim_start().starts_with('(')
        })
        .count()
}

/// Every renderer of the native UNAVAILABLE reasons, read from `src/`: the
/// reason codes are spelled once, in the route builder, and every reason
/// value is passed to that builder, which attaches the route.
#[test]
fn every_native_unavailable_renderer_goes_through_the_adoption_route_builder() {
    const BUILDER: &str = "src/session_goal/adoption.rs";
    let sources = production();
    assert!(sources.iter().any(|(path, _)| path == BUILDER));
    let mut renderers = Vec::new();
    for (relative, source) in &sources {
        let relative = relative.as_str();
        for code in [
            "native_session_goal_unavailable",
            "native_session_goal_context_invalid",
            "native Goal owner record is absent",
        ] {
            assert!(
                relative == BUILDER || !literals(source).iter().any(|lit| lit.text.contains(code)),
                "{relative} renders {code} outside the adoption route builder"
            );
        }
        if relative == BUILDER {
            continue;
        }
        let calls = adoption_calls(source, "unavailable");
        let reasons = code_words(source, "Unavailable")
            .into_iter()
            .chain(code_words(source, "ContextInvalid"))
            .filter(|at| blank(source)[..*at].trim_end().ends_with("Reason::"))
            .count();
        assert_eq!(
            calls, reasons,
            "{relative}: every native reason is passed to adoption::unavailable"
        );
        for _ in 0..calls {
            renderers.push(relative.to_owned());
        }
        if adoption_calls(source, "hook_text") > 0 {
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
    let builder = &sources.iter().find(|(path, _)| path == BUILDER).unwrap().1;
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

/// The owner record a session that adopted a journal keeps in its own root.
fn adopted_owner_records(p: &Project) -> Vec<PathBuf> {
    let (_, store) = p.native_session.as_ref().unwrap();
    let mut found = Vec::new();
    for entry in fs::read_dir(store).unwrap() {
        let owner = entry.unwrap().path().join("owner.json");
        if fs::read_to_string(&owner).is_ok_and(|text| text.contains("journalSessionKey")) {
            found.push(owner);
        }
    }
    found
}

fn adopt(p: &Project, command: &[String]) -> Value {
    let output = p.call_exact_argv(command);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// F14 review D4/D5: a crash between the journal append and B's owner record
/// is completed by running the same route, which writes only the owner
/// record and says so; adopting again writes nothing.
#[test]
fn a_rerun_after_a_partial_adoption_writes_only_the_owner_record_and_says_so() {
    let mut p = Project::new("f14-partial").with_native_session(SESSION_A);
    p.setup();
    p.begin("R1,R2,R3", "sh check.sh all");
    p.set_native_session(SESSION_B);
    let command = argv(&p.status()["goalProgress"]["adoptRoute"]);
    let adopted = adopt(&p, &command);
    assert_eq!(adopted["effect"], "recorded", "{adopted}");
    assert_eq!(
        adopted["written"],
        json!(["goalJournal", "sessionOwnerRecord"])
    );
    let revision = p.status()["revision"].clone();

    // The crash: the journal lists B, B's owner record was never written.
    let owners = adopted_owner_records(&p);
    assert_eq!(owners.len(), 1, "{owners:?}");
    fs::remove_file(&owners[0]).unwrap();
    assert!(p.status()["goalProgress"].get("adoptRoute").is_some());
    let rerun = adopt(&p, &command);
    assert_eq!(rerun["effect"], "recorded", "{rerun}");
    assert_eq!(rerun["written"], json!(["sessionOwnerRecord"]), "{rerun}");
    assert_eq!(p.status()["revision"], revision, "the journal is unchanged");
    assert_eq!(adopted_owner_records(&p).len(), 1);

    // Adopting again writes nothing.
    let before = p.native_store_snapshot();
    let again = adopt(&p, &command);
    assert_eq!(again["effect"], "unchanged", "{again}");
    assert_eq!(again["written"], json!([]), "{again}");
    assert_eq!(p.native_store_snapshot(), before);
}

/// F14 review D5: a sidecar naming the journal counts only in the project
/// whose state root holds it and whose owner the journal names. Moved to
/// another project, it makes no route there (another owner) and no longer
/// one here (no Work of this project names the key); both acts refuse with
/// nothing written.
#[test]
fn a_sidecar_moved_to_another_project_adopts_in_neither() {
    let mut p = Project::new("f14-sidecar").with_native_session(SESSION_A);
    p.setup();
    p.begin("R1,R2,R3", "sh check.sh all");
    p.set_native_session(SESSION_B);
    let command = argv(&p.status()["goalProgress"]["adoptRoute"]);
    let mut other = Project::new("f14-sidecar-other");
    other.native_session = p.native_session.clone();
    let runs = p.root.join(".exitbind/runs");
    let other_runs = other.root.join(".exitbind/runs");
    fs::create_dir_all(&other_runs).unwrap();
    let mut moved = 0;
    for entry in fs::read_dir(runs).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if name.ends_with(".session-goal.json") {
            fs::rename(&path, other_runs.join(&name)).unwrap();
            moved += 1;
        }
    }
    assert!(moved > 0, "the Work names its session in a sidecar");

    let before = p.native_store_snapshot();
    let here = p.call_exact_argv(&command);
    assert!(!here.status.success(), "{here:?}");
    assert!(
        stderr(&here).contains("no Work of this project names that session key")
            && stderr(&here).contains("nothing was written"),
        "{here:?}"
    );
    let mut elsewhere = command.clone();
    let config = elsewhere
        .iter()
        .position(|word| word == "--config")
        .unwrap()
        + 1;
    elsewhere[config] = other
        .root
        .join("exitbind.json")
        .to_str()
        .unwrap()
        .to_owned();
    let there = other.call_exact_argv(&elsewhere);
    assert!(!there.status.success(), "{there:?}");
    assert!(
        stderr(&there).contains("belongs to another project"),
        "{there:?}"
    );
    assert_eq!(p.native_store_snapshot(), before);
}

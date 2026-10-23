//! R55 F13 (FR55-12): a pause is a product act whose record a new session
//! reads. The fixture replays the R55 `RESUME.md`: session A records the
//! step and two in-flight Lead decisions through the emitted form while a
//! check observation runs; session B adopts the Goal and reads, from the
//! hook text, `session recovery show --json` and `work next` together, the
//! in-flight Work and its next action, the open assignment, the running
//! observation, the saved step and both decisions, and the unmapped
//! requirements with their recorded sources. A second `work check` from B is
//! refused naming the running observation. Two sessions sharing the journal
//! share one record: the last writer wins and is named.
use super::session_adoption::session_start;
use super::*;

const SESSION_A: &str = "f13-session-a";
const SESSION_B: &str = "f13-session-b";
const STEP: &str = "Accept F7a via work fill after the meow review";
const DECISIONS: [&str; 2] = [
    "F7a form: environmental is a work disposition decision; no new event",
    "An attested environmental failure still counts toward the repeated-failure limit",
];

fn argv(route: &Value) -> Vec<String> {
    route["command"]
        .as_array()
        .unwrap_or_else(|| panic!("a record route: {route}"))
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// The printed form with its one registered placeholder filled, nothing else
/// edited.
fn filled(route: &Value, slot: &str, value: &str) -> Vec<String> {
    let mut words = argv(route);
    let option = route["fill"][slot].as_str().unwrap();
    let at = words.iter().position(|word| word == option).unwrap();
    assert_eq!(words[at + 1], format!("<{slot}>"), "{route}");
    words[at + 1] = value.to_owned();
    words
}

fn ok_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Drive the Work to its Lead check: scope, worker permit and completion.
fn to_check(p: &Project, work: &str) {
    for _ in 0..6 {
        let next = p.value(&["work", "next", work, "--json", "--full"], None)["next"].clone();
        match next["action"].as_str().unwrap() {
            "lead_decision" => p.value(
                &[
                    "work",
                    "return",
                    work,
                    next["assignment"].as_str().unwrap(),
                    "--outcome",
                    "scoped",
                ],
                Some("Fixture scope.\n"),
            ),
            "spawn" => {
                let assignment = next["assignment"].as_str().unwrap();
                p.value(
                    &[
                        "work",
                        "permit",
                        work,
                        assignment,
                        "--operation",
                        "Return fixture evidence",
                    ],
                    None,
                );
                p.value(
                    &["work", "return", work, assignment, "--outcome", "completed"],
                    Some("Fixture result.\n"),
                )
            }
            "check" => return,
            action => panic!("unexpected {action}: {next}"),
        };
    }
    panic!("the Work did not reach its check");
}

/// `work check` in session A, left running in the background.
fn spawn_check(p: &Project, work: &str) -> std::process::Child {
    let (identity, store) = p.native_session.as_ref().unwrap();
    let mut command = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&p.root)
        .args(["work", "check", work, "--config"])
        .arg(p.root.join("exitbind.json"))
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("CLAUDE_ENV_FILE")
        .env_remove("XDG_STATE_HOME")
        .env("CODEX_THREAD_ID", identity)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn().unwrap()
}

fn ledger(p: &Project, work: &str) -> PathBuf {
    p.root.join(format!(
        ".exitbind/runs/work-{}.jsonl",
        work.strip_prefix("smw_").unwrap()
    ))
}

#[test]
fn a_new_session_reads_the_pause_record_and_the_running_check_after_adoption() {
    let mut p = Project::new("f13-pause").with_native_session(SESSION_A);
    p.setup();
    let release = p.root.with_file_name(format!(
        "{}-release",
        p.root.file_name().unwrap().to_string_lossy()
    ));
    let _ = fs::remove_file(&release);
    p.file(
        "check.sh",
        &format!(
            "set -eu\ni=0\nwhile [ ! -e '{}' ]; do i=$((i+1)); [ $i -lt 1200 ] || exit 9; sleep 0.05; done\n",
            release.display()
        ),
    );
    let work = p.begin("R1", "sh check.sh R1");
    to_check(&p, &work);

    // The record form on the three Lead views D19 names is one route.
    let status = p.status();
    let routes = status["pauseRecord"].clone();
    assert_eq!(routes["actor"], "lead", "{status}");
    assert_eq!(routes["saveStep"]["fill"], json!({"SAVED_STEP": "--step"}));
    assert_eq!(
        routes["saveDecision"]["fill"],
        json!({"LEAD_DECISION": "--decision"})
    );
    let resume = p.value(&["work", "resume"], None);
    assert_eq!(resume["pauseRecord"], routes, "{resume}");
    let text = p.call(&["goal", "status"], None);
    let text = String::from_utf8_lossy(&text.stdout).into_owned();
    for line in routes["text"].as_array().unwrap() {
        assert!(text.contains(line.as_str().unwrap()), "{text}");
    }

    // Session A pauses: the step and two decisions through the form, unedited.
    let saved = ok_json(&p.call_exact_argv(&filled(&routes["saveStep"], "SAVED_STEP", STEP)));
    assert_eq!(saved["savedStep"], STEP, "{saved}");
    for decision in DECISIONS {
        let route = &routes["saveDecision"];
        ok_json(&p.call_exact_argv(&filled(route, "LEAD_DECISION", decision)));
    }
    let shown = p.value(&["session", "recovery", "show", "--json"], None);
    assert_eq!(shown["pauseRecord"], routes, "{shown}");

    // A's Lead check runs and stays running.
    let mut check = spawn_check(&p, &work);
    let ledger = ledger(&p, &work);
    let mut admitted = false;
    for _ in 0..600 {
        if fs::read_to_string(&ledger)
            .unwrap()
            .contains("\"check_observation\"")
        {
            admitted = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(admitted, "the check observation was never admitted");

    // Session B adopts the Goal and reads the three surfaces.
    p.set_native_session(SESSION_B);
    let adopt = p.status()["goalProgress"]["adoptRoute"].clone();
    ok_json(&p.call_exact_argv(&argv(&adopt)));
    let hook = session_start(&p);
    let shown = p.value(&["session", "recovery", "show", "--json"], None);
    let next = p.value(&["work", "next", &work, "--json"], None);
    let context = format!("HOOK:\n{hook}\nSHOW:\n{shown:#}\nNEXT:\n{next:#}");

    // The in-flight Work, its next action and open assignment.
    let current = &shown["currentWorks"]["works"][0];
    assert_eq!(current["work"], work, "{context}");
    assert_eq!(current["nextAction"], "lead_decision", "{context}");
    let assignment = current["openAssignments"][0]["assignment"]
        .as_str()
        .unwrap();
    assert_eq!(current["openAssignments"][0]["role"], "lead", "{context}");
    assert!(
        hook.contains(&format!("current Work {work}"))
            && hook.contains("next action: lead_decision")
            && hook.contains(&format!("Open assignments: {assignment} (lead")),
        "{context}"
    );
    assert_eq!(next["current"]["recipient"]["assignment"], assignment);
    // The running observation, on all three.
    assert_eq!(current["checkObservation"]["state"], "running", "{context}");
    let event = current["checkObservation"]["admissionEventSha256"]
        .as_str()
        .unwrap();
    assert!(
        hook.contains(&format!("Check observation {}", &event[..8])) && hook.contains(": running."),
        "{context}"
    );
    assert_eq!(
        next["current"]["actionForm"]["state"], "unresolved_check_observation",
        "{context}"
    );
    // The saved step and both decisions, with the revision they were saved at.
    let active = &shown["activeSavedStep"];
    assert_eq!(active["step"], STEP, "{context}");
    assert_eq!(active["leadDecisions"], json!(DECISIONS), "{context}");
    assert_eq!(active["goalCurrent"], false, "adoption advanced the Goal");
    // The saved-state line agrees with the active step: saved, not current.
    let saved = shown["savedState"].as_str().unwrap();
    assert!(
        saved.starts_with("historical: the Goal moved past the revision the step was saved at"),
        "{saved}"
    );
    // The unmapped requirements with their recorded sources.
    let unmapped = shown["unmappedRequirements"].as_array().unwrap();
    let ids: Vec<&str> = unmapped
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["R2", "R3"], "{context}");
    let source = unmapped[0]["source"].as_str().unwrap();
    assert!(source.contains("request.txt"), "{context}");
    assert!(
        hook.contains(&format!(
            "Requirements without a Work: R2 (source {source}), R3 (source {source})"
        )),
        "{context}"
    );

    // A second check from B is refused naming the running observation; the
    // route it names runs unedited and nothing was written.
    let before = fs::read(&ledger).unwrap();
    let refused = p.call(&["work", "check", &work], None);
    assert!(!refused.status.success(), "{refused:?}");
    let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        refusal.contains(&format!("is running on Work {work}"))
            && refusal.contains("no check was launched"),
        "{refusal}"
    );
    let route: Vec<String> = refusal
        .split("Read it with ")
        .nth(1)
        .unwrap_or_else(|| panic!("no read route: {refusal}"))
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(route[0], env!("CARGO_BIN_EXE_exitbind"), "{refusal}");
    assert!(p.call_exact_argv(&route).status.success(), "{route:?}");
    assert_eq!(fs::read(&ledger).unwrap(), before);

    // The check ends and records its result.
    fs::write(&release, "").unwrap();
    let ended = check.wait().unwrap();
    let _ = fs::remove_file(&release);
    assert!(ended.success(), "{ended:?}");

    // One record for two sessions: the last writer wins and is named.
    let by_b =
        ok_json(&p.call_exact_argv(&filled(&routes["saveStep"], "SAVED_STEP", "B continues")));
    p.set_native_session(SESSION_A);
    let shown_a = p.value(&["session", "recovery", "show", "--json"], None);
    assert_eq!(shown_a["activeSavedStep"]["step"], "B continues");
    assert_eq!(
        shown_a["activeSavedStep"]["leadDecisions"],
        json!(DECISIONS)
    );
    let journal = shown_a["savedHint"]["sessionKey"].as_str().unwrap();
    let writer_b = by_b["recordedBySessionKey"].as_str().unwrap();
    assert_ne!(writer_b, journal);
    assert_eq!(shown_a["activeSavedStep"]["recordedBySessionKey"], writer_b);
    let by_a =
        ok_json(&p.call_exact_argv(&filled(&routes["saveStep"], "SAVED_STEP", "A continues")));
    assert_eq!(by_a["recordedBySessionKey"], journal);
}

/// A decision before any saved step is refused with the step route and
/// writes nothing; outside an owned Goal no view prints the form.
#[test]
fn a_decision_without_a_saved_step_is_refused_and_no_goal_prints_no_form() {
    let p = Project::new("f13-no-step").with_native_session(SESSION_A);
    assert!(p.status().get("pauseRecord").is_none());
    p.setup();
    let routes = p.status()["pauseRecord"].clone();
    let before = p.native_store_snapshot();
    let refused = p.call_exact_argv(&filled(&routes["saveDecision"], "LEAD_DECISION", "x"));
    assert!(!refused.status.success(), "{refused:?}");
    let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        refusal.contains("no step is saved for this Goal yet; nothing was written")
            && refusal.contains("--step"),
        "{refusal}"
    );
    assert_eq!(p.native_store_snapshot(), before);
}

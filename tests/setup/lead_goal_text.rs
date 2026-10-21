//! R54 Step 8: the Lead's Goal text names the journey (member count, how it
//! closes, the read route for members not yet current) and the project-lesson
//! state, in the `setup --apply` response, `goal status` and the SessionStart
//! hook, for a project with carried lessons and one without.
use super::lesson_carry::{accept, previous_project, repository};
use super::support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Output, Stdio},
};

const CARD: &str = "# R9 task\n\n| Step | Do | Done when |\n| --- | --- | --- |\n| 1 | Add the parser | The parser rejects empty input. |\n| 2 | Add the report | The report shows the count. |\n";
/// The journey read route, printed with the executable under test.
fn read_route() -> String {
    format!(
        "{} goal status --json (goalProgress.journeyCompletion.unresolved)",
        env!("CARGO_BIN_EXE_exitbind")
    )
}
const CLOSES: &str =
    "closes when each member is accepted and current, or by one final --journey-integration Work";

/// The binary under a synthetic Claude Code session with a private Goal store.
fn session_call(root: &Path, session: &str, args: &[&str], input: &[u8]) -> Output {
    let store = root.with_extension("session-goals");
    let mut command = support::git_topology::command(args[0]);
    command
        .current_dir(root)
        .args(&args[1..])
        .env("CLAUDE_CODE_SESSION_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", &store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "EXITBIND_NATIVE_SESSION_ID",
        "CLAUDE_ENV_FILE",
    ] {
        command.env_remove(name);
    }
    let mut child = command.spawn().unwrap();
    // A command may refuse and exit before reading stdin; its output is still asserted.
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

fn json_of(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_facts(text: &str, journey: &str, lessons: &str) {
    assert!(text.contains(journey), "journey fact missing: {text}");
    assert!(text.contains(lessons), "lesson fact missing: {text}");
}

fn lead_reads_both_facts(label: &str, previous: Option<&Path>, lessons: &str) {
    let fresh = repository(label);
    let root = fresh.0.as_path();
    let session = format!("r54-goal-text-{label}");
    fs::create_dir_all(root.join("work/r9")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("work/r9/00_TASK.md"), CARD).unwrap();
    fs::write(
        root.join("scripts/check.sh"),
        "test -s work/r9/00_TASK.md\n",
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_exitbind");
    let from = previous.map(|path| path.join("exitbind.json"));
    let mut argv = vec![
        exe,
        "setup",
        "--json",
        "--mode",
        "portable",
        "--root",
        root.to_str().unwrap(),
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
    ];
    if let Some(from) = from.as_ref() {
        argv.extend(["--lessons-from", from.to_str().unwrap()]);
    }
    let report = json_of(session_call(root, &session, &argv, b""));
    let setup_text = report["goalText"].as_str().unwrap();
    assert_facts(
        setup_text,
        &format!("Journey: 0 members; each Work begun in this conversation joins it; {CLOSES}"),
        lessons,
    );

    for command in report["next"]["argv"].as_array().unwrap() {
        let command = command
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect::<Vec<_>>();
        json_of(session_call(root, &session, &command, b""));
    }
    let config = fs::canonicalize(root.join("exitbind.json")).unwrap();
    let status = json_of(session_call(
        root,
        &session,
        &[
            exe,
            "goal",
            "status",
            "--json",
            "--config",
            config.to_str().unwrap(),
        ],
        b"",
    ));
    let text = status["goalProgress"]["systemText"].as_str().unwrap();
    assert!(text.starts_with("Goal: IN PROGRESS"), "{text}");
    assert_facts(text, "Journey: 1 member, 0 current; ", lessons);
    assert!(
        text.contains(CLOSES) && text.contains(&read_route()),
        "{text}"
    );
    assert!(text.chars().count() <= 512, "{text}");

    let payload = json!({"hook_event_name":"SessionStart","source":"resume",
        "session_id":session,"cwd":root});
    let hook = json_of(session_call(
        root,
        &session,
        &[exe, "hook-run"],
        payload.to_string().as_bytes(),
    ));
    let context = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert_facts(
        context,
        &format!(
            "Goal r9: Journey: 1 member; {CLOSES}; not yet current: {}",
            read_route()
        ),
        lessons,
    );
    assert!(context.contains("current Work smw_"), "{context}");
}

#[test]
fn lead_goal_text_names_the_journey_and_carried_lessons() {
    let previous = previous_project();
    accept(
        previous.0.as_path(),
        "carry-me",
        "Trace every delivery consumer.",
        "2100-01-01T00:00:00Z",
    );
    lead_reads_both_facts(
        "goal-text-lessons",
        Some(previous.0.as_path()),
        "Lessons: 1 deliverable",
    );
}

#[test]
fn lead_goal_text_states_that_no_lessons_are_configured() {
    lead_reads_both_facts("goal-text-none", None, "Lessons: none configured");
}

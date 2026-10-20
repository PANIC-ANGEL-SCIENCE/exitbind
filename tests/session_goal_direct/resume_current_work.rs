//! After a host resume with no saved recovery hint, the SessionStart hook and
//! `session recovery show` name the open Work from the native Goal's journey,
//! its open assignments, and checkout trees with uncommitted changes.

use crate::fixture::*;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Output, Stdio},
};

const SELECTORS: [&str; 5] = [
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "CLAUDE_CODE_SESSION_ID",
    "EXITBIND_NATIVE_SESSION_ID",
    "CLAUDE_ENV_FILE",
];

/// The binary under test in `cwd` with exactly the given selectors.
fn exitbind(
    cwd: &Path,
    store: &Path,
    selectors: &[(&str, &str)],
    args: &[&str],
    stdin: &[u8],
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
    for name in SELECTORS {
        command.env_remove(name);
    }
    for (name, value) in selectors {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn ok_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn config(project: &Project) -> String {
    fs::canonicalize(project.root.join("exitbind.json"))
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

fn claude(session: &str) -> Vec<(&'static str, &str)> {
    vec![("CLAUDE_CODE_SESSION_ID", session)]
}

fn in_session(project: &Project, session: &str, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    let config = config(project);
    args.extend(["--config", config.as_str()]);
    ok_json(exitbind(
        &project.root,
        &project.session_store,
        &claude(session),
        &args,
        b"",
    ))
}

fn resume_hook(project: &Project, session: &str, selectors: &[(&str, &str)]) -> String {
    let payload = json!({
        "hook_event_name":"SessionStart",
        "source":"resume",
        "session_id":session,
        "cwd":project.root,
    });
    let output = exitbind(
        &project.root,
        &project.session_store,
        selectors,
        &["hook-run"],
        &serde_json::to_vec(&payload).unwrap(),
    );
    let value = ok_json(output);
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn incorporate_and_begin(project: &Project, session: &str, goal_id: &str) -> String {
    in_session(
        project,
        session,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            goal_id,
            "--goal",
            "Carry this conversation across a host resume",
            "--obligation",
            "Name the open Work after the resume",
        ],
    );
    let started = in_session(
        project,
        session,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Plain-route Work under the session Goal",
            "--check-command",
            "true",
        ],
    );
    started["work"].as_str().unwrap().to_owned()
}

fn files_under(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
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

/// The exact `exitbind work detail ...` command the hook printed, as argv.
fn printed_detail_command(context: &str, work: &str) -> Vec<String> {
    let start = context
        .find(&format!("exitbind work detail {work}"))
        .unwrap_or_else(|| panic!("hook names the detail command: {context}"));
    let end = context[start..].find(';').unwrap() + start;
    context[start..end]
        .split(' ')
        .skip(1)
        .map(str::to_owned)
        .collect()
}

#[test]
fn resume_hook_names_the_plain_route_member_work_and_its_detail_command() {
    let project = Project::new("resume-current-work");
    let session = "claude-resume-current-work";
    let work = incorporate_and_begin(&project, session, "resume-goal");
    let config = config(&project);

    // (a) The plain-route Work is a journey member, not context-only.
    let status = in_session(&project, session, &["goal", "status", "--json"]);
    assert!(status["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|member| member["work"] == work.as_str()));
    let action = in_session(
        &project,
        session,
        &["work", "detail", &work, "--view", "action", "--json"],
    );
    assert_eq!(action["goalAssociation"]["kind"], "journey_member");
    assert_eq!(action["goalAssociation"]["goalId"], "resume-goal");

    // (b) No saved hint: the resume hook derives the Work from the Goal.
    let state_before = files_under(&project.root.join(".exitbind"));
    let goals_before = files_under(&project.session_store);
    let context = resume_hook(&project, session, &claude(session));
    assert!(
        context.contains(&format!(
            "current Work {work} (member of Goal resume-goal); read: exitbind work detail {work} --view action --config {config}; next action: "
        )),
        "{context}"
    );
    assert!(
        context.contains("no saved recovery hint selects a Work"),
        "{context}"
    );
    assert!(
        !context.contains("No current Work is selected"),
        "{context}"
    );
    assert!(context.len() <= 4096);
    assert_eq!(files_under(&project.root.join(".exitbind")), state_before);
    assert_eq!(files_under(&project.session_store), goals_before);

    // The printed command runs as printed.
    let argv = printed_detail_command(&context, &work);
    let argv = argv.iter().map(String::as_str).collect::<Vec<_>>();
    let detail = exitbind(
        &project.root,
        &project.session_store,
        &claude(session),
        &argv,
        b"",
    );
    assert!(detail.status.success(), "{detail:?}");
    assert!(String::from_utf8_lossy(&detail.stdout).contains(&work));

    // (c) `session recovery show --json` carries the same derived Work.
    let show = ok_json(exitbind(
        &std::env::temp_dir(),
        &project.session_store,
        &claude(session),
        &["session", "recovery", "show", "--json"],
        b"",
    ));
    let derived = &show["currentWorks"];
    assert_eq!(derived["goalId"], "resume-goal");
    assert_eq!(derived["works"].as_array().unwrap().len(), 1);
    assert_eq!(derived["works"][0]["work"], work.as_str());
    assert_eq!(derived["works"][0]["relation"], "journey_member");
    let show_argv = derived["works"][0]["workDetailArgv"].as_array().unwrap();
    assert_eq!(
        show_argv[1..]
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["work", "detail", &work, "--view", "action", "--config", &config]
    );
    let shown_detail = super::support::git_topology::command(show_argv[0].as_str().unwrap())
        .args(show_argv[1..].iter().map(|value| value.as_str().unwrap()))
        .current_dir(&project.root)
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("CLAUDE_CODE_SESSION_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", &project.session_store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    assert!(shown_detail.status.success(), "{shown_detail:?}");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_project_that_does_not_own_the_session_goal_still_reads_context_only() {
    let owner = Project::new("resume-owner-project");
    let other = Project::new("resume-other-project");
    let session = "claude-resume-owner-mismatch";
    let owner_work = incorporate_and_begin(&owner, session, "owned-goal");
    let shared_store = owner.session_store.as_path();
    let other_config = config(&other);
    let started = ok_json(exitbind(
        &other.root,
        shared_store,
        &claude(session),
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Related Work in a project that does not own the Goal",
            "--check-command",
            "true",
            "--config",
            &other_config,
        ],
        b"",
    ));
    let other_work = started["work"].as_str().unwrap();
    assert_eq!(started["next"]["goalProgress"]["overall"], "context_only");
    assert_eq!(
        started["next"]["goalProgress"]["workAssociation"]["kind"],
        "context_only"
    );
    let action = ok_json(exitbind(
        &other.root,
        shared_store,
        &claude(session),
        &[
            "work",
            "detail",
            other_work,
            "--view",
            "action",
            "--json",
            "--config",
            &other_config,
        ],
        b"",
    ));
    assert_eq!(action["goalAssociation"]["kind"], "context_only");
    let owner_action = in_session(
        &owner,
        session,
        &["work", "detail", &owner_work, "--view", "action", "--json"],
    );
    assert_eq!(owner_action["goalAssociation"]["kind"], "journey_member");
    fs::remove_dir_all(owner.root).unwrap();
    fs::remove_dir_all(other.root).unwrap();
}

#[test]
fn a_stale_claude_export_after_clear_yields_to_the_host_session_without_any_notice() {
    let project = Project::new("resume-stale-export");
    let session = "claude-current-after-clear";
    let work = incorporate_and_begin(&project, session, "after-clear");
    let stale = [
        ("CLAUDE_CODE_SESSION_ID", session),
        ("EXITBIND_NATIVE_SESSION_ID", "claude-cleared-before-clear"),
    ];
    let config = config(&project);
    let output = exitbind(
        &project.root,
        &project.session_store,
        &stale,
        &["goal", "status", "--json", "--config", &config],
        b"",
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{combined}");
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["goalId"], "after-clear");
    let context = resume_hook(&project, session, &stale);
    assert!(
        context.contains(&format!("current Work {work}")),
        "{context}"
    );
    for text in [&combined, &context] {
        let lower = text.to_lowercase();
        for word in [
            "disagree",
            "selector",
            "cleared",
            "stale export",
            "exitbind_native_session_id",
        ] {
            assert!(!lower.contains(word), "{word} shown to the agent: {text}");
        }
    }
    fs::remove_dir_all(project.root).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let output = super::support::git_topology::command("git")
        .current_dir(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

#[test]
fn resume_hook_names_the_open_worker_assignment_and_the_dirty_checkout_trees() {
    let project = Project::new("resume-worker-trees");
    let session = "claude-resume-worker-trees";
    let root = fs::canonicalize(&project.root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join(".git/info/exclude"), ".exitbind/\n").unwrap();
    git(&root, &["add", "-A"]);
    git(
        &root,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    let extra = root.with_file_name(format!(
        "{}-worktree",
        root.file_name().unwrap().to_str().unwrap()
    ));
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "worker-tree",
            extra.to_str().unwrap(),
        ],
    );
    let work = incorporate_and_begin(&project, session, "worker-goal");

    // The Lead scopes the Work; the worker assignment is then open.
    let next = in_session(
        &project,
        session,
        &["work", "next", &work, "--json", "--full"],
    );
    let lead = next["next"]["assignment"].as_str().unwrap().to_owned();
    let config = config(&project);
    ok_json(exitbind(
        &project.root,
        &project.session_store,
        &claude(session),
        &[
            "work",
            "return",
            &work,
            &lead,
            "--outcome",
            "scoped",
            "--config",
            &config,
        ],
        b"Deterministic fixture scope; not semantic review.\n",
    ));
    let next = in_session(
        &project,
        session,
        &["work", "next", &work, "--json", "--full"],
    );
    assert_eq!(next["next"]["action"], "spawn", "{next}");
    let worker = next["next"]["assignment"].as_str().unwrap().to_owned();
    let agent = next["next"]["agent"].as_str().unwrap().to_owned();

    // Uncommitted changes in the checkout and in the extra worktree.
    fs::write(root.join("worker-edit.txt"), "uncommitted\n").unwrap();
    fs::write(extra.join("worktree-edit.txt"), "uncommitted\n").unwrap();
    let context = resume_hook(&project, session, &claude(session));
    assert!(
        context.contains(&format!("current Work {work}")),
        "{context}"
    );
    assert!(
        context.contains(&format!("Open assignments: {worker} (")),
        "{context}"
    );
    assert!(context.contains(&format!("'{agent}', open)")), "{context}");
    assert!(
        context.contains("Uncommitted checkout trees: "),
        "{context}"
    );
    assert!(
        context.contains(&format!("{} (branch main, 1 changed)", root.display())),
        "{context}"
    );
    assert!(
        context.contains(&format!(
            "{} (branch worker-tree, 1 changed)",
            fs::canonicalize(&extra).unwrap().display()
        )),
        "{context}"
    );
    let show = ok_json(exitbind(
        &std::env::temp_dir(),
        &project.session_store,
        &claude(session),
        &["session", "recovery", "show", "--json"],
        b"",
    ));
    let derived = &show["currentWorks"];
    assert_eq!(
        derived["works"][0]["openAssignments"][0]["assignment"],
        worker.as_str()
    );
    assert_eq!(derived["works"][0]["openAssignments"][0]["state"], "open");
    assert_eq!(
        derived["uncommittedTrees"]["trees"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Clean trees: the assignment is still named, no tree is.
    fs::remove_file(root.join("worker-edit.txt")).unwrap();
    fs::remove_file(extra.join("worktree-edit.txt")).unwrap();
    let clean = resume_hook(&project, session, &claude(session));
    assert!(
        clean.contains(&format!("Open assignments: {worker} (")),
        "{clean}"
    );
    assert!(!clean.contains("Uncommitted checkout trees"), "{clean}");
    let _ = fs::remove_dir_all(&extra);
    fs::remove_dir_all(project.root).unwrap();
}

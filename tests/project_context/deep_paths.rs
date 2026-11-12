//! R55 session-summary budget with deep paths (re-review 9dd1dca).
use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;

fn assert_tight_mandatory_core(context: &str) {
    for clause in [
        "Preserve the existing root host conversation; project context only augments it.",
        "Do not replace, reset, fork, or request compaction for role loading or handoff.",
        "Keep recent user corrections and rejected approaches with reasons; refer frozen-run conflicts to the existing lead for explicit supersession.",
        "Native conversational recall differs from durable role memory.",
        "Print non-null Work/Goal `presentation.terminal` verbatim on its own line.",
        "Print a Goal's `presentation.terminal` once per `completionEvent.id` from the normal closure response; do not poll status.",
        "Recover lost closure replies via their exact read-only routes.",
        "No model was selected or launched; declarations are not an OS sandbox.",
    ] {
        assert!(context.contains(clause), "missing {clause}: {context}");
    }
}

fn assert_active_goal_context(base: &Path, root: &Path, exe: &Path, profile: &str) {
    let session = "deep-context-goal-session";
    let store = base.join("session-store");
    fs::create_dir(&store).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    let native = |program: &Path| {
        let mut command = support::git_topology::command(program);
        command
            .current_dir(root)
            .env("CODEX_THREAD_ID", session)
            .env("EXITBIND_SESSION_GOALS_DIR", &store)
            .env("EXITBIND_NO_UPDATE_CHECK", "1");
        command
    };
    let goal = native(exe)
        .args([
            "goal",
            "incorporate",
            "--goal-id",
            "deep-context-goal",
            "--goal",
            "Preserve the synthetic native recovery context",
            "--obligation",
            "Keep the verified Goal and Work routes",
            "--json",
            "--config",
        ])
        .arg(root.join("exitbind.json"))
        .output()
        .unwrap();
    assert!(goal.status.success(), "{goal:?}");

    let mut command = native(exe);
    command
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = support::spawn(&mut command).unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"hook_event_name":"SessionStart", "session_id":session, "cwd":root})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    let (base_text, _) = context
        .split_once("\nExitbind recovered native Goal ")
        .expect("the active Goal recovery follows the project context");
    assert!(
        base_text.len() <= 3072,
        "{} bytes: {context}",
        base_text.len()
    );
    assert!(context.len() <= 4096, "{} bytes: {context}", context.len());
    assert!(
        context.contains("Config above: native Goal owner."),
        "{context}"
    );
    assert!(context.contains(profile), "{context}");
    assert_tight_mandatory_core(context);

    let facts = context
        .split_once("Current project facts: `")
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(route, _)| route)
        .expect("the exact project facts route is present");
    let resume = context
        .split('`')
        .find(|part| part.contains(" work resume --json"))
        .expect("the exact Work resume route is present");
    for route in [facts, resume] {
        let output = native(Path::new("/bin/sh"))
            .args(["-c", route])
            .output()
            .unwrap();
        assert!(output.status.success(), "{route}: {output:?}");
        let _: Value = serde_json::from_slice(&output.stdout).unwrap();
    }
    eprintln!(
        "active Goal SessionStart: base {} bytes, aggregate {} bytes",
        base_text.len(),
        context.len()
    );
}

/// R55: with a deep checkout and a deep executable path, the session context
/// still fits 3072 bytes, keeps the mandatory core (the full sentence, or its
/// compact form when the full summary cannot fit), and prints the full
/// project-context route; the complete-lists route is what gives way first.
/// The SessionStart context for a project and an executable copy, each
/// `depth` long directory segments below the temporary base.
fn deep_context(label: &str, depth: usize) -> (String, String) {
    let base = support::temp(label);
    let deep = |name: &str| {
        (0..depth).fold(base.join(name), |path, index| {
            path.join(format!("a-deep-checkout-directory-segment-{index:02}"))
        })
    };
    let root = deep("project");
    fs::create_dir_all(&root).unwrap();
    support::git_topology::repository(&root);
    let rules = [
        ("AGENTS.md", "Keep the user's correction."),
        ("CLAUDE.md", "Do not publish."),
    ];
    for (name, body) in rules {
        fs::write(root.join(name), body).unwrap();
    }
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let worker = config["agents"]["worker"].clone();
    for index in 0..100 {
        config["agents"][format!("extra_agent_{index:03}")] = worker.clone();
    }
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let bin = deep("bin");
    fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("exitbind");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &exe);
    let context = session_hook_with(&exe, &root);
    let command = context
        .split_once("Current project facts: `")
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(command, _)| command)
        .unwrap_or_else(|| panic!("project facts route missing: {context}"));
    let output = support::git_topology::command("/bin/sh")
        .args(["-c", command])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {output:?}");
    let facts: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut omitted = 0;
    for (name, body) in rules {
        let rule = facts["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["path"] == name)
            .unwrap();
        assert_eq!(rule["state"], "current");
        assert_eq!(
            rule["sha256"],
            format!("{:x}", Sha256::digest(body.as_bytes()))
        );
        if !context.contains(&format!("Project rule: {name} ")) {
            omitted += 1;
        }
    }
    if omitted > 0 {
        assert!(
            context.contains(&format!(
                "{omitted} more references; run the current project facts route above."
            )),
            "{context}"
        );
    }
    if depth == 3 && root.to_str().unwrap().len() > 250 {
        assert!(
            omitted > 0,
            "fixture must exercise reference omission: {context}"
        );
        assert!(
            context.contains("Work's --config <WORK_CONFIG> last"),
            "fixture must exercise the tight Work route: {context}"
        );
        eprintln!("tight SessionStart context: {} bytes", context.len());
    }
    let lead = &facts["lead"];
    let profile_bytes = fs::read(root.join(lead["profilePath"].as_str().unwrap())).unwrap();
    assert!(!profile_bytes.is_empty());
    assert_eq!(
        lead["profileSha256"],
        format!("{:x}", Sha256::digest(&profile_bytes))
    );
    let profile = format!(
        "Lead profile: {} -> {} (sha256 {}).",
        lead["agentId"].as_str().unwrap(),
        lead["nativeName"].as_str().unwrap(),
        lead["profileSha256"].as_str().unwrap()
    );
    assert!(context.contains(&profile), "{context}");
    let resume = context
        .split('`')
        .find(|part| part.contains(" work resume --json"))
        .unwrap_or_else(|| panic!("resume route missing: {context}"));
    let output = support::git_topology::command("/bin/sh")
        .args(["-c", resume])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{resume}: {output:?}");
    let _: Value = serde_json::from_slice(&output.stdout).unwrap();
    if context.contains("Work's --config <WORK_CONFIG> last") {
        assert_tight_mandatory_core(&context);
        let foreign = project("tight-foreign 'project'");
        let started = call(
            &foreign,
            &[
                "work",
                "begin",
                "change",
                "--goal",
                "foreign Work",
                "--check-command",
                "true",
            ],
        );
        let work = started["work"].as_str().unwrap();
        let route = context
            .split('`')
            .find(|part| part.starts_with("{\"argv\":[\"work\",\"detail\","))
            .expect("bounded Work detail descriptor");
        let mut descriptor: Value = serde_json::from_str(route).unwrap();
        assert_eq!(descriptor["sameExecutableRequired"], true);
        assert_eq!(descriptor["sameConfigRequired"], true);
        let argv = descriptor["argv"].as_array_mut().unwrap();
        let work_slot = argv.iter_mut().find(|value| **value == "<WORK>").unwrap();
        *work_slot = json!(work);
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let config = foreign.join("exitbind.json");
        let argv = support::reconstruct_work_command(&descriptor, &exe, &config, &root);
        assert_eq!(argv[1..4], ["work", "detail", work]);
        assert_eq!(argv[argv.len() - 2], "--config");
        assert_eq!(argv.last().unwrap(), config.to_str().unwrap());
        let detail = argv
            .iter()
            .map(|value| quote(value))
            .collect::<Vec<_>>()
            .join(" ");
        let output = support::git_topology::command("/bin/sh")
            .args(["-c", &detail])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{detail}: {output:?}");
        let detail: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(detail["work"], work);
        assert_eq!(detail["complete"], true);
        assert!(context.contains("Read full detail anew"), "{context}");
        assert!(context.contains("Focus: no authority"), "{context}");
        assert!(
            context.contains("continuation.receive bind/prepare"),
            "{context}"
        );
        assert!(context.contains("Keep other-Goal sidecars"), "{context}");
        assert!(context.contains("no raw-state recovery"), "{context}");
    }
    if depth == 3 && root.to_str().unwrap().len() > 250 {
        assert_active_goal_context(&base, &root, &exe, &profile);
    }
    let route = format!("{} project context --json --config ", exe.display());
    fs::remove_dir_all(base).unwrap();
    (context, route)
}

/// R55: with deep checkout and executable paths the session context still
/// fits 3072 bytes and prints full routes (the project-context route at a
/// moderate depth, the Work read route at any depth). The
/// complete-lists route gives way first, then the full mandatory core gives
/// way to its compact form; the core itself is never cut.
#[test]
fn deep_paths_keep_the_session_context_bounded_with_its_core() {
    let (moderate, route) = deep_context("project-context-deep-1", 1);
    assert!(moderate.len() <= 3072, "{} bytes", moderate.len());
    assert!(moderate.contains(&route), "{moderate}");
    assert!(
        moderate.contains("Preserve the existing root"),
        "{moderate}"
    );

    let (deepest, route) = deep_context("project-context-deep-3", 3);
    assert!(route.len() > 150, "{route}");
    assert!(deepest.len() <= 3072, "{} bytes", deepest.len());
    // Under pressure the concrete resume route supplies the current Work
    // detail; project facts and the mandatory core remain available.
    let exe = route.split(" project context").next().unwrap();
    assert!(
        deepest.contains(&format!("{exe} work resume --json --config ")),
        "{deepest}"
    );
    assert!(deepest.contains(&route), "{deepest}");
    assert!(
        deepest.contains("Preserve the existing root host conversation"),
        "{deepest}"
    );
    assert!(
        deepest.contains("do not run Exitbind commands, initialize a project"),
        "{deepest}"
    );
    assert!(
        deepest.contains("Classification is the lead's job")
            || deepest.contains("the Lead classifies"),
        "{deepest}"
    );
    assert!(deepest.contains("Lead profile:"), "{deepest}");
}

#[test]
fn session_start_refuses_when_complete_mandatory_routes_cannot_fit() {
    let base = support::temp("project-context-over-budget");
    let deep = |name: &str| {
        (0..5).fold(base.join(name), |path, index| {
            path.join(format!("long-{index:02}-{}", "x".repeat(80)))
        })
    };
    let root = deep("project");
    fs::create_dir_all(&root).unwrap();
    support::git_topology::repository(&root);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");

    let exe = deep("bin").join("exitbind");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &exe);
    let mut command = support::git_topology::command(&exe);
    command
        .arg("hook-run")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = support::spawn(&mut command).unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"hook_event_name":"SessionStart", "cwd":root})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("complete mandatory session context and routes exceed 3072 bytes"),
        "{output:?}"
    );
}

//! R55 session-summary budget with deep paths (re-review 9dd1dca).
use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;

fn deep_session_hook(executable: &Path, root: &Path) -> String {
    let mut command = support::git_topology::command(executable);
    command
        .arg("hook-run")
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
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
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_tight_mandatory_core(context: &str) {
    for clause in [
        "Keep the original root host conversation; project context only augments it.",
        "Never replace, reset, fork or request compaction for role loading or handoff.",
        "Keep recent user corrections and rejected approaches with reasons; refer frozen-run conflicts to Lead for explicit supersession.",
        "Native recall is not durable role memory.",
        "Print non-null Work/Goal `presentation.terminal` verbatim on its own line;",
        "Goal once per `completionEvent.id` from normal closure reply; no status polls.",
        "Recover lost closure replies via exact read-only routes.",
        "No model chosen or run; declarations aren't OS sandboxes.",
    ] {
        assert!(context.contains(clause), "missing {clause}: {context}");
    }
    assert!(context.contains("Minor reversible edits: do not run Exitbind commands, initialize a project or ask workflow/review; the Lead classifies effects, not filenames. Reuse scoped authority/review decisions; ask only new choices."), "{context}");
}

fn assert_active_goal_context(
    base: &Path,
    root: &Path,
    exe: &Path,
    profile: &str,
    base_limit: usize,
) {
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
    let ledger = fs::read_dir(&store)
        .unwrap()
        .map(|entry| entry.unwrap().path().join(".exitbind/session-goal.jsonl"))
        .find(|path| path.is_file())
        .expect("the isolated Goal ledger");
    let ledger_before = fs::read(&ledger).unwrap();
    let config_before = fs::read(root.join("exitbind.json")).unwrap();

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
        base_text.len() <= base_limit,
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
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), config_before);
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
fn deep_context(label: &str, depth: usize, width_sum: Option<usize>) -> (String, String) {
    let base = support::temp(label);
    let padding = if let Some(width_sum) = width_sum {
        // Emulate the combined decimal widths of the helper's u32 PID and
        // AtomicU64 sequence, without changing either value or the temp root.
        // A shorter platform temp parent is padded to the selected 194-byte
        // fixture parent so this case exercises the same absolute lengths.
        let parent_bytes = base.parent().unwrap().as_os_str().as_encoded_bytes().len();
        assert!(parent_bytes <= 194, "fixture parent exceeds 194 bytes");
        let name = base.file_name().unwrap().to_str().unwrap();
        let sequence = name.rsplit_once('-').unwrap().1;
        let actual_width = std::process::id().to_string().len() + sequence.len();
        194 - parent_bytes + width_sum.checked_sub(actual_width).unwrap()
    } else {
        0
    };
    let deep = |name: &str| {
        (0..depth).fold(base.join(name), |path, index| {
            let extra = match index {
                0 => padding.min(100),
                1 => padding.saturating_sub(100),
                _ => 0,
            };
            path.join(format!(
                "a-deep-checkout-directory-segment-{index:02}{}",
                "p".repeat(extra)
            ))
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
    if let Some(width_sum) = width_sum {
        assert_eq!(root.as_os_str().as_encoded_bytes().len(), 347 + width_sum);
        assert_eq!(exe.as_os_str().as_encoded_bytes().len(), 352 + width_sum);
        assert_eq!(
            config_path.as_os_str().as_encoded_bytes().len(),
            361 + width_sum
        );
    }
    let context = deep_session_hook(&exe, &root);
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
                "Current project facts: `{command}`. +{omitted} references via this."
            )),
            "{context}"
        );
    } else {
        assert!(!context.contains(" references via this."), "{context}");
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
    assert!(
        context.contains(&format!("{profile} Read fully before material choices.")),
        "{context}"
    );
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
                "--preservation-check-command",
                "true",
                "--preserve-requirement",
                "foreign:foreign Work",
            ],
        );
        let work = started["work"].as_str().unwrap();
        call_input(
            &foreign,
            &["work", "record", work],
            &json!({
                "action":"init",
                "sourceRef":"owner:fixture",
                "sourceText":"foreign Work",
                "requirements":[{"id":"foreign", "text":"foreign Work"}]
            }),
        );
        let ledger = foreign
            .join(".exitbind/runs")
            .join(format!("work-{}.jsonl", work.strip_prefix("smw_").unwrap()));
        let ledger_before = fs::read(&ledger).unwrap();
        let goal_ledger = foreign.join(".exitbind/session-goal.jsonl");
        let goal_ledger_before = fs::read(&goal_ledger).unwrap();
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
        let output = support::git_topology::command(&exe)
            .current_dir(&root)
            .args(["work", "continuation", work, "--json", "--config"])
            .arg(&config)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let continuation: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(continuation["receive"]["bind"].is_array());
        assert_eq!(fs::read(&ledger).unwrap(), ledger_before);
        assert_eq!(fs::read(&goal_ledger).unwrap(), goal_ledger_before);
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
        assert_active_goal_context(
            &base,
            &root,
            &exe,
            &profile,
            if width_sum.is_some() { 3040 } else { 3072 },
        );
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
    let (moderate, route) = deep_context("project-context-deep-1", 1, None);
    assert!(moderate.len() <= 3072, "{} bytes", moderate.len());
    assert!(moderate.contains(&route), "{moderate}");
    assert!(
        moderate.contains("Preserve the existing root host conversation")
            || moderate.contains("Keep the original root host conversation"),
        "{moderate}"
    );

    let (deepest, route) = deep_context("project-context-deep-3", 3, None);
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
        deepest.contains("Preserve the existing root host conversation")
            || deepest.contains("Keep the original root host conversation"),
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
fn maximum_width_session_context_keeps_active_goal_headroom() {
    // The 20-digit counter is paired with 6-, 7-, and 10-digit PID widths.
    // This pads the total path length; it does not claim those actual IDs.
    for pid_width in [6, 7, 10] {
        let (context, _) = deep_context("project-context-deep-3", 3, Some(pid_width + 20));
        assert_tight_mandatory_core(&context);
    }
}

#[test]
fn reference_pointer_keeps_the_exact_quoted_multiline_facts_route() {
    let base = support::temp("context-'quoted'\npath");
    let parent_bytes = base.parent().unwrap().as_os_str().as_encoded_bytes().len();
    assert!(parent_bytes <= 194, "fixture parent exceeds 194 bytes");
    let padding = 194 - parent_bytes;
    let first = padding.min(100);
    let second = padding - first;
    let root = base
        .join("project")
        .join("r".repeat(100 + first))
        .join("s".repeat(1 + second));
    fs::create_dir_all(&root).unwrap();
    support::git_topology::repository(&root);
    let init = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .args(["init", "--mode", "portable", "--root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let path = root.join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
    let seed = base.join("seed-exitbind");
    support::place_executable(Path::new(env!("CARGO_BIN_EXE_exitbind")), &seed);
    let without_references = deep_session_hook(&seed, &root);
    assert!(!without_references.contains(" references via this."));

    fs::write(root.join("AGENTS.md"), "First current rule.").unwrap();
    fs::write(root.join("CLAUDE.md"), "Second current rule.").unwrap();
    let mut partial = false;
    for width in (4..=164).step_by(4) {
        let bin = base
            .join("bin")
            .join("x".repeat(1 + first))
            .join("y".repeat(width + second));
        fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("exitbind");
        fs::hard_link(&seed, &exe).unwrap();
        let context = deep_session_hook(&exe, &root);
        let shown = ["AGENTS.md", "CLAUDE.md"]
            .iter()
            .filter(|name| context.contains(&format!("\nProject rule: {name} ")))
            .count();
        if shown != 1 {
            continue;
        }
        let route = context
            .split_once("Current project facts: `")
            .and_then(|(_, tail)| tail.split_once('`'))
            .map(|(route, _)| route)
            .expect("complete facts route");
        assert!(route.contains('\n') && route.contains('\''));
        assert!(
            context.contains(&format!(
                "Current project facts: `{route}`. +1 references via this."
            )),
            "{context}"
        );
        let output = support::git_topology::command("/bin/sh")
            .current_dir(&root)
            .args(["-c", route])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let facts: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            facts["rules"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|rule| rule["state"] == "current")
                .count(),
            2
        );
        partial = true;
        break;
    }
    assert!(partial, "the fixture must show one rule and omit the other");
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

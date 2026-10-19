use super::*;
use serde_json::json;
use std::path::{Path, PathBuf};

#[test]
fn codex_compaction_checkpoints_preserve_only_owner_bound_navigation_metadata() {
    let project = Project::new("native-session-recovery-hint");
    let context_project = Project::new("native-session-recovery-context-only");
    let session = "codex-recovery-fixture";
    let store = project.session_store.as_path();
    let guidance =
        "Current fixture instructions; caller read these bytes before recording the hint.\n";
    fs::write(project.root.join("recovery-guidance.md"), guidance).unwrap();
    let producer = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(producer.status.success());
    let producer: Value = serde_json::from_slice(&producer.stdout).unwrap();
    let producer_sha = producer["executableSha256"].as_str().unwrap();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "recovery-goal",
            "--goal",
            "Recover this existing conversation",
            "--obligation",
            "Reread current Goal and Work details before continuing",
        ],
    );

    let started = project.value_in_session(
        "codex",
        session,
        store,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Inspect and recover this existing conversation Work",
            "--check-command",
            "true",
        ],
    );
    let work = started["work"].as_str().unwrap().to_owned();
    let goal_after_start =
        project.value_in_session("codex", session, store, &["goal", "status", "--json"]);
    let member = goal_after_start["journeyMembers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["work"] == work)
        .expect("Work start is an exact native Goal member");

    let mut record = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    record
        .current_dir(&project.root)
        .args(["session", "recovery", "record", "--config"])
        .arg(project.root.join("exitbind.json"))
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "CODEX_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "EXITBIND_NATIVE_SESSION_ID",
    ] {
        record.env_remove(name);
    }
    let input = json!({
        "sessionId": session,
        "currentStep": "Inspect the exact Goal and known Work details",
        "currentWork": {"state":"known","work":work},
        "workRefs": [{
            "work":member["work"], "configPath":member["configPath"],
            "configSha256":member["configSha256"], "startEventSha256":member["startEventSha256"],
            "relation":"member"
        }],
        "latestCorrections": ["Do not infer a current Work from history"],
        "guidanceRefs": [{
            "label":"current fixture instructions", "root":"product", "path":"recovery-guidance.md",
            "work":work,
            "sha256":digest_text(guidance), "producerExecutableSha256":producer_sha,
            "consumption":"caller_reported_read"
        }]
    });
    let mut child = record.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let recorded = child.wait_with_output().unwrap();
    assert!(
        recorded.status.success(),
        "{}{}",
        String::from_utf8_lossy(&recorded.stdout),
        String::from_utf8_lossy(&recorded.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&recorded.stdout).unwrap()["readOnlyAuthority"],
        false
    );

    let hint_path = fs::read_dir(store)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("recovery-hint.json"))
        .find(|path| path.is_file())
        .expect("private recovery hint is recorded beneath the selected session root");
    let initial_hint = fs::read(&hint_path).unwrap();
    let goal_path = find_named(store, "session-goal.jsonl")
        .expect("canonical native Goal ledger exists in the private session store");
    let original_goal = fs::read(&goal_path).unwrap();

    let initial_next = project.value_in_session(
        "codex",
        session,
        store,
        &["work", "next", &work, "--json", "--full"],
    );
    let lead = &initial_next["next"];
    assert_eq!(lead["action"], "lead_decision");
    assert!(lead["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outcome| outcome == "scoped"));
    let assignment = lead["assignment"].as_str().unwrap();
    return_in_session(
        &project,
        session,
        &work,
        assignment,
        "scoped",
        b"Deterministic fixture scope; not semantic review.\n",
    );
    assert_eq!(
        fs::read(&goal_path).unwrap(),
        original_goal,
        "a Work-only scoped return does not change the native Goal event"
    );

    let mut wrong_session_event = json!({
        "hook_event_name":"PreCompact",
        "session_id":"different-session",
        "cwd":context_project.root,
    });
    let rejected = recovery_hook(&context_project, store, session, &wrong_session_event);
    assert!(rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert_eq!(fs::read(&hint_path).unwrap(), initial_hint);

    for event in ["PreCompact", "PostCompact"] {
        let payload = json!({
            "hook_event_name":event,
            "session_id":session,
            "cwd":context_project.root,
        });
        let output = recovery_hook(&context_project, store, session, &payload);
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stdout.is_empty(),
            "compaction stdout is not a context channel"
        );
        let hint: Value = serde_json::from_slice(&fs::read(&hint_path).unwrap()).unwrap();
        assert_eq!(hint["lastCheckpoint"]["event"], event);
        assert_eq!(hint["currentWork"]["state"], "known");
        assert_eq!(hint["currentWork"]["work"], work);
    }

    let stale_precompact = json!({
        "hook_event_name":"PreCompact", "session_id":session, "cwd":context_project.root.display().to_string(),
    });
    let record_bytes = serde_json::to_vec(&input).unwrap();
    let (recorded_again, checkpoint_again) = std::thread::scope(|scope| {
        let record_thread = scope.spawn(|| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
            command
                .current_dir(&project.root)
                .args(["session", "recovery", "record", "--config"])
                .arg(project.root.join("exitbind.json"))
                .env_remove("CODEX_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("EXITBIND_NATIVE_SESSION_ID")
                .env("CODEX_THREAD_ID", session)
                .env("EXITBIND_SESSION_GOALS_DIR", store)
                .env("EXITBIND_NO_UPDATE_CHECK", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&record_bytes)
                .unwrap();
            child.wait_with_output().unwrap()
        });
        let hook_thread =
            scope.spawn(|| recovery_hook(&context_project, store, session, &stale_precompact));
        (record_thread.join().unwrap(), hook_thread.join().unwrap())
    });
    assert!(checkpoint_again.status.success(), "{checkpoint_again:?}");
    assert!(checkpoint_again.stdout.is_empty());
    if !recorded_again.status.success() {
        assert!(
            String::from_utf8_lossy(&recorded_again.stderr).contains("busy"),
            "concurrent record must either succeed or report a bounded lock-busy refusal: {recorded_again:?}"
        );
        let before_retry: Value = serde_json::from_slice(&fs::read(&hint_path).unwrap()).unwrap();
        assert_eq!(before_retry["lastCheckpoint"]["event"], "PostCompact");
        assert_eq!(before_retry["currentStep"], input["currentStep"]);
        let mut retry = Command::new(env!("CARGO_BIN_EXE_exitbind"))
            .current_dir(&project.root)
            .args(["session", "recovery", "record", "--config"])
            .arg(project.root.join("exitbind.json"))
            .env("CODEX_THREAD_ID", session)
            .env("EXITBIND_SESSION_GOALS_DIR", store)
            .env("EXITBIND_NO_UPDATE_CHECK", "1")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("EXITBIND_NATIVE_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        retry
            .stdin
            .take()
            .unwrap()
            .write_all(&record_bytes)
            .unwrap();
        let retried = retry.wait_with_output().unwrap();
        assert!(
            retried.status.success(),
            "explicit retry after busy record must succeed: {retried:?}"
        );
    }
    let concurrent_hint: Value = serde_json::from_slice(&fs::read(&hint_path).unwrap()).unwrap();
    assert_eq!(concurrent_hint["lastCheckpoint"]["event"], "PostCompact");
    assert_eq!(
        concurrent_hint["currentStep"],
        "Inspect the exact Goal and known Work details"
    );

    let start = json!({
        "hook_event_name":"SessionStart",
        "source":"resume",
        "session_id":session,
        "cwd":context_project.root,
    });
    let output = recovery_hook(&context_project, store, session, &start);
    assert!(output.status.success(), "{output:?}");
    let context: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = context["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("Host lifecycle: resume."));
    assert!(!context.contains("Inspect the exact Goal and known Work details"));
    assert!(context.contains("historical until the exact Work detail is reread"));
    assert!(context.contains("no current Work/action binding was acquired"));
    assert!(context.contains("Goal-member locator"));
    assert!(context.contains(&work));
    assert!(context.contains("session recovery show --json"));
    assert!(context.len() <= 4096);

    let show = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(std::env::temp_dir())
        .args(["session", "recovery", "show", "--json"])
        .env("PATH", "")
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .output()
        .unwrap();
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let shown: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(shown["activeRecovery"], Value::Null);
    assert_eq!(shown["activeSavedStep"], Value::Null);
    assert_eq!(
        shown["savedHint"]["currentStep"],
        "Inspect the exact Goal and known Work details"
    );
    assert_eq!(shown["workSelection"]["state"], "unresolved");
    assert_eq!(shown["currentWorkReadSnapshot"], Value::Null);
    assert_eq!(shown["freshness"]["goalCurrent"], true);
    assert_eq!(shown["freshness"]["declaredGuidanceSourcesCurrent"], true);
    assert_eq!(shown["freshness"]["savedStepCurrent"], false);
    assert_eq!(shown["milestone"]["protocol"], 1);
    assert_eq!(
        shown["milestone"]["state"], "current_detail_observed",
        "milestone projection: {}",
        shown["milestone"]
    );
    assert_eq!(shown["milestone"]["goal"], "recovery-goal");
    assert_eq!(shown["milestone"]["goalCurrent"], true);
    assert!(shown["milestone"]["workRef"]
        .as_str()
        .unwrap()
        .contains("workDetailArgv"));
    assert_eq!(shown["milestone"]["work"], work);
    assert_eq!(shown["milestone"]["assignment"]["state"], "known");
    assert_eq!(shown["milestone"]["phase"]["stage"], 2);
    assert_eq!(shown["milestone"]["phase"]["status"], "running");
    assert_eq!(
        shown["milestone"]["checkReferences"]["historical"][0]["savedStepRouteRef"],
        "savedHint.currentStep"
    );
    assert_eq!(
        shown["milestone"]["checkReferences"]["historical"][0]["current"],
        false
    );
    assert_eq!(
        shown["milestone"]["checkReferences"]["current"],
        Value::Null
    );
    assert_eq!(
        shown["milestone"]["currentWorkDetail"]["bindingSha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        shown["milestone"]["checkReferences"]["current"],
        Value::Null
    );
    assert!(shown["milestone"]["routes"].as_array().unwrap().len() <= 3);
    assert_eq!(shown["milestone"]["routes"][0]["kind"], "work_detail");
    assert_eq!(
        shown["milestone"]["routes"][0]["routeRef"],
        "readRoutes.workDetailArgv[0]"
    );
    assert_eq!(shown["milestone"]["routes"][1]["kind"], "action_refresh");
    assert_eq!(
        shown["milestone"]["routes"][1]["routeRef"],
        "readRoutes.workDetailArgv[0]"
    );
    let milestone_bytes = serde_json::to_vec_pretty(&shown["milestone"]).unwrap();
    assert!(milestone_bytes.len() <= 4 * 1024);
    assert!(
        milestone_bytes
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            < 50
    );
    assert_eq!(shown["readRoutes"]["workDetailArgv"][0]["work"], work);
    let work_argv = shown["readRoutes"]["workDetailArgv"][0]["argv"]
        .as_array()
        .unwrap();
    assert_eq!(
        work_argv[0],
        fs::canonicalize(env!("CARGO_BIN_EXE_exitbind"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(work_argv.last().unwrap(), &member["configPath"]);
    let detail_route = work_argv
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    let detail = Command::new(detail_route[0])
        .args(&detail_route[1..])
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .output()
        .unwrap();
    assert!(
        detail.status.success(),
        "exact executable-bound Work detail route: {detail:?}"
    );
    assert_eq!(
        shown["readRoutes"]["recoveryShowArgv"],
        json!([
            fs::canonicalize(env!("CARGO_BIN_EXE_exitbind"))
                .unwrap()
                .to_str()
                .unwrap(),
            "session",
            "recovery",
            "show",
            "--json"
        ])
    );

    wrong_session_event["hook_event_name"] = json!("SessionStart");
    let stale = recovery_hook(&context_project, store, session, &wrong_session_event);
    assert!(stale.status.success(), "{stale:?}");
    let stale: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert!(stale["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("disagrees with current session selectors"));
    assert_eq!(fs::read(&goal_path).unwrap(), original_goal);

    let requirement = "Reread exact Goal and Work details after the Goal advances.";
    fs::write(
        project.root.join("goal-recovery-requirement.txt"),
        format!("{requirement}\n"),
    )
    .unwrap();
    project.value_in_session(
        "codex",
        session,
        store,
        &[
            "goal",
            "require",
            "--goal-id",
            "recovery-goal",
            "--requirement",
            "recovery-currentness",
            "--obligation",
            requirement,
            "--artifact",
            "goal-recovery-requirement.txt",
        ],
    );
    let advanced_goal = fs::read(&goal_path).unwrap();
    let after_advance = recovery_hook(&context_project, store, session, &start);
    assert!(after_advance.status.success());
    let after_advance: Value = serde_json::from_slice(&after_advance.stdout).unwrap();
    let after_context = after_advance["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(after_context.contains("goalCurrent=false"));
    assert!(!after_context.contains("Inspect the exact Goal and known Work details"));
    assert!(after_context.contains("session recovery show --json"));
    let shown_after_advance = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(std::env::temp_dir())
        .args(["session", "recovery", "show", "--json"])
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .output()
        .unwrap();
    assert!(shown_after_advance.status.success());
    let shown_after_advance: Value = serde_json::from_slice(&shown_after_advance.stdout).unwrap();
    assert_eq!(shown_after_advance["activeRecovery"], Value::Null);
    assert_eq!(shown_after_advance["freshness"]["goalCurrent"], false);
    assert_eq!(
        shown_after_advance["milestone"]["goalCurrent"], false,
        "Goal advancement is reported independently from a current Work detail"
    );
    assert_eq!(
        shown_after_advance["milestone"]["state"],
        "current_detail_observed"
    );

    fs::write(
        project.root.join("recovery-guidance.md"),
        "changed guidance bytes\n",
    )
    .unwrap();
    let changed_guidance = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(std::env::temp_dir())
        .args(["session", "recovery", "show", "--json"])
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .output()
        .unwrap();
    assert!(changed_guidance.status.success());
    let changed_guidance: Value = serde_json::from_slice(&changed_guidance.stdout).unwrap();
    assert_eq!(
        changed_guidance["freshness"]["declaredGuidanceSourcesCurrent"],
        false
    );
    assert_eq!(changed_guidance["activeRecovery"], Value::Null);

    let mut corrupt = fs::read(&hint_path).unwrap();
    let needle = b"Inspect the exact Goal and known Work details";
    let location = corrupt
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap();
    corrupt[location] = b'X';
    fs::write(&hint_path, corrupt).unwrap();
    let corrupt_context = recovery_hook(&context_project, store, session, &start);
    assert!(corrupt_context.status.success());
    let corrupt_context: Value = serde_json::from_slice(&corrupt_context.stdout).unwrap();
    assert!(corrupt_context["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("hint is unusable"));
    let corrupt_show = Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(std::env::temp_dir())
        .args(["session", "recovery", "show", "--json"])
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .output()
        .unwrap();
    assert!(corrupt_show.status.success());
    let corrupt_show: Value = serde_json::from_slice(&corrupt_show.stdout).unwrap();
    assert_eq!(corrupt_show["milestone"]["state"], "unknown");
    assert_eq!(corrupt_show["milestone"]["workState"], "unknown");
    let unknown_bytes = serde_json::to_vec_pretty(&corrupt_show["milestone"]).unwrap();
    assert!(unknown_bytes.len() <= 4 * 1024);
    assert!(unknown_bytes.iter().filter(|byte| **byte == b'\n').count() < 50);
    assert_eq!(fs::read(&goal_path).unwrap(), advanced_goal);
}

fn recovery_hook(
    project: &Project,
    session_store: &Path,
    session: &str,
    payload: &Value,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&project.root)
        .args(["hook-run"])
        .env("CODEX_THREAD_ID", session)
        .env("EXITBIND_SESSION_GOALS_DIR", session_store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env_remove("CLAUDE_ENV_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "CODEX_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "EXITBIND_NATIVE_SESSION_ID",
    ] {
        command.env_remove(name);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(payload).unwrap())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(root).ok()? {
        let path = entry.ok()?.path();
        if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_named(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

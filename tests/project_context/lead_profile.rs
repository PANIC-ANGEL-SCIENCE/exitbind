//! The pre-decision Lead profile route delivers complete current bytes.
use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::process::Output;

fn route(root: &Path, command: &Value) -> Output {
    let argv = command.as_array().unwrap();
    let mut process = support::git_topology::command(argv[0].as_str().unwrap());
    process
        .current_dir(root)
        .args(argv[1..].iter().map(|v| v.as_str().unwrap()));
    process.output().unwrap()
}

fn refusal(output: &Output) -> String {
    assert!(!output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.get("content").is_none(), "{value}");
    value["error"].as_str().unwrap().to_owned()
}

fn drift_refusal(output: &Output) -> Value {
    assert!(!output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.get("content").is_none(), "{value}");
    assert_eq!(value["classification"], "lead_profile_binding_changed");
    assert_eq!(value["boundSha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["nextAction"]["readOnly"], true);
    let command = value["nextAction"]["command"].as_array().unwrap();
    assert_eq!(
        &command[1..4],
        &[json!("project"), json!("context"), json!("--json")]
    );
    assert_eq!(command[4], "--config");
    value
}

fn configured_profile(root: &Path) -> std::path::PathBuf {
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["captain"] = config["agents"]["lead"].clone();
    config["orchestration"]["lead"] = json!("captain");
    let profile = root.join("exitbind/agents/custom captain.md");
    fs::write(&profile, "Custom Lead instruction\n").unwrap();
    config["agents"]["captain"]["profile"] = json!("exitbind/agents/custom captain.md");
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    profile
}

fn session_profile_command(context: &str) -> &str {
    context
        .split_once("Read Lead profile: `")
        .and_then(|(_, rest)| rest.split_once("`."))
        .map(|(command, _)| command)
        .unwrap_or_else(|| panic!("complete profile route missing: {context}"))
}

fn run_hook_command(root: &Path, command: &str) -> Output {
    support::git_topology::command("/bin/sh")
        .current_dir(root)
        .args(["-c", command])
        .output()
        .unwrap()
}

fn native_session_hook(root: &Path, store: &Path, reported_session: &str) -> String {
    let payload = json!({"hook_event_name":"SessionStart", "cwd":root,
        "session_id":reported_session});
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .arg("hook-run")
        .env("CODEX_THREAD_ID", "profile-resume-session")
        .env("EXITBIND_SESSION_GOALS_DIR", store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut process = support::spawn(&mut command).unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn start_and_resume_offer_the_current_bound_profile_without_a_facts_read() {
    let root = project("lead-profile-short-route");
    let profile_path = configured_profile(&root);
    let store = support::temp("lead-profile-session-store");
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    let goal = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .env("CODEX_THREAD_ID", "profile-resume-session")
        .env("EXITBIND_SESSION_GOALS_DIR", &store)
        .args([
            "goal",
            "incorporate",
            "--goal-id",
            "profile-resume",
            "--goal",
            "Deliver the requested result",
            "--obligation",
            "Keep the owner's constraint",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(goal.status.success(), "{goal:?}");
    let initial = native_session_hook(&root, &store, "profile-resume-session");
    assert!(
        initial.contains("Deliver the requested result"),
        "{initial}"
    );
    assert!(initial.contains("Keep the owner's constraint"), "{initial}");
    let initial_command = session_profile_command(&initial);
    let output = run_hook_command(&root, initial_command);
    assert!(output.status.success(), "{output:?}");
    let acquired: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(acquired["content"], "Custom Lead instruction\n");
    assert_eq!(acquired["complete"], true);

    let resumed = native_session_hook(&root, &store, "profile-resume-session");
    assert!(
        resumed.contains("Deliver the requested result"),
        "{resumed}"
    );
    assert!(resumed.contains("Keep the owner's constraint"), "{resumed}");
    let resumed_command = session_profile_command(&resumed);
    assert_eq!(resumed_command, initial_command);
    assert!(resumed.contains("Reuse complete bytes acquired for this verified binding."));
    let resumed_page = run_hook_command(&root, resumed_command);
    assert!(resumed_page.status.success(), "{resumed_page:?}");
    let resumed_page: Value = serde_json::from_slice(&resumed_page.stdout).unwrap();
    assert_eq!(resumed_page["binding"], acquired["binding"]);
    let goal_status = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .env("CODEX_THREAD_ID", "profile-resume-session")
        .env("EXITBIND_SESSION_GOALS_DIR", &store)
        .args(["goal", "status", "--json", "--config", "exitbind.json"])
        .output()
        .unwrap();
    assert!(goal_status.status.success(), "{goal_status:?}");
    let goal_status: Value = serde_json::from_slice(&goal_status.stdout).unwrap();
    assert_eq!(goal_status["goal"], "Deliver the requested result");
    assert!(goal_status["obligations"]
        .to_string()
        .contains("Keep the owner's constraint"));
    let mismatched = native_session_hook(&root, &store, "other-session");
    assert!(
        !mismatched.contains("Deliver the requested result"),
        "{mismatched}"
    );
    assert!(
        !mismatched.contains("Keep the owner's constraint"),
        "{mismatched}"
    );

    fs::write(&profile_path, "Changed Lead instruction\n").unwrap();
    drift_refusal(&run_hook_command(&root, initial_command));
    let changed = native_session_hook(&root, &store, "profile-resume-session");
    let changed_command = session_profile_command(&changed);
    assert_ne!(changed_command, initial_command);
    let output = run_hook_command(&root, changed_command);
    assert!(output.status.success(), "{output:?}");
    let current: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(current["content"], "Changed Lead instruction\n");
    assert_ne!(current["binding"], acquired["binding"]);
    assert!(changed.contains("do not run Exitbind commands, initialize a project"));
}

#[test]
fn selected_custom_profile_is_acquired_in_bound_complete_pages_before_work() {
    let root = project("lead-profile-pages");
    let path = configured_profile(&root);
    let profile = format!("{}한{}", "x".repeat(4095), " more\n".repeat(12_000));
    fs::write(path, &profile).unwrap();
    let context = session_hook(&root);
    assert!(context.len() <= 3072, "{}", context.len());
    assert!(context.contains("Read fully before material choices."));
    assert!(context.contains("do not run Exitbind commands, initialize a project"));
    let skill = fs::read_to_string(root.join(".agents/skills/exitbind/SKILL.md")).unwrap();
    assert!(skill.contains("Reuse complete host-supplied bytes when they match the current profile identity; a hash alone is not acquisition."));
    assert!(skill.contains(
        "Follow every exact page route, verify contiguous offsets, total bytes and profile SHA-256"
    ));

    let facts = call(&root, &["project", "context", "--json"]);
    assert_eq!(facts["focus"]["state"], "none");
    assert_eq!(facts["lead"]["agentId"], "captain");
    let digest = format!("{:x}", Sha256::digest(profile.as_bytes()));
    assert_eq!(facts["lead"]["profileSha256"], digest);
    let binding = facts["lead"]["profileAccess"]["binding"].clone();
    let mut command = facts["lead"]["profileAccess"]["command"].clone();
    let mut received = String::new();
    let mut pages = 0;
    loop {
        let out = route(&root, &command);
        assert!(out.status.success(), "{out:?}");
        assert!(out.stdout.len() < 64 * 1024);
        let page: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(page["agent"], "captain");
        assert_eq!(page["binding"], binding);
        assert_eq!(page["profileSha256"], digest);
        assert_eq!(page["totalBytes"], profile.len());
        assert_eq!(page["offset"], received.len());
        assert_eq!(page["pageComplete"], true);
        let content = page["content"].as_str().unwrap();
        assert_eq!(page["bytes"], content.len());
        assert!(content.len() <= 4096);
        received.push_str(content);
        pages += 1;
        if page["next"].is_null() {
            assert_eq!(page["complete"], false);
            break;
        }
        command = page["next"]["command"].clone();
    }
    assert!(pages > 16, "fixture must exceed the Work section bound");
    assert_eq!(received, profile);
    let legacy = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args(["profile", "captain", "--config", "exitbind.json"])
        .output()
        .unwrap();
    assert!(legacy.status.success(), "{legacy:?}");
    assert_eq!(legacy.stdout, profile.as_bytes());
    assert_eq!(
        call(&root, &["project", "context", "--json"])["focus"]["state"],
        "none"
    );
}

#[test]
fn profile_or_configuration_change_refuses_old_pages_and_offers_refresh() {
    let root = project("lead-profile-refresh");
    let path = configured_profile(&root);
    fs::write(&path, "Original profile\n".repeat(300)).unwrap();
    let facts = call(&root, &["project", "context", "--json"]);
    let original = facts["lead"]["profileAccess"]["command"].clone();
    let first = route(&root, &original);
    assert!(first.status.success(), "{first:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let next = first["next"]["command"].clone();
    fs::write(&path, "Changed Lead instruction\n").unwrap();
    let error = drift_refusal(&route(&root, &next));
    assert_eq!(
        error["boundSha256"],
        facts["lead"]["profileAccess"]["binding"]
    );
    let refreshed = call(&root, &["project", "context", "--json"]);
    assert_eq!(
        error["currentSha256"],
        refreshed["lead"]["profileAccess"]["binding"]
    );
    let refresh_output = route(&root, &error["nextAction"]["command"]);
    assert!(refresh_output.status.success(), "{refresh_output:?}");
    let current = refreshed["lead"]["profileAccess"]["command"].clone();
    let page = route(&root, &current);
    assert!(page.status.success(), "{page:?}");
    let page: Value = serde_json::from_slice(&page.stdout).unwrap();
    assert_eq!(page["content"], "Changed Lead instruction\n");
    assert_eq!(page["complete"], true);

    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["agents"]["captain"]["purpose"] = json!("Changed purpose");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let stale_config = drift_refusal(&route(&root, &current));
    assert!(stale_config["currentSha256"].is_string());

    let current =
        call(&root, &["project", "context", "--json"])["lead"]["profileAccess"]["command"].clone();
    config["orchestration"]["lead"] = json!("worker");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let wrong_lead = drift_refusal(&route(&root, &current));
    assert!(wrong_lead["currentSha256"].is_string());
}

#[test]
fn invalid_or_unavailable_profile_never_becomes_complete_delivery() {
    let root = project("lead-profile-invalid");
    let path = configured_profile(&root);
    fs::write(&path, b"invalid \xff profile").unwrap();
    let facts = call(&root, &["project", "context", "--json"]);
    let command = facts["lead"]["profileAccess"]["command"].clone();
    let invalid = refusal(&route(&root, &command));
    assert!(invalid.contains("not complete UTF-8"));

    fs::remove_file(&path).unwrap();
    let outside = support::temp("lead-profile-outside");
    fs::write(outside.join("private.md"), "outside profile").unwrap();
    symlink(outside.join("private.md"), &path).unwrap();
    let unavailable = drift_refusal(&route(&root, &command));
    assert!(unavailable["currentSha256"].is_null());
    assert!(unavailable["error"]
        .as_str()
        .unwrap()
        .starts_with("lead profile: "));
    assert!(!unavailable.to_string().contains("outside profile"));
}

#[test]
fn empty_profile_and_invalid_utf8_offset_are_distinct() {
    let root = project("lead-profile-offset");
    let path = configured_profile(&root);
    fs::write(&path, "xé").unwrap();
    let facts = call(&root, &["project", "context", "--json"]);
    let mut command = facts["lead"]["profileAccess"]["command"].clone();
    command[5] = json!("2");
    let invalid = refusal(&route(&root, &command));
    assert!(invalid.contains("UTF-8"));

    fs::write(&path, "").unwrap();
    let facts = call(&root, &["project", "context", "--json"]);
    let empty = route(&root, &facts["lead"]["profileAccess"]["command"]);
    assert!(empty.status.success(), "{empty:?}");
    let page: Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert_eq!(page["totalBytes"], 0);
    assert_eq!(page["content"], "");
    assert_eq!(page["complete"], true);
    assert!(page["next"].is_null());

    let malformed = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(&root)
        .args([
            "project",
            "context",
            "lead-profile",
            "--json",
            "--config",
            "exitbind.json",
        ])
        .output()
        .unwrap();
    assert!(!malformed.status.success());
    assert!(!String::from_utf8_lossy(&malformed.stderr).contains("panicked"));
    assert!(refusal(&malformed).contains("requires its emitted binding and offset"));
}

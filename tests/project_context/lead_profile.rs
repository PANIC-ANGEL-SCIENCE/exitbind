//! The pre-decision Lead profile route delivers complete current bytes.
use super::*;
use sha2::{Digest, Sha256};
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

#[test]
fn selected_custom_profile_is_acquired_in_bound_complete_pages_before_work() {
    let root = project("lead-profile-pages");
    let path = configured_profile(&root);
    let profile = format!("{}한{}", "x".repeat(4095), " more\n".repeat(12_000));
    fs::write(path, &profile).unwrap();
    let context = session_hook(&root);
    assert!(context.len() <= 3072, "{}", context.len());
    assert!(context.contains("Before a consequential choice"));
    assert!(context.contains("unless the host already supplied the same complete bytes"));
    assert!(context.contains("do not run Exitbind commands, initialize a project"));

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

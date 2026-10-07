//! The shipped approved-facts route delivers a usable current start response.
mod support;

use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn invoke(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    command.current_dir(root).args(args).output().unwrap()
}

fn value(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn setup(root: &Path, apply: bool) -> Value {
    let mut args = vec![
        "setup",
        "--json",
        "--root",
        ".",
        "--mode",
        "portable",
        "--worker-observe",
        "README.md",
        "--worker-write",
        "src",
        "--worker-commands",
        "true",
        "--reviewer-observe",
        "README.md,src",
        "--reviewer-write",
        "none",
        "--reviewer-commands",
        "none",
        "--goal",
        "Deliver a bounded useful change",
        "--check-command",
        "true",
        "--review-policy",
        "required",
    ];
    if apply {
        args.push("--apply");
    }
    let bin = support::git_topology::git_only_path(root);
    let mut command = Command::new(env!("CARGO_BIN_EXE_exitbind"));
    support::git_topology::isolate(&mut command);
    value(
        command
            .current_dir(root)
            .env("PATH", bin)
            .args(args)
            .output()
            .unwrap(),
    )
}

#[test]
fn applied_setup_delivers_current_context_without_a_second_discovery() {
    let root = support::temp("start-context with spaces");
    support::git_topology::repository(&root);
    fs::write(root.join("README.md"), "Approved project rules\n").unwrap();
    fs::write(
        root.join("AGENTS.md"),
        "Keep current decisions and reviewer boundaries.\n",
    )
    .unwrap();
    assert_eq!(setup(&root, false)["status"], "preview");
    assert!(!root.join("exitbind.json").exists());
    let applied = setup(&root, true);
    assert_eq!(applied["status"], "applied");
    let config_before = fs::read(root.join("exitbind.json")).unwrap();
    let config: Value = serde_json::from_slice(&config_before).unwrap();
    assert_eq!(config["agents"]["worker"]["write"], json!(["src"]));
    assert_eq!(config["agents"]["reviewer"]["write"], json!([]));
    assert_eq!(setup(&root, true)["status"], "unchanged");
    assert_eq!(fs::read(root.join("exitbind.json")).unwrap(), config_before);
    let argv = applied["next"]["argv"][0].as_array().unwrap();
    assert!(argv.contains(&json!("--detail")));
    let mut command = Command::new(argv[0].as_str().unwrap());
    support::git_topology::isolate(&mut command);
    let started = value(
        command
            .current_dir(std::env::temp_dir())
            .args(argv[1..].iter().map(|v| v.as_str().unwrap()))
            .output()
            .unwrap(),
    );
    assert_eq!(started["kind"], "work_detail");
    assert_eq!(started["complete"], true);
    assert_eq!(started["valid"], true);
    assert_eq!(started["creation"]["effect"], "recorded");
    assert_eq!(started["recipient"]["role"], "lead");
    assert_eq!(started["actionForms"]["state"], "initial_scope");
    assert!(started["recipientContext"]["profile"]["content"]
        .as_str()
        .unwrap()
        .contains("Own the accepted goal"));
    assert!(started["recipientContext"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["content"]
            .as_str()
            .unwrap()
            .contains("reviewer boundaries")));
    for section in ["assignment", "evidence", "tasks"] {
        assert!(started["sections"][section].is_object());
    }
    let work = started["work"].as_str().unwrap();
    let config_path = root.join("exitbind.json");
    let current = value(invoke(
        &root,
        &[
            "work",
            "detail",
            work,
            "--json",
            "--config",
            config_path.to_str().unwrap(),
        ],
    ));
    let mut same = started.clone();
    same.as_object_mut().unwrap().remove("creation");
    assert_eq!(
        same, current,
        "start must use the existing complete detail owner"
    );
    let resumed = value(invoke(
        &root,
        &[
            "work",
            "resume",
            "--json",
            "--config",
            config_path.to_str().unwrap(),
        ],
    ));
    assert_eq!(
        resumed["work"], work,
        "lost reply recovers the existing Work"
    );
    let mut changed_config = config;
    changed_config["agents"]["reviewer"]["commands"] = json!(["true"]);
    fs::write(&config_path, serde_json::to_vec(&changed_config).unwrap()).unwrap();
    let stale = invoke(
        &root,
        &[
            "work",
            "expand",
            work,
            started["reference"].as_str().unwrap(),
            "--json",
            "--config",
            config_path.to_str().unwrap(),
        ],
    );
    assert!(
        !stale.status.success(),
        "old detail binding must refuse after approved configuration drift"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unrequested_start_keeps_its_historical_envelope() {
    let root = support::temp("start-compatible");
    support::git_topology::repository(&root);
    setup(&root, true);
    let v = value(invoke(
        &root,
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Existing caller",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
    ));
    assert!(v["next"].is_object());
    assert!(v["focus"].is_object());
    assert!(v.get("creation").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn protected_invalid_marker_is_diagnosed_before_preview_or_application() {
    let root = support::temp("start-invalid-marker");
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join("work.txt"), "Preserve useful work\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join(".git"), fs::Permissions::from_mode(0o500)).unwrap();
    }
    for apply in [false, true] {
        let mut args = vec!["setup", "--root", ".", "--mode", "portable", "--json"];
        if apply {
            args.push("--apply");
        }
        let out = invoke(&root, &args);
        assert!(!out.status.success());
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(text.contains("no files changed"), "{text}");
        assert!(text.contains("Git topology preflight failed"), "{text}");
        assert!(!root.join("exitbind.json").exists());
        assert_eq!(
            fs::read_to_string(root.join("work.txt")).unwrap(),
            "Preserve useful work\n"
        );
        assert_eq!(fs::read_dir(root.join(".git")).unwrap().count(), 0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join(".git"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}

//! R55 F2 (FR55-02): a configuration grown after `setup --lessons-from`
//! (worker `phaseGuidance`) has one typed refusal that names the kept
//! stamped copy and the exact `memory revalidate` route; run unedited with a
//! reason it records the change, and the worker then receives the body. A
//! lesson-authority change (`memoryWrite`) still refuses.
use super::lesson_carry::{accept, previous_project, repository, Root};
use super::support;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Output};

pub(super) fn exitbind(root: &Path, args: &[&str]) -> Output {
    support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap()
}

pub(super) fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(super) fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) const BEGIN: [&str; 11] = [
    "work",
    "begin",
    "change",
    "--goal",
    "Carry the reviewed lessons",
    "--check-command",
    "true",
    "--review-policy",
    "required",
    "--config",
    "exitbind.json",
];

pub(super) fn set_config(root: &Path, edit: impl FnOnce(&mut Value)) {
    let path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut config);
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

/// A fresh project set up with `--lessons-from` a previous project holding
/// one accepted lesson; returns both roots and the kept stamped copy.
pub(super) fn carried(label: &str) -> (Root, Root, String) {
    let previous = previous_project();
    accept(
        &previous.0,
        "carry-me",
        "Trace every delivery consumer.",
        "2100-01-01T00:00:00Z",
    );
    let fresh = repository(label);
    let root = fresh.0.as_path();
    let from = previous.0.join("exitbind.json");
    let applied = exitbind(
        root,
        &[
            "setup",
            "--json",
            "--apply",
            "--mode",
            "portable",
            "--root",
            root.to_str().unwrap(),
            "--scope",
            "worker",
            "--observe",
            "notes",
            "--write",
            "notes",
            "--commands",
            "true",
            "--check-command",
            "true",
            "--goal",
            "Carry the reviewed lessons",
            "--review-policy",
            "required",
            "--hosts",
            "claude",
            "--skip-skills",
            "--lessons-from",
            from.to_str().unwrap(),
        ],
    );
    assert!(applied.status.success(), "{}", text(&applied));
    let report: Value = serde_json::from_slice(&applied.stdout).unwrap();
    // Setup keeps the exact bytes it stamped the carried events with.
    let copy = report["lessons"]["stampedConfig"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        fs::read(&copy).unwrap(),
        fs::read(root.join("exitbind.json")).unwrap()
    );
    (previous, fresh, copy)
}

/// Run an emitted `memory revalidate` argv unedited with only REASON filled.
pub(super) fn run_route(root: &Path, argv: &[String], reason: &str) -> Output {
    let filled = argv
        .iter()
        .map(|item| {
            if item == "<REASON>" {
                reason.to_owned()
            } else {
                item.clone()
            }
        })
        .collect::<Vec<_>>();
    support::git_topology::command(&filled[0])
        .current_dir(root)
        .args(&filled[1..])
        .output()
        .unwrap()
}

#[test]
fn a_phase_guidance_change_after_lesson_carry_names_its_revalidate_route_and_delivers() {
    let (_previous, fresh, copy) = carried("changed-after-binding");
    let root = fresh.0.as_path();
    let stamped_sha = sha(&fs::read(&copy).unwrap());

    // The Lead adds worker phase guidance in place, as R54 and R55 did.
    fs::create_dir_all(root.join("guidance")).unwrap();
    fs::write(
        root.join("guidance/triage.md"),
        "Reproduce only the failed tests first.\n",
    )
    .unwrap();
    set_config(root, |config| {
        config["agents"]["worker"]["phaseGuidance"] =
            json!([{"id": "triage", "phase": "test", "path": "guidance/triage.md"}]);
    });
    let current_sha = sha(&fs::read(root.join("exitbind.json")).unwrap());

    // The first read that reaches the binding refuses with the typed refusal,
    // the same facts in text and --json.
    let mut detail = BEGIN.to_vec();
    detail.push("--detail");
    let refused = exitbind(root, &detail);
    assert!(!refused.status.success(), "{}", text(&refused));
    let stderr = String::from_utf8_lossy(&refused.stderr).into_owned();
    for expected in [
        "configuration changed since memory event",
        "memory_configuration_changed",
        stamped_sha.as_str(),
        current_sha.as_str(),
        copy.as_str(),
        " memory revalidate lead ",
    ] {
        assert!(stderr.contains(expected), "{expected}: {stderr}");
    }
    let mut json_args = detail.clone();
    json_args.push("--json");
    let machine = exitbind(root, &json_args);
    assert!(!machine.status.success());
    let machine: Value = serde_json::from_slice(&machine.stdout).unwrap();
    assert_eq!(machine["classification"], "memory_configuration_changed");
    assert_eq!(machine["boundSha256"], stamped_sha.as_str());
    assert_eq!(machine["currentSha256"], current_sha.as_str());
    assert_eq!(machine["keptCopy"], copy.as_str());
    assert_eq!(machine["nextAction"]["fill"], json!({"REASON": "--reason"}));
    let argv = machine["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(argv[0], env!("CARGO_BIN_EXE_exitbind"));
    assert_eq!(&argv[1..3], ["memory", "revalidate"]);

    // Run unedited with only the reason filled.
    let revalidated = run_route(root, &argv, "Reviewed read-only phase guidance");
    assert!(revalidated.status.success(), "{}", text(&revalidated));
    let event: Value = serde_json::from_slice(&revalidated.stdout).unwrap();
    assert_eq!(event["status"], "revalidated", "{event}");
    assert_eq!(
        event["revalidation"]["differences"],
        json!([{"agent": "worker", "field": "phaseGuidance", "before": null,
            "after": [{"id": "triage", "phase": "test", "path": "guidance/triage.md"}]}])
    );

    // The Work now begins and the worker's action view delivers both bodies.
    let begun = exitbind(root, &BEGIN);
    assert!(begun.status.success(), "{}", text(&begun));
    let begun: Value = serde_json::from_slice(&begun.stdout).unwrap();
    let work = begun["work"].as_str().unwrap();
    let scoped = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args([
            "work",
            "return",
            work,
            begun["next"]["assignment"].as_str().unwrap(),
            "--outcome",
            "scoped",
            "--config",
            "exitbind.json",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"bounded fixture scope\n")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(scoped.status.success(), "{}", text(&scoped));
    let action = exitbind(
        root,
        &[
            "work",
            "detail",
            work,
            "--view",
            "action",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(action.status.success(), "{}", text(&action));
    let action = text(&action);
    assert!(
        action.contains("Reproduce only the failed tests first."),
        "{action}"
    );
    assert!(
        action.contains("Trace every delivery consumer."),
        "{action}"
    );

    // A lesson-authority change is still refused, now with its typed route.
    let reviewed = fs::read(root.join("exitbind.json")).unwrap();
    let kept = root.join("reviewed.json");
    fs::write(&kept, &reviewed).unwrap();
    set_config(root, |config| {
        config["agents"]["worker"]["memoryWrite"] = json!(["project-lessons.v1"]);
    });
    let ledger = argv[4].clone();
    let authority = exitbind(
        root,
        &[
            "memory",
            "revalidate",
            "lead",
            &ledger,
            "--from-config",
            kept.to_str().unwrap(),
            "--reason",
            "widen writes",
            "--apply",
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(!authority.status.success(), "{}", text(&authority));
    let authority: Value = serde_json::from_slice(&authority.stdout).unwrap();
    assert_eq!(
        authority["classification"], "lesson_authority_changed",
        "{authority}"
    );
    assert_eq!(authority["boundSha256"], sha(&reviewed).as_str());
    assert_eq!(authority["nextAction"]["readOnly"], true, "{authority}");
}

//! Exercise the real typed activation entry, including its unconfigured path.
#![cfg(unix)]

mod support;

use serde_json::Value;
use std::{fs, process::Command};

fn call(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn classify(
    root: &std::path::Path,
    material_consequence: bool,
    promotion_required: bool,
    configured: bool,
) -> Value {
    let material = material_consequence.to_string();
    let promotion = promotion_required.to_string();
    let mut args = vec![
        "work",
        "classify",
        "--material-consequence",
        &material,
        "--promotion-required",
        &promotion,
        "--json",
    ];
    let config = root.join("exitbind.json");
    let config_text = config.to_str().unwrap();
    if configured {
        args.extend(["--config", config_text]);
    }
    let output = call(root, &args);
    assert!(
        output.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn classify_uses_consequence_and_promotion_with_real_cli_entry() {
    let root = support::temp("activation-classify");
    for index in 0..20 {
        fs::write(root.join(format!("prototype-{index}.txt")), b"disposable\n").unwrap();
    }
    fs::write(root.join("AGENTS.md"), b"Use concise updates.\n").unwrap();
    fs::write(root.join("settings.json"), b"{}\n").unwrap();
    let before = fs::read_dir(&root).unwrap().count();
    let direct = classify(&root, false, false, false);
    assert_eq!(direct["assessment"]["activation"], "direct");
    assert_eq!(direct["assessment"]["reason"], "no_material_consequence");
    assert_eq!(direct["availability"]["configuration"], false);
    assert_eq!(fs::read_dir(&root).unwrap().count(), before);

    fs::write(
        root.join("authentication-boundary.rs"),
        b"one consequential line\n",
    )
    .unwrap();
    let blocked = classify(&root, true, false, false);
    assert_eq!(blocked["assessment"]["activation"], "blocked");
    assert_eq!(blocked["assessment"]["reason"], "governance_unavailable");
    assert_eq!(blocked["provenance"], "host_reported");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn classify_requires_both_explicit_typed_facts() {
    let root = support::temp("activation-classify-missing");
    let output = call(
        &root,
        &[
            "work",
            "classify",
            "--material-consequence",
            "true",
            "--json",
        ],
    );
    assert!(!output.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostic.contains("promotion-required"), "{diagnostic}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn classify_reports_governance_available_after_initialization() {
    let root = support::temp("activation-classify-configured");
    let init = call(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(init.status.success(), "{init:?}");
    let direct = classify(&root, false, false, true);
    assert_eq!(direct["assessment"]["activation"], "direct");
    assert_eq!(
        fs::read_dir(root.join(".exitbind/runs")).unwrap().count(),
        0
    );
    let consequence = classify(&root, true, false, true);
    assert_eq!(consequence["assessment"]["activation"], "governed");
    assert_eq!(consequence["assessment"]["reason"], "governance_available");
    let promotion = classify(&root, false, true, true);
    assert_eq!(promotion["assessment"]["activation"], "governed");
    assert_eq!(promotion["facts"]["promotionRequired"], true);
    fs::remove_dir_all(root).unwrap();
}

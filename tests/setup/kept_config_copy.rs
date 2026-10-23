//! R55 F2a (FR55-09, FR55-08): a project set up before the stamped copy
//! existed (0.31.20: carried lessons, no copy). A product-driven
//! configuration change goes through the one configuration writer, which
//! keeps the stamped bytes it replaces: after a later compatible change
//! `work begin` refuses once with one route naming the kept copy and every
//! ledger, and that route runs unedited. A hand edit the product never saw still refuses once, with one
//! route covering every ledger and the `<PREVIOUS_CONFIG>` slot.
use super::changed_after_binding::{carried_lessons, exitbind, run_route, sha, text, BEGIN};
use super::lesson_carry::Root;
use serde_json::{json, Value};
use std::{fs, path::Path};

const LESSONS: [(&str, &str); 3] = [
    ("carry-one", "Trace every delivery consumer."),
    ("carry-two", "Run the affected feature tests."),
    ("carry-three", "Keep one cargo process at a time."),
];

/// The 0.31.20 state shape: lessons carried, no kept copy.
fn without_copy(label: &str) -> (Root, Root, String) {
    let (previous, fresh, copy) = carried_lessons(label, &LESSONS);
    let stamped = sha(&fs::read(&copy).unwrap());
    fs::remove_file(&copy).unwrap();
    (previous, fresh, stamped)
}

fn config_bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join("exitbind.json")).unwrap()
}

/// `work begin --detail --json` refused; returns the refusal and its route.
fn refused_begin(root: &Path) -> (Value, Vec<String>) {
    let mut detail = BEGIN.to_vec();
    detail.extend(["--detail", "--json"]);
    let output = exitbind(root, &detail);
    assert!(!output.status.success(), "{}", text(&output));
    let refused: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", text(&output)));
    assert_eq!(refused["classification"], "memory_configuration_changed");
    let route = refused["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(&route[1..3], ["memory", "revalidate"], "{route:?}");
    (refused, route)
}

/// The ledgers and the `--from-config` value a revalidate route names.
fn named(route: &[String]) -> (Vec<String>, String) {
    let end = route
        .iter()
        .position(|word| word == "--from-config")
        .unwrap();
    (route[4..end].to_vec(), route[end + 1].clone())
}

fn kept_copies(root: &Path) -> Vec<String> {
    let mut found = fs::read_dir(root.join(".exitbind/memory"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("stamped-config-"))
        .collect::<Vec<_>>();
    found.sort();
    found
}

/// Setup re-applied with another worker command: a product-driven change
/// that replaces exitbind.json through the one configuration writer.
fn setup_commands(root: &Path, command: &str) {
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
            command,
            "--check-command",
            "true",
            "--goal",
            "Carry the reviewed lessons",
            "--review-policy",
            "required",
            "--hosts",
            "claude",
            "--skip-skills",
        ],
    );
    assert!(applied.status.success(), "{}", text(&applied));
}

#[test]
fn a_product_configuration_change_keeps_the_stamped_bytes_and_refuses_once() {
    let (_previous, fresh, stamped) = without_copy("kept-config-product");
    let root = fresh.0.as_path();
    assert!(kept_copies(root).is_empty(), "0.31.20 shape has no copy");
    let stamped_bytes = config_bytes(root);
    assert_eq!(sha(&stamped_bytes), stamped);

    // `project agents --apply` projects native profiles; it is no
    // configuration writer, so the stamped bytes stay current.
    let agents = exitbind(
        root,
        &[
            "project",
            "agents",
            "--apply",
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    assert_eq!(config_bytes(root), stamped_bytes, "{}", text(&agents));

    // A product-driven configuration change.
    setup_commands(root, "git status");
    assert_ne!(
        config_bytes(root),
        stamped_bytes,
        "setup changed the configuration"
    );
    let copies = kept_copies(root);
    assert_eq!(copies.len(), 1, "{copies:?}");
    let copy = root.join(".exitbind/memory").join(&copies[0]);
    assert_eq!(
        fs::read(&copy).unwrap(),
        stamped_bytes,
        "the replaced bytes, exactly"
    );

    // Keeping is idempotent: back to the stamped bytes and away again, the
    // writer finds the one existing copy.
    setup_commands(root, "true");
    assert_eq!(config_bytes(root), stamped_bytes);
    setup_commands(root, "git status");
    assert_eq!(kept_copies(root), copies);

    // Back to the stamped bytes through the writer, then the Lead's named
    // agent: no product writer makes a lesson-compatible change today, so
    // the compatible difference is the owner's, against the kept bytes.
    setup_commands(root, "true");
    assert_eq!(config_bytes(root), stamped_bytes);
    let mut config: Value = serde_json::from_slice(&stamped_bytes).unwrap();
    config["agents"]["worker"]["nativeName"] = json!("sonic");
    fs::write(
        root.join("exitbind.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();

    // One refusal names the kept copy and all three ledgers.
    let (refused, route) = refused_begin(root);
    assert_eq!(refused["boundSha256"], stamped.as_str());
    let (ledgers, from) = named(&route);
    assert_eq!(ledgers.len(), LESSONS.len(), "{route:?}");
    assert_eq!(
        fs::canonicalize(from).unwrap(),
        fs::canonicalize(&copy).unwrap()
    );

    // The route runs unedited (REASON filled) → every ledger revalidated.
    let revalidated = run_route(root, &route, "Owner named the worker agent");
    assert!(revalidated.status.success(), "{}", text(&revalidated));
    let batch: Value = serde_json::from_slice(&revalidated.stdout).unwrap();
    assert_eq!(batch["status"], "revalidated", "{batch}");
    assert_eq!(batch["ledgers"].as_array().unwrap().len(), LESSONS.len());
    let begun = exitbind(root, &BEGIN);
    assert!(begun.status.success(), "{}", text(&begun));
    // The copy is never read as current configuration.
    assert_ne!(config_bytes(root), fs::read(&copy).unwrap());
}

#[test]
fn a_hand_edit_refuses_once_with_every_ledger_and_the_previous_config_slot() {
    let (_previous, fresh, stamped) = without_copy("kept-config-hand");
    let root = fresh.0.as_path();
    let stamped_bytes = config_bytes(root);
    let backup = root.join("before-hand-edit.json");
    fs::write(&backup, &stamped_bytes).unwrap();

    // The owner edits the configuration by hand; no product writer saw it.
    let path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&stamped_bytes).unwrap();
    config["agents"]["worker"]["nativeName"] = json!("sonic");
    fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    assert!(kept_copies(root).is_empty(), "a hand edit keeps nothing");

    let (refused, route) = refused_begin(root);
    assert_eq!(refused["boundSha256"], stamped.as_str());
    let (ledgers, from) = named(&route);
    assert_eq!(ledgers.len(), LESSONS.len(), "{route:?}");
    assert_eq!(from, "<PREVIOUS_CONFIG>", "{route:?}");

    // The owner's backup fills the slot → every ledger revalidated at once.
    let filled = route
        .iter()
        .map(|word| {
            if word == "<PREVIOUS_CONFIG>" {
                backup.to_str().unwrap().to_owned()
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>();
    let revalidated = run_route(root, &filled, "Owner named the worker agent");
    assert!(revalidated.status.success(), "{}", text(&revalidated));
    let batch: Value = serde_json::from_slice(&revalidated.stdout).unwrap();
    assert_eq!(batch["ledgers"].as_array().unwrap().len(), LESSONS.len());
    let begun = exitbind(root, &BEGIN);
    assert!(begun.status.success(), "{}", text(&begun));
}

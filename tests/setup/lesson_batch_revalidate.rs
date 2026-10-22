//! R55 FR55-08: with several carried lessons, `work begin` prints one
//! `memory revalidate` route covering every ledger stamped with the same kept
//! copy. The route checks every ledger before appending to any: a preview
//! shows each ledger's differences, one refusing ledger appends nothing, and
//! an interrupted run completes by running the same route again.
use super::changed_after_binding::{carried_lessons, exitbind, run_route, set_config, text, BEGIN};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn route_of(root: &Path) -> Vec<String> {
    let mut detail = BEGIN.to_vec();
    detail.push("--json");
    let refused = exitbind(root, &detail);
    assert!(!refused.status.success(), "{}", text(&refused));
    let value: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(value["classification"], "memory_configuration_changed");
    value["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

fn ledgers_in(route: &[String]) -> Vec<String> {
    let end = route
        .iter()
        .position(|word| word == "--from-config")
        .unwrap();
    route[4..end].to_vec()
}

/// The same route naming only `ledgers`.
fn with_ledgers(route: &[String], ledgers: &[String]) -> Vec<String> {
    let end = route
        .iter()
        .position(|word| word == "--from-config")
        .unwrap();
    let mut argv = route[..4].to_vec();
    argv.extend(ledgers.iter().cloned());
    argv.extend(route[end..].iter().cloned());
    argv
}

const REASON: &str = "Reviewed the worker's native name; rights unchanged";

#[test]
fn one_route_revalidates_every_ledger_or_none_and_a_rerun_completes() {
    let (_previous, fresh, _copy) = carried_lessons(
        "lesson-batch-revalidate",
        &[
            ("first-lesson", "Trace every delivery consumer."),
            ("second-lesson", "Run the affected feature tests."),
        ],
    );
    let root = fresh.0.as_path();
    set_config(root, |config| {
        config["agents"]["worker"]["nativeName"] = json!("sonic");
    });
    let route = route_of(root);
    let ledgers = ledgers_in(&route);
    assert_eq!(ledgers.len(), 2, "{route:?}");
    let bytes = |ledger: &str| fs::read(root.join(ledger)).unwrap();
    let originals = ledgers.iter().map(|l| bytes(l)).collect::<Vec<_>>();

    // Preview: per-ledger differences, nothing appended.
    let preview_argv = route
        .iter()
        .filter(|word| *word != "--apply")
        .cloned()
        .collect::<Vec<_>>();
    let preview = run_route(root, &preview_argv, REASON);
    assert!(preview.status.success(), "{}", text(&preview));
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["status"], "preview", "{preview}");
    let expected = json!([{"agent":"worker","field":"nativeName","before":null,"after":"sonic"}]);
    for (entry, ledger) in preview["ledgers"].as_array().unwrap().iter().zip(&ledgers) {
        assert_eq!(entry["ledger"], ledger.as_str());
        assert_eq!(entry["revalidation"]["differences"], expected, "{entry}");
    }
    for (ledger, original) in ledgers.iter().zip(&originals) {
        assert_eq!(&bytes(ledger), original);
    }

    // One refusing ledger (its lesson source changed): nothing is appended
    // to any ledger, and the refusal names that ledger.
    let inspected = exitbind(
        root,
        &[
            "memory",
            "inspect",
            &ledgers[1],
            "--json",
            "--config",
            "exitbind.json",
        ],
    );
    let inspected: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    let source = root.join(inspected["items"][0]["source"]["path"].as_str().unwrap());
    let source_bytes = fs::read(&source).unwrap();
    fs::write(&source, b"changed lesson source").unwrap();
    let refused = run_route(root, &route, REASON);
    assert!(!refused.status.success(), "{}", text(&refused));
    let message = text(&refused);
    assert!(message.contains("appended nothing"), "{message}");
    assert!(message.contains(ledgers[1].as_str()), "{message}");
    for (ledger, original) in ledgers.iter().zip(&originals) {
        assert_eq!(&bytes(ledger), original, "{ledger}");
    }
    fs::write(&source, source_bytes).unwrap();

    // Interrupted after the first ledger: rerunning the same route skips it
    // and completes the second.
    let partial = run_route(root, &with_ledgers(&route, &ledgers[..1]), REASON);
    assert!(partial.status.success(), "{}", text(&partial));
    assert_ne!(bytes(&ledgers[0]), originals[0]);
    assert_eq!(bytes(&ledgers[1]), originals[1]);
    let after_first = bytes(&ledgers[0]);
    let rerun = run_route(root, &route, REASON);
    assert!(rerun.status.success(), "{}", text(&rerun));
    let rerun: Value = serde_json::from_slice(&rerun.stdout).unwrap();
    assert_eq!(rerun["ledgers"][0]["status"], "already_current", "{rerun}");
    assert_eq!(rerun["ledgers"][1]["status"], "revalidated", "{rerun}");
    assert_eq!(bytes(&ledgers[0]), after_first);
    assert_ne!(bytes(&ledgers[1]), originals[1]);

    // `work begin` now passes.
    let begun = exitbind(root, &BEGIN);
    assert!(begun.status.success(), "{}", text(&begun));
}

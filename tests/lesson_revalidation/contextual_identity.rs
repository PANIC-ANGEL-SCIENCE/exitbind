//! R55 F4a: the contextual opt-in reads the same list of agent fields that
//! are not lesson authority as `memory revalidate`. A recipient's name or
//! profile changed together with the opt-in is recorded as a difference, for
//! project lessons and for specialist scopes; a runtime change still refuses.
// Shared with tests/contextual_memory.rs; this file uses part of it.
#[allow(dead_code)]
#[path = "../contextual_memory/fixture.rs"]
mod fixture;
use super::support;
use fixture::{Fixture, LEDGER};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;

fn revalidate(f: &Fixture, ledger: &str, apply: bool) -> std::process::Output {
    let mut args = vec![
        "memory",
        "revalidate",
        "lead",
        ledger,
        "--from-config",
        "prior.json",
        "--reason",
        "Lead reviewed the contextual opt-in and the worker's named agent.",
        "--json",
    ];
    if apply {
        args.push("--apply");
    }
    f.call(&args)
}

/// Opt in and rename one recipient; with `profile`, also repoint its profile
/// (refused for an actor on a specialist ledger: qualification evidence).
fn opt_in_with_named(f: &Fixture, agent: &str, profile: bool) -> Value {
    fs::copy(f.root.join("exitbind.json"), f.root.join("prior.json")).unwrap();
    fs::write(f.root.join("sonic.md"), "Sonic implements.\n").unwrap();
    let mut c = f.config();
    c["memory"]["contextual"] = json!({"version":1,"sourcePath":"current-task.json"});
    if profile {
        c["agents"][agent]["profile"] = json!("sonic.md");
    }
    c["agents"][agent]["nativeName"] = json!("sonic");
    c["agents"][agent]["displayName"] = json!("Sonic");
    f.save(&c);
    c
}

fn expected(prior: &Value, c: &Value, agent: &str) -> Value {
    let mut found = vec![json!({"field":"memory.contextual","before":null,
        "after":c["memory"]["contextual"]})];
    for field in ["profile", "displayName", "nativeName"] {
        let (before, after) = (&prior["agents"][agent][field], &c["agents"][agent][field]);
        if before != after {
            found.push(json!({"agent":agent,"field":field,"before":before,"after":after}));
        }
    }
    json!(found)
}

#[test]
fn contextual_opt_in_records_a_recipient_name_change_and_still_refuses_runtime() {
    let f = Fixture::new(false);
    f.accept_lesson();
    let prior = f.config();
    let c = opt_in_with_named(&f, "worker", true);
    assert_eq!(expected(&prior, &c, "worker").as_array().unwrap().len(), 4);
    let ledger = fs::read(f.root.join(LEDGER)).unwrap();
    let preview = revalidate(&f, LEDGER, false);
    assert!(preview.status.success(), "{preview:?}");
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(
        preview["revalidation"]["differences"],
        expected(&prior, &c, "worker")
    );
    let mut runtime = c.clone();
    runtime["agents"]["worker"]["runtime"] = json!({"model": "other-model"});
    f.save(&runtime);
    assert!(!revalidate(&f, LEDGER, true).status.success());
    assert_eq!(fs::read(f.root.join(LEDGER)).unwrap(), ledger);
    f.save(&c);
    let applied = revalidate(&f, LEDGER, true);
    assert!(applied.status.success(), "{applied:?}");
    let applied: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(applied["effect"], "recorded");
    f.ok(&["memory", "inspect", LEDGER, "--json"]);
}

#[test]
fn specialist_contextual_evidence_accepts_recorded_identity_differences() {
    let f = Fixture::specialist_mode(false);
    let ledger = ".exitbind/memory/specialist.jsonl";
    let prior = f.config();
    // The reviewer's profile is specialist qualification evidence and stays exact.
    let c = opt_in_with_named(&f, "reviewer", false);
    assert_eq!(
        expected(&prior, &c, "reviewer").as_array().unwrap().len(),
        3
    );
    let applied = revalidate(&f, ledger, true);
    assert!(applied.status.success(), "{applied:?}");
    let applied: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(applied["effect"], "recorded");
    assert_eq!(
        applied["event"]["revalidation"]["differences"],
        expected(&prior, &c, "reviewer")
    );
    // Reading the ledger back validates the recorded evidence.
    let inspected = f.ok(&["memory", "inspect", ledger, "--json"]);
    let events = inspected["events"].as_array().unwrap();
    assert_eq!(
        events.last().unwrap()["action"],
        "revalidate",
        "{inspected}"
    );
}

#[test]
fn a_specialist_actor_profile_change_with_the_opt_in_refuses_before_any_append() {
    let f = Fixture::specialist_mode(false);
    let ledger = ".exitbind/memory/specialist.jsonl";
    let before = fs::read(f.root.join(ledger)).unwrap();
    for actor in ["reviewer", "record-specialist"] {
        opt_in_with_named(&f, actor, true);
        let refused = revalidate(&f, ledger, true);
        assert!(!refused.status.success(), "{actor}: {refused:?}");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&refused.stdout),
            String::from_utf8_lossy(&refused.stderr)
        );
        assert!(
            text.contains(&format!("specialist ledger actor '{actor}'")),
            "{actor}: {text}"
        );
        assert_eq!(fs::read(f.root.join(ledger)).unwrap(), before, "{actor}");
        // Restore the prior configuration for the next actor.
        fs::copy(f.root.join("prior.json"), f.root.join("exitbind.json")).unwrap();
    }
    // The ledger still delivers to the specialist.
    let ok = f.ok(&[
        "memory",
        "candidates",
        "record-specialist",
        "--task",
        fixture::SPECIALIST_TASK,
        "--host",
        "codex",
        "--json",
    ]);
    assert!(ok.is_object(), "{ok}");
}

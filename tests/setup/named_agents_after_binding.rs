//! R55 F4a: the F4 probe as a fixture. After `setup --lessons-from`, the
//! Lead points the worker and reviewer at named agent profiles (`profile` +
//! `nativeName` → `sonic`/`meow`). A recipient's name is not lesson
//! authority: `work begin` refuses with the typed route, the route records
//! the differences, and `work next` then shows the named agent and its
//! `profileSha256`. A seeded `memoryWrite`, `runtime` or `retention` change
//! in the same project still refuses `lesson_authority_changed`.
use super::changed_after_binding::{
    carried_lessons, exitbind, run_route, set_config, sha, text, BEGIN,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

const SONIC: &str = "your.soulmate/agents/sonic/profile.md";
const MEOW: &str = "your.soulmate/agents/meow/profile.md";

fn json_of(root: &Path, args: &[&str]) -> (bool, Value) {
    let output = exitbind(root, args);
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{args:?}: {}", text(&output)));
    (output.status.success(), value)
}

fn argv(value: &Value) -> Vec<String> {
    value["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn named_agent_profiles_after_lesson_carry_revalidate_and_reach_work_next() {
    // Two carried lessons, each in its own ledger: one route covers both.
    let (_previous, fresh, copy) = carried_lessons(
        "named-agents-after-binding",
        &[
            ("carry-me", "Trace every delivery consumer."),
            ("carry-two", "Run the affected feature tests."),
        ],
    );
    let root = fresh.0.as_path();
    let stamped_sha = sha(&fs::read(copy).unwrap());
    let reviewer = {
        let config: Value =
            serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
        config["agents"]["reviewer"].is_object()
    };
    assert!(reviewer, "setup with required review configures a reviewer");

    // The owner's named profiles are copied under ControlRoot.
    for (path, body) in [(SONIC, "Sonic implements.\n"), (MEOW, "Meow reviews.\n")] {
        fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        fs::write(root.join(path), body).unwrap();
    }
    let before: Value =
        serde_json::from_slice(&fs::read(root.join("exitbind.json")).unwrap()).unwrap();
    set_config(root, |config| {
        config["agents"]["worker"]["profile"] = json!(SONIC);
        config["agents"]["worker"]["nativeName"] = json!("sonic");
        config["agents"]["reviewer"]["profile"] = json!(MEOW);
        config["agents"]["reviewer"]["nativeName"] = json!("meow");
    });
    let current_sha = sha(&fs::read(root.join("exitbind.json")).unwrap());

    // `work begin` → the typed refusal with the revalidate route.
    let mut detail = BEGIN.to_vec();
    detail.extend(["--detail", "--json"]);
    let (ok, refused) = json_of(root, &detail);
    assert!(!ok, "{refused}");
    assert_eq!(refused["classification"], "memory_configuration_changed");
    assert_eq!(refused["boundSha256"], stamped_sha.as_str());
    assert_eq!(refused["currentSha256"], current_sha.as_str());
    let route = argv(&refused);
    assert_eq!(&route[1..3], ["memory", "revalidate"]);
    let end = route
        .iter()
        .position(|word| word == "--from-config")
        .unwrap();
    let ledgers = route[4..end].to_vec();
    assert_eq!(ledgers.len(), 2, "{route:?}");
    let ledger = ledgers[0].clone();

    // The one route → each ledger revalidated, each identity difference recorded.
    let revalidated = run_route(root, &route, "Owner's named agents; rights unchanged");
    assert!(revalidated.status.success(), "{}", text(&revalidated));
    let batch: Value = serde_json::from_slice(&revalidated.stdout).unwrap();
    assert_eq!(batch["status"], "revalidated", "{batch}");
    let events = batch["ledgers"].as_array().unwrap();
    assert_eq!(events.len(), 2, "{batch}");
    let was = |agent: &str, field: &str| before["agents"][agent][field].clone();
    for event in events {
        assert_eq!(event["status"], "revalidated", "{event}");
        assert_eq!(
            event["event"]["revalidation"]["priorLedgerHeadSha256"],
            event["event"]["previousEventSha256"]
        );
        assert_eq!(
            event["revalidation"]["differences"],
            json!([
                {"agent":"reviewer","field":"profile","before":was("reviewer","profile"),"after":MEOW},
                {"agent":"reviewer","field":"nativeName","before":was("reviewer","nativeName"),"after":"meow"},
                {"agent":"worker","field":"profile","before":was("worker","profile"),"after":SONIC},
                {"agent":"worker","field":"nativeName","before":was("worker","nativeName"),"after":"sonic"},
            ])
        );
    }

    // `work begin` passes after that one route; `work next` → the named
    // agent and its profileSha256.
    let (ok, begun) = json_of(root, &BEGIN);
    assert!(ok, "{begun}");
    let work = begun["work"].as_str().unwrap().to_owned();
    let scoped = support_scope(root, &work, begun["next"]["assignment"].as_str().unwrap());
    assert!(scoped.status.success(), "{}", text(&scoped));
    let (ok, next) = json_of(
        root,
        &[
            "work",
            "next",
            &work,
            "--json",
            "--full",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(ok, "{next}");
    let packet = &next["next"]["packet"];
    assert_eq!(packet["agent"], "worker", "{next}");
    assert_eq!(packet["nativeTaskName"], "sonic", "{next}");
    assert_eq!(
        packet["profileSha256"],
        sha(b"Sonic implements.\n").as_str(),
        "{next}"
    );

    // Authority stays exact: each seeded change refuses lesson_authority_changed.
    let reviewed = fs::read(root.join("exitbind.json")).unwrap();
    let kept = root.join("reviewed.json");
    fs::write(&kept, &reviewed).unwrap();
    for (field, value) in [
        ("memoryWrite", json!(["project-lessons.v1"])),
        ("runtime", json!({"model": "other-model"})),
        ("retention", json!("until-revoked")),
    ] {
        set_config(root, |config| {
            config["agents"]["worker"][field] = value.clone()
        });
        let (ok, authority) = json_of(
            root,
            &[
                "memory",
                "revalidate",
                "lead",
                &ledger,
                "--from-config",
                kept.to_str().unwrap(),
                "--reason",
                "seeded authority change",
                "--apply",
                "--json",
                "--config",
                "exitbind.json",
            ],
        );
        assert!(!ok, "{field}: {authority}");
        assert_eq!(
            authority["classification"], "lesson_authority_changed",
            "{field}: {authority}"
        );
        assert_eq!(authority["boundSha256"], sha(&reviewed).as_str());
        fs::write(root.join("exitbind.json"), &reviewed).unwrap();
    }
}

fn support_scope(root: &Path, work: &str, assignment: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args([
            "work",
            "return",
            work,
            assignment,
            "--outcome",
            "scoped",
            "--config",
            "exitbind.json",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"bounded fixture scope\n")
        .unwrap();
    child.wait_with_output().unwrap()
}

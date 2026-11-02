//! Verified same-host Goal projection for host callbacks.
use super::{run, text, Fixture, CHILD, OTHER, PROFILE_BODY, SESSION};
use serde_json::{json, Value};
use std::{fs, process::Output};

#[test]
fn selector_free_same_host_delivery_matches_owner_goal_and_returns_after_permit() {
    let fixture = Fixture::new_with_check_and_write(
        "r60-same-host-delivery",
        "true --mode strict",
        Some("fixture.txt"),
    );
    fs::write(
        fixture.product.join("AGENTS.md"),
        "Preserve the fixture flag.\n",
    )
    .unwrap();
    fixture.prepare_sonic();
    let owner = fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
    let payload = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic"});
    let argv = [env!("CARGO_BIN_EXE_exitbind").to_owned(), "hook-run".into()];
    let output = run(
        &argv,
        &fixture.product,
        &fixture.store,
        None,
        payload.to_string().as_bytes(),
    );
    assert!(output.status.success(), "{}", text(&output));
    let hook: Value = serde_json::from_slice(&output.stdout).unwrap();
    let start = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        output.stdout.len() <= 16 * 1024,
        "host bytes: {}",
        output.stdout.len()
    );
    assert!(start.contains("Preserve the fixture flag."), "{start}");
    let packet = start
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("complete assignment unavailable, {} bytes: {start}", output.stdout.len()));
    let packet: Value = serde_json::from_str(packet).unwrap();
    assert_eq!(packet["checkCommand"], "true --mode strict");
    assert_eq!(
        packet["checkPreparation"]["workingDirectory"],
        json!(fixture.product)
    );
    assert_eq!(packet["currentAction"]["binding"], owner["binding"]);
    assert_eq!(
        packet["goal"],
        owner["sections"]["assignment"]["assignment"]["goal"]
    );
    assert_eq!(packet["taskGoal"], owner["sections"]["tasks"]["goal"]);
    assert_ne!(packet["goal"], packet["taskGoal"]);
    if owner["sections"]["tasks"]["tasks"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        assert!(packet["tasks"].is_null());
    } else {
        assert_eq!(packet["tasks"], owner["sections"]["tasks"]["tasks"]);
    }
    assert_eq!(
        packet["taskGoal"],
        "A same-host lens needs no cross-host protocol"
    );
    assert_eq!(
        packet["goal"],
        "Spawn sonic with its lenses from the Lead session"
    );
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "presented"
    );
    eprintln!(
        "same-host fixture hook stdout: {} bytes; complete delivery: true",
        output.stdout.len()
    );
    let form = &packet["currentAction"]["beforeEditing"];
    assert_eq!(form["required"], true);
    assert_eq!(*form, owner["actionForms"]["beforeEditing"]);
    let mut permit = form["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for word in &mut permit {
        if word == "<OPERATION>" {
            *word = "Update fixture".into();
        }
    }
    let granted = run(
        &permit,
        &fixture.product,
        &fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(granted.status.success(), "{}", text(&granted));
    let granted: Value = serde_json::from_slice(&granted.stdout).unwrap();
    assert_eq!(
        granted["childDeliveryContinuity"]["state"], "current",
        "{granted}"
    );
    fs::write(fixture.product.join("fixture.txt"), "permitted edit\n").unwrap();
    fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"Edited fixture","reason":""}"#}));
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_string(), "{intent}");
    assert!(intent["rawObservation"].is_null(), "{intent}");
}

#[test]
fn legacy_exact_delivery_remains_eligible_after_its_matching_permit() {
    let fixture = Fixture::new_with_check_and_write(
        "r60-same-host-legacy-delivery",
        "true",
        Some("fixture.txt"),
    );
    fixture.prepare_sonic();
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    let packet = start
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next()).unwrap();
    let packet: Value = serde_json::from_str(packet).unwrap();
    let path = fixture.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    // Model the old producer's record, which predates all three fields.
    assert!(saved["intents"][0]
        .as_object_mut()
        .unwrap()
        .remove("selectedDeliveryProtocol")
        .is_some());
    let delivery = saved["intents"][0]["assignmentDelivery"]
        .as_object_mut()
        .unwrap();
    assert!(delivery.remove("actorFactsSha256").is_some());
    assert!(delivery.remove("actorContractSha256").is_some());
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut permit = packet["currentAction"]["beforeEditing"]["command"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for word in &mut permit {
        if word == "<OPERATION>" {
            *word = "Update fixture".into();
        }
    }
    let granted = run(
        &permit,
        &fixture.product,
        &fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(granted.status.success(), "{}", text(&granted));
    let granted: Value = serde_json::from_slice(&granted.stdout).unwrap();
    assert_eq!(granted["childDeliveryContinuity"]["state"], "current");
    fs::write(fixture.product.join("fixture.txt"), "permitted edit\n").unwrap();
    fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"Edited fixture","reason":""}"#}));
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_string(), "{intent}");
    assert!(intent["rawObservation"].is_null(), "{intent}");
}

#[test]
fn selected_delivery_refuses_transformed_or_incomplete_required_instructions() {
    let long_phase = "required phase instruction\n".repeat(200);
    let redacted_profile = format!(
        "Never edit {}\n",
        ["/", "home", "/", "account", "/project"].concat()
    );
    let scenarios = [
        (
            "phase-complete",
            PROFILE_BODY,
            Some("phase.md"),
            Some("retain this phase instruction\n"),
            true,
        ),
        (
            "phase-overflow",
            PROFILE_BODY,
            Some("phase.md"),
            Some(long_phase.as_str()),
            false,
        ),
        (
            "phase-unavailable",
            PROFILE_BODY,
            Some("missing-phase.md"),
            None,
            false,
        ),
        (
            "profile-redacted",
            redacted_profile.as_str(),
            None,
            None,
            false,
        ),
    ];
    for (name, profile, path, body, expected) in scenarios {
        let phase = path.map(|path| (path, body));
        let fixture = Fixture::new_with_material(name, "true", None, profile, phase);
        fixture.prepare_sonic();
        let start = fixture
            .hook(
                json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
            )
            .unwrap();
        let intent = &fixture.intents().unwrap()[0];
        assert_eq!(
            intent["assignmentDelivery"]["state"] == "presented",
            expected,
            "{name}: {start}"
        );
        if expected {
            assert!(start.contains("retain this phase instruction"), "{start}");
            continue;
        }
        assert!(
            start.contains("no automatic return") || start.contains("cannot receive credit"),
            "{start}"
        );
        assert_eq!(intent["state"], "claimed", "{name}: {intent}");
        assert_eq!(intent["assignmentDelivery"]["state"], "unavailable");
        let unavailable = intent["assignmentDelivery"].clone();
        let duplicate = fixture
            .hook(
                json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
            )
            .unwrap();
        assert!(
            !duplicate.contains("Automatic Work return. ENTIRE final message"),
            "{duplicate}"
        );
        assert_eq!(
            fixture.intents().unwrap()[0]["assignmentDelivery"],
            unavailable
        );
        let message = r#"{"outcome":"completed","summary":"uncredited","reason":""}"#;
        fixture.hook(
            json!({"hook_event_name":"SubagentStop","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic",
            "last_assistant_message":message}),
        );
        let retained = &fixture.intents().unwrap()[0];
        assert!(
            retained["returnEventSha256"].is_null(),
            "{name}: {retained}"
        );
        assert!(
            retained.get("pendingReturn").is_none(),
            "{name}: {retained}"
        );
        assert!(retained["rawObservation"].is_object(), "{name}: {retained}");
        let raw = fixture.value(
            Some(SESSION),
            &[
                "work",
                "child",
                "context",
                &fixture.work,
                retained["id"].as_str().unwrap(),
                "--view",
                "result",
            ],
        );
        assert_eq!(raw["resultText"], message);
        assert_eq!(raw["credit"], "none");
    }
}

#[test]
fn duplicate_start_cannot_upgrade_an_unavailable_delivery_even_when_renderable() {
    let fixture = Fixture::new("r60-unavailable-duplicate");
    fixture.prepare_sonic();
    let first = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(
        first.contains("Automatic Work return. ENTIRE final message"),
        "{first}"
    );
    let path = fixture.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved["intents"][0]["assignmentDelivery"]["state"],
        "presented"
    );
    // Model an earlier unavailable observation while all current actor facts
    // remain renderable; the second callback cannot create delivery credit.
    saved["intents"][0]["assignmentDelivery"] = json!({"state":"unavailable","binding":null});
    let unavailable = saved["intents"][0]["assignmentDelivery"].clone();
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let payload = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic"});
    let output = hook_with_selector(&fixture, payload, "CLAUDE_CODE_SESSION_ID", SESSION);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("cannot receive credit"), "{context}");
    assert!(!context.contains("Automatic Work return. ENTIRE final message"));
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"],
        unavailable
    );
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"completed","summary":"uncredited","reason":""}"#}),
    );
    let retained = &fixture.intents().unwrap()[0];
    assert!(retained["rawObservation"].is_object(), "{retained}");
    assert!(retained["returnEventSha256"].is_null(), "{retained}");
    assert!(retained.get("pendingReturn").is_none(), "{retained}");
}

fn hook_with_selector(fixture: &Fixture, payload: Value, selector: &str, identity: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut command = super::super::support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&fixture.product)
        .arg("hook-run")
        .env("EXITBIND_SESSION_GOALS_DIR", &fixture.store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env(selector, identity)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn same_host_callback_rejects_conflicting_native_selectors_without_credit() {
    let fixture = Fixture::new("r60-same-host-selector-conflict");
    fixture.prepare_sonic();
    fixture.hook(json!({"hook_event_name":"SubagentStart","session_id":OTHER,
        "agent_id":CHILD,"agent_type":"sonic"}));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "prepared");
    let start = json!({"hook_event_name":"SubagentStart","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic"});
    let wrong_host = hook_with_selector(&fixture, start, "CODEX_THREAD_ID", SESSION);
    assert!(wrong_host.status.success(), "{}", text(&wrong_host));
    assert!(text(&wrong_host).contains("assignment context is unavailable"));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "prepared");
    fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "presented"
    );
    let valid = r#"{"outcome":"completed","summary":"Fixture","reason":""}"#;
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"foreign-child","agent_type":"sonic","last_assistant_message":valid}),
    );
    assert_eq!(fixture.intents().unwrap()[0]["state"], "claimed");
    let stop = json!({"hook_event_name":"SubagentStop","cwd":fixture.product,
        "session_id":SESSION,"agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid});
    let wrong_session = hook_with_selector(&fixture, stop, "CLAUDE_CODE_SESSION_ID", OTHER);
    assert!(wrong_session.status.success(), "{}", text(&wrong_session));
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_null(), "{intent}");
    assert!(
        intent["rawObservation"]["reason"]
            .as_str()
            .unwrap()
            .contains("conflicts"),
        "{intent}"
    );
    assert!(intent.get("pendingReturn").is_none(), "{intent}");
}

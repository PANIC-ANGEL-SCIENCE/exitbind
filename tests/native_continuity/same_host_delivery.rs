//! Verified same-host Goal projection for host callbacks.
use super::{run, text, Fixture, CHILD, OTHER, SESSION};
use serde_json::{json, Value};
use std::{fs, process::Output};

#[test]
fn selector_free_same_host_delivery_matches_owner_goal_and_returns_after_permit() {
    let fixture =
        Fixture::new_with_check_and_write("r60-same-host-delivery", "true", Some("fixture.txt"));
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
        output.stdout.len() <= 16 * 1024 + 1,
        "host bytes: {}",
        output.stdout.len()
    );
    assert!(start.contains("Preserve the fixture flag."), "{start}");
    let packet = start
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("complete assignment unavailable, {} bytes: {start}", output.stdout.len()));
    let packet: Value = serde_json::from_str(packet).unwrap();
    assert_eq!(packet["currentAction"]["binding"], owner["binding"]);
    assert_eq!(packet["tasks"]["goal"], owner["sections"]["tasks"]["goal"]);
    assert_eq!(
        packet["tasks"]["tasks"],
        owner["sections"]["tasks"]["tasks"]
    );
    assert_eq!(
        packet["tasks"]["goal"],
        "A same-host lens needs no cross-host protocol"
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

//! Selected reviewer delivery after a checked worker result.
use super::{argv, run, sha, text, Fixture, CHILD, REVIEWER_PROFILE_BODY, SESSION};
use serde_json::{json, Value};
use std::fs;

fn checked_reviewer_return(fixture: &Fixture, worker_content: &[u8], explicit: bool) {
    fixture.value(Some(SESSION), &["work", "check", &fixture.work]);
    let full = fixture.value(Some(SESSION), &["work", "next", &fixture.work, "--full"]);
    let reviewer = &full["next"];
    assert_eq!(reviewer["role"], "reviewer", "{reviewer}");
    let detail = fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
    assert_eq!(
        detail["recipientContext"]["profile"]["content"],
        REVIEWER_PROFILE_BODY
    );
    assert_eq!(
        detail["recipientContext"]["profile"]["sha256"],
        sha(REVIEWER_PROFILE_BODY.as_bytes())
    );
    let checks = &detail["sections"]["evidence"]["checkEvidence"];
    assert!(
        checks
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["status"] == "passed")),
        "{checks}"
    );
    assert_eq!(
        detail["sections"]["assignment"]["assignment"]["checkEvidence"],
        *checks
    );
    let governor = &reviewer["packet"]["context"]["loop"];
    let history = governor["seenMutations"].as_array().unwrap();
    assert_eq!(history.len(), 1, "{history:?}");
    if explicit {
        assert_eq!(history[0]["identity"]["unit"], "worker-mutation");
        assert_eq!(history[0]["evidence"], Value::Null);
        assert_eq!(governor["currentMutation"]["unit"], "worker");
    } else {
        assert_eq!(history[0]["identity"]["unit"], "worker");
        assert_eq!(history[0]["evidence"], sha(worker_content));
    }
    let worker_sha = sha(worker_content);
    let references = reviewer["packet"]["context"]["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "submission" && item["artifact"]["sha256"] == worker_sha)
        .map(|item| item["rawEventRef"]["id"].clone())
        .collect::<Vec<_>>();
    assert_eq!(references.len(), 1, "{reviewer}");

    let route = argv(&fixture.next(Some(SESSION))["prepareChild"]);
    let native_index = route
        .iter()
        .position(|word| word == "--agent-type")
        .unwrap();
    let native = &route[native_index + 1];
    let prepared = run(&route, &fixture.product, &fixture.store, Some(SESSION), b"");
    assert!(prepared.status.success(), "{}", text(&prepared));
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":"reviewer-child","agent_type":native}),
        )
        .expect("reviewer start must present complete context");
    assert!(start.contains(REVIEWER_PROFILE_BODY.trim_end()), "{start}");
    let actor = start
        .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("complete reviewer actor facts unavailable: {start}"));
    let actor: Value = serde_json::from_str(actor).unwrap();
    assert_eq!(actor["role"], "reviewer");
    assert_eq!(actor["evidence"]["checkEvidence"], *checks);
    assert_eq!(
        actor["governor"]["currentMutation"],
        governor["currentMutation"]
    );
    assert_eq!(
        actor["governor"]["seenMutations"],
        governor["seenMutations"]
    );
    let template = start
        .split_once("Template: ")
        .and_then(|(_, rest)| rest.split_once(". Fill actual outcome"))
        .map(|(template, _)| template)
        .unwrap_or_else(|| panic!("reviewer result template unavailable: {start}"));
    let template: Value = serde_json::from_str(template).unwrap();
    assert_eq!(template["outcome"], "");
    assert_eq!(template["summary"], "");
    assert_eq!(template["evidenceReferences"], json!(references));
    let intents = fixture.intents().unwrap();
    let intent = intents
        .iter()
        .find(|intent| intent["nativeChild"] == "reviewer-child")
        .unwrap();
    assert_eq!(intent["assignmentDelivery"]["state"], "presented");
    let mut result = template;
    result["outcome"] = json!("approved");
    result["summary"] = json!("Reviewed the current worker result and check.");
    fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"reviewer-child","agent_type":native,"last_assistant_message":result.to_string()}));
    let intents = fixture.intents().unwrap();
    let intent = intents
        .iter()
        .find(|intent| intent["nativeChild"] == "reviewer-child")
        .unwrap();
    assert!(intent["returnEventSha256"].is_string(), "{intent}");
    assert!(intent["rawObservation"].is_null(), "{intent}");
    assert_eq!(fixture.next(Some(SESSION))["role"], "lead");
}

#[test]
fn reviewer_after_implicit_completion_receives_exact_checked_facts_and_returns() {
    let fixture = Fixture::new_for_reviewer("reviewer-implicit-check", None, None);
    let worker = fixture.next(Some(SESSION));
    assert_eq!(worker["role"], "worker", "{worker}");
    let content = b"Completed worker result for reviewer.\n";
    let returned = fixture.call(
        Some(SESSION),
        &[
            "work",
            "return",
            &fixture.work,
            worker["assignment"].as_str().unwrap(),
            "--outcome",
            "completed",
        ],
        content,
        true,
    );
    assert!(returned.status.success(), "{}", text(&returned));
    checked_reviewer_return(&fixture, content, false);
}

#[test]
fn reviewer_after_explicit_permit_completion_receives_exact_checked_facts_and_returns() {
    let fixture = Fixture::new_for_reviewer("reviewer-permit-check", Some("fixture.txt"), None);
    fixture.prepare_sonic();
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    let packet = start.split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("complete worker actor facts unavailable: {start}"));
    let packet: Value = serde_json::from_str(packet).unwrap();
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
    fs::write(fixture.product.join("fixture.txt"), "permitted edit\n").unwrap();
    let content = r#"{"outcome":"completed","summary":"Edited fixture","reason":""}"#;
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":content}),
    );
    assert!(fixture.intents().unwrap()[0]["returnEventSha256"].is_string());
    checked_reviewer_return(&fixture, content.as_bytes(), true);
}

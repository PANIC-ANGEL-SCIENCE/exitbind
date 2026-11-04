//! Model-free recovery of one ended, uncredited reviewer.
#![cfg(target_os = "linux")]

use super::{argv, delivery::BlockedStart, run, text, Fixture, OTHER, PROFILE_BODY, SESSION};
use serde_json::{json, Value};
use std::fs;

struct FailedReviewer {
    fixture: Fixture,
    old_id: String,
    native: String,
    old_result: String,
}

fn failed_reviewer(name: &str) -> FailedReviewer {
    failed_reviewer_with(Fixture::new(name))
}

fn prepare_reviewer(fixture: &Fixture) -> (String, String, Value) {
    let worker = fixture.next(Some(SESSION));
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
        b"completed worker result\n",
        true,
    );
    assert!(returned.status.success(), "{}", text(&returned));
    fixture.value(Some(SESSION), &["work", "check", &fixture.work]);
    let next = fixture.next(Some(SESSION));
    assert_eq!(next["role"], "reviewer", "{next}");
    let route = argv(&next["prepareChild"]);
    let native = route[route
        .iter()
        .position(|word| word == "--agent-type")
        .unwrap()
        + 1]
    .clone();
    let prepared = run(&route, &fixture.product, &fixture.store, Some(SESSION), b"");
    assert!(prepared.status.success(), "{}", text(&prepared));
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let old_id = prepared["intent"].as_str().unwrap().to_owned();
    (old_id, native, prepared)
}

fn failed_reviewer_with(fixture: Fixture) -> FailedReviewer {
    let (old_id, native, _) = prepare_reviewer(&fixture);
    let old_result = unavailable_start(&fixture, "old-reviewer", &native);
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "unavailable"
    );
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"old-reviewer","agent_type":native,"last_assistant_message":old_result}),
    );
    let old = &fixture.intents().unwrap()[0];
    assert!(old["rawObservation"].is_object(), "{old}");
    assert!(old["returnEventSha256"].is_null(), "{old}");
    FailedReviewer {
        fixture,
        old_id,
        native,
        old_result,
    }
}

// This synthetic input deliberately has no delivery credit. Its cause is
// legacy envelope capacity, independently of the post-render rules-race helper.
fn capacity_uncredited_reviewer(name: &str) -> FailedReviewer {
    let fixture = Fixture::new(name);
    fs::write(
        fixture.product.join("AGENTS.md"),
        format!("Capacity fixture rule.\n{}", "r".repeat(9_000)),
    )
    .unwrap();
    let (old_id, native, prepared) = prepare_reviewer(&fixture);
    assert!(prepared["transportPreflight"]["parts"].as_u64().unwrap() > 1);
    let context = fixture.value(
        Some(SESSION),
        &["work", "child", "context", &fixture.work, &old_id],
    );
    let contract = &context["hostResultContract"];
    assert!(contract["schema"]["properties"]["outcome"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("approved")));
    let result = json!({"outcome":"approved","summary":"Synthetic uncredited observation for integrity testing.","reason":"","evidenceReferences":contract["requiredEvidenceReferences"]}).to_string();
    let output = fixture.hook(json!({"hook_event_name":"SubagentStart","session_id":SESSION,"agent_id":"capacity-reviewer","agent_type":native})).unwrap();
    assert!(output.contains("Complete assignment exceeds the host context envelope; automatic return cannot receive credit."), "{output}");
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "unavailable"
    );
    fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,"agent_id":"capacity-reviewer","agent_type":native,"last_assistant_message":result}));
    let old = &fixture.intents().unwrap()[0];
    assert_eq!(old["state"], "recorded");
    assert_eq!(old["nativeChild"], "capacity-reviewer");
    assert_eq!(old["session"], SESSION);
    assert!(old["rawObservation"].is_object());
    assert!(old["returnEventSha256"].is_null());
    assert!(old["pendingReturn"].is_null() || old["pendingReturn"] == false);
    FailedReviewer {
        fixture,
        old_id,
        native,
        old_result: result,
    }
}

fn unavailable_start(fixture: &Fixture, child: &str, native: &str) -> String {
    let rules_path = fixture.product.join("AGENTS.md");
    assert!(!rules_path.exists());
    let blocked = BlockedStart::for_child(fixture, child, native);
    fs::write(&rules_path, "Changed after presentation.\n").unwrap();
    let output = blocked.finish();
    assert!(output.status.success(), "{}", text(&output));
    fs::remove_file(&rules_path).unwrap();
    let hook: Value = serde_json::from_slice(&output.stdout).unwrap();
    let presented = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        presented.contains("Complete current assignment, evidence and task conditions"),
        "{presented}"
    );
    let template = presented
        .split_once("Template: ")
        .and_then(|(_, rest)| rest.split_once(". Fill actual outcome"))
        .map(|(template, _)| template)
        .expect("old presentation contains a result template");
    let mut result: Value = serde_json::from_str(template).unwrap();
    result["outcome"] = json!("approved");
    result["summary"] = json!("Reviewed the current checked result.");
    result.to_string()
}

fn prepare_retry(failed: &FailedReviewer) -> (Vec<String>, String) {
    let next = failed.fixture.next(Some(SESSION));
    let route = argv(&next["prepareChild"]);
    assert!(
        route
            .windows(2)
            .any(|words| words[0] == "--retry-of" && words[1] == failed.old_id),
        "{route:?}"
    );
    let result = run(
        &route,
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(result.status.success(), "{}", text(&result));
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["effect"], "prepared");
    (route, value["intent"].as_str().unwrap().to_owned())
}

fn retry_intent(failed: &FailedReviewer, id: &str) -> Value {
    failed
        .fixture
        .intents()
        .unwrap()
        .into_iter()
        .find(|intent| intent["id"] == id)
        .unwrap()
}

#[test]
fn retry_replays_preparation_and_requires_a_new_presented_child_for_return() {
    let failed = failed_reviewer("reviewer-retry-success");
    let state_path = failed.fixture.product.join(".exitbind/child-intents.json");
    let mut initial: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(initial["version"], 1);
    initial["fixtureMetadata"] = json!({"retained":true});
    initial["intents"].as_array_mut().unwrap().push(json!({"id":"c".repeat(64),
        "work":"another-work", "state":"recorded", "nativeChild":"earlier-child", "session":SESSION}));
    fs::write(&state_path, serde_json::to_vec(&initial).unwrap()).unwrap();
    let old_raw = failed.fixture.intents().unwrap()[0]["rawObservation"].clone();
    let (route, retry_id) = prepare_retry(&failed);
    let upgraded: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(upgraded["version"], 2);
    assert_eq!(upgraded["fixtureMetadata"], initial["fixtureMetadata"]);
    let replay = run(
        &route,
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(replay.status.success(), "{}", text(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["effect"], "unchanged");
    assert_eq!(replay["intent"], retry_id);
    let changed_request = direct_retry(&failed, &failed.old_id);
    assert!(
        !changed_request.status.success(),
        "{}",
        text(&changed_request)
    );

    failed.fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":"old-reviewer","agent_type":failed.native}),
    );
    assert_eq!(retry_intent(&failed, &retry_id)["state"], "prepared");
    failed.fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":"earlier-child","agent_type":failed.native}),
    );
    assert_eq!(retry_intent(&failed, &retry_id)["state"], "prepared");
    failed.fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"old-reviewer","agent_type":failed.native,"last_assistant_message":failed.old_result}));
    assert!(retry_intent(&failed, &retry_id)["returnEventSha256"].is_null());

    let start = json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":"fresh-reviewer","agent_type":failed.native});
    let context = failed.fixture.hook(start.clone()).unwrap();
    assert!(
        context.contains("Complete current assignment, evidence and task conditions"),
        "{context}"
    );
    let claimed = retry_intent(&failed, &retry_id);
    assert_eq!(claimed["state"], "claimed");
    assert_eq!(claimed["assignmentDelivery"]["state"], "presented");
    failed.fixture.hook(start);
    assert_eq!(retry_intent(&failed, &retry_id), claimed);
    let after_claim = run(
        &route,
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(!after_claim.status.success());
    failed.fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"fresh-reviewer","agent_type":failed.native,"last_assistant_message":failed.old_result}));
    let retried = retry_intent(&failed, &retry_id);
    assert!(retried["returnEventSha256"].is_string(), "{retried}");
    assert_eq!(retried["retryOf"], failed.old_id);
    let old = failed
        .fixture
        .intents()
        .unwrap()
        .into_iter()
        .find(|item| item["id"] == failed.old_id)
        .unwrap();
    assert_eq!(old["rawObservation"], old_raw);
    assert!(old["returnEventSha256"].is_null());
    assert_eq!(failed.fixture.next(Some(SESSION))["role"], "lead");
    let after_terminal = run(
        &route,
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(!after_terminal.status.success());
    let mut final_state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(final_state["version"], 2);
    final_state["intents"]
        .as_array_mut()
        .unwrap()
        .extend((0..40).map(|index| json!({"id":format!("cleanup-{index}"),"state":"superseded"})));
    fs::write(&state_path, serde_json::to_vec(&final_state).unwrap()).unwrap();
    let cleanup = failed.fixture.call(
        Some(SESSION),
        &[
            "work",
            "child",
            "prepare",
            &failed.fixture.work,
            "cleanup-probe",
        ],
        b"",
        true,
    );
    assert!(cleanup.status.success(), "{}", text(&cleanup));
    let retained: Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    assert_eq!(retained["version"], 2);
    let intents = retained["intents"].as_array().unwrap();
    assert!(intents.iter().any(|item| item["id"] == failed.old_id));
    assert!(intents
        .iter()
        .any(|item| item["id"] == retry_id && item["retryOf"] == failed.old_id));
}

#[test]
fn retry_refuses_stale_inputs_binding_config_and_other_session() {
    for change in [
        "inputs", "binding", "check", "config", "fence", "marker", "owner", "session",
    ] {
        let failed = failed_reviewer(&format!("reviewer-retry-stale-{change}"));
        let (_route, retry_id) = prepare_retry(&failed);
        let companion = failed.fixture.companion();
        let mut saved_owner = None;
        match change {
            "inputs" => fs::write(failed.fixture.product.join("changed.txt"), "changed\n").unwrap(),
            "binding" => {
                failed.fixture.value(
                    Some(SESSION),
                    &[
                        "goal",
                        "incorporate",
                        "--goal-id",
                        "f5b-goal",
                        "--goal",
                        "A same-host lens needs no cross-host protocol",
                        "--obligation",
                        "A changed requirement",
                    ],
                );
            }
            "check" => {
                let token = failed.fixture.work.strip_prefix("smw_").unwrap();
                let ledger = failed
                    .fixture
                    .product
                    .join(format!(".exitbind/runs/work-{token}.jsonl"));
                let source = fs::read_to_string(ledger).unwrap();
                let check = source
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .find(|event| event["action"] == "check")
                    .unwrap();
                let stdout = check["stdout"]["path"].as_str().unwrap();
                fs::write(failed.fixture.product.join(stdout), "tampered check output").unwrap();
            }
            "config" => {
                let path = failed.fixture.product.join("exitbind.json");
                let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                config["agents"]["reviewer"]["purpose"] = json!("Changed reviewer purpose");
                fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
            }
            "fence" | "marker" => {
                let path = failed.fixture.product.join(".exitbind/child-intents.json");
                let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                let intent = state["intents"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|item| item["id"] == retry_id)
                    .unwrap();
                if change == "fence" {
                    intent["retryFence"]["binding"] = json!("f".repeat(64));
                } else {
                    intent.as_object_mut().unwrap().remove("retryOf");
                }
                fs::write(path, serde_json::to_vec(&state).unwrap()).unwrap();
                assert!(failed.fixture.next(Some(SESSION))["prepareChild"].is_null());
            }
            "owner" => {
                saved_owner = Some(fs::read(&companion).unwrap());
                fs::remove_file(&companion).unwrap();
            }
            "session" => {}
            _ => unreachable!(),
        }
        let session = if change == "session" { OTHER } else { SESSION };
        failed.fixture.hook(
            json!({"hook_event_name":"SubagentStart","session_id":session,
            "agent_id":"fresh-reviewer","agent_type":failed.native}),
        );
        failed.fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":session,
            "agent_id":"fresh-reviewer","agent_type":failed.native,"last_assistant_message":failed.old_result}));
        if let Some(bytes) = saved_owner {
            fs::write(companion, bytes).unwrap();
        }
        let retry = retry_intent(&failed, &retry_id);
        assert!(retry["returnEventSha256"].is_null(), "{change}: {retry}");
        assert_ne!(
            retry["assignmentDelivery"]["state"], "presented",
            "{change}: {retry}"
        );
        assert_eq!(failed.fixture.intents().unwrap()[0]["id"], failed.old_id);
    }
}

#[test]
fn retry_stop_refuses_inputs_changed_after_complete_presentation() {
    let failed = failed_reviewer("reviewer-retry-stop-stale-inputs");
    let (_, retry_id) = prepare_retry(&failed);
    let start = failed
        .fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":"fresh-reviewer","agent_type":failed.native}),
        )
        .unwrap();
    assert!(start.contains("Complete current assignment, evidence and task conditions"));
    assert_eq!(
        retry_intent(&failed, &retry_id)["assignmentDelivery"]["state"],
        "presented"
    );
    fs::write(
        failed.fixture.product.join("changed.txt"),
        "changed after Start\n",
    )
    .unwrap();
    failed.fixture.hook(json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"fresh-reviewer","agent_type":failed.native,"last_assistant_message":failed.old_result}));
    let retry = retry_intent(&failed, &retry_id);
    assert!(retry["returnEventSha256"].is_null(), "{retry}");
    assert!(retry["rawObservation"].is_object(), "{retry}");
}

fn direct_retry(failed: &FailedReviewer, source: &str) -> std::process::Output {
    let next = failed.fixture.value(
        Some(SESSION),
        &["work", "next", &failed.fixture.work, "--full"],
    );
    let next = &next["next"];
    failed.fixture.call(
        Some(SESSION),
        &[
            "work",
            "child",
            "prepare",
            &failed.fixture.work,
            next["assignment"].as_str().unwrap(),
            "--agent-type",
            &failed.native,
            "--current-binding",
            next["current"]["binding"].as_str().unwrap(),
            "--retry-of",
            source,
        ],
        b"",
        true,
    )
}

#[test]
fn retry_refuses_write_rights_and_corrupted_original_observation() {
    let fixture = Fixture::new_with_reviewer_write(
        "reviewer-retry-write-right",
        "true",
        None,
        PROFILE_BODY,
        None,
        Some("review-output.txt"),
    );
    let failed = failed_reviewer_with(fixture);
    assert!(failed.fixture.next(Some(SESSION))["prepareChild"].is_null());
    let refused = direct_retry(&failed, &failed.old_id);
    assert!(!refused.status.success(), "{}", text(&refused));

    let failed = capacity_uncredited_reviewer("reviewer-retry-corrupt-observation");
    // Positive control: the exact ended observation is eligible before the
    // sole corruption; refusal must not come from another retry prerequisite.
    let retry = argv(&failed.fixture.next(Some(SESSION))["prepareChild"]);
    assert!(retry
        .windows(2)
        .any(|args| args[0] == "--retry-of" && args[1] == failed.old_id));
    let path = failed.fixture.product.join(".exitbind/child-intents.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let original = state.clone();
    state["intents"][0]["rawObservation"]["sha256"] = json!("f".repeat(64));
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(failed.fixture.next(Some(SESSION))["prepareChild"].is_null());
    let refused = direct_retry(&failed, &failed.old_id);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("captured raw observation changed"),
        "{}",
        text(&refused)
    );
    let after: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        after, state,
        "refusal must not change the retained observation or add an intent"
    );
    assert_eq!(after["version"], 1);
    assert_eq!(after["intents"].as_array().unwrap().len(), 1);
    assert!(after["intents"][0]["returnEventSha256"].is_null());
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let restored = argv(&failed.fixture.next(Some(SESSION))["prepareChild"]);
    assert!(restored
        .windows(2)
        .any(|args| args[0] == "--retry-of" && args[1] == failed.old_id));
}

#[test]
fn retry_upgrade_refuses_live_unknown_or_pending_siblings_anywhere_in_root() {
    for (label, extra) in [
        ("prepared", json!({"state":"prepared"})),
        ("claimed", json!({"state":"claimed","nativeChild":"live"})),
        (
            "unknown",
            json!({"state":"abandoned","nativeChild":"unknown"}),
        ),
        (
            "pending",
            json!({"state":"recorded","nativeChild":"ended","pendingResult":"result"}),
        ),
    ] {
        let failed = failed_reviewer(&format!("reviewer-retry-sibling-{label}"));
        let path = failed.fixture.product.join(".exitbind/child-intents.json");
        let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut sibling = extra;
        sibling["id"] = json!("b".repeat(64));
        sibling["work"] = json!("different-work-in-same-state-root");
        state["intents"].as_array_mut().unwrap().push(sibling);
        fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(
            failed.fixture.next(Some(SESSION))["prepareChild"].is_null(),
            "{label}"
        );
        let refused = direct_retry(&failed, &failed.old_id);
        assert!(!refused.status.success(), "{label}: {}", text(&refused));
        let unchanged: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(unchanged, state, "{label}");
    }
}

#[test]
fn a_failed_retry_cannot_be_renewed_as_a_new_retry_source() {
    let failed = failed_reviewer("reviewer-retry-no-chain");
    let (_, retry_id) = prepare_retry(&failed);
    let result = unavailable_start(&failed.fixture, "fresh-reviewer", &failed.native);
    failed.fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"fresh-reviewer","agent_type":failed.native,"last_assistant_message":result}),
    );
    let retry = retry_intent(&failed, &retry_id);
    assert_eq!(retry["assignmentDelivery"]["state"], "unavailable");
    assert!(retry["rawObservation"].is_object());
    assert!(failed.fixture.next(Some(SESSION))["prepareChild"].is_null());
    let refused = direct_retry(&failed, &retry_id);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert_eq!(retry_intent(&failed, &retry_id), retry);
}

#[test]
fn retry_recovery_does_not_attribute_a_matching_committed_manual_event() {
    let failed = failed_reviewer("reviewer-retry-ambiguous-return");
    let (_, retry_id) = prepare_retry(&failed);
    let start = failed
        .fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":"fresh-reviewer","agent_type":failed.native}),
        )
        .unwrap();
    assert!(start.contains("Complete current assignment, evidence and task conditions"));
    let next = failed.fixture.value(
        Some(SESSION),
        &["work", "next", &failed.fixture.work, "--full"],
    );
    let assignment = next["next"]["assignment"].as_str().unwrap();
    let manual = failed.fixture.call(
        Some(SESSION),
        &[
            "work",
            "return",
            &failed.fixture.work,
            assignment,
            "--outcome",
            "approved",
        ],
        failed.old_result.as_bytes(),
        true,
    );
    assert!(manual.status.success(), "{}", text(&manual));

    // Model the durable pre-append marker and pending bytes left by a lost
    // reply/sidecar save. The Work event has identical bytes but no child ID.
    let path = failed.fixture.product.join(".exitbind/child-intents.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let intent = state["intents"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == retry_id)
        .unwrap();
    intent["state"] = json!("recorded");
    intent["pendingResult"] = json!(failed.old_result);
    intent["pendingReturn"] = json!(true);
    intent["resultSha256"] = json!(super::sha(failed.old_result.as_bytes()));
    intent["retryReturnAttempted"] = json!(true);
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let mut damaged = state.clone();
    damaged["intents"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == retry_id)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("retryOf");
    fs::write(&path, serde_json::to_vec(&damaged).unwrap()).unwrap();
    let refused = failed.fixture.call(
        Some(SESSION),
        &["work", "child", "recover", &failed.fixture.work, &retry_id],
        b"",
        true,
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    let unchanged: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(unchanged, damaged);
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let recover = failed.fixture.call(
        Some(SESSION),
        &["work", "child", "recover", &failed.fixture.work, &retry_id],
        b"",
        true,
    );
    assert!(recover.status.success(), "{}", text(&recover));
    let recovered: Value = serde_json::from_slice(&recover.stdout).unwrap();
    assert_eq!(recovered["effect"], "retained");
    let retry = retry_intent(&failed, &retry_id);
    assert!(retry["returnEventSha256"].is_null(), "{retry}");
    assert_eq!(retry["pendingReturn"], true);
}

#[test]
#[ignore = "requires an earlier producer executable in EXITBIND_TEST_OLD_PRODUCER"]
fn older_producer_refuses_v2_prepared_child_paths() {
    let previous = std::env::var("EXITBIND_TEST_OLD_PRODUCER").expect("old producer path");
    let failed = failed_reviewer("reviewer-retry-old-producer");
    let (route, retry_id) = prepare_retry(&failed);
    let state_path = failed.fixture.product.join(".exitbind/child-intents.json");
    let before = fs::read(&state_path).unwrap();
    let mut old_route = route;
    old_route[0] = previous.clone();
    // The old CLI does not know --retry-of; test its ordinary prepare path.
    let retry_flag = old_route
        .iter()
        .position(|word| word == "--retry-of")
        .unwrap();
    old_route.drain(retry_flag..=retry_flag + 1);
    let prepare = run(
        &old_route,
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(!prepare.status.success(), "{}", text(&prepare));
    let context = run(
        &[
            previous.clone(),
            "work".into(),
            "child".into(),
            "context".into(),
            failed.fixture.work.clone(),
            retry_id.clone(),
            "--config".into(),
            failed.fixture.config.clone(),
        ],
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(!context.status.success(), "{}", text(&context));
    let recover = run(
        &[
            previous.clone(),
            "work".into(),
            "child".into(),
            "recover".into(),
            failed.fixture.work.clone(),
            retry_id,
            "--config".into(),
            failed.fixture.config.clone(),
        ],
        &failed.fixture.product,
        &failed.fixture.store,
        Some(SESSION),
        b"",
    );
    assert!(!recover.status.success(), "{}", text(&recover));
    let payload = json!({"hook_event_name":"SubagentStart","cwd":failed.fixture.product,
        "session_id":SESSION,"agent_id":"old-reader-child","agent_type":failed.native});
    let callback = run(
        &[previous, "hook-run".into()],
        &failed.fixture.product,
        &failed.fixture.store,
        None,
        payload.to_string().as_bytes(),
    );
    assert!(callback.status.success(), "{}", text(&callback));
    assert!(
        text(&callback).contains("child intent state is malformed"),
        "{}",
        text(&callback)
    );
    assert_eq!(fs::read(state_path).unwrap(), before);
}

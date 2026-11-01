//! R55 F5b follow-up: a failed same-host child's retained final message is
//! recovered without a cross-host context, from the session that prepared it,
//! and the existing refusals a retained child causes name it with the
//! recover line, which runs unedited and clears them.
use super::{argv, run, sha, text, Fixture, CHILD, OTHER, RESULT, SESSION};
use serde_json::{json, Value};
use std::{fs, process::Output};

#[test]
fn worker_receives_the_frozen_check_input_observation_before_work() {
    let fixture = Fixture::new_with_check("r60-check-preparation", "sh ./missing-check.sh");
    let detail = fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
    assert_eq!(
        detail["sections"]["assignment"]["checkPreparation"]["input"]["state"], "missing",
        "{detail}"
    );
    assert_eq!(
        detail["sections"]["assignment"]["checkPreparation"]["input"]["argument"],
        "./missing-check.sh"
    );
    fixture.prepare_sonic();
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(start.contains("missing-check.sh"), "{start}");
    assert!(start.contains("\"state\":\"missing\""), "{start}");
}

#[test]
fn permit_successor_rejects_changed_recipient_and_unrelated_transitions() {
    for change in [
        "request",
        "pre-input",
        "ledger",
        "goal",
        "profile",
        "rights",
        "rules",
        "old-proof",
    ] {
        let fixture = Fixture::new(&format!("r60-permit-{change}"));
        if change == "rules" {
            fs::write(
                fixture.product.join("AGENTS.md"),
                "Original required rule.\n",
            )
            .unwrap();
        }
        fixture.prepare_sonic();
        let start = fixture
            .hook(
                json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
            )
            .unwrap();
        assert_eq!(
            fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
            "presented",
            "{change}: {start}"
        );
        if change == "rules" {
            assert!(start.contains("Original required rule."), "{start}");
        }
        let assignment = start
            .split_once("Complete current assignment, evidence and task conditions (verified for this recipient):\n")
            .and_then(|(_, rest)| rest.lines().next()).unwrap();
        let assignment: Value = serde_json::from_str(assignment).unwrap();
        let form = &assignment["currentAction"]["beforeEditing"];
        assert_eq!(form["required"], true, "{change}: {start}");
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
        if change == "request" {
            let index = permit
                .iter()
                .position(|word| word == "--request-id")
                .unwrap();
            permit[index + 1] = "unmatched-request".into();
        }
        if change == "old-proof" {
            let path = fixture.product.join(".exitbind/child-intents.json");
            let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            saved["intents"][0]["assignmentDelivery"]
                .as_object_mut()
                .unwrap()
                .remove("permitForm");
            fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
        }
        if change == "pre-input" {
            fs::write(
                fixture.product.join("changed-before-permit.txt"),
                "unpermitted change\n",
            )
            .unwrap();
        }
        let result = run(
            &permit,
            &fixture.product,
            &fixture.store,
            Some(SESSION),
            b"",
        );
        assert!(result.status.success(), "{change}: {}", text(&result));
        let result: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(result["allowed"], true, "{change}: {result}");
        if !matches!(change, "request" | "pre-input" | "old-proof") {
            assert_eq!(
                result["childDeliveryContinuity"]["state"], "current",
                "{change}: {result}"
            );
        }
        match change {
            "ledger" => {
                let before =
                    fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
                let index = permit
                    .iter()
                    .position(|word| word == "--request-id")
                    .unwrap();
                permit[index + 1] = "second-unrelated-request".into();
                let _second = run(
                    &permit,
                    &fixture.product,
                    &fixture.store,
                    Some(SESSION),
                    b"",
                );
                let after =
                    fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
                assert_ne!(
                    before["binding"], after["binding"],
                    "the second event must actually advance the ledger"
                );
            }
            "goal" => {
                fixture.value(
                    Some(SESSION),
                    &[
                        "goal",
                        "incorporate",
                        "--goal-id",
                        "f5b-goal",
                        "--goal",
                        "A same-host lens needs no cross-host protocol",
                        "--obligation",
                        "A later independent obligation",
                    ],
                );
            }
            "profile" => fs::write(
                fixture.product.join(super::PROFILE),
                "Changed role instruction.\n",
            )
            .unwrap(),
            "rights" => {
                let path = fixture.product.join("exitbind.json");
                let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                config["agents"]["worker"]["write"] = json!(["changed-scope"]);
                fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
            }
            "rules" => {
                let before =
                    fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
                fs::write(
                    fixture.product.join("AGENTS.md"),
                    "Changed required rule.\n",
                )
                .unwrap();
                let after =
                    fixture.value(Some(SESSION), &["work", "detail", &fixture.work, "--json"]);
                assert_eq!(
                    before["binding"], after["binding"],
                    "recipient proof must catch rule drift when Work binding is unchanged"
                );
            }
            _ => {}
        }
        fixture.hook(
            json!({"hook_event_name":"SubagentStop","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic",
            "last_assistant_message":r#"{"outcome":"completed","summary":"Fixture","reason":""}"#}),
        );
        let intent = &fixture.intents().unwrap()[0];
        assert!(intent["returnEventSha256"].is_null(), "{change}: {intent}");
        assert!(intent["rawObservation"].is_object(), "{change}: {intent}");
        assert!(intent.get("pendingReturn").is_none(), "{change}: {intent}");
    }
}

#[test]
fn oversized_host_rules_remain_uncredited_and_preserve_the_observation() {
    let fixture = Fixture::new("r60-oversized-host-rules");
    fs::write(fixture.product.join("AGENTS.md"), "r".repeat(20_000)).unwrap();
    fixture.prepare_sonic();
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(
        start.contains("Complete assignment exceeds the host context envelope"),
        "{start}"
    );
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "unavailable"
    );
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic",
        "last_assistant_message":r#"{"outcome":"blocked","summary":"Fixture","reason":""}"#}),
    );
    let intent = &fixture.intents().unwrap()[0];
    assert!(intent["returnEventSha256"].is_null());
    assert!(intent["rawObservation"].is_object(), "{intent}");
    assert!(intent.get("pendingReturn").is_none());
}

#[test]
fn typed_host_return_uses_the_bound_assignment_once_and_rejects_foreign_or_invalid_results() {
    let fixture = Fixture::new("r60-host-return");
    fixture.prepare_sonic();
    let start = fixture
        .hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
        )
        .unwrap();
    assert!(start.contains("automatic Work return"), "{start}");
    assert_eq!(
        fixture.intents().unwrap()[0]["assignmentDelivery"]["state"],
        "presented"
    );
    let valid = r#"{"outcome":"blocked","summary":"Blocked on the fixture","reason":""}"#;
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":"foreign-child","agent_type":"sonic","last_assistant_message":valid}),
    );
    assert_eq!(fixture.intents().unwrap()[0]["state"], "claimed");
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    let recorded = fixture.intents().unwrap()[0].clone();
    assert!(recorded["returnEventSha256"].is_string(), "{recorded}");
    assert!(recorded.get("pendingResult").is_none(), "{recorded}");
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        recorded,
        "a duplicate hook must not resubmit"
    );
    let ledger = fixture.product.join(".exitbind/runs").join(format!(
        "work-{}.jsonl",
        fixture.work.strip_prefix("smw_").unwrap()
    ));
    let before_replay = fs::read(&ledger).unwrap();
    let state = fixture.product.join(".exitbind/child-intents.json");
    let mut interrupted: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    interrupted["intents"][0]["pendingResult"] = json!(valid);
    interrupted["intents"][0]["pendingReturn"] = json!(true);
    interrupted["intents"][0]
        .as_object_mut()
        .unwrap()
        .remove("returnEventSha256");
    fs::write(&state, serde_json::to_vec(&interrupted).unwrap()).unwrap();
    let replay = fixture.recover(Some(SESSION), recorded["id"].as_str().unwrap(), &[]);
    assert!(replay.status.success(), "{}", text(&replay));
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["idempotent"], true, "{replay}");
    assert_eq!(
        fs::read(&ledger).unwrap(),
        before_replay,
        "replay must not append or execute"
    );
    let mut held: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    held["intents"][0]["pendingResult"] = json!(valid);
    held["intents"][0]["pendingReturn"] = json!(true);
    held["intents"][0]["held"] = json!({"reference":"existing-held-observation"});
    held["intents"][0]
        .as_object_mut()
        .unwrap()
        .remove("returnEventSha256");
    fs::write(&state, serde_json::to_vec(&held).unwrap()).unwrap();
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        held["intents"][0],
        "a duplicate stop must not resubmit an admitted held result"
    );
    assert_eq!(fs::read(&ledger).unwrap(), before_replay);

    let invalid = Fixture::new("r60-host-return-invalid");
    let duplicate_route = argv(&invalid.next(Some(SESSION))["prepareChild"]);
    invalid.prepare_sonic();
    invalid.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let bad = r#"{"outcome":"approved","summary":"Wrong role","reason":""}"#;
    invalid.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":bad}),
    );
    let retained = invalid.intents().unwrap()[0].clone();
    assert_eq!(retained["rawObservation"]["bytes"], bad.len());
    assert!(retained["rawObservation"].get("text").is_none());
    assert!(retained["rawObservation"]["reason"].as_str().is_some());
    assert!(retained.get("pendingResult").is_none());
    assert!(retained.get("pendingReturn").is_none());
    assert!(retained["returnEventSha256"].is_null(), "{retained}");
    let notice = invalid.value(Some(SESSION), &["work", "next", &invalid.work]);
    assert_eq!(notice["current"]["uncreditedChild"]["count"], 1, "{notice}");
    assert!(notice["next"].get("prepareChild").is_none(), "{notice}");
    let duplicate = run(
        &duplicate_route,
        &invalid.product,
        &invalid.store,
        Some(SESSION),
        b"",
    );
    assert!(
        !duplicate.status.success(),
        "a started assignment must not be relaunched"
    );
    let read_route = notice["current"]["uncreditedChild"]["latest"]["readRoute"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let manual = invalid.full_next(Some(SESSION));
    assert_eq!(manual["action"], "spawn");
    assert!(manual.get("prepareChild").is_none(), "{manual}");
    let manual_return = invalid.call(
        Some(SESSION),
        &[
            "work",
            "return",
            &invalid.work,
            manual["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        b"A separate governed manual report",
        true,
    );
    assert!(manual_return.status.success(), "{}", text(&manual_return));
    let raw = run(
        &read_route,
        &invalid.product,
        &invalid.store,
        Some(SESSION),
        b"",
    );
    assert!(raw.status.success(), "{}", text(&raw));
    let raw: Value = serde_json::from_slice(&raw.stdout).unwrap();
    assert_eq!(raw["resultText"], bad);
    assert_eq!(raw["credit"], "none");

    let unavailable = Fixture::new("r60-host-return-undelivered");
    unavailable.prepare_sonic();
    unavailable.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let state = unavailable.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    saved["intents"][0]
        .as_object_mut()
        .unwrap()
        .remove("assignmentDelivery");
    fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    unavailable.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    let retained = unavailable.intents().unwrap()[0].clone();
    assert_eq!(retained["rawObservation"]["bytes"], valid.len());
    assert!(retained["rawObservation"].get("text").is_none());
    assert!(
        retained["rawObservation"]["reason"]
            .as_str()
            .unwrap()
            .contains("presentation is unavailable"),
        "{retained}"
    );
    assert!(retained.get("pendingResult").is_none());
    assert!(retained.get("pendingReturn").is_none());
    assert!(retained["returnEventSha256"].is_null(), "{retained}");
    assert_eq!(unavailable.full_next(Some(SESSION))["action"], "spawn");

    let changed_binding = Fixture::new("r60-host-return-binding-changed");
    changed_binding.prepare_sonic();
    changed_binding.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let state = changed_binding.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    saved["intents"][0]["bindingRevision"] = json!(1);
    fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    changed_binding.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    let retained = changed_binding.intents().unwrap()[0].clone();
    assert!(
        retained["rawObservation"]["reason"]
            .as_str()
            .unwrap()
            .contains("binding changed"),
        "{retained}"
    );
    assert!(retained.get("pendingResult").is_none());
    assert!(retained.get("pendingReturn").is_none());
    assert_eq!(changed_binding.full_next(Some(SESSION))["action"], "spawn");

    let stale_delivery = Fixture::new("r60-host-return-delivery-changed");
    stale_delivery.prepare_sonic();
    stale_delivery.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let state = stale_delivery.product.join(".exitbind/child-intents.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    saved["intents"][0]["assignmentDelivery"]["binding"] = json!("stale-delivery-binding");
    fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    stale_delivery.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    let retained = stale_delivery.intents().unwrap()[0].clone();
    assert!(
        retained["rawObservation"]["reason"]
            .as_str()
            .unwrap()
            .contains("presented Work binding changed"),
        "{retained}"
    );
    assert!(retained.get("pendingResult").is_none());
    assert!(retained.get("pendingReturn").is_none());
    assert_eq!(stale_delivery.full_next(Some(SESSION))["action"], "spawn");

    let stale = Fixture::new("r60-host-return-stale");
    stale.prepare_sonic();
    stale.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let pending = stale.next(Some(SESSION));
    let prior = stale.call(
        Some(SESSION),
        &[
            "work",
            "return",
            &stale.work,
            pending["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        b"Prior report",
        true,
    );
    assert!(prior.status.success(), "{}", text(&prior));
    stale.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":valid}),
    );
    let retained = stale.intents().unwrap()[0].clone();
    assert_eq!(retained["rawObservation"]["bytes"], valid.len());
    assert!(retained["rawObservation"].get("text").is_none());
    assert!(retained.get("pendingResult").is_none());
    assert!(retained["returnEventSha256"].is_null(), "{retained}");
    assert!(
        retained["rawObservation"]["reason"]
            .as_str()
            .unwrap()
            .contains("assignment changed"),
        "{retained}"
    );
}

impl Fixture {
    /// The canonical action when the bounded projection asks for inspection.
    fn full_next(&self, session: Option<&str>) -> Value {
        self.value(session, &["work", "next", &self.work, "--full"])["next"].clone()
    }

    /// `work child recover WORK INTENT` with `extra`, from `session`.
    fn recover(&self, session: Option<&str>, intent: &str, extra: &[&str]) -> Output {
        let mut args = vec!["work", "child", "recover", self.work.as_str(), intent];
        args.extend(extra);
        self.call(session, &args, b"", true)
    }
}

/// Review F5b R2: a failed same-host child's retained final message is
/// reattached without a cross-host context. Recover without `--context`
/// refuses another session and no session and changes nothing; with
/// `--context` it keeps the cross-host route and its refusal; from the Lead
/// session that holds the Work again it records the hash and state, as a
/// same-host stop does, with no continuation record.
#[test]
fn a_failed_same_host_child_is_recovered_from_the_session_that_holds_the_work() {
    let fixture = Fixture::new("f5b-same-host-recover");
    fixture.prepare_sonic_legacy();
    fixture.hook(
        json!({"hook_event_name":"SubagentStart","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic"}),
    );
    let companion = fs::read(fixture.companion()).unwrap();
    fs::remove_file(fixture.companion()).unwrap();
    fixture.hook(
        json!({"hook_event_name":"SubagentStop","session_id":SESSION,
        "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
    );
    let failed = fixture.intents().unwrap()[0].clone();
    assert_eq!(failed["state"], "failed", "{failed}");
    assert_eq!(failed["pendingResult"], RESULT, "{failed}");
    let intent = failed["id"].as_str().unwrap().to_owned();
    fs::write(fixture.companion(), companion).unwrap();

    let other = fixture.recover(Some(OTHER), &intent, &[]);
    assert!(!other.status.success(), "{}", text(&other));
    assert!(
        text(&other).contains("this native session does not hold the Work"),
        "{}",
        text(&other)
    );
    let none = fixture.recover(None, &intent, &[]);
    assert!(!none.status.success(), "{}", text(&none));
    assert!(
        text(&none).contains("uses the --context token from work continuation"),
        "{}",
        text(&none)
    );
    let token = format!("{}:1:0:00", fixture.work);
    let context = fixture.recover(Some(SESSION), &intent, &["--context", &token]);
    assert!(!context.status.success(), "{}", text(&context));
    assert!(
        text(&context).contains("work context token is invalid"),
        "{}",
        text(&context)
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        failed,
        "refusals change nothing"
    );

    let recovered = fixture.recover(Some(SESSION), &intent, &[]);
    assert!(recovered.status.success(), "{}", text(&recovered));
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["effect"], "recorded", "{recovered}");
    assert_eq!(recovered["intent"], intent.as_str());
    let intent = &fixture.intents().unwrap()[0];
    assert_eq!(intent["state"], "recorded", "{intent}");
    assert_eq!(intent["nativeChild"], CHILD);
    assert_eq!(intent["resultSha256"], sha(RESULT.as_bytes()));
    assert!(intent.get("pendingResult").is_none(), "{intent}");
    assert!(intent.get("failure").is_none(), "{intent}");
    fixture.assert_no_continuation();
    let again = fixture.recover(Some(SESSION), intent["id"].as_str().unwrap(), &[]);
    assert!(
        text(&again).contains("this child has no retained result to recover"),
        "{}",
        text(&again)
    );
}

impl Fixture {
    /// Prepare and start sonic, then break the same-host binding so its stop
    /// fails it with the final message retained; the binding is restored.
    /// Returns the failed intent.
    fn failed_child(&self) -> Value {
        self.prepare_sonic_legacy();
        self.hook(
            json!({"hook_event_name":"SubagentStart","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic"}),
        );
        let companion = fs::read(self.companion()).unwrap();
        fs::remove_file(self.companion()).unwrap();
        self.hook(
            json!({"hook_event_name":"SubagentStop","session_id":SESSION,
            "agent_id":CHILD,"agent_type":"sonic","last_assistant_message":RESULT}),
        );
        fs::write(self.companion(), companion).unwrap();
        let failed = self.intents().unwrap()[0].clone();
        assert_eq!(failed["state"], "failed", "{failed}");
        assert_eq!(failed["pendingResult"], RESULT, "{failed}");
        failed
    }
}

/// Review D2: a later session that adopted the Lead session's Goal journal
/// holds the Work, but the child ran under the Lead session: recover without
/// `--context` from it is refused and changes nothing.
#[test]
fn a_session_that_adopted_the_goal_cannot_claim_another_sessions_child() {
    let fixture = Fixture::new("f5b-recover-adopted");
    let failed = fixture.failed_child();
    let companion: Value = serde_json::from_slice(&fs::read(fixture.companion()).unwrap()).unwrap();
    let key = companion["sessionKey"].as_str().unwrap();
    let adopted = fixture.call(
        Some(OTHER),
        &["goal", "adopt", "--session-key", key],
        b"",
        true,
    );
    assert!(adopted.status.success(), "{}", text(&adopted));
    let intent = failed["id"].as_str().unwrap();
    let refused = fixture.recover(Some(OTHER), intent, &[]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("the child ran under another receiving session"),
        "{}",
        text(&refused)
    );
    assert_eq!(
        fixture.intents().unwrap()[0],
        failed,
        "the refusal changes nothing"
    );
}

/// The printed recover line from a refusal, run unedited in `session`.
fn run_line(fixture: &Fixture, refusal: &str, session: &str) -> Output {
    let (_, line) = refusal
        .split_once("record its retained result with: ")
        .unwrap_or_else(|| panic!("no recover line: {refusal}"));
    let line = line.lines().next().unwrap().trim_end();
    let argv = ["sh".to_owned(), "-c".to_owned(), line.to_owned()];
    run(&argv, &fixture.product, &fixture.store, Some(session), b"")
}

/// Review D1: a retained same-host child blocks check replacement; the
/// refusal names the child and prints its recover line, which runs unedited
/// from the Lead session and clears the block.
#[test]
fn a_retained_child_refusal_prints_the_recover_line_that_clears_it() {
    let fixture = Fixture::new("f5b-recover-from-refusal");
    let failed = fixture.failed_child();
    let worker = fixture.next(Some(SESSION))["assignment"]
        .as_str()
        .unwrap()
        .to_owned();
    let work = fixture.work.clone();
    let blocked = fixture.call(
        Some(SESSION),
        &["work", "return", &work, &worker, "--outcome", "blocked"],
        b"blocked on a retained child",
        true,
    );
    assert!(blocked.status.success(), "{}", text(&blocked));
    let replace = |fixture: &Fixture| {
        let next = fixture.value(Some(SESSION), &["work", "next", &work, "--json"]);
        let binding = next["current"]["binding"].as_str().unwrap().to_owned();
        fixture.call(
            Some(SESSION),
            &[
                "work",
                "replace-check",
                &work,
                "--check-command",
                "true",
                "--reason",
                "the frozen check is wrong",
                "--classification",
                "defective-contract",
                "--current-binding",
                &binding,
            ],
            b"",
            true,
        )
    };
    let refused = replace(&fixture);
    assert!(!refused.status.success(), "{}", text(&refused));
    let refusal = text(&refused);
    let intent = failed["id"].as_str().unwrap();
    assert!(
        refusal.contains(&format!(
            "check replacement is blocked by a live, prepared, or retained child result: child {intent} ("
        )),
        "{refusal}"
    );
    assert!(!refusal.contains("--context"), "{refusal}");
    assert_eq!(fixture.intents().unwrap()[0], failed);

    let recovered = run_line(&fixture, &refusal, SESSION);
    assert!(recovered.status.success(), "{}", text(&recovered));
    assert_eq!(fixture.intents().unwrap()[0]["state"], "recorded");
    let again = replace(&fixture);
    assert!(
        !text(&again).contains("retained child"),
        "the block is cleared: {}",
        text(&again)
    );
}

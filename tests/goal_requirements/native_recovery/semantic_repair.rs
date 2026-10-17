use super::*;

fn recovery_integration(project: &Project, config: &Path, review_policy: &str) -> String {
    project.value_with_config(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "Integrate the exact recovered Source journey",
            "--check-command",
            "sh check.sh",
            "--review-policy",
            review_policy,
            "--artifact",
            "check.sh",
            "--journey-integration",
        ],
        None,
        config,
    )["work"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn make_reviewer_input_stale(project: &Project, work: &str) {
    let token = work.strip_prefix("smw_").unwrap();
    let path = project
        .root
        .join(format!(".exitbind/runs/work-{token}.jsonl"));
    let raw = fs::read_to_string(&path).unwrap();
    let mut events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let reviewer = events
        .iter_mut()
        .find(|event| event["role"] == "reviewer" && event["outcome"] == "approved")
        .expect("accepted required-review integration has a Reviewer approval");
    reviewer["inputsSha256"] = json!("0".repeat(64));

    let mut previous = Value::Null;
    let mut lines = Vec::with_capacity(events.len());
    for mut event in events {
        event["previousEventSha256"] = previous.clone();
        let object = event.as_object_mut().unwrap();
        object.remove("eventSha256");
        let digest = format!("{:x}", Sha256::digest(canonical(&event).as_bytes()));
        event["eventSha256"] = json!(digest);
        previous = event["eventSha256"].clone();
        lines.push(serde_json::to_string(&event).unwrap());
    }
    fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

#[test]
fn native_recovery_preserves_stale_home_support_from_its_baseline_integration() {
    let (owner, source) = owner_and_source("native-recovery-stale-home-baseline");
    owner.file("owner-input.txt", "before integration\n");

    let historical = begin_in(
        &owner,
        &owner.root.join("exitbind.json"),
        "Retain the verified old fixture",
        "sh owner-check.sh",
        &[],
    );
    owner.drive(&historical, true);
    let home = begin_in(
        &owner,
        &owner.root.join("exitbind.json"),
        "Keep the real owner contribution active",
        "sh owner-check.sh",
        &[],
    );
    owner.drive(&home, true);
    owner.file("owner-input.txt", "changed after the home contribution\n");

    let owner_integration = begin_in(
        &owner,
        &owner.root.join("exitbind.json"),
        "Refresh the baseline owner journey",
        "sh owner-check.sh",
        &["--artifact", "owner-check.sh", "--journey-integration"],
    );
    owner.drive(&owner_integration, true);

    let integration_config = source.root.join("exitbind-recovery-integration.json");
    let integration_settings: Value =
        serde_json::from_slice(&fs::read(source.root.join("exitbind.json")).unwrap()).unwrap();
    fs::write(
        &integration_config,
        serde_json::to_vec_pretty(&integration_settings).unwrap(),
    )
    .unwrap();
    let source_work = source_member(&source);
    let preview = recovery_preview(&owner, &source, &historical, "stale-home-keeps-baseline");
    apply_recovery(&owner, &preview);

    let before_source_integration = owner.status();
    let before_journey = &before_source_integration["goalProgress"]["journeyCompletion"];
    assert_eq!(
        before_journey["state"], "blocked",
        "{before_source_integration}"
    );
    assert_eq!(
        before_journey["members"]["integrationSupported"], 1,
        "{before_source_integration}"
    );
    assert!(before_journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["work"] == source_work }));

    let integration = recovery_integration(&source, &integration_config, "required");
    source.drive_with_config(&integration, true, &integration_config);

    let ready = owner.status();
    let journey = &ready["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "current", "{ready}");
    assert_eq!(journey["members"]["integrationSupported"], 1, "{ready}");
    assert_eq!(journey["integrationWork"], owner_integration);
}

#[test]
fn native_recovery_keeps_an_unaccepted_unselected_home_member_blocking() {
    let (owner, source) = owner_and_source("native-recovery-pending-home");
    owner.file("owner-input.txt", "owner fixture\n");
    let pending_home = begin_in(
        &owner,
        &owner.root.join("exitbind.json"),
        "Keep this real owner contribution pending",
        "sh owner-check.sh",
        &[],
    );
    let source_work = source_member(&source);
    let preview = recovery_preview(&owner, &source, "", "pending-home-remains-required");
    apply_recovery(&owner, &preview);

    let integration = recovery_integration(&source, &source.root.join("exitbind.json"), "required");
    source.drive(&integration, true);

    let status = owner.status();
    let journey = &status["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "blocked", "{status}");
    assert!(journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["work"] == pending_home }));
    assert!(!journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["work"] == source_work }));
}

#[test]
fn native_recovery_omitted_review_cannot_qualify_its_integration() {
    let (owner, source) = owner_and_source("native-recovery-review-boundary");
    let source_work = source_member(&source);
    let preview = recovery_preview(&owner, &source, "", "review-is-required");
    apply_recovery(&owner, &preview);

    let omitted = recovery_integration(&source, &source.root.join("exitbind.json"), "omitted");
    source.drive(&omitted, true);
    let after_omitted = owner.status();
    let omitted_journey = &after_omitted["goalProgress"]["journeyCompletion"];
    assert_eq!(omitted_journey["state"], "blocked", "{after_omitted}");
    assert!(omitted_journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["work"] == source_work }));

    let denied_close = owner.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &source_work,
        ],
        None,
    );
    assert!(!denied_close.status.success());

    let reviewed = recovery_integration(&source, &source.root.join("exitbind.json"), "required");
    source.drive(&reviewed, true);
    let ready = owner.status();
    assert_eq!(
        ready["goalProgress"]["journeyCompletion"]["state"], "current",
        "{ready}"
    );
    assert_eq!(
        ready["goalProgress"]["journeyCompletion"]["integrationWork"],
        reviewed
    );
    let closed = owner.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "original-conversation",
            "--result-ref",
            &source_work,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
}

#[test]
fn native_recovery_rework_without_current_reviewer_approval_stays_blocked() {
    let (owner, source) = owner_and_source("native-recovery-review-rework");
    let source_work = source_member(&source);
    let preview = recovery_preview(&owner, &source, "", "rework-review-remains-blocking");
    apply_recovery(&owner, &preview);

    let integration = recovery_integration(&source, &source.root.join("exitbind.json"), "required");
    let mut reworked = false;
    for _ in 0..8 {
        let detail = source.value(&["work", "next", &integration, "--json", "--full"], None);
        let next = &detail["next"];
        match next["action"].as_str().unwrap() {
            "spawn" if next["role"] == "worker" => {
                source.value(
                    &[
                        "work",
                        "permit",
                        &integration,
                        next["assignment"].as_str().unwrap(),
                        "--operation",
                        "Return the deterministic integration worker fixture",
                    ],
                    None,
                );
                source.return_stage_with_config(
                    &integration,
                    next,
                    "completed",
                    &source.root.join("exitbind.json"),
                );
            }
            "check" => {
                let output = source.call(&["work", "check", &integration], None);
                assert!(output.status.success(), "{output:?}");
            }
            "spawn" if next["role"] == "reviewer" => {
                source.return_stage_with_config(
                    &integration,
                    next,
                    "rework",
                    &source.root.join("exitbind.json"),
                );
                reworked = true;
                break;
            }
            "lead_decision" => {
                let scoped = next["outcomes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item == "scoped");
                source.return_stage_with_config(
                    &integration,
                    next,
                    if scoped { "scoped" } else { "accepted" },
                    &source.root.join("exitbind.json"),
                );
            }
            action => panic!("unexpected integration action {action}: {detail}"),
        }
    }
    assert!(
        reworked,
        "fixture reached and recorded the required Reviewer rework"
    );

    let status = owner.status();
    let journey = &status["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "blocked", "{status}");
    assert!(journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["work"] == source_work }));
}

#[test]
fn native_recovery_refuses_a_disposable_integration_with_stale_reviewer_inputs() {
    let (owner, source) = owner_and_source("native-recovery-stale-review-inputs");
    let source_work = source_member(&source);
    let preview = recovery_preview(&owner, &source, "", "stale-review-remains-blocking");
    apply_recovery(&owner, &preview);

    let integration = recovery_integration(&source, &source.root.join("exitbind.json"), "required");
    source.drive(&integration, true);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    make_reviewer_input_stale(&source, &integration);
    let status = owner.status();
    let journey = &status["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "blocked", "{status}");
    assert!(journey["unresolved"].as_array().unwrap().iter().any(|item| {
        item["reason"].as_str().is_some_and(|reason| {
            reason.contains("canonical acceptance requires reviewer approval")
        })
    }), "the ordinary run acceptance gate rejects stale Reviewer input before recovery qualification: {status}");
    assert!(journey["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["work"] == source_work }));
}

#[test]
fn native_recovery_invalidates_integration_on_endpoint_checker_decision_and_member_changes() {
    let (owner, source) = owner_and_source("native-recovery-integration-invalidation");
    let endpoint_config = source.root.join("exitbind-selected-endpoint.json");
    let original_config: Value =
        serde_json::from_slice(&fs::read(source.root.join("exitbind.json")).unwrap()).unwrap();
    let mut endpoint_settings = original_config.clone();
    endpoint_settings["orchestration"]["maxParallel"] = json!(3);
    fs::write(
        &endpoint_config,
        serde_json::to_vec_pretty(&endpoint_settings).unwrap(),
    )
    .unwrap();
    let source_work = begin_in(
        &source,
        &endpoint_config,
        "Validate source evidence under its selected endpoint",
        "sh check.sh",
        &[],
    );
    source.drive_with_config(&source_work, true, &endpoint_config);
    let endpoint_map = serde_json::to_string(&std::collections::BTreeMap::from([(
        source_work.clone(),
        endpoint_config.to_str().unwrap().to_owned(),
    )]))
    .unwrap();

    let preview = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--endpoint-configs",
            &endpoint_map,
            "--request-id",
            "integration-invalidation-initial",
            "--reason",
            "Bind the endpoint used by the recovery integration fixture",
        ],
        None,
    );
    apply_recovery(&owner, &preview);
    let integration = recovery_integration(&source, &endpoint_config, "required");
    source.drive_with_config(&integration, true, &endpoint_config);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    let mut changed_endpoint = endpoint_settings.clone();
    changed_endpoint["orchestration"]["maxParallel"] = json!(4);
    fs::write(
        &endpoint_config,
        serde_json::to_vec_pretty(&changed_endpoint).unwrap(),
    )
    .unwrap();
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked",
        "an endpoint binding change invalidates the accepted recovery integration"
    );
    fs::write(
        &endpoint_config,
        serde_json::to_vec_pretty(&endpoint_settings).unwrap(),
    )
    .unwrap();
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    source.file(
        "check.sh",
        "set -eu\ntest -f source-input.txt\nprintf 'changed checker'\n",
    );
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked",
        "changing the selected integration checker invalidates its accepted evidence"
    );
    source.file("check.sh", "set -eu\ntest -f source-input.txt\n");
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    let changed_decision = owner.value(
        &[
            "goal",
            "recover",
            "--source-config",
            source.root.join("exitbind.json").to_str().unwrap(),
            "--historical-fixtures",
            "",
            "--endpoint-configs",
            &endpoint_map,
            "--request-id",
            "integration-invalidation-new-decision",
            "--reason",
            "Refresh the recovery decision while preserving its exact endpoint",
        ],
        None,
    );
    apply_recovery(&owner, &changed_decision);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "blocked",
        "a new recovery decision requires a newly bound integration"
    );

    let refreshed = recovery_integration(&source, &endpoint_config, "required");
    source.drive_with_config(&refreshed, true, &endpoint_config);
    assert_eq!(
        owner.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    let added_member = source_member(&source);
    let changed_membership = owner.status();
    assert_eq!(
        changed_membership["goalProgress"]["journeyCompletion"]["state"], "blocked",
        "a newly observed Source member cannot hide behind the prior integration"
    );
    assert!(
        changed_membership["goalProgress"]["journeyCompletion"]["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["work"] == added_member)
    );
}

#[test]
fn native_recovery_keeps_failed_and_stale_artifact_source_contributors_unresolved() {
    let (owner, source) = owner_and_source("native-recovery-invalid-source-evidence");
    source.file("fail.sh", "exit 9\n");
    let stale_artifact = source_member(&source);
    let failed = begin_in(
        &source,
        &source.root.join("exitbind.json"),
        "Fail this recovered Source contribution",
        "sh fail.sh",
        &[],
    );
    source.drive(&failed, false);
    fs::remove_file(source.root.join("check.sh")).unwrap();

    let preview = recovery_preview(
        &owner,
        &source,
        "",
        "invalid-source-evidence-remains-required",
    );
    apply_recovery(&owner, &preview);
    let status = owner.status();
    let journey = &status["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "blocked", "{status}");
    let unresolved = journey["unresolved"].as_array().unwrap();
    assert!(
        unresolved.iter().any(|item| item["work"] == failed),
        "{status}"
    );
    assert!(
        unresolved.iter().any(|item| item["work"] == stale_artifact),
        "{status}"
    );
}

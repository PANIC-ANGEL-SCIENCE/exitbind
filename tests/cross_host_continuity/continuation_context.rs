use super::*;

#[test]
fn product_actions_use_emitted_context_and_preserve_child_bytes() {
    let f = Fixture::new("w2ch-product-actions");
    f.initialize();
    f.ok(
        json!({"action":"bind","expectedRevision":1,"expectedBindingRevision":0,
        "host":"codex","session":"codex-session","hostVersion":"1"}),
    );

    let context = f.view()["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let before_refusals = f.history();
    let mut invalid_context = context.clone();
    invalid_context.pop();
    invalid_context.push(if context.ends_with('0') { '1' } else { '0' });
    assert_ne!(invalid_context, context);
    let refused = f.call(&[
        "work",
        "bind",
        &f.work,
        "--context",
        &invalid_context,
        "--host",
        "claude",
        "--session",
        "claude-session",
        "--host-version",
        "2",
    ]);
    assert!(!refused.status.success());
    let missing_session = f.call(&[
        "work",
        "bind",
        &f.work,
        "--context",
        &context,
        "--host",
        "claude",
        "--host-version",
        "2",
    ]);
    assert!(!missing_session.status.success());
    let unsupported_host = f.call(&[
        "work",
        "bind",
        &f.work,
        "--context",
        &context,
        "--host",
        "unknown",
        "--session",
        "unknown-session",
        "--host-version",
        "2",
    ]);
    assert!(!unsupported_host.status.success());
    assert_eq!(f.history(), before_refusals);

    let rebound = f.call(&[
        "work",
        "bind",
        &f.work,
        "--context",
        &context,
        "--host",
        "claude",
        "--session",
        "claude-session",
        "--host-version",
        "2",
    ]);
    assert!(rebound.status.success(), "{rebound:?}");
    let rebound: Value = serde_json::from_slice(&rebound.stdout).unwrap();
    assert_eq!(rebound["goalRevision"], 3);
    let after_rebind = f.history();
    let stale = f.call(&[
        "work",
        "bind",
        &f.work,
        "--context",
        &context,
        "--host",
        "codex",
        "--session",
        "another-session",
        "--host-version",
        "2",
    ]);
    assert!(!stale.status.success());
    assert_eq!(f.history(), after_rebind);

    let context = f.view()["mutationContext"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let missing_child_id = f.call(&[
        "work",
        "child",
        &f.work,
        "inspect",
        "--context",
        &context,
        "--result",
        "exact child bytes\n",
    ]);
    assert!(!missing_child_id.status.success());
    assert_eq!(f.history(), after_rebind);
    let child = f.call_with_stdin(
        &[
            "work",
            "child",
            &f.work,
            "inspect",
            "--context",
            &context,
            "--native-child",
            "claude-child",
        ],
        b"exact child bytes\n",
    );
    assert!(child.status.success(), "{child:?}");
    let child: Value = serde_json::from_slice(&child.stdout).unwrap();
    assert_eq!(child["goalRevision"], 4);
    assert_eq!(f.view()["children"][0]["resultText"], "exact child bytes\n");

    let retry = f.call(&[
        "work",
        "child",
        &f.work,
        "inspect",
        "--context",
        &context,
        "--native-child",
        "claude-child",
        "--result",
        "exact child bytes\n",
    ]);
    assert!(retry.status.success(), "{retry:?}");
    let retry: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(retry["effect"], "unchanged");

    let before_negative_child = f.history();
    let conflict = f.call(&[
        "work",
        "child",
        &f.work,
        "inspect",
        "--context",
        &context,
        "--native-child",
        "claude-child",
        "--result",
        "changed child bytes",
    ]);
    assert!(!conflict.status.success());
    let oversized = "x".repeat(8 * 1024 + 1);
    let oversized = f.call(&[
        "work",
        "child",
        &f.work,
        "oversized",
        "--context",
        &context,
        "--native-child",
        "claude-oversized",
        "--result",
        &oversized,
    ]);
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("assessed artifact route"));
    assert_eq!(f.history(), before_negative_child);
}

#[test]
fn continuation_attaches_to_existing_goal_without_replacing_its_history() {
    let f = Fixture::new("w2ch-existing-goal-attach");
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Existing goal",
        "--obligation",
        "Existing obligation",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let before = f.history();
    let input = json!({"action":"init","expectedRevision":1,
        "sourceRef":"fixture:requirements-v1",
        "sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]});
    let attached = f.ok(input.clone());
    assert_eq!(attached["goalRevision"], 2);
    assert_eq!(attached["effect"], "appended");
    let after = f.history();
    assert!(after.starts_with(&before));
    let status = f.call(&["goal", "status", "--json"]);
    assert!(status.status.success(), "{status:?}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["obligations"][0]["id"], "Existing obligation");
    assert_eq!(status["goal"], "Existing goal");
    assert_eq!(f.ok(input.clone())["effect"], "unchanged");
    assert_eq!(f.history(), after);
    f.ok(
        json!({"action":"bind","expectedRevision":2,"expectedBindingRevision":0,
        "host":"codex","session":"same-work","hostVersion":"0.156.1"}),
    );
    f.ok(
        json!({"action":"refine","expectedRevision":3,"bindingRevision":1,
        "id":"first","text":"Refined first requirement.",
        "sourceRef":"operator:refinement","expectedRequirementRevision":1}),
    );
    let refined = f.history();
    assert_eq!(f.ok(input.clone())["effect"], "unchanged");
    assert_eq!(f.history(), refined);
    let mut conflict = input;
    conflict["sourceRef"] = json!("fixture:conflict");
    assert!(!f.record(conflict).status.success());
    assert_eq!(f.history(), refined);
}

#[test]
fn stale_attach_is_refused_before_a_goal_history_append() {
    let f = Fixture::new("w2ch-stale-attach");
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Existing goal",
        "--obligation",
        "Existing obligation",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let before = f.history();
    let refused = f.record(json!({"action":"init","expectedRevision":0,
        "sourceRef":"fixture:requirements-v1","sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}));
    assert!(!refused.status.success());
    assert_eq!(f.history(), before);
}

#[test]
fn closed_goal_cannot_be_reopened_by_continuation_init() {
    let f = Fixture::new("w2ch-closed-attach");
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        &f.work,
        "--goal",
        "Finished goal",
        "--none-applicable",
        "obligations,findings,blockers,decisions,externalActions",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let closed = f.call(&["goal", "close", "--goal-id", &f.work, "--direct"]);
    assert!(closed.status.success(), "{closed:?}");
    let before = f.history();
    let refused = f.record(json!({"action":"init","expectedRevision":2,
        "sourceRef":"fixture:requirements-v1","sourceText":"First requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}));
    assert!(!refused.status.success());
    assert_eq!(f.history(), before);
}

#[test]
fn explicitly_incorporated_successor_keeps_predecessor_history() {
    let f = Fixture::new("w2ch-attached-successor");
    f.initialize();
    let predecessor = f.history();
    let started = f.call(&[
        "work",
        "begin",
        "change",
        "--goal",
        "Successor work",
        "--check-command",
        "true",
        "--preserve-requirement",
        "successor:Successor requirement.",
        "--preservation-check-command",
        "true",
        "--review-policy",
        "required",
    ]);
    assert!(started.status.success(), "{started:?}");
    let successor: Value = serde_json::from_slice(&started.stdout).unwrap();
    let work = successor["work"].as_str().unwrap();
    let incorporated = f.call(&[
        "goal",
        "incorporate",
        "--goal-id",
        work,
        "--goal",
        "Successor work",
        "--obligation",
        "Successor obligation",
    ]);
    assert!(incorporated.status.success(), "{incorporated:?}");
    let incorporated: Value = serde_json::from_slice(&incorporated.stdout).unwrap();
    let revision = incorporated["revision"].as_u64().unwrap();
    let attached = f.record_work(
        work,
        json!({"action":"init","expectedRevision":revision,
        "sourceRef":"fixture:successor","sourceText":"Successor requirement.",
        "requirements":[{"id":"successor","text":"Successor requirement."}]}),
    );
    assert!(attached.status.success(), "{attached:?}");
    let after = f.history();
    assert!(after.starts_with(&predecessor));
    let viewed = f.call(&["work", "continuation", work]);
    assert!(viewed.status.success(), "{viewed:?}");
    let viewed: Value = serde_json::from_slice(&viewed.stdout).unwrap();
    assert_eq!(viewed["work"], work);
    assert_eq!(viewed["requirements"][0]["requirement"]["id"], "successor");
    let first: Value =
        serde_json::from_slice(predecessor.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(first["goalId"], f.work);
}

#[test]
fn unsupported_named_requirements_are_refused_before_canonical_state_write() {
    let f = Fixture::without_preservation("w2ch-unsupported-work-support");
    let initialized = f.record(json!({
        "action":"init",
        "sourceRef":"fixture:unsupported-requirement",
        "sourceText":"Named requirement.",
        "requirements":[{"id":"named","text":"Named requirement."}]
    }));
    assert!(!initialized.status.success(), "{initialized:?}");
    let error = String::from_utf8_lossy(&initialized.stderr);
    assert!(error.contains("named requirements unsupported"), "{error}");
    assert!(error.contains("named"), "{error}");
    assert!(error.contains("--preserve-requirement"), "{error}");
    assert!(
        error.contains("no canonical goal state was written"),
        "{error}"
    );
    assert!(!f.root.join(".exitbind/session-goal.jsonl").exists());
}

#[test]
fn multiple_requirements_report_the_current_one_per_work_limit() {
    let f = Fixture::new("w2ch-multiple-work-support-limit");
    let initialized = f.record(json!({
        "action":"init",
        "sourceRef":"fixture:multiple-requirements",
        "sourceText":"First requirement. Second requirement.",
        "requirements":[
            {"id":"first","text":"First requirement."},
            {"id":"second","text":"Second requirement."}
        ]
    }));
    assert!(!initialized.status.success(), "{initialized:?}");
    let error = String::from_utf8_lossy(&initialized.stderr);
    assert!(error.contains("second"), "{error}");
    assert!(
        error.contains("one preservation requirement per Work"),
        "{error}"
    );
    assert!(
        error.contains("no canonical goal state was written"),
        "{error}"
    );
    assert!(!f.root.join(".exitbind/session-goal.jsonl").exists());
}

#[test]
fn legacy_incompatible_continuation_view_explains_the_limit_without_mutating_history() {
    let f = Fixture::new("w2ch-legacy-incompatible-view");
    f.ok(json!({"action":"init","sourceRef":"fixture:legacy-goal",
        "sourceText":"First requirement. Second requirement.",
        "requirements":[{"id":"first","text":"First requirement."}]}));
    let path = f.root.join(".exitbind/session-goal.jsonl");
    let bytes = fs::read(&path).unwrap();
    let mut record: Value =
        serde_json::from_slice(bytes.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    record["continuation"]["requirements"][0]["id"] = json!("second");
    record["continuation"]["requirements"][0]["text"] = json!("Second requirement.");
    reseal_goal_record(&mut record);
    let mut legacy = serde_json::to_vec(&record).unwrap();
    legacy.push(b'\n');
    fs::write(&path, &legacy).unwrap();

    let view = f.call(&["work", "continuation", &f.work]);
    assert!(view.status.success(), "{view:?}");
    let view: Value = serde_json::from_slice(&view.stdout).unwrap();
    assert_eq!(view["readOnly"], true);
    assert_eq!(view["workSupport"]["compatible"], false);
    assert_eq!(
        view["workSupport"]["missingRequirements"][0]["id"],
        "second"
    );
    assert!(view["workSupport"]["nextAction"]
        .as_str()
        .unwrap()
        .contains("goal incorporate"));
    assert_eq!(view["wholeGoalReady"], false);
    assert_eq!(fs::read(&path).unwrap(), legacy);
}

// Tests validation of native Goal recovery endpoint state.
// The case rejects a mismatched state root; see `src/session_goal/recovery.rs`.
use super::record::validate_state;
use super::*;

fn hash(byte: char) -> String {
    std::iter::repeat(byte).take(64).collect()
}

#[test]
fn validate_state_rejects_endpoint_state_root_mismatch_before_digest_only_checks() {
    let source_config = ConfigBinding {
        path: "/source/exitbind.json".into(),
        config_sha256: hash('a'),
        product_root: "/source".into(),
        state_root: "/source/.exitbind".into(),
        project_id: Some("source-project".into()),
        project_identity_sha256: hash('b'),
        local_identity_sha256: hash('c'),
    };
    let member = super::super::journey::Member {
        work: format!("smw_{}", hash('d')),
        start_event_sha256: hash('e'),
        parent_goal_id: "original-goal".into(),
        parent_observed_goal_event_sha256: hash('f'),
        work_goal_sha256: hash('a'),
        host: "codex".into(),
        session_key: hash('b'),
        owner_identity_sha256: hash('c'),
        owner_project_id: None,
        project_identity_sha256: source_config.project_identity_sha256.clone(),
        local_identity_sha256: source_config.local_identity_sha256.clone(),
        config_path: source_config.path.clone(),
        config_sha256: source_config.config_sha256.clone(),
        product_root: source_config.product_root.clone(),
        state_root: source_config.state_root.clone(),
    };
    let source_member = SourceMember {
        member_sha256: super::super::journey::member_identity(&member),
        member,
        endpoint_config: source_config.clone(),
    };
    let mut active = ActiveDecision {
        goal_id: "original-goal".into(),
        applied_from_revision: 1,
        applied_from_event_sha256: hash('d'),
        host: "codex".into(),
        session_key: hash('e'),
        owner_identity_sha256: hash('f'),
        owner_project_id: None,
        reason: "Keep the endpoint state binding exact".into(),
        source_config,
        source_members: vec![source_member],
        historical_fixtures: Vec::new(),
        decision_sha256: String::new(),
    };
    active.decision_sha256 = decision_digest(&active).unwrap();
    let mut state = RecoveryState {
        version: VERSION,
        active,
        requests: vec![AppliedRequest {
            request_id: "unit-state-root-binding".into(),
            request_sha256: hash('a'),
            preview_sha256: hash('b'),
        }],
    };

    assert!(validate_state(&state).is_ok(), "the control state is valid");
    state.active.source_members[0].endpoint_config.state_root = "/source/other-state".into();
    state.active.decision_sha256 = decision_digest(&state.active).unwrap();
    assert_eq!(
        validate_state(&state).unwrap_err(),
        "native Goal recovery source-member binding is invalid"
    );
}

#[test]
fn no_retirement_keeps_the_existing_request_and_preview_digests() {
    let request = Request {
        source_config: "/source/exitbind.json",
        historical_fixtures: "",
        endpoint_configs: None,
        retirements: None,
        source_writers_quiesced: false,
        request_id: "unchanged",
        reason: "Preserve the original recovery request",
        apply: false,
        current_binding: None,
    };
    let legacy = hash::value(&json!({"sourceConfig":request.source_config,
        "historicalFixtures":Vec::<String>::new(),"endpointConfigs":BTreeMap::<String,String>::new(),
        "reason":request.reason}));
    assert_eq!(request_digest(&request).unwrap(), legacy);
    assert_eq!(retirement_preview(&hash('d'), None), hash('d'));
}

#[test]
fn retirement_requires_explicit_quiescence_and_unique_exact_inputs() {
    let first = format!("smw_{}", hash('a'));
    let replacement = format!("smw_{}", hash('b'));
    let item = json!({"work":first,"reason":"Selected alternative", "replacement":replacement});
    let raw = json!([item.clone()]).to_string();
    assert!(retirement::request(None, false).unwrap().is_none());
    assert!(retirement::request(None, true).is_err());
    assert!(retirement::request(Some(&raw), false).is_err());
    assert_eq!(
        retirement::request(Some(&raw), true)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    assert!(
        retirement::request(Some(&json!([item.clone(), item.clone()]).to_string()), true).is_err()
    );
    let mut unknown = item.clone();
    unknown["unrecognized"] = json!(true);
    assert!(retirement::request(Some(&json!([unknown]).to_string()), true).is_err());
    let mut same = item;
    same["replacement"] = same["work"].clone();
    assert!(retirement::request(Some(&json!([same]).to_string()), true).is_err());
}

#[test]
fn retirement_selection_binds_prior_recovery_and_latest_absence_wins() {
    let source = ConfigBinding {
        path: "/source/exitbind.json".into(),
        config_sha256: hash('a'),
        product_root: "/source".into(),
        state_root: "/source/.exitbind".into(),
        project_id: Some("source-project".into()),
        project_identity_sha256: hash('b'),
        local_identity_sha256: hash('c'),
    };
    let member = |token: char| super::super::journey::Member {
        work: format!("smw_{}", hash(token)),
        start_event_sha256: hash(token),
        parent_goal_id: "goal".into(),
        parent_observed_goal_event_sha256: hash('e'),
        work_goal_sha256: hash('f'),
        host: "codex".into(),
        session_key: hash('1'),
        owner_identity_sha256: hash('2'),
        owner_project_id: None,
        project_identity_sha256: source.project_identity_sha256.clone(),
        local_identity_sha256: source.local_identity_sha256.clone(),
        config_path: source.path.clone(),
        config_sha256: source.config_sha256.clone(),
        product_root: source.product_root.clone(),
        state_root: source.state_root.clone(),
    };
    let blocked = member('3');
    let replacement = member('4');
    let source_member = |member| SourceMember {
        member_sha256: super::super::journey::member_identity(&member),
        member,
        endpoint_config: source.clone(),
    };
    let mut prior = ActiveDecision {
        goal_id: "goal".into(),
        applied_from_revision: 1,
        applied_from_event_sha256: hash('5'),
        host: "codex".into(),
        session_key: hash('1'),
        owner_identity_sha256: hash('2'),
        owner_project_id: None,
        reason: "Existing recovery".into(),
        source_config: source.clone(),
        source_members: vec![
            source_member(blocked.clone()),
            source_member(replacement.clone()),
        ],
        historical_fixtures: Vec::new(),
        decision_sha256: String::new(),
    };
    prior.decision_sha256 = decision_digest(&prior).unwrap();
    let mut active = prior.clone();
    active.applied_from_revision = 2;
    active.applied_from_event_sha256 = hash('6');
    active.decision_sha256 = decision_digest(&active).unwrap();
    let selection = retirement::Selection {
        version: 1,
        goal_id: active.goal_id.clone(),
        goal_sha256: hash::text("original Goal text"),
        prior_recovery_decision_sha256: prior.decision_sha256.clone(),
        recovery_decision_sha256: active.decision_sha256.clone(),
        owner_identity_sha256: active.owner_identity_sha256.clone(),
        session_key: active.session_key.clone(),
        source_config_sha256: active.source_config.config_sha256.clone(),
        source_writers_quiesced: true,
        items: vec![retirement::Item {
            member: blocked.clone(),
            reason: "Owner chose the accepted alternative".into(),
            blocked: crate::work::supersede::UncreditedRetirementSnapshot {
                work: blocked.work.clone(),
                start_event_sha256: blocked.start_event_sha256.clone(),
                head_event_sha256: hash('7'),
                config_sha256: blocked.config_sha256.clone(),
                raw_ledger_sha256: hash('8'),
                governor_sha256: hash('9'),
                governor_spent: Some(1),
            },
            replacement: retirement::Replacement {
                member: replacement,
                accepted_endpoint_sha256: hash('a'),
            },
        }],
    };
    retirement::validate(&selection, &active).unwrap();
    let selected_digest = retirement::integration_digest(&active, Some(&selection));
    assert_ne!(selected_digest, active.decision_sha256);
    let mut changed = selection.clone();
    changed.items[0].reason = "A different owner decision".into();
    assert_ne!(
        selected_digest,
        retirement::integration_digest(&active, Some(&changed))
    );
    changed.items[0].blocked.config_sha256 = hash('b');
    assert!(retirement::validate(&changed, &active).is_err());
    let state = |active| RecoveryState {
        version: VERSION,
        active,
        requests: vec![AppliedRequest {
            request_id: "fixture".into(),
            request_sha256: hash('b'),
            preview_sha256: hash('c'),
        }],
    };
    let mut previous = json!({"goalId":"goal", "goal":"original Goal text"});
    previous[FIELD] = serde_json::to_value(state(prior)).unwrap();
    let mut selected = json!({"goalId":"goal", "goal":"original Goal text"});
    selected[FIELD] = serde_json::to_value(state(active.clone())).unwrap();
    selected[retirement::FIELD] = serde_json::to_value(&selection).unwrap();
    selected["journeyMembershipProtocol"] = json!(1);
    selected["journeyMembers"] = json!([
        selection.items[0].member.clone(),
        selection.items[0].replacement.member.clone()
    ]);
    selected["journeyIntegrations"] = json!([]);
    super::record::validate_record(&selected, Some(&previous), 2).unwrap();
    assert_eq!(
        retirement::selected(&selected, &active).unwrap(),
        Some(selection.clone())
    );

    // An old recovery carries the exact raw selection into a new decision.
    // It remains structurally checked history but supplies no authority.
    let mut later_active = active.clone();
    later_active.applied_from_revision = 3;
    later_active.applied_from_event_sha256 = hash('b');
    later_active.decision_sha256 = decision_digest(&later_active).unwrap();
    let mut stale = selected.clone();
    stale[FIELD] = serde_json::to_value(state(later_active.clone())).unwrap();
    super::record::validate_record(&stale, Some(&selected), 3).unwrap();
    assert_eq!(stale[retirement::FIELD], selected[retirement::FIELD]);
    assert_eq!(retirement::selected(&stale, &later_active).unwrap(), None);
    assert_eq!(super::retirement_binding(Some(&stale)).unwrap(), None);
    assert_eq!(
        retirement::integration_digest(&later_active, None),
        later_active.decision_sha256
    );

    let mut malformed = stale.clone();
    malformed[retirement::FIELD]["unknown"] = json!(true);
    assert!(retirement::selected(&malformed, &later_active).is_err());
    assert!(super::record::validate_record(&malformed, Some(&stale), 4).is_err());
    let mut unsupported = stale.clone();
    unsupported[retirement::FIELD]["version"] = json!(99);
    assert!(retirement::selected(&unsupported, &later_active).is_err());
    assert!(super::record::validate_record(&unsupported, Some(&stale), 4).is_err());
    let mut malformed_hash = stale.clone();
    malformed_hash[retirement::FIELD]["recoveryDecisionSha256"] = json!("not-a-digest");
    assert!(retirement::selected(&malformed_hash, &later_active).is_err());
    assert!(super::record::validate_record(&malformed_hash, Some(&stale), 4).is_err());
    let mut unconfirmed = stale.clone();
    unconfirmed[retirement::FIELD]["sourceWritersQuiesced"] = json!(false);
    assert!(retirement::selected(&unconfirmed, &later_active).is_err());
    let mut stale_foreign = stale.clone();
    stale_foreign[retirement::FIELD]["items"][0]["replacement"]["member"]["work"] =
        json!(format!("smw_{}", hash('a')));
    assert!(retirement::selected(&stale_foreign, &later_active).is_err());
    let mut injected = stale.clone();
    injected[retirement::FIELD]["items"][0]["reason"] = json!("an injected old choice");
    assert!(super::record::validate_record(&injected, Some(&stale), 4).is_err());

    let mut next_active = later_active.clone();
    next_active.applied_from_revision = 4;
    next_active.applied_from_event_sha256 = hash('c');
    next_active.decision_sha256 = decision_digest(&next_active).unwrap();
    let mut carried = stale.clone();
    carried[FIELD] = serde_json::to_value(state(next_active.clone())).unwrap();
    super::record::validate_record(&carried, Some(&stale), 4).unwrap();
    assert_eq!(retirement::selected(&carried, &next_active).unwrap(), None);
    let mut resurrected = carried.clone();
    resurrected[FIELD] = serde_json::to_value(state(active.clone())).unwrap();
    assert!(super::record::validate_record(&resurrected, Some(&carried), 5).is_err());

    let mut fresh_active = next_active.clone();
    fresh_active.applied_from_revision = 5;
    fresh_active.applied_from_event_sha256 = hash('d');
    fresh_active.decision_sha256 = decision_digest(&fresh_active).unwrap();
    let mut fresh_selection = selection.clone();
    fresh_selection.prior_recovery_decision_sha256 = next_active.decision_sha256.clone();
    fresh_selection.recovery_decision_sha256 = fresh_active.decision_sha256.clone();
    let mut reselected = carried.clone();
    reselected[FIELD] = serde_json::to_value(state(fresh_active.clone())).unwrap();
    reselected[retirement::FIELD] = serde_json::to_value(&fresh_selection).unwrap();
    super::record::validate_record(&reselected, Some(&carried), 5).unwrap();
    assert_eq!(
        retirement::selected(&reselected, &fresh_active).unwrap(),
        Some(fresh_selection)
    );

    let mut foreign_owner = selected.clone();
    foreign_owner[retirement::FIELD]["ownerIdentitySha256"] = json!(hash('3'));
    assert!(super::retirement_binding(Some(&foreign_owner)).is_err());
    let mut foreign_member = selected.clone();
    foreign_member[retirement::FIELD]["items"][0]["replacement"]["member"]["work"] =
        json!(format!("smw_{}", hash('a')));
    assert!(super::retirement_binding(Some(&foreign_member)).is_err());
    let mut same_goal = json!({"goalId":"goal", "goal":"original Goal text"});
    super::record::preserve_same_goal(&selected, &mut same_goal);
    assert_eq!(same_goal[retirement::FIELD], selected[retirement::FIELD]);
    let mut changed_goal = selected.clone();
    changed_goal["goal"] = json!("A materially different Goal");
    super::record::preserve_same_goal(&selected, &mut changed_goal);
    assert!(changed_goal.get(retirement::FIELD).is_none());
    let mut forged = selected.clone();
    forged["goal"] = json!("A materially different Goal");
    assert!(super::retirement_binding(Some(&forged)).is_err());
    let mut dropped = selected.clone();
    dropped.as_object_mut().unwrap().remove(retirement::FIELD);
    super::record::validate_record(&dropped, Some(&selected), 3).unwrap();
    assert_eq!(super::retirement_binding(Some(&dropped)).unwrap(), None);
    let mut new_write = json!({"goalId":"goal"});
    super::record::preserve_same_goal(&dropped, &mut new_write);
    assert!(new_write.get(retirement::FIELD).is_none());
    assert!(
        super::record::validate_record(&selected, Some(&dropped), 4).is_err(),
        "replaying an old selection cannot restore its authority"
    );
}

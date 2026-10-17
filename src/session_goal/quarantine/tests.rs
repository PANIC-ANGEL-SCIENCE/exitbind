use super::*;

fn hash(byte: char) -> String {
    std::iter::repeat(byte).take(64).collect()
}

fn endpoint(work: &str, member: char, old: bool) -> Endpoint {
    Endpoint {
        work: work.into(),
        member_sha256: hash(member),
        start_event_sha256: hash('a'),
        head_event_sha256: hash('b'),
        ledger_sha256: hash('c'),
        config_sha256: hash('d'),
        run_id: "run-id".into(),
        subject_sha256: hash('e'),
        artifact_sha256: hash('f'),
        inputs_sha256: hash('1'),
        command_sha256: hash('2'),
        check_event_sha256: Some(hash('3')),
        review_event_sha256: (!old).then(|| hash('4')),
        acceptance_event_sha256: (!old).then(|| hash('5')),
    }
}

fn journey_member(work: &str, config: char) -> super::super::journey::Member {
    let (config_path, product_root, state_root) = if config == '9' {
        (
            "/original/exitbind.json",
            "/original",
            "/original/.exitbind",
        )
    } else {
        ("/source/exitbind.json", "/source", "/source/.exitbind")
    };
    super::super::journey::Member {
        work: work.into(),
        start_event_sha256: hash('a'),
        parent_goal_id: "goal".into(),
        parent_observed_goal_event_sha256: hash('b'),
        work_goal_sha256: hash('c'),
        host: "codex".into(),
        session_key: hash('d'),
        owner_identity_sha256: hash('e'),
        owner_project_id: None,
        project_identity_sha256: hash('7'),
        local_identity_sha256: hash('8'),
        config_path: config_path.into(),
        config_sha256: hash(config),
        product_root: product_root.into(),
        state_root: state_root.into(),
    }
}

fn record() -> Value {
    let old_member = journey_member(&format!("smw_{}", hash('9')), '9');
    let verification_member = journey_member(&format!("smw_{}", hash('0')), 'd');
    let mut residual = Residual {
        request_id: "req".into(),
        request_sha256: hash('6'),
        reason: "unknown remains uncredited".into(),
        source_config: super::super::recovery::ConfigBinding {
            path: "/source/exitbind.json".into(),
            config_sha256: hash('d'),
            product_root: "/source".into(),
            state_root: "/source/.exitbind".into(),
            project_id: Some("source".into()),
            project_identity_sha256: hash('7'),
            local_identity_sha256: hash('8'),
        },
        old_member: old_member.clone(),
        old: endpoint(&format!("smw_{}", hash('1')), 'a', true),
        verification: endpoint(&verification_member.work, 'b', false),
        verified_artifact_sha256: hash('2'),
        verification_environment_sha256: hash('3'),
        verification_decision_sha256: hash('1'),
        old_accounting: json!({"governor":{"spent":9,"budget":3},"usageAccount":{"status":"unknown","totals":null}}),
        decision_sha256: String::new(),
    };
    residual.old.member_sha256 = super::super::journey::member_identity(&old_member);
    residual.verification.member_sha256 =
        super::super::journey::member_identity(&verification_member);
    residual.decision_sha256 = decision_digest(&residual).unwrap();
    json!({"format":3,"goalId":"goal","journeyMembershipProtocol":3,"journeyMembers":[
            old_member,verification_member],
            "journeyIntegrations":[],"journeyAdmissions":[],
            "quarantineResiduals":{"protocol":1,"goalId":"goal","host":"codex","sessionKey":hash('d'),
                "ownerIdentitySha256":hash('e'),"ownerProjectId":null,"residuals":[residual]}})
}

#[test]
fn residual_is_uncredited_and_strictly_bound_to_both_real_goal_members() {
    let fixture = record();
    assert!(
        state(&fixture).is_ok(),
        "invalid residual fixture: {:?}",
        state(&fixture)
    );
    let mut changed = record();
    changed[FIELD]["residuals"][0]["oldAccounting"]["governor"]["spent"] = json!(0);
    assert!(state(&changed).is_err());
    let mut missing = record();
    missing[FIELD]["residuals"][0]["verification"]["work"] =
        missing[FIELD]["residuals"][0]["old"]["work"].clone();
    assert!(state(&missing).is_err());
    let mut forged = record();
    forged[FIELD]["residuals"][0]["verification"]["reviewEventSha256"] = Value::Null;
    assert!(state(&forged).is_err());
}

#[test]
fn format_three_requires_extension_and_unknown_fields_are_rejected() {
    assert!(state(&json!({"format":3,"goalId":"goal"})).is_err());
    let mut changed = record();
    changed[FIELD]["unreviewed"] = json!(true);
    assert!(state(&changed).is_err());
    let ordinary = json!({"goalId":"ordinary","version":2});
    assert!(state(&ordinary).unwrap().is_none());

    let mut ordinary_v2 = record();
    ordinary_v2.as_object_mut().unwrap().remove(FIELD);
    ordinary_v2.as_object_mut().unwrap().remove("format");
    ordinary_v2["journeyMembershipProtocol"] = json!(2);
    assert!(super::super::journey::state(&ordinary_v2).is_ok());
    ordinary_v2["journeyMembershipProtocol"] = json!(3);
    assert!(super::super::journey::state(&ordinary_v2).is_err());
}

#[test]
fn residual_cannot_credit_or_remove_the_old_unknown_or_reseed_its_endpoint() {
    let mut credited = record();
    credited[FIELD]["residuals"][0]["oldCredited"] = json!(true);
    assert!(state(&credited).is_err());

    let mut removed = record();
    removed["journeyMembers"].as_array_mut().unwrap().remove(0);
    assert!(state(&removed).is_err());

    let mut missing_proof = record();
    missing_proof[FIELD]["residuals"][0]["verification"]["reviewEventSha256"] = Value::Null;
    assert!(state(&missing_proof).is_err());

    let mut reseeded = record();
    let mut selected: Residual =
        serde_json::from_value(reseeded[FIELD]["residuals"][0].clone()).unwrap();
    selected.request_id = "second-request".into();
    selected.decision_sha256 = decision_digest(&selected).unwrap();
    reseeded[FIELD]["residuals"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(selected).unwrap());
    assert!(state(&reseeded).is_err());
}

#[test]
fn residual_rejects_an_unrelated_endpoint_or_forged_original_member_lineage() {
    let mut unrelated = record();
    let mut residual: Residual =
        serde_json::from_value(unrelated[FIELD]["residuals"][0].clone()).unwrap();
    residual.old.member_sha256 = hash('4');
    residual.decision_sha256 = decision_digest(&residual).unwrap();
    unrelated[FIELD]["residuals"][0] = serde_json::to_value(&residual).unwrap();
    assert!(state(&unrelated).is_err());

    let mut forged = record();
    let mut residual: Residual =
        serde_json::from_value(forged[FIELD]["residuals"][0].clone()).unwrap();
    residual.old_member.work = format!("smw_{}", hash('4'));
    residual.decision_sha256 = decision_digest(&residual).unwrap();
    forged[FIELD]["residuals"][0] = serde_json::to_value(residual).unwrap();
    assert!(state(&forged).is_err());
}

#[test]
fn current_writers_must_preserve_the_format_three_residual() {
    let previous = record();
    let mut next = json!({"goalId":"goal","version":2});
    preserve_same_goal(&previous, &mut next);
    assert_eq!(next["format"], 3);
    assert_eq!(next[FIELD], previous[FIELD]);
    assert!(state(&json!({"goalId":"goal","format":3})).is_err());

    let mut second = previous.clone();
    let mut added: Residual =
        serde_json::from_value(second[FIELD]["residuals"][0].clone()).unwrap();
    added.old.work = format!("smw_{}", hash('4'));
    added.verification.work = format!("smw_{}", hash('5'));
    added.request_id = "second-request".into();
    added.decision_sha256 = decision_digest(&added).unwrap();
    second[FIELD]["residuals"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(added).unwrap());
    let mut next = json!({"goalId":"goal","format":3,"journeyMembershipProtocol":3,
            "journeyMembers":previous["journeyMembers"],"journeyIntegrations":[],
            "journeyAdmissions":[],"quarantineResiduals":second[FIELD]});
    preserve_same_goal(&previous, &mut next);
    assert_eq!(next[FIELD]["residuals"].as_array().unwrap().len(), 2);
    assert!(next[FIELD]["residuals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == &previous[FIELD]["residuals"][0]));
}

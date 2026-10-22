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

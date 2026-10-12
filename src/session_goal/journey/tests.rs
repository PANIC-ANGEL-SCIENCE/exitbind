use serde_json::json;

#[test]
fn journey_protocol_refuses_unknown_fields_and_preserves_empty_native_extension() {
    let record =
        json!({"journeyMembershipProtocol":1,"journeyMembers":[],"journeyIntegrations":[]});
    assert_eq!(super::state(&record).unwrap().unwrap().protocol, 1);
    let admitted = json!({
        "journeyMembershipProtocol":2,
        "journeyMembers":[],
        "journeyIntegrations":[],
        "journeyAdmissions":[{
            "work":format!("smw_{}", "a".repeat(64)),
            "startIntentSha256":"b".repeat(64),
            "parentGoalId":"goal",
            "parentObservedGoalEventSha256":"c".repeat(64),
            "workGoalSha256":"d".repeat(64),
            "host":"codex",
            "sessionKey":"e".repeat(64),
            "ownerIdentitySha256":"f".repeat(64),
            "ownerProjectId":"project",
            "projectIdentitySha256":"1".repeat(64),
            "localIdentitySha256":"2".repeat(64),
            "configPath":"/tmp/project/exitbind.json",
            "configSha256":"3".repeat(64),
            "productRoot":"/tmp/project",
            "stateRoot":"/tmp/project"
        }]
    });
    assert_eq!(
        super::state(&admitted).unwrap().unwrap().admissions.len(),
        1
    );
    let unknown =
        json!({"journeyMembershipProtocol":9,"journeyMembers":[],"journeyIntegrations":[]});
    assert!(super::state(&unknown).is_err());
}

#[test]
fn goal_successor_carries_pending_admissions_with_their_original_parent() {
    let previous = json!({
        "goalId":"original",
        "journeyMembershipProtocol":2,
        "journeyMembers":[],
        "journeyIntegrations":[],
        "journeyAdmissions":[{
            "work":format!("smw_{}", "a".repeat(64)),
            "startIntentSha256":"b".repeat(64),
            "parentGoalId":"original",
            "parentObservedGoalEventSha256":"c".repeat(64),
            "workGoalSha256":"d".repeat(64),
            "host":"codex",
            "sessionKey":"e".repeat(64),
            "ownerIdentitySha256":"f".repeat(64),
            "ownerProjectId":"project",
            "projectIdentitySha256":"1".repeat(64),
            "localIdentitySha256":"2".repeat(64),
            "configPath":"/tmp/project/exitbind.json",
            "configSha256":"3".repeat(64),
            "productRoot":"/tmp/project",
            "stateRoot":"/tmp/project"
        }]
    });
    assert!(super::state(&previous).unwrap().is_some());
    let expected_admissions = previous["journeyAdmissions"].clone();
    let mut successor = json!({"goalId":"successor"});
    super::transition(Some(&previous), &mut successor, true).unwrap();
    assert_eq!(successor["journeyMembershipProtocol"], 2);
    assert_eq!(successor["journeyAdmissions"], expected_admissions);
    assert_eq!(
        successor["journeyAdmissions"][0]["parentGoalId"],
        "original"
    );
}

#[test]
fn integration_markers_are_separate_bounded_entries() {
    let covered = super::CoveredMember {
        work: format!("smw_{}", "a".repeat(64)),
        member_sha256: "a".repeat(64),
        accepted_endpoint_sha256: "b".repeat(64),
    };
    let marker = super::integration::marker(super::MEMBER_PREFIX, &covered).unwrap();
    assert!(marker.len() <= super::MAX_MARKER_BYTES);
    let parsed: super::CoveredMember =
        serde_json::from_str(marker.strip_prefix(super::MEMBER_PREFIX).unwrap()).unwrap();
    assert_eq!(parsed, covered);
}

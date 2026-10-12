use serde_json::{json, Value};
use std::fs;

fn goal(goal_id: &str, revision: u64) -> Value {
    json!({
        "kind": "lead_session_goal",
        "version": 2,
        "goalId": goal_id,
        "revision": revision,
        "predecessor": Value::Null,
        "successorOf": Value::Null,
        "closure": {"closed": false, "revision": Value::Null, "resultRefs": []},
        "continuation": Value::Null,
    })
}

fn sealed(mut value: Value, previous: Option<&Value>) -> Value {
    super::super::sealed(value.take(), previous)
}

#[test]
fn legacy_records_remain_readable_and_unknown_protocols_refuse() {
    let legacy = sealed(goal("legacy", 1), None);
    let raw = format!("{}\n", legacy);
    assert!(super::super::records_from_raw(Some(&raw)).is_ok());

    let mut current = goal("current", 1);
    current["workAssociationProtocol"] = json!(super::PROTOCOL);
    current["workAssociations"] = json!([{
        "kind": "context_only",
        "work": format!("smw_{}", "a".repeat(64)),
        "startEventSha256": "b".repeat(64),
    }]);
    let current = sealed(current, None);
    let raw = format!("{}\n", current);
    assert!(super::super::records_from_raw(Some(&raw)).is_ok());

    let mut unknown = current;
    unknown["workAssociationProtocol"] = json!(super::PROTOCOL + 1);
    let unknown = sealed(unknown, None);
    let raw = format!("{}\n", unknown);
    assert!(super::super::records_from_raw(Some(&raw)).is_err());
}

#[test]
fn same_goal_sealing_preserves_associations_but_successors_start_unbound() {
    let mut previous = goal("same", 1);
    previous["workAssociationProtocol"] = json!(super::PROTOCOL);
    previous["workAssociations"] = json!([{
        "kind": "context_only",
        "work": format!("smw_{}", "a".repeat(64)),
        "startEventSha256": "b".repeat(64),
    }]);
    let previous = sealed(previous, None);

    let mut same = goal("same", 2);
    same["predecessor"] = json!({"goalId":"same", "revision":1});
    let same = sealed(same, Some(&previous));
    assert_eq!(same["workAssociationProtocol"], super::PROTOCOL);
    assert_eq!(same["workAssociations"], previous["workAssociations"]);

    let mut successor = goal("next", 2);
    successor["predecessor"] = json!({"goalId":"same", "revision":1});
    successor["successorOf"] = json!({"goalId":"same", "revision":1});
    let successor = sealed(successor, Some(&previous));
    assert!(successor.get("workAssociationProtocol").is_none());
    assert!(successor.get("workAssociations").is_none());
}

#[test]
fn prestart_admission_reopens_closed_goal_blocks_readiness_and_survives_successor() {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let project_root = std::env::temp_dir().join(format!(
        "exitbind-session-admission-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&project_root).expect("create private test project root");
    let config_path =
        crate::project::onboarding::init_with_options(crate::project::onboarding::InitOptions {
            product_root: project_root.to_str().expect("UTF-8 test project path"),
            coffee: false,
            skip_skills: true,
            mode: Some("portable"),
            project_id: None,
            control_root: None,
            state_root: None,
        })
        .expect("initialize isolated fixture project");
    let loaded = crate::config::load(Some(
        config_path.to_str().expect("UTF-8 fixture config path"),
    ))
    .expect("load isolated fixture project");
    let session_root = project_root.join("native-session-store");
    crate::project::portability::create_private_directory(&session_root)
        .expect("create isolated private native session root");
    crate::project::portability::create_private_directory(
        &session_root.join(crate::project::layout_types::state_namespace()),
    )
    .expect("create isolated private native session journal directory");
    let goal = super::super::incorporate(
        &session_root,
        &loaded,
        "main",
        "Complete the full conversation journey",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("obligations,findings,blockers,decisions,externalActions"),
        true,
    )
    .expect("incorporate native session Goal");
    let closed = super::super::close_direct(&session_root, &loaded, "main", None)
        .expect("close empty journey before the next side Work");
    assert_eq!(closed["closure"]["closed"], true);
    let owner = super::super::native::owner_for(&loaded).expect("fixture owner identity");
    let work = format!("smw_{}", "a".repeat(64));
    let candidate = super::Candidate {
        goal_id: goal["goalId"].as_str().unwrap_or("main").to_owned(),
        revision: closed["revision"].as_u64().expect("closed Goal revision"),
        event_sha256: closed["eventSha256"]
            .as_str()
            .expect("closed Goal event")
            .to_owned(),
        session_key: "b".repeat(64),
        host: "codex",
        owner_project_id: owner.project_id.clone(),
        owner_identity_sha256: owner.identity_sha256,
    };
    let reopened = super::admit(
        &session_root,
        &loaded,
        &candidate,
        &work,
        &"c".repeat(64),
        "A separate side request",
    )
    .expect("record admission before any Work start");
    assert_eq!(reopened["closure"]["closed"], false);
    assert_eq!(reopened["journeyAdmissions"].as_array().unwrap().len(), 1);
    let history = super::super::records(&session_root).expect("read Goal history");
    assert!(history
        .iter()
        .any(|record| record["closure"]["closed"] == true));
    assert!(history.iter().all(|record| record["goalId"] == "main"));
    let readiness = super::super::journey::readiness_for_state(
        &loaded,
        super::super::journey::state(&reopened).expect("validate pending journey"),
    );
    assert_eq!(readiness.state, "blocked");
    assert_eq!(readiness.pending_admissions, 1);
    assert_eq!(
        readiness.reason.as_deref(),
        Some("native_journey_has_unresolved_work_admissions")
    );

    let successor = super::super::incorporate(
        &session_root,
        &loaded,
        "side-follow-up",
        "Keep all requests in this conversation",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("obligations,findings,blockers,decisions,externalActions"),
        true,
    )
    .expect("incorporate explicit successor Goal");
    assert_eq!(successor["journeyMembershipProtocol"], 2);
    assert_eq!(
        successor["journeyAdmissions"],
        reopened["journeyAdmissions"]
    );
    assert_eq!(successor["journeyAdmissions"][0]["parentGoalId"], "main");
    let _ = fs::remove_dir_all(project_root);
}

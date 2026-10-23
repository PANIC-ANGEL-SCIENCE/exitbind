use serde_json::{json, Value};
use std::{env, ffi::OsString, fs};

struct NativeEnvironmentGuard(Vec<(&'static str, Option<OsString>)>);

impl NativeEnvironmentGuard {
    fn codex(identity: &str, store: &std::path::Path) -> Self {
        let names = [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
            "CLAUDE_ENV_FILE",
            "XDG_STATE_HOME",
        ];
        let saved = names
            .iter()
            .map(|name| (*name, env::var_os(name)))
            .collect::<Vec<_>>();
        for name in names {
            env::remove_var(name);
        }
        env::set_var("CODEX_THREAD_ID", identity);
        env::set_var("EXITBIND_SESSION_GOALS_DIR", store);
        Self(saved)
    }
}

impl Drop for NativeEnvironmentGuard {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            match value {
                Some(value) => env::set_var(name, value),
                None => env::remove_var(name),
            }
        }
    }
}

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

fn capacity_fixture(
    sequence: u64,
) -> (
    crate::test_root::TempRoot,
    crate::config::Loaded,
    std::path::PathBuf,
    super::super::native::Session,
    super::super::native::Owner,
    Value,
    String,
    std::path::PathBuf,
) {
    let project_root = crate::test_root::temp("session-capacity");
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
    let identity = format!("capacity-session-{sequence}");
    let session_store = project_root.join("native-session-store");
    crate::project::portability::create_private_directory(&session_store)
        .expect("create isolated private native session store");
    let _environment = NativeEnvironmentGuard::codex(&identity, &session_store);
    let scope = super::super::native::scope(&loaded, true)
        .expect("resolve and own isolated native session");
    let session = scope.session.expect("fixture native session");
    let owner = scope.owner.expect("fixture native owner");
    let session_root = session.root.clone();
    let goal = super::super::incorporate(
        &session_root,
        &loaded,
        "main",
        "Complete the conversation journey",
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
    (
        project_root,
        loaded,
        session_root,
        session,
        owner,
        goal,
        identity,
        session_store,
    )
}

fn capacity_member(
    index: usize,
    loaded: &crate::config::Loaded,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    parent_goal_id: &str,
    parent_event_sha256: &str,
) -> Value {
    json!({
        "work": format!("smw_{index:064x}"),
        "startEventSha256": "a".repeat(64),
        "parentGoalId": parent_goal_id,
        "parentObservedGoalEventSha256": parent_event_sha256,
        "workGoalSha256": "b".repeat(64),
        "host": session.host,
        "sessionKey": session.key,
        "ownerIdentitySha256": owner.identity_sha256,
        "ownerProjectId": owner.project_id,
        "projectIdentitySha256": super::super::requirements::identity(loaded).unwrap(),
        "localIdentitySha256": owner.identity_sha256,
        "configPath": std::fs::canonicalize(&loaded.path)
            .unwrap()
            .to_str()
            .expect("canonical configuration path is UTF-8")
            .to_owned(),
        "configSha256": crate::evidence::hash::text(&loaded.source),
        "productRoot": std::fs::canonicalize(&loaded.product_root)
            .unwrap()
            .to_str()
            .expect("canonical product root is UTF-8")
            .to_owned(),
        "stateRoot": std::fs::canonicalize(&loaded.state_root)
            .unwrap()
            .to_str()
            .expect("canonical state root is UTF-8")
            .to_owned(),
    })
}

fn capacity_admission(
    index: usize,
    loaded: &crate::config::Loaded,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    parent_goal_id: &str,
    parent_event_sha256: &str,
) -> Value {
    json!({
        "work": format!("smw_{index:064x}"),
        "startIntentSha256": "c".repeat(64),
        "parentGoalId": parent_goal_id,
        "parentObservedGoalEventSha256": parent_event_sha256,
        "workGoalSha256": "d".repeat(64),
        "host": session.host,
        "sessionKey": session.key,
        "ownerIdentitySha256": owner.identity_sha256,
        "ownerProjectId": owner.project_id,
        "projectIdentitySha256": super::super::requirements::identity(loaded).unwrap(),
        "localIdentitySha256": owner.identity_sha256,
        "configPath": std::fs::canonicalize(&loaded.path)
            .unwrap()
            .to_str()
            .expect("canonical configuration path is UTF-8")
            .to_owned(),
        "configSha256": crate::evidence::hash::text(&loaded.source),
        "productRoot": std::fs::canonicalize(&loaded.product_root)
            .unwrap()
            .to_str()
            .expect("canonical product root is UTF-8")
            .to_owned(),
        "stateRoot": std::fs::canonicalize(&loaded.state_root)
            .unwrap()
            .to_str()
            .expect("canonical state root is UTF-8")
            .to_owned(),
    })
}

fn install_capacity_state(
    root: &std::path::Path,
    loaded: &crate::config::Loaded,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    members: usize,
    admissions: usize,
) -> Value {
    super::super::mutate(root, |previous| {
        let previous = previous.ok_or("canonical Goal is missing")?;
        let parent_goal_id = previous["goalId"].as_str().ok_or("Goal id is missing")?;
        let parent_event_sha256 = previous["eventSha256"]
            .as_str()
            .ok_or("Goal event identity is missing")?;
        let mut next = previous.clone();
        next["journeyMembershipProtocol"] = json!(2);
        next["journeyMembers"] = json!((0..members)
            .map(|index| capacity_member(
                index,
                loaded,
                session,
                owner,
                parent_goal_id,
                parent_event_sha256
            ))
            .collect::<Vec<_>>());
        next["journeyIntegrations"] = json!([]);
        next["journeyAdmissions"] = json!((members..members + admissions)
            .map(|index| capacity_admission(
                index,
                loaded,
                session,
                owner,
                parent_goal_id,
                parent_event_sha256
            ))
            .collect::<Vec<_>>());
        let revision = previous["revision"]
            .as_u64()
            .ok_or("Goal revision is missing")?;
        next.as_object_mut()
            .ok_or("Goal record is malformed")?
            .remove("eventSha256");
        next["revision"] = json!(revision + 1);
        next["predecessor"] = json!({"goalId":previous["goalId"],"revision":revision});
        next["successorOf"] = Value::Null;
        Ok(super::super::sealed(next, Some(previous)))
    })
    .expect("install bounded synthetic journey occupancy")
}

#[test]
fn pending_admissions_reserve_the_last_member_slot_before_start() {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (project_root, loaded, session_root, session, owner, _, identity, session_store) =
        capacity_fixture(sequence);
    let _project_root = project_root;
    let current = install_capacity_state(&session_root, &loaded, &session, &owner, 31, 1);
    let scope = super::super::native::Scope {
        root: session_root.clone(),
        session: Some(session.clone()),
        owner: Some(owner.clone()),
        owner_matches: true,
    };

    assert!(
        super::prepare(&scope).is_err(),
        "31 members plus one pending admission must reserve the final member slot before another Work starts"
    );

    let candidate = super::Candidate {
        goal_id: current["goalId"].as_str().unwrap().to_owned(),
        revision: current["revision"].as_u64().unwrap(),
        event_sha256: current["eventSha256"].as_str().unwrap().to_owned(),
        session_key: session.key.clone(),
        host: session.host,
        owner_project_id: owner.project_id.clone(),
        owner_identity_sha256: owner.identity_sha256.clone(),
    };
    let attempted = format!("smw_{}", "f".repeat(64));
    assert!(
        super::admit(
            &session_root,
            &loaded,
            &candidate,
            &attempted,
            &"1".repeat(64),
            "A side task must not overbook the bounded journey",
        )
        .is_err(),
        "the locked admission must recheck member capacity"
    );
    assert_eq!(
        super::super::read(&session_root).unwrap().as_ref(),
        Some(&current),
        "a refused admission must leave the Goal journal unchanged"
    );

    // With one fewer pending admission, the actual common Work-start path
    // reserves the last slot, starts, and links member 32 in one operation.
    install_capacity_state(&session_root, &loaded, &session, &owner, 31, 0);
    let _environment = NativeEnvironmentGuard::codex(&identity, &session_store);
    let started = crate::work::begin(
        &loaded,
        crate::work::BeginOptions {
            workflow: "change",
            goal: "Use the final bounded conversation member slot",
            check_command: "true",
            boundary: None,
            harness_receipt: None,
            proof_origin: None,
            preserve_requirement: None,
            preservation_check_command: None,
            preservation_proof_origin: None,
            basis: crate::work::BasisOrigin::Caller(None),
            review_policy: Some("required"),
            auto_goal_context: true,
        },
    )
    .expect("the final reserved member slot remains usable");
    let work = started["work"].as_str().expect("actual Work handle");
    let linked_record = super::super::read(&session_root)
        .unwrap()
        .expect("linked capacity Goal");
    assert_eq!(
        linked_record["journeyMembers"].as_array().unwrap().len(),
        32
    );
    assert_eq!(
        linked_record["journeyAdmissions"].as_array().unwrap().len(),
        0
    );
    let ledger = loaded
        .state_root
        .join(crate::work::resolve(&loaded, work).expect("resolve actual Work ledger"));
    let ledger_before_relink = fs::read(&ledger).expect("read immutable Work start ledger");
    let journal_before_relink = fs::read(
        session_root
            .join(crate::project::layout_types::state_namespace())
            .join("session-goal.jsonl"),
    )
    .expect("read linked Goal journal");
    let relinked = super::link_session(&loaded, work).expect("exact slot-32 relink is idempotent");
    assert_eq!(relinked["state"], "linked");
    assert_eq!(relinked["workLedger"], "unchanged");
    assert_eq!(fs::read(&ledger).unwrap(), ledger_before_relink);
    assert_eq!(
        fs::read(
            session_root
                .join(crate::project::layout_types::state_namespace())
                .join("session-goal.jsonl")
        )
        .unwrap(),
        journal_before_relink,
        "relinking the exact member must not mutate its Goal or Work"
    );
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
    let _sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let project_root = crate::test_root::temp("session-admission");
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

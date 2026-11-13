// Tests grant offers after project-continuation admission.
// A changed selected mandate suppresses the prior offer; see `src/work/readable_tests/grant_offer.rs`.
//! Actual project selection/admission, followed by the canonical form owner.
use super::*;
use crate::project::continuation;

#[test]
fn changed_selected_mandate_suppresses_a_previously_current_grant_offer() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    let (root, initial) = configured("continuation-grant-offer");
    for directory in ["docs", "src", ".planning"] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    const OUTCOME: &str = "Preserve useful records.";
    const OBLIGATION: &str = "Keep existing records.";
    let brief = format!("# Direction\n{OUTCOME}\n{OBLIGATION}\n");
    std::fs::write(root.join("docs/brief.md"), &brief).unwrap();
    std::fs::write(root.join("src/input.txt"), "original records\n").unwrap();
    std::fs::write(root.join("checks.sh"), "test -s src/input.txt\n").unwrap();
    let mut config: Value = serde_json::from_slice(&std::fs::read(&initial.path).unwrap()).unwrap();
    for agent in config["agents"].as_object_mut().unwrap().values_mut() {
        agent["observe"] = json!(["docs", "src", ".planning", "checks.sh"]);
        agent["write"] = json!(["src"]);
        agent["commands"] = json!(["sh checks.sh"]);
    }
    std::fs::write(&initial.path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let loaded = crate::config::load(initial.path.to_str()).unwrap();
    let mut spec = json!({"version":1,"projectId":"records","mandate":{
        "id":"advance-records","revision":1,"outcome":OUTCOME,
        "mode":"continue","allowedSources":["brief"],"write":["src"],
        "commands":["sh checks.sh"],"roles":["lead","worker","reviewer"],
        "preserve":[OBLIGATION],"priorityPolicy":"Preserve useful records first.",
        "expiresAt":"2099-01-01T00:00:00Z","maxWorks":3,"maxActions":12,
        "maxParallel":2,"maxElapsedSeconds":3600,"externalPublication":false},
        "sources":[{"id":"brief","kind":"export","repository":null,
            "revision":"supplied-v1","path":"docs/brief.md","section":"# Direction",
            "sha256":crate::evidence::hash::text(&brief),"acquiredAt":"2026-01-01T00:00:00Z",
            "currentness":"declared_export","trust":"ownerDirection"}]});
    let plan = json!({"version":1,"id":"first-plan","taskId":"preserve-records",
        "title":"Preserve useful records","rationale":"Exercise the existing admission owner.",
        "outcome":OUTCOME,"acceptance":[OBLIGATION],"preservation":[OBLIGATION],
        "sourceIds":["brief"],"dependencies":[],"workflow":"change","write":["src"],
        "checkCommand":"sh checks.sh","preservationCheckCommand":"sh checks.sh",
        "estimatedActions":4,"overlapReason":"No previous claim owns this fixture task.",
        "goalId":"records","requirements":["records"],"checker":"checks.sh","integration":false});
    const SPEC: &str = ".planning/spec.json";
    const PLAN: &str = ".planning/plan.json";
    let write_spec = |spec: &Value| {
        std::fs::write(root.join(SPEC), serde_json::to_vec_pretty(spec).unwrap()).unwrap();
    };
    write_spec(&spec);
    std::fs::write(root.join(PLAN), serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    crate::session_goal::incorporate(
        &loaded.state_root,
        &loaded,
        "records",
        OUTCOME,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("findings,blockers,decisions,externalActions"),
        false,
    )
    .unwrap();
    crate::session_goal::requirements::require(
        &loaded.state_root,
        &loaded,
        "records",
        "records",
        OBLIGATION,
        "docs/brief.md",
    )
    .unwrap();
    crate::session_goal::requirements::cover(&loaded.state_root, &loaded, "records").unwrap();
    let select = || {
        let preview =
            continuation::select(&loaded, SPEC, "Bounded fixture mandate.", None, false).unwrap();
        continuation::select(
            &loaded,
            SPEC,
            "Bounded fixture mandate.",
            preview["currentBinding"].as_str(),
            true,
        )
        .unwrap();
    };
    select();
    let preview = continuation::admit(&loaded, SPEC, PLAN, None, false).unwrap();
    let admitted = continuation::admit(
        &loaded,
        SPEC,
        PLAN,
        preview["currentBinding"].as_str(),
        true,
    )
    .unwrap();
    let work = admitted["work"].as_str().unwrap();
    scope_current_lead(&loaded, work);
    let ledger = crate::work::resolve(&loaded, work).unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &ledger).unwrap();
    let next = crate::work::next_from(&loaded, work, &snapshot).unwrap();
    crate::work::permit_response::permit(
        &loaded,
        work,
        next["assignment"].as_str().unwrap(),
        "Edit the fixture",
        Some("continuation-grant-offer"),
    )
    .unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &ledger).unwrap();
    let next = crate::work::next_from(&loaded, work, &snapshot).unwrap();
    assert!(next["packet"]["projectContinuation"].is_object());
    assert!(crate::work::action_forms::current_worker_grant(&next["packet"]).is_some());
    let history = next["packet"]["context"]["loop"]["currentMutation"].clone();
    spec["mandate"]["revision"] = json!(2);
    spec["mandate"]["expiresAt"] = json!("2099-01-03T00:00:00Z");
    spec["mandate"]["maxElapsedSeconds"] = json!(172800);
    write_spec(&spec);
    select();
    // Use the eligible packet acquired before the changed selection. This
    // proves owner suppression, without bypassing native delivery's guards.
    let (forms, offer) = crate::work::action_forms::full_with_grant_offer(
        &loaded,
        work,
        &next,
        next["current"]["binding"].as_str().unwrap(),
    );
    assert_eq!(forms["state"], "held_time_amendment_required");
    assert!(forms["currentGrant"].is_null());
    assert!(offer.is_none());
    assert_eq!(
        next["packet"]["context"]["loop"]["currentMutation"],
        history
    );
}

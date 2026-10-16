use super::*;

#[test]
fn form_proposal_and_previews_do_not_mutate_state_or_create_work() {
    let mut project = Project::new("continuation-propose", "export");
    project.spec["mandate"]["mode"] = json!("propose");
    project.save_spec();
    let before = project.state();
    let form = project.value(&["project", "advance", "form"]);
    assert_eq!(form["readOnly"], true);
    assert!(form["modes"]
        .as_array()
        .unwrap()
        .contains(&json!("propose")));
    let proposal = project.value(&["project", "advance", "propose", SPEC]);
    assert_eq!(proposal["state"], "proposal");
    assert_eq!(proposal["effect"], "no-change");
    assert_eq!(proposal["sources"][0]["currentness"], "declared_export");
    assert!(proposal["externalProgress"]
        .as_str()
        .unwrap()
        .contains("unchanged"));
    let preview = project.value(&[
        "project",
        "advance",
        "select",
        SPEC,
        "--reason",
        "Bounded proposal only.",
    ]);
    assert_eq!(preview["state"], "preview");
    assert_eq!(project.state(), before);
    project.apply(&preview);
    let selected = project.state();
    project.reject(
        &["project", "advance", "admit", SPEC, "--proposal", PLAN],
        "propose-only",
    );
    assert_eq!(project.state(), selected);
    assert!(project.runs().is_empty());
}

#[test]
fn git_and_export_admission_share_work_resume_and_stable_task_replay() {
    for kind in ["git", "export"] {
        let mut project = Project::new("continuation-replay", kind);
        project.select();
        let selected = project.state();
        let preview = project.preview();
        assert_eq!(preview["state"], "preview");
        assert_eq!(
            project.state(),
            selected,
            "admission preview must be read-only"
        );
        let started = project.apply(&preview);
        assert_eq!(started["state"], "work_started", "{started}");
        let work = started["work"].as_str().unwrap();
        let work_bytes = fs::read(project.work_path(work)).unwrap();
        let status = project.status(); // Each CLI invocation is a fresh process.
        assert_eq!(status["claims"][0]["work"], work);
        assert_eq!(status["claims"][0]["actualWork"]["status"], "running");
        assert_eq!(status["budget"]["actualWorks"], 1);
        let replay = project.value(&["project", "advance", "admit", SPEC, "--proposal", PLAN]);
        assert_eq!(replay["state"], "existing_work");
        assert_eq!(replay["work"], work);
        project.plan["id"] = json!("renamed-plan");
        project.save_plan();
        let renamed = project.value(&["project", "advance", "admit", SPEC, "--proposal", PLAN]);
        assert_eq!(renamed["state"], "existing_work");
        assert_eq!(
            renamed["work"], work,
            "plan label cannot reset stable task ownership"
        );
        assert_eq!(project.runs().len(), 1);
        assert_eq!(fs::read(project.work_path(work)).unwrap(), work_bytes);
        project.plan["acceptance"] = json!(["A materially different result."]);
        project.save_plan();
        let before = project.state();
        project.reject(
            &["project", "advance", "admit", SPEC, "--proposal", PLAN],
            "already claimed with different plan",
        );
        assert_eq!(project.state(), before);
    }
}

#[test]
fn concurrent_claimants_never_create_two_work_ledgers() {
    let project = Project::new("continuation-concurrent", "export");
    project.select();
    let preview = project.preview();
    let barrier = std::sync::Barrier::new(2);
    let outputs = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            project.apply_command(&preview).output().unwrap()
        });
        let second = scope.spawn(|| {
            barrier.wait();
            project.apply_command(&preview).output().unwrap()
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    let mut started = 0;
    for output in outputs {
        if output.status.success() {
            let result = successful(output);
            assert!(
                matches!(
                    result["state"].as_str(),
                    Some("work_started" | "existing_work" | "reserved_unresolved")
                ),
                "{result}"
            );
            if result["state"] == "reserved_unresolved" {
                // The durable claim is visible after releasing the account
                // lock and before its single Work start. The other caller
                // must hold this claim, never wait with the account or start.
                assert_eq!(result["effect"], "no-change");
                assert!(result["unresolved"].is_string());
            }
            started += usize::from(result["state"] == "work_started");
        } else {
            let error = message(&output);
            assert!(
                error.contains("lock")
                    || error.contains("run ledger is busy; no mutation was made")
                    || error.contains("another planner")
                    || error.contains("preview is stale"),
                "{output:?}"
            );
        }
    }
    assert_eq!(started, 1);
    assert_eq!(project.runs().len(), 1);
    let status = project.status();
    assert_eq!(status["claims"].as_array().unwrap().len(), 1);
    let replay = project.value(&["project", "advance", "admit", SPEC, "--proposal", PLAN]);
    assert_eq!(replay["state"], "existing_work");
    assert_eq!(replay["work"], status["claims"][0]["work"]);
}

#[test]
fn unknown_started_effect_stays_reserved_and_is_not_restarted() {
    let project = Project::new("continuation-unknown", "export");
    project.select();
    let started = project.admit();
    let work = started["work"].as_str().unwrap();
    // Controlled loss of an owned fixture ledger models unavailable readback.
    fs::remove_file(project.work_path(work)).unwrap();
    let before = project.state();
    let replay = project.value(&["project", "advance", "admit", SPEC, "--proposal", PLAN]);
    assert_eq!(replay["state"], "reserved_unresolved");
    assert_eq!(replay["work"], work);
    assert!(replay["unresolved"].is_string());
    assert!(!project.work_path(work).exists());
    assert_eq!(project.state(), before);
    assert_eq!(project.status()["budget"]["actualActions"], 1);
    assert_eq!(project.status()["budget"]["estimatedActions"], 4);
}

#[test]
fn admission_requires_existing_goal_coverage_and_accepted_dependencies() {
    let mut project = Project::new("continuation-goal", "export");
    project.select();
    project.plan["goalId"] = json!("invented-goal");
    project.save_plan();
    let before = project.state();
    project.reject(
        &["project", "advance", "admit", SPEC, "--proposal", PLAN],
        "canonical goal identity does not match",
    );
    assert_eq!(project.state(), before);
    project.plan["goalId"] = json!("record-import");
    project.plan["dependencies"] = json!(["missing-task"]);
    project.save_plan();
    project.reject(
        &["project", "advance", "admit", SPEC, "--proposal", PLAN],
        "no prior claim",
    );
    project.plan["dependencies"] = json!([]);
    project.save_plan();
    project.admit();
    project.plan["taskId"] = json!("dependent-task");
    project.plan["id"] = json!("dependent-plan");
    project.plan["dependencies"] = json!(["preserve-records"]);
    project.save_plan();
    let before = project.state();
    project.reject(
        &["project", "advance", "admit", SPEC, "--proposal", PLAN],
        "requires current accepted Work evidence",
    );
    assert_eq!(project.state(), before);
    assert_eq!(project.runs().len(), 1);
}

#[test]
fn accepted_work_requires_current_check_inputs_before_a_dependent_start() {
    let mut project = Project::new("continuation-accepted-drift", "export");
    project.select();
    let admitted = project.admit();
    let work = admitted["work"].as_str().unwrap();
    project.accept_fixture_work(work);
    let accepted = project.status();
    assert_eq!(accepted["claims"][0]["actualWork"]["status"], "accepted");
    assert_eq!(
        accepted["claims"][0]["actualWork"]["currentEvidence"]["state"],
        "current"
    );
    project.plan["taskId"] = json!("dependent-task");
    project.plan["id"] = json!("dependent-plan");
    project.plan["dependencies"] = json!(["preserve-records"]);
    project.save_plan();
    assert_eq!(
        project.preview()["state"],
        "preview",
        "current accepted dependency should be eligible"
    );
    fs::write(
        project.root.join("src/import.txt"),
        "changed after acceptance\n",
    )
    .unwrap();
    let original_work = fs::read(project.work_path(work)).unwrap();
    let before = project.state();
    project.reject(
        &["project", "advance", "admit", SPEC, "--proposal", PLAN],
        "requires current accepted Work evidence",
    );
    assert_eq!(project.state(), before);
    assert_eq!(fs::read(project.work_path(work)).unwrap(), original_work);
    let stale = project.status();
    assert_eq!(
        stale["claims"][0]["actualWork"]["status"], "accepted",
        "historical lifecycle is preserved"
    );
    assert_ne!(
        stale["claims"][0]["actualWork"]["currentEvidence"]["state"],
        "current"
    );
    assert_eq!(project.runs().len(), 1);
}

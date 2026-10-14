use super::*;

#[test]
fn checker_coverage_and_intake_are_guarded_before_start() {
    let p = Project::new("goal-checker-coverage");
    p.reject(
        &[
            "goal",
            "require",
            "--goal-id",
            "missing",
            "--requirement",
            "R1",
            "--obligation",
            "x",
            "--artifact",
            "missing.txt",
        ],
        "source",
    );
    p.setup();
    p.reject(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R4",
            "--obligation",
            "invented requirement",
            "--artifact",
            "request.txt",
        ],
        "exact excerpt",
    );
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "wrong checker",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
        "name its declared",
    );
    let mut git = Command::new("git");
    git.current_dir(&p.root).args(["init", "-q"]);
    assert!(support::run(&mut git).status.success());
    p.file(".gitignore", "check.sh\n");
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "ignored checker",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh R1",
            "--review-policy",
            "required",
        ],
        "outside tested",
    );
    assert_eq!(row(&p.status(), "R1")["state"], "unmapped");
    for child in ["require", "cover", "assign", "status"] {
        let help = p.call(&["goal", child, "--help"], None);
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains(child));
    }
}

#[test]
fn relocated_project_cannot_reuse_a_named_goal_identity() {
    let mut p = Project::new("goal-project-identity");
    p.setup();
    let relocated = support::temp("goal-other-root");
    fs::remove_dir(&relocated).unwrap();
    fs::rename(&p.root, &relocated).unwrap();
    p.root = relocated;
    p.reject(&["goal", "status", "--json"], "another project");
}

#[test]
fn full_mapping_of_individual_passes_is_not_an_integration_declaration() {
    let p = Project::new("goal-integration-declaration");
    p.setup();
    let work = p.begin(
        "R1,R2,R3",
        "sh check.sh R1 && sh check.sh R2 && sh check.sh R3",
    );
    p.drive(&work, true);
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        "integration Work",
    );
    let next = p.status()["requirements"]["nextAction"]["command"].clone();
    assert!(next
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "integration"));
}

#[test]
fn ordinary_work_without_basis_has_no_named_projection_diagnostic() {
    let p = Project::new("ordinary-work-without-basis");
    let work = p.begin_ordinary("Make an ordinary change", "true", &[]);
    let detail = p.value(&["work", "detail", &work, "--json"], None);
    assert_eq!(detail["valid"], true);
    assert_eq!(detail["complete"], true);
    assert!(
        detail.get("projectGoalProgress").is_none(),
        "absent optional named-contract metadata is not malformed: {}",
        detail["projectGoalProgress"]
    );
}

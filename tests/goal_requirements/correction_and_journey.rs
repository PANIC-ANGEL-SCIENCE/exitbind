use super::*;

#[test]
fn correction_retains_meaning_history_and_refuses_unrelated_or_old_work() {
    let p = Project::new("goal-correction");
    p.setup();
    let a = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&a, true);
    let before = p.status()["revision"].clone();
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            REQUIREMENTS[0].1,
            "--artifact",
            "request.txt",
        ],
        None,
    );
    assert_eq!(p.status()["revision"], before);
    let corrected = "Reject malformed input before persistence.";
    p.file(
        "request.txt",
        &format!(
            "{corrected}\n{}\n{}\n",
            REQUIREMENTS[1].1, REQUIREMENTS[2].1
        ),
    );
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            corrected,
            "--artifact",
            "request.txt",
        ],
        None,
    );
    let changed = p.status();
    assert_eq!(row(&changed, "R1")["revision"], 2);
    assert_eq!(row(&changed, "R1")["history"][0]["text"], REQUIREMENTS[0].1);
    assert_eq!(changed["goalProgress"]["decomposition"]["total"], 3);
    assert_eq!(changed["requirements"]["coverageConfirmed"], false);
    p.reject(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--result-ref",
            &a,
        ],
        "exact requirement revision",
    );
    let unrelated = p.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "unrelated pass",
            "--check-command",
            "true",
            "--review-policy",
            "omitted",
        ],
        None,
    )["work"]
        .as_str()
        .unwrap()
        .to_owned();
    p.drive(&unrelated, true);
    p.reject(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R2",
            "--result-ref",
            &unrelated,
        ],
        "frozen named requirement",
    );
    p.reject(
        &["goal", "close", "--goal-id", "import", "--direct"],
        "named requirements",
    );
}

#[test]
fn journey_integration_refreshes_only_the_frozen_accepted_member_set() {
    let p = Project::new("journey-integration-freeze")
        .with_native_session("journey-integration-session");
    p.setup();

    let first = p.begin("R1", "sh check.sh R1");
    p.drive(&first, true);
    assert_eq!(
        p.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    // A later source change makes the accepted first Work historically valid
    // but no longer current against this project's checked inputs.
    p.file("format.txt", "|");
    p.file("data.txt", "alpha|x\nalpha|y\nbeta|x\n");
    let integration = p.begin_ordinary(
        "Revalidate the accepted conversation work on current inputs",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--scope",
            "integration",
            "--journey-integration",
        ],
    );
    p.drive(&integration, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integration,
            "--disposition",
            "replace",
        ],
        None,
    );
    let supported = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(
        supported["state"], "current",
        "journey after integration: {supported}"
    );
    assert_eq!(supported["members"]["total"], 1);
    assert_eq!(supported["members"]["current"], 0);
    assert_eq!(supported["members"]["integrationSupported"], 1);
    assert_eq!(supported["integrationWork"], integration);

    // A later ordinary side Work is new membership, so the earlier frozen
    // integration no longer covers the complete journey.
    let second = p.begin_ordinary(
        "Complete a separate side task in the same conversation",
        "true",
        &[],
    );
    p.drive(&second, true);
    let expanded = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(expanded["state"], "blocked");
    assert_eq!(expanded["members"]["total"], 2);
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &integration,
        ],
        "journey",
    );

    let updated = p.begin_ordinary(
        "Revalidate every accepted member after the side task",
        "sh check.sh all",
        &[
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--scope",
            "integration",
            "--journey-integration",
        ],
    );
    p.drive(&updated, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &updated,
            "--disposition",
            "replace",
        ],
        None,
    );
    let current = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(current["state"], "current");
    assert_eq!(current["members"]["total"], 2);
    assert_eq!(current["members"]["integrationSupported"], 1);
    assert_eq!(current["integrationWork"], updated);
    let closed = p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &updated,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);

    // A new unaccepted member cannot be included by starting integration.
    let pending = p.begin_ordinary("A final side task still needs acceptance", "false", &[]);
    p.drive(&pending, false);
    let blocked = p.status()["goalProgress"]["journeyCompletion"].clone();
    assert_eq!(blocked["state"], "blocked");
    let after_pending = p.status();
    assert_eq!(after_pending["closure"]["closed"], false);
    assert!(after_pending["presentation"]["terminal"].is_null());
    p.reject(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "This must not invent acceptance for the pending task",
            "--check-command",
            "sh check.sh all",
            "--artifact",
            "check.sh",
            "--journey-integration",
            "--review-policy",
            "required",
        ],
        "no terminal accepted result",
    );
}

#[test]
fn native_store_snapshot_distinguishes_missing_and_empty_roots() {
    let project = Project::new("native-store-empty-root-snapshot");
    let missing = project.native_store_snapshot();
    assert_eq!(missing, (false, Vec::new()));

    fs::create_dir_all(&project.fixture_store).unwrap();
    let empty = project.native_store_snapshot();
    assert_eq!(empty, (true, Vec::new()));
    assert_ne!(missing, empty);
}

#[test]
fn copied_journey_basis_markers_are_rejected_before_any_work_or_goal_mutation() {
    let p = Project::new("journey-marker-origin-boundary")
        .with_native_session("journey-marker-origin-session");
    p.file(
        "request.txt",
        "R1: reject invalid input before persistence.\n",
    );
    p.file("check.sh", "set -eu\nexit 0\n");
    p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "import",
            "--goal",
            "Validate input safely",
            "--none-applicable",
            "findings,blockers,decisions,externalActions",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "require",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--obligation",
            "R1: reject invalid input before persistence.",
            "--artifact",
            "request.txt",
        ],
        None,
    );
    p.value(&["goal", "cover", "--goal-id", "import"], None);

    let first = p.begin_ordinary("Accepted main conversation task", "sh check.sh", &[]);
    p.drive(&first, true);
    let integration = p.begin_ordinary(
        "Create a legitimate frozen journey integration basis",
        "sh check.sh",
        &["--artifact", "check.sh", "--journey-integration"],
    );
    let basis = p.start_basis(&integration);
    let parsed_basis: Value = serde_json::from_str(&basis).unwrap();
    let constraints = parsed_basis["constraints"].as_array().unwrap();
    assert!(constraints.iter().any(|item| item
        .as_str()
        .is_some_and(|value| { value.starts_with("exitbind.session-journey-integration.") })));
    assert!(constraints.iter().all(|item| !item
        .as_str()
        .is_some_and(|value| value.starts_with("exitbind.goal-support.v1:"))));

    p.value(
        &[
            "goal",
            "incorporate",
            "--project",
            "--goal-id",
            "legacy",
            "--goal",
            "Project-local named contract",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "require",
            "--project",
            "--goal-id",
            "legacy",
            "--requirement",
            "P1",
            "--obligation",
            "R1: reject invalid input before persistence.",
            "--artifact",
            "request.txt",
        ],
        None,
    );
    p.value(&["goal", "cover", "--project", "--goal-id", "legacy"], None);

    let ordinary = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&ordinary);

    let standalone = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--check-command",
        "true",
        "--review-policy",
        "required",
        "--standalone",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&standalone);

    let native_named = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
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
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&native_named);

    let project_named = [
        "work",
        "begin",
        "change",
        "--goal",
        "Reject a copied reserved marker",
        "--goal-id",
        "legacy",
        "--requirement",
        "P1",
        "--artifact",
        "check.sh",
        "--check-command",
        "sh check.sh R1",
        "--review-policy",
        "required",
        "--project-goal",
        "--basis",
        basis.as_str(),
    ];
    p.assert_marker_refusal_without_effects(&project_named);

    let no_identity = Project::new("journey-marker-no-native-identity");
    no_identity.assert_marker_refusal_without_effects(&ordinary);
    let native_without_goal = Project::new("journey-marker-native-without-goal")
        .with_native_session("journey-marker-native-without-goal-session");
    native_without_goal.assert_marker_refusal_without_effects(&ordinary);
}

#[test]
fn a_closed_predecessor_cannot_emit_terminal_after_a_new_session_goal_is_incorporated() {
    let p = Project::new("journey-successor-suppresses-old-room")
        .with_native_session("journey-successor-session");
    p.setup();
    let work = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&work, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &work,
            "--disposition",
            "replace",
        ],
        None,
    );
    let closed = p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &work,
        ],
        None,
    );
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    assert_eq!(p.status()["presentation"]["terminal"], TERMINAL);
    let prior_next = p.value(&["work", "next", &work, "--json", "--full"], None);
    assert_eq!(prior_next["presentation"]["terminal"], TERMINAL);

    let successor = p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "successor",
            "--goal",
            "A later unresolved side request in the same conversation",
            "--obligation",
            "The later request still needs work",
        ],
        None,
    );
    assert_eq!(successor["journeyMembers"].as_array().unwrap().len(), 1);
    assert_eq!(successor["journeyMembers"][0]["work"], work);
    assert_eq!(successor["journeyMembers"][0]["parentGoalId"], "import");

    let old_next = p.value(&["work", "next", &work, "--json", "--full"], None);
    let old_detail = p.value(&["work", "detail", &work, "--json"], None);
    assert!(old_next["presentation"]["terminal"].is_null());
    assert_eq!(
        old_next["presentation"]["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert_eq!(old_detail["goalAssociation"]["kind"], "context_only");
    assert_eq!(
        old_detail["sections"]["tasks"]["goal"],
        "Import names safely and show a count"
    );
    assert_eq!(old_detail["sections"]["tasks"]["overall"], "in_progress");
    assert_eq!(p.status()["goalId"], "successor");
    assert!(p.status()["presentation"]["terminal"].is_null());
}

use super::*;

#[test]
fn native_generic_owner_close_and_status_render_only_verified_main_and_side_work() {
    let mut p = Project::new("native-generic-owner-terminal");

    // This accepted Work predates the Goal and remains an explicit closure
    // result only; it is never retroactively inserted into journey membership.
    p.file("history.txt", "original evidence\n");
    let historical = p.begin_ordinary(
        "Validate the existing main-task evidence",
        "test \"$(cat history.txt)\" = 'original evidence'",
        &[],
    );
    p.drive(&historical, true);
    p.set_native_session("native-generic-owner-terminal-session");
    let empty = p.status();
    assert!(empty["presentation"]["terminal"].is_null());

    let incorporated = incorporate_native_generic_goal(
        &p,
        "native-main",
        "Complete the main task and all side tasks in this conversation",
    );
    assert_eq!(incorporated["journeyMembershipProtocol"], 2);
    assert!(incorporated["journeyMembers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(incorporated.get("requirements").is_none());

    let side = p.begin_ordinary("Complete a side task in this conversation", "true", &[]);
    p.drive(&side, true);
    let current = p.status();
    let journey = &current["goalProgress"]["journeyCompletion"];
    assert_eq!(journey["state"], "current");
    assert_eq!(journey["members"]["total"], 1);
    assert_eq!(journey["members"]["current"], 1);
    let members = current["journeyMembers"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["work"], side);
    assert!(!members.iter().any(|member| member["work"] == historical));
    assert!(current.get("requirements").is_none());

    let close = p.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        None,
    );
    assert!(close.status.success(), "{close:?}");
    assert_eq!(
        String::from_utf8(close.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );
    let closed = p.status();
    assert_eq!(closed["closure"]["closed"], true);
    assert_eq!(closed["currentReadiness"]["state"], "current");
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    assert!(closed.get("requirements").is_none());
    let status = p.call(&["goal", "status"], None);
    assert!(status.status.success(), "{status:?}");
    assert_eq!(
        String::from_utf8(status.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );

    // The explicit legacy selector keeps project-local status and closure
    // separate from this native session journal.
    let local = p.value(&["goal", "status", "--project", "--json"], None);
    assert!(local["presentation"]["terminal"].is_null());
    p.reject(
        &[
            "goal",
            "close",
            "--project",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        "no canonical session goal is open",
    );

    // A different configured project can read only the context summary and
    // cannot close the owner journal.
    let (identity, store) = p.native_session.as_ref().unwrap();
    let mut child = Project::new("native-generic-nonowner");
    child.native_session = Some((identity.clone(), store.clone()));
    let child_status = child.value(&["goal", "status", "--json"], None);
    assert_eq!(child_status["contextOnly"], true);
    assert_eq!(child_status["authority"], "none");
    assert!(child_status["presentation"]["terminal"].is_null());
    child.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        "owned by another configured project",
    );
    child.native_session = None;

    // Starting a later failed side task reopens whole-journey readiness;
    // the prior closure remains historical and cannot emit the terminal.
    let pending = p.begin_ordinary("A later side task remains unfinished", "false", &[]);
    p.drive(&pending, false);
    let blocked = p.status();
    assert_eq!(blocked["closure"]["closed"], false);
    assert_eq!(
        blocked["goalProgress"]["journeyCompletion"]["state"],
        "blocked"
    );
    assert!(blocked["presentation"]["terminal"].is_null());
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "native-main",
            "--result-ref",
            &historical,
        ],
        "journey",
    );
}

#[test]
fn native_generic_owner_terminal_suppresses_stale_unadmitted_result_proof() {
    let mut p = Project::new("native-generic-stale-proof");
    p.file("proof.txt", "original\n");
    let historical = p.begin_ordinary(
        "Validate the original proof source",
        "test \"$(cat proof.txt)\" = original",
        &[],
    );
    p.drive(&historical, true);
    p.set_native_session("native-generic-stale-proof-session");

    let incorporated = incorporate_native_generic_goal(
        &p,
        "proof-goal",
        "Finish the main task using current accepted evidence",
    );
    assert!(incorporated["journeyMembers"]
        .as_array()
        .unwrap()
        .is_empty());
    let closed = p.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "proof-goal",
            "--result-ref",
            &historical,
        ],
        None,
    );
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    assert_eq!(
        p.status()["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );

    p.file("proof.txt", "changed after acceptance\n");
    let stale = p.status();
    assert_eq!(stale["closure"]["closed"], true);
    assert_eq!(stale["currentReadiness"]["state"], "stale");
    assert_eq!(
        stale["goalProgress"]["journeyCompletion"]["state"],
        "current"
    );
    assert!(stale["presentation"]["terminal"].is_null());
}

#[test]
fn native_direct_closure_does_not_expose_the_governed_terminal() {
    let p = Project::new("native-direct-terminal-boundary")
        .with_native_session("native-direct-terminal-boundary-session");
    p.value(
        &[
            "goal",
            "incorporate",
            "--direct",
            "--goal-id",
            "direct-goal",
            "--goal",
            "Complete this direct task",
            "--finding",
            "The lead completed the direct task",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "direct-goal",
            "--goal",
            "Complete this direct task",
            "--none-applicable",
            "obligations,blockers,decisions,externalActions",
        ],
        None,
    );
    let closed = p.value(
        &["goal", "close", "--direct", "--goal-id", "direct-goal"],
        None,
    );
    assert_eq!(closed["closure"]["kind"], "direct");
    assert!(closed["presentation"]["terminal"].is_null());
    let status = p.status();
    assert_eq!(status["closure"]["kind"], "direct");
    assert!(status["presentation"]["terminal"].is_null());
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("EXIT READY"));
}

#[test]
fn three_requirements_multiple_works_interruption_drift_composition_and_closure() {
    let p = Project::new("goal-import-scenario");
    p.setup();
    assert_eq!(row(&p.status(), "R3")["state"], "unmapped");
    let a = p.begin("R1,R2", "sh check.sh R1 && sh check.sh R2");
    p.drive(&a, true);
    let partial = p.status();
    assert_eq!(row(&partial, "R1")["state"], "current");
    assert_eq!(row(&partial, "R2")["state"], "current");
    assert_eq!(row(&partial, "R3")["state"], "unmapped");
    assert_eq!(partial["requirements"]["coverageCurrent"], false);
    assert!(partial["presentation"]["terminal"].is_null());
    p.reject(
        &["goal", "close", "--goal-id", "import", "--result-ref", &a],
        "coverage",
    );
    // Every call is a new CLI process. Recovery uses only persisted supported views.
    let recovered = p.status();
    assert_eq!(recovered["revision"], partial["revision"]);
    let detail = p.value(&["work", "next", &a, "--json", "--full"], None);
    assert_eq!(detail["next"]["goalProgress"]["decomposition"]["total"], 3);
    assert!(detail["presentation"]["terminal"].is_null());
    let compact = p.value(&["work", "next", &a, "--json"], None);
    assert!(compact["presentation"]["terminal"].is_null());
    let b = p.begin("R3", "sh check.sh R3");
    p.drive(&b, true);
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    assert!(p.status()["presentation"]["terminal"].is_null());
    p.reject(
        &["goal", "close", "--goal-id", "import", "--result-ref", &b],
        "integration Work",
    );
    // The producer now uses another format. Unit checks still pass, composition fails.
    p.file("format.txt", "|");
    assert_eq!(row(&p.status(), "R1")["state"], "stale");
    let c = p.begin("R1,R2", "sh check.sh R1 && sh check.sh R2");
    p.drive(&c, true);
    let d = p.begin("R3", "sh check.sh R3");
    p.drive(&d, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2",
            "--result-ref",
            &c,
            "--disposition",
            "replace",
        ],
        None,
    );
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R3",
            "--result-ref",
            &d,
            "--disposition",
            "replace",
        ],
        None,
    );
    assert_eq!(p.status()["requirements"]["coverageCurrent"], true);
    let failed = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&failed, false);
    assert_eq!(row(&p.status(), "R1")["state"], "failed");
    p.reject(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &failed,
        ],
        "coverage",
    );
    // Repair shared data, create a legitimate validation Work, keep old Works immutable.
    p.file("data.txt", "alpha|x\nalpha|y\nbeta|x\n");
    let integrated = p.begin("R1,R2,R3", "sh check.sh all");
    p.drive(&integrated, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integrated,
            "--disposition",
            "replace",
        ],
        None,
    );
    let before_repeat = p.status()["revision"].clone();
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--result-ref",
            &integrated,
            "--disposition",
            "replace",
        ],
        None,
    );
    assert_eq!(p.status()["revision"], before_repeat);
    let close = p.call(
        &[
            "goal",
            "close",
            "--goal-id",
            "import",
            "--result-ref",
            &integrated,
        ],
        None,
    );
    assert!(close.status.success(), "{close:?}");
    assert_eq!(
        String::from_utf8(close.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );
    let closed = p.status();
    assert_eq!(closed["currentReadiness"]["state"], "current");
    assert_eq!(closed["presentation"]["terminal"], TERMINAL);
    let work = p.value(&["work", "next", &integrated, "--json"], None);
    let full = p.value(&["work", "next", &integrated, "--json", "--full"], None);
    let after_read = p.status();
    assert_eq!(
        work["presentation"]["terminal"],
        TERMINAL,
        "{}",
        json!({"compactPresentation":work["presentation"],
            "fullPresentation":full["presentation"], "currentReadiness":after_read["currentReadiness"],
            "goalTerminal":after_read["presentation"]["terminal"], "reason":work["reason"],
            "work":integrated, "mappedWorks":after_read["continuation"]["mappings"]
                .as_array().map(|items|items.iter().map(|item|item["work"].clone()).collect::<Vec<_>>())})
    );
    assert_eq!(full["presentation"]["terminal"], TERMINAL);
    let repeat = p.call(&["goal", "status"], None);
    assert!(repeat.status.success(), "{repeat:?}");
    assert_eq!(
        String::from_utf8(repeat.stdout).unwrap(),
        format!("{TERMINAL}\n")
    );
    assert_eq!(p.status()["revision"], closed["revision"]);
    for (id, _) in REQUIREMENTS {
        let evidence = &row(&closed, id)["support"][0]["evidence"];
        assert_eq!(evidence["check"]["acquisition"], "observed");
        assert_eq!(evidence["accepted"], true);
    }
    println!("Process-only scenario: 3 requirements; multi-Work; partial; process recovery; shared-input drift; failed composition; new checked integration; fixture review/Lead; explicit current closure.");
    p.file("format.txt", ",");
    let stale = p.status();
    assert_eq!(stale["closure"], closed["closure"]);
    assert_eq!(stale["currentReadiness"]["state"], "stale");
    assert!(stale["presentation"]["terminal"].is_null());
    let work = p.value(&["work", "next", &integrated, "--json"], None);
    assert!(work["presentation"]["terminal"].is_null());
}

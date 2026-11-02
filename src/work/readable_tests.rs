//! Focused current-detail and native Goal race controls.
use super::*;

fn configured(label: &str) -> (crate::test_root::TempRoot, Loaded) {
    let root = crate::test_root::temp(label);
    let mut git = std::process::Command::new("git");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        git.env_remove(key);
    }
    assert!(git
        .args(["-C", root.to_str().unwrap(), "init", "--quiet"])
        .status()
        .unwrap()
        .success());
    let config =
        crate::project::onboarding::init_with_options(crate::project::onboarding::InitOptions {
            product_root: root.to_str().unwrap(),
            coffee: false,
            skip_skills: true,
            mode: Some("portable"),
            project_id: None,
            control_root: None,
            state_root: None,
        })
        .unwrap();
    let loaded = crate::config::load(config.to_str()).unwrap();
    std::fs::write(root.join("AGENTS.md"), "before\n").unwrap();
    (root, loaded)
}

fn begin(loaded: &Loaded, check: &str, auto_goal_context: bool) -> String {
    let begun = crate::work::begin(
        loaded,
        crate::work::BeginOptions {
            workflow: "change",
            goal: "mid-read detail",
            check_command: check,
            boundary: None,
            harness_receipt: None,
            proof_origin: None,
            preserve_requirement: None,
            preservation_check_command: None,
            preservation_proof_origin: None,
            basis: crate::work::BasisOrigin::Caller(None),
            review_policy: Some("required"),
            auto_goal_context,
        },
    )
    .unwrap();
    begun["work"].as_str().unwrap().to_owned()
}

fn fixture(label: &str, check: &str) -> (crate::test_root::TempRoot, Loaded, String) {
    let (root, loaded) = configured(label);
    let work = begin(&loaded, check, false);
    (root, loaded, work)
}

fn revise_goal(
    loaded: &Loaded,
    root: &std::path::Path,
    goal_id: &str,
    obligation: &str,
    native_session: bool,
) -> Result<Value, String> {
    crate::session_goal::incorporate(
        root,
        loaded,
        goal_id,
        "Bound fixture Goal",
        Some(obligation),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("findings,blockers,decisions,externalActions"),
        native_session,
    )
}

#[test]
fn rules_changed_during_actual_detail_assembly_are_refused() {
    // Grouped current-Work delivery is not a historical Soulmate protocol.
    if !crate::compatibility::is_exitbind() {
        return;
    }
    let (root, loaded, work) = fixture("detail-mid-read", "true");
    let snapshot =
        RunSnapshot::capture(&loaded, &crate::work::resolve(&loaded, &work).unwrap()).unwrap();
    let next = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
    let error = read_after_reads(&loaded, &work, &snapshot, &next, || {
        std::fs::write(root.join("AGENTS.md"), "after\n").map_err(|e| e.to_string())
    })
    .unwrap_err();
    assert!(error.contains("recipient instructions changed"), "{error}");
}

#[test]
fn goal_revision_during_detail_assembly_refuses_unchanged_assignment() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_NATIVE_GOAL_READ_RACE";
    if std::env::var_os(CHILD).is_none() {
        let outer = crate::test_root::temp("native-goal-read-child");
        let store = outer.join("session-store");
        crate::project::portability::create_private_directory(&store).unwrap();
        // raw-spawn: run this unit test with test-owned native selectors and storage.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        for key in [
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "EXITBIND_NATIVE_SESSION_ID",
            "EXITBIND_SESSION_GOALS_DIR",
            "CLAUDE_ENV_FILE",
            "XDG_STATE_HOME",
        ] {
            child.env_remove(key);
        }
        let output = child
                .args([
                    "work::readable::tests::goal_revision_during_detail_assembly_refuses_unchanged_assignment",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .env("CLAUDE_CODE_SESSION_ID", "fixture-native-goal-read-race")
                .env("EXITBIND_SESSION_GOALS_DIR", &store)
                .env("TMPDIR", &*outer)
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    let (_root, loaded) = configured("goal-mid-read");
    let scope = crate::session_goal::native::scope(&loaded, true).unwrap();
    assert!(scope.is_session());
    scope.require_owner().unwrap();
    let goal_root = scope.root;
    let goal_id = "fixture-native-read-goal";
    revise_goal(&loaded, &goal_root, goal_id, "Original obligation", true).unwrap();
    let work = begin(&loaded, "true", true);
    let snapshot =
        RunSnapshot::capture(&loaded, &crate::work::resolve(&loaded, &work).unwrap()).unwrap();
    let next = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
    let assignment = next["assignment"].clone();
    let original_binding = next["current"]["binding"].clone();
    let error = read_after_reads(&loaded, &work, &snapshot, &next, || {
        revise_goal(
            &loaded,
            &goal_root,
            goal_id,
            "Later independent obligation",
            true,
        )
        .map(|_| ())
    })
    .unwrap_err();
    assert!(error.contains("work detail changed"), "{error}");
    let fresh =
        RunSnapshot::capture(&loaded, &crate::work::resolve(&loaded, &work).unwrap()).unwrap();
    let fresh = crate::work::next_from(&loaded, &work, &fresh).unwrap();
    assert_eq!(fresh["assignment"], assignment);
    assert_ne!(fresh["current"]["binding"], original_binding);
}

#[test]
fn selected_check_snapshot_refuses_changes_during_detail_assembly() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    for change in ["ledger", "config", "profile"] {
        let (root, loaded, work) = fixture(&format!("detail-mid-{change}"), "true --mode strict");
        let snapshot =
            RunSnapshot::capture(&loaded, &crate::work::resolve(&loaded, &work).unwrap()).unwrap();
        let next = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
        let (assignment, command) =
            crate::work::details::required_assignment_with_check(&loaded, &work, &next).unwrap();
        assert_eq!(command, json!("true --mode strict"));
        assert_eq!(assignment["binding"], next["current"]["binding"]);
        assert_eq!(
            assignment["recipientSha256"],
            json!(crate::work::details::recipient_fingerprint(&loaded, &next).unwrap())
        );
        assert_eq!(
            assignment,
            crate::work::details::required_assignment(&loaded, &work, &next).unwrap()
        );
        let error = read_after_reads(&loaded, &work, &snapshot, &next, || match change {
            "ledger" => crate::work::return_result_impl::return_result_with(
                &loaded,
                &work,
                next["assignment"].as_str().unwrap(),
                "scoped",
                None,
                None,
                None,
                Some(b"Scoped fixture work.\n".to_vec()),
                None,
            )
            .map(|_| ()),
            "config" => std::fs::write(&loaded.path, format!("{}\n", loaded.source))
                .map_err(|error| error.to_string()),
            "profile" => {
                let agent = loaded.agent(next["agent"].as_str().unwrap()).unwrap();
                std::fs::write(root.join(&agent.profile), "Changed profile.\n")
                    .map_err(|error| error.to_string())
            }
            _ => unreachable!(),
        })
        .unwrap_err();
        match change {
            "ledger" => assert!(error.contains("work detail changed"), "{error}"),
            "config" => assert!(
                error.starts_with("configuration ")
                    && error.contains("during work detail consumption"),
                "{error}"
            ),
            "profile" => assert!(
                error.contains("recipient") && error.contains("changed"),
                "{error}"
            ),
            _ => unreachable!(),
        }
    }
}

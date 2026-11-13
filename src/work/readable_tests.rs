// Tests public Work detail aliases and native required facts.
// Unequal residual facts remain in these cases; see `src/work/readable.rs`.
//! Focused current-detail and native Goal race controls.
use super::*;

#[cfg(unix)]
#[path = "readable_tests/readonly.rs"]
mod readonly;

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

#[test]
fn embedded_delivery_hides_reference_transport_without_changing_full_detail() {
    let reference = json!({"itemId":"item","sourceSha256":"hash",
        "sourcePath":"private/source","ledgerPath":"private/ledger",
        "contextSelection":{"root":"private/root","path":"private/path"}});
    let recipient = json!({"profile":{"path":"agent.md"},
        "guidance":{"items":[{"reference":reference,"content":{"path":"body, not a reference"}}],
            "overflow":[{"reference":reference,"readRoute":{"command":["read","item"]}}]},
        "projectLessons":{"items":[{"reference":reference,"lesson":{"fact":"keep"}}],
            "detail":{"command":["resolve","lesson"]}},
        "specialistExperience":{"snapshotId":"canonical-hash",
            "experience":[{"reference":reference,"content":"keep"}]}});
    let mut direct = json!({"kind":"work_detail","recipientContext":recipient});
    let unchanged = direct.clone();
    public_embedded_delivery(&mut direct);
    assert_eq!(direct, unchanged);

    let mut step = json!({"currentDelivery":direct});
    public_embedded_delivery(&mut step);
    let public = &step["currentDelivery"]["recipientContext"];
    for pointer in [
        "/guidance/items/0/reference",
        "/guidance/overflow/0/reference",
        "/projectLessons/items/0/reference",
        "/specialistExperience/experience/0/reference",
    ] {
        let projected = public.pointer(pointer).unwrap();
        assert_eq!(projected["itemId"], "item");
        assert_eq!(projected["sourceSha256"], "hash");
        assert!(projected.get("sourcePath").is_none());
        assert!(projected.get("ledgerPath").is_none());
        assert!(projected["contextSelection"].get("root").is_none());
        assert!(projected["contextSelection"].get("path").is_none());
    }
    assert_eq!(public["profile"]["path"], "agent.md");
    assert_eq!(
        public["guidance"]["items"][0]["content"]["path"],
        "body, not a reference"
    );
    assert_eq!(
        public["guidance"]["overflow"][0]["readRoute"]["command"],
        json!(["read", "item"])
    );
    assert_eq!(
        public["projectLessons"]["detail"]["command"],
        json!(["resolve", "lesson"])
    );
    assert_eq!(
        public["specialistExperience"]["snapshotId"],
        "canonical-hash"
    );
}

#[test]
fn public_detail_removes_only_recorded_equal_residual_recovery_facts() {
    let original = json!({"kind":"work_detail", "complete":true,
        "sections":{"assignment":{"projection":{"equalAliases":[]},
            "residual":{"context":{"digest":"original digest",
                "goal":"a long goal ".repeat(30), "scope":{"owner":"lead"},
                "subject":{"sha256":"a".repeat(64)}, "evidence":[],
                "obligations":["check"], "loop":{"state":"ready"},
                "next":{"action":"spawn"},
                "recovery":{"goal":"a long goal ".repeat(30),
                    "scope":{"owner":"lead"}, "subject":{"sha256":"different"},
                    "current":[], "missing":["check"],
                    "next":{"action":"different"}, "stale":["retain"]}}}},
            "evidence":{"complete":true,"items":[{"content":"full evidence"}]}},
        "recipientContext":{"complete":true,"profile":{"content":"full profile"}},
        "actionForms":{"choices":["keep"]}});
    let mut public = original.clone();
    public_delivery(&mut public);
    let assignment = &mut public["sections"]["assignment"];
    let aliases = assignment["projection"]["residualRecoveryAliases"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(aliases.get("current"), Some(&json!("evidence")));
    assert!(!aliases.contains_key("subject"));
    assert!(!aliases.contains_key("next"));
    assert!(!aliases.contains_key("loop"));
    for (removed, source) in aliases {
        let context = &mut assignment["residual"]["context"];
        assert!(context["recovery"].get(&removed).is_none());
        let replacement = context[source.as_str().unwrap()].clone();
        context["recovery"][removed] = replacement;
    }
    assignment["projection"]
        .as_object_mut()
        .unwrap()
        .remove("residualRecoveryAliases");
    assert_eq!(public, original);
    let mut action = json!({"kind":"work_action"});
    public_delivery(&mut action);
    assert_eq!(action, json!({"kind":"work_action"}));
}

#[test]
fn public_detail_does_not_change_native_required_facts() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    let (_root, loaded, work) = fixture("detail-public-native-proof", "true");
    let snapshot =
        RunSnapshot::capture(&loaded, &crate::work::resolve(&loaded, &work).unwrap()).unwrap();
    let next = crate::work::next_from(&loaded, &work, &snapshot).unwrap();
    let before = crate::work::details::required_assignment(&loaded, &work, &next).unwrap();
    let detail = current(&loaded, &work).unwrap();
    let mut public = detail.clone();
    public_delivery(&mut public);
    assert_ne!(
        public, detail,
        "fixture must exercise the public projection"
    );
    let after = crate::work::details::required_assignment(&loaded, &work, &next).unwrap();
    assert_eq!(
        before, after,
        "public display must not alter native proof facts"
    );
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

fn scope_current_lead(loaded: &Loaded, work: &str) {
    let snapshot =
        RunSnapshot::capture(loaded, &crate::work::resolve(loaded, work).unwrap()).unwrap();
    let next = crate::work::next_from(loaded, work, &snapshot).unwrap();
    assert_eq!(next["role"], "lead");
    crate::work::return_result_impl::return_result_with(
        loaded,
        work,
        next["assignment"].as_str().unwrap(),
        "scoped",
        None,
        None,
        None,
        Some(b"Scoped fixture work.\n".to_vec()),
        None,
    )
    .unwrap();
}

#[test]
fn action_view_preserves_the_canonical_grant_and_held_result_suppression() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    let (_root, loaded, work) = fixture("action-view-current-grant", "true");
    scope_current_lead(&loaded, &work);
    let next = crate::work::next(&loaded, &work).unwrap();
    let assignment = next["next"]["assignment"].as_str().unwrap();
    crate::work::permit_response::permit(
        &loaded,
        &work,
        assignment,
        "Edit the fixture",
        Some("action-view-current-grant"),
    )
    .unwrap();
    let detail = current(&loaded, &work).unwrap();
    let action = action_view(&loaded, &work).unwrap();
    assert_eq!(action["binding"], detail["binding"]);
    assert!(detail["actionForms"]["currentGrant"].is_object());
    assert_eq!(
        action["effectiveAction"]["currentGrant"],
        detail["actionForms"]["currentGrant"]
    );

    crate::work::held::store(&loaded, &work, assignment, b"retained result").unwrap();
    let detail = current(&loaded, &work).unwrap();
    let action = action_view(&loaded, &work).unwrap();
    assert_eq!(action["binding"], detail["binding"]);
    assert!(detail["actionForms"]["currentGrant"].is_null());
    assert!(action["effectiveAction"]["currentGrant"].is_null());
    assert!(action["effectiveAction"]["beforeEditing"].is_null());
}

#[test]
fn journey_projections_match_and_ancestor_changes_refuse_detail() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_NATIVE_CARRY_READ_RACE";
    if std::env::var_os(CHILD).is_none() {
        for change in ["ledger", "claim"] {
            let outer = crate::test_root::temp("native-carry-read-child");
            let store = outer.join("session-store");
            crate::project::portability::create_private_directory(&store).unwrap();
            // raw-spawn: isolate the fixture's native selectors and state.
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
                    "work::readable::tests::journey_projections_match_and_ancestor_changes_refuse_detail",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(CHILD, change)
                .env("CLAUDE_CODE_SESSION_ID", "fixture-native-carry-read-race")
                .env("EXITBIND_SESSION_GOALS_DIR", &store)
                .env("TMPDIR", &*outer)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{change}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        }
        return;
    }

    let change = std::env::var(CHILD).unwrap();
    let (_root, loaded) = configured("carry-mid-read");
    let scope = crate::session_goal::native::scope(&loaded, true).unwrap();
    scope.require_owner().unwrap();
    revise_goal(
        &loaded,
        &scope.root,
        "fixture-carry-goal",
        "Original obligation",
        true,
    )
    .unwrap();
    let ancestor = begin(&loaded, "true", true);
    scope_current_lead(&loaded, &ancestor);
    let successor = crate::work::supersede::supersede(&loaded, &ancestor, "Fixture successor")
        .unwrap()["work"]
        .as_str()
        .unwrap()
        .to_owned();
    scope_current_lead(&loaded, &successor);
    let current_ledger = crate::work::resolve(&loaded, &successor).unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &current_ledger).unwrap();
    let next = crate::work::next_from(&loaded, &successor, &snapshot).unwrap();
    assert_eq!(next["role"], "worker");
    let mut shared = crate::session_goal::WorkGoalObservation::acquire(&loaded, &successor);
    let bounded = shared.progress(&next["progress"]).unwrap();
    let detail = shared.detail(&next["progress"]).unwrap();
    assert!(bounded["journeyCompletion"].is_object(), "{bounded}");
    assert_eq!(
        bounded,
        crate::session_goal::progress_for_work(&loaded, &successor, &next["progress"]).unwrap()
    );
    assert_eq!(
        detail,
        crate::session_goal::progress_detail_for_work(&loaded, &successor, &next["progress"])
            .unwrap()
    );

    let ancestor_path = loaded
        .state_root
        .join(crate::work::resolve(&loaded, &ancestor).unwrap());
    let changed_path = if change == "claim" {
        let mut path = ancestor_path.into_os_string();
        path.push(".supersede");
        std::path::PathBuf::from(path)
    } else {
        ancestor_path
    };
    let current_path = loaded.state_root.join(&current_ledger);
    let before = std::fs::read(&current_path).unwrap();
    let error = read_after_reads(&loaded, &successor, &snapshot, &next, || {
        std::fs::write(&changed_path, b"invalid fixture evidence\n").map_err(|e| e.to_string())
    })
    .unwrap_err();
    let fresh_error = RunSnapshot::capture(&loaded, &current_ledger)
        .err()
        .expect("changed ancestor must fail its independent fresh capture");
    assert_eq!(error, fresh_error, "{change}");
    assert_eq!(std::fs::read(current_path).unwrap(), before, "{change}");
}

#[cfg(unix)]
#[test]
fn shared_journey_observation_rechecks_its_exact_inputs() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_SHARED_JOURNEY_RACE";
    if std::env::var_os(CHILD).is_none() {
        for change in [
            "ledger",
            "ledger-replaced",
            "ledger-symlink",
            "claim",
            "claim-replaced",
            "terminal-claim",
            "config",
            "goal",
            "owner",
        ] {
            let outer = crate::test_root::temp("shared-journey-child");
            let store = outer.join("sessions");
            crate::project::portability::create_private_directory(&store).unwrap();
            // raw-spawn: each mutation owns its native selector and fixture.
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
                    "work::readable::tests::shared_journey_observation_rechecks_its_exact_inputs",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(CHILD, change)
                .env("CLAUDE_CODE_SESSION_ID", "fixture-shared-journey")
                .env("EXITBIND_SESSION_GOALS_DIR", &store)
                .env("TMPDIR", &*outer)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{change}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        }
        return;
    }
    let change = std::env::var(CHILD).unwrap();
    let (_root, loaded) = configured("shared-journey-race");
    let scope = crate::session_goal::native::scope(&loaded, true).unwrap();
    scope.require_owner().unwrap();
    revise_goal(
        &loaded,
        &scope.root,
        "shared-journey-goal",
        "Original",
        true,
    )
    .unwrap();
    let ancestor = begin(&loaded, "true", true);
    scope_current_lead(&loaded, &ancestor);
    let successor = crate::work::supersede::supersede(&loaded, &ancestor, "Fixture successor")
        .unwrap()["work"]
        .as_str()
        .unwrap()
        .to_owned();
    scope_current_lead(&loaded, &successor);
    let current = crate::work::resolve(&loaded, &successor).unwrap();
    let snapshot = RunSnapshot::capture(&loaded, &current).unwrap();
    let next = crate::work::next_from(&loaded, &successor, &snapshot).unwrap();
    let current_path = loaded.state_root.join(&current);
    let current_before = std::fs::read(&current_path).unwrap();
    let mut observation = crate::session_goal::WorkGoalObservation::acquire(&loaded, &successor);
    observation.progress(&next["progress"]).unwrap();
    assert!(
        observation.has_journey_observation(),
        "fixture did not select shared observation"
    );
    assert!(
        observation.used_blocked_close_route(),
        "fixture did not use the blocked close-route observation"
    );
    let ledger = loaded
        .state_root
        .join(crate::work::resolve(&loaded, &ancestor).unwrap());
    let mut claim = ledger.as_os_str().to_os_string();
    claim.push(".supersede");
    let claim = std::path::PathBuf::from(claim);
    match change.as_str() {
        "ledger" | "claim" => std::fs::write(
            if change == "ledger" { &ledger } else { &claim },
            b"invalid\n",
        )
        .unwrap(),
        "ledger-replaced" | "claim-replaced" => {
            let path = if change == "ledger-replaced" {
                &ledger
            } else {
                &claim
            };
            let replacement = path.with_extension("replacement");
            std::fs::write(&replacement, std::fs::read(path).unwrap()).unwrap();
            std::fs::rename(replacement, path).unwrap();
        }
        "ledger-symlink" => {
            let target = ledger.with_extension("saved");
            std::fs::rename(&ledger, &target).unwrap();
            std::os::unix::fs::symlink(&target, &ledger).unwrap();
        }
        "terminal-claim" => {
            let mut path = current_path.as_os_str().to_os_string();
            path.push(".supersede");
            std::fs::write(std::path::PathBuf::from(path), b"{}\n").unwrap();
        }
        "config" => {
            let mut bytes = std::fs::read(&loaded.path).unwrap();
            bytes.push(b'\n');
            std::fs::write(&loaded.path, bytes).unwrap();
        }
        "goal" => {
            revise_goal(&loaded, &scope.root, "shared-journey-goal", "Later", true).unwrap();
        }
        "owner" => std::fs::write(scope.root.join("owner.json"), b"{}\n").unwrap(),
        _ => unreachable!(),
    }
    let error = observation
        .finish()
        .expect_err("changed observed facts must refuse");
    assert!(!error.is_empty());
    assert_eq!(std::fs::read(current_path).unwrap(), current_before);
}

#[cfg(unix)]
#[test]
fn shared_journey_preserves_first_error_and_only_falls_back_for_unsupported_shape() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_SHARED_JOURNEY_ERRORS";
    if std::env::var_os(CHILD).is_none() {
        for case in [
            "marked-competing-errors",
            "unrelated-members",
            "later-successor",
        ] {
            let outer = crate::test_root::temp("shared-journey-errors-child");
            let store = outer.join("sessions");
            crate::project::portability::create_private_directory(&store).unwrap();
            // raw-spawn: the native selector and state belong to this fixture.
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
            let output = child.args([
                "work::readable::tests::shared_journey_preserves_first_error_and_only_falls_back_for_unsupported_shape",
                "--exact", "--test-threads=1",
            ]).env(CHILD, case)
                .env("CLAUDE_CODE_SESSION_ID", "fixture-shared-journey-errors")
                .env("EXITBIND_SESSION_GOALS_DIR", &store)
                .env("TMPDIR", &*outer).output().unwrap();
            assert!(
                output.status.success(),
                "{case}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        }
        return;
    }
    let case = std::env::var(CHILD).unwrap();
    let (_root, loaded) = configured("shared-journey-errors");
    let scope = crate::session_goal::native::scope(&loaded, true).unwrap();
    scope.require_owner().unwrap();
    revise_goal(
        &loaded,
        &scope.root,
        "journey-errors-goal",
        "Original",
        true,
    )
    .unwrap();
    let first = begin(&loaded, "true", true);
    scope_current_lead(&loaded, &first);
    let selected = if case == "marked-competing-errors" || case == "later-successor" {
        let value =
            crate::work::supersede::supersede(&loaded, &first, "Fixture successor").unwrap();
        let work = value["work"].as_str().unwrap().to_owned();
        scope_current_lead(&loaded, &work);
        if case == "later-successor" {
            first.clone()
        } else {
            work
        }
    } else {
        let work = begin(&loaded, "true", true);
        scope_current_lead(&loaded, &work);
        work
    };
    let record = crate::session_goal::read(&scope.root).unwrap().unwrap();
    if case == "marked-competing-errors" {
        let first_path = loaded
            .state_root
            .join(crate::work::resolve(&loaded, &first).unwrap());
        let mut claim = first_path.as_os_str().to_os_string();
        claim.push(".supersede");
        std::fs::write(std::path::PathBuf::from(claim), b"invalid claim\n").unwrap();
        let selected_path = loaded
            .state_root
            .join(crate::work::resolve(&loaded, &selected).unwrap());
        std::fs::write(selected_path, b"invalid ledger\n").unwrap();
    }
    let (fresh, observed, used_fresh) =
        crate::session_goal::compare_test_journey(&loaded, &record, &selected).unwrap();
    assert_eq!(observed, fresh, "{case}");
    if case == "marked-competing-errors" {
        assert!(
            !used_fresh,
            "a validation error must not cause a fresh retry"
        );
        assert!(fresh.unwrap()["unresolved"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("invalid supersession claim"));
    } else {
        assert!(
            used_fresh,
            "unrelated or later chains must use the original fresh route"
        );
    }
}

#[cfg(unix)]
#[test]
fn close_route_reuses_only_matching_known_blocked_work_readiness() {
    if !crate::compatibility::is_exitbind() {
        return;
    }
    const CHILD: &str = "EXITBIND_TEST_SHARED_JOURNEY_CLOSE_ROUTE";
    if std::env::var_os(CHILD).is_none() {
        for case in [
            "direct",
            "governed",
            "project-root",
            "other-record",
            "actual-close",
        ] {
            let outer = crate::test_root::temp("shared-close-route-child");
            let store = outer.join("sessions");
            crate::project::portability::create_private_directory(&store).unwrap();
            // raw-spawn: each route comparison owns its native session and state.
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
            let output = child.args([
                "work::readable::tests::close_route_reuses_only_matching_known_blocked_work_readiness",
                "--exact", "--test-threads=1",
            ]).env(CHILD, case)
                .env("CLAUDE_CODE_SESSION_ID", "fixture-shared-close-route")
                .env("EXITBIND_SESSION_GOALS_DIR", &store)
                .env("TMPDIR", &*outer).output().unwrap();
            assert!(
                output.status.success(),
                "{case}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        }
        return;
    }
    let case = std::env::var(CHILD).unwrap();
    let (_root, loaded) = configured("shared-close-route");
    let scope = crate::session_goal::native::scope(&loaded, true).unwrap();
    scope.require_owner().unwrap();
    revise_goal(&loaded, &scope.root, "shared-close-goal", "Original", true).unwrap();
    let work = begin(&loaded, "true", true);
    scope_current_lead(&loaded, &work);
    if case == "governed" {
        crate::session_goal::incorporate(
            &scope.root,
            &loaded,
            "shared-close-goal",
            "Bound fixture Goal",
            Some("Original"),
            None,
            None,
            None,
            None,
            None,
            Some("accepted"),
            Some(&work),
            None,
            Some("findings,blockers,decisions,externalActions"),
            true,
        )
        .unwrap();
    }
    let record = crate::session_goal::read(&scope.root).unwrap().unwrap();
    if case == "actual-close" {
        let journal = scope
            .root
            .join(crate::project::layout_types::state_namespace())
            .join("session-goal.jsonl");
        let changed = b"not a canonical Goal journal\n";
        let error = crate::session_goal::test_close_route_change(
            &loaded,
            &record,
            &work,
            &scope.root,
            || {
                std::fs::write(&journal, changed).unwrap();
                let error = crate::session_goal::close_direct(
                    &scope.root,
                    &loaded,
                    "shared-close-goal",
                    None,
                )
                .expect_err("actual close must freshly reject the malformed journal");
                assert!(
                    error.contains("invalid canonical session goal line"),
                    "{error}"
                );
                assert_eq!(std::fs::read(&journal).unwrap(), changed);
            },
        )
        .expect_err("changed Goal witness must prevent Work view publication");
        assert!(!error.is_empty());
        return;
    }
    let mut route_record = record.clone();
    if case == "other-record" {
        route_record["goalId"] = json!("a-different-goal");
    }
    let root = if case == "project-root" {
        &loaded.state_root
    } else {
        &scope.root
    };
    let (fresh, observed, shortcut) = crate::session_goal::compare_test_close_routes(
        &loaded,
        &record,
        &route_record,
        &work,
        root,
    )
    .unwrap();
    assert_eq!(observed, fresh, "{case}");
    assert_eq!(
        shortcut,
        matches!(case.as_str(), "direct" | "governed"),
        "{case}"
    );
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
        let (assignment, command, _) =
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

#[path = "readable_tests/grant_offer.rs"]
mod grant_offer;

#[test]
fn same_recipient_continuity_refuses_changed_content_and_unknown_facts() {
    let before = json!({"kind":"work_detail","complete":true,"work":"w",
        "binding":"old","recipient":{"assignment":"a"},
        "recipientContext":{"profile":{"content":"instructions"}},
        "sections":{"assignment":{"assignment":{"context":{
            "goal":"goal", "unknown":{"keep":true},
            "loop":{"state":"ready","spent":0,"currentReplan":null},
            "evidence":[{"evidenceId":"event","rawEventRef":{"kind":"ledger_event",
                "work":"w","selector":"event","id":"ref:old","eventCount":1,
                "headEventSha256":"old","sha256":"old"}}],
            "expansions":[{"kind":"ledger_history","id":"ref:old","eventCount":1,
                "headEventSha256":"old","sha256":"old"}],"digest":"old"}},
            "residual":{"snapshot":{"eventCount":1,"headEventSha256":"old",
                "inputsSha256":"input"}}},
            "evidence":{"items":[{"reference":"ref:old","artifact":{"content":"evidence"}}]},
            "tasks":{"x":1}}});
    let mut after = before.clone();
    after["binding"] = json!("new");
    let context = &mut after["sections"]["assignment"]["assignment"]["context"];
    context["loop"]["spent"] = json!(1);
    context["loop"]["currentMutation"] = json!({"operation":"edit"});
    context["evidence"][0]["rawEventRef"]["id"] = json!("ref:new");
    context["evidence"][0]["rawEventRef"]["eventCount"] = json!(2);
    context["expansions"][0]["id"] = json!("ref:new");
    context["expansions"][0]["eventCount"] = json!(2);
    context["digest"] = json!("new");
    after["sections"]["assignment"]["residual"]["snapshot"]["eventCount"] = json!(2);
    after["sections"]["assignment"]["residual"]["snapshot"]["headEventSha256"] = json!("new");
    after["sections"]["assignment"]["residual"]["snapshot"]["inputsSha256"] = json!("new input");
    after["sections"]["evidence"]["items"][0]["reference"] = json!("ref:new");
    let continuity = public_context_continuity(&before, &after).unwrap();
    assert_eq!(
        continuity["contextIdentity"],
        public_context_identities(&before).unwrap()
    );
    assert_eq!(continuity["governor"]["spent"], 1);
    assert_eq!(continuity["inputsSha256"], "new input");
    let mut changed = after.clone();
    changed["sections"]["assignment"]["assignment"]["context"]["unknown"]["keep"] = json!(false);
    assert!(public_context_continuity(&before, &changed).is_none());
    changed = after.clone();
    changed["sections"]["assignment"]["assignment"]["context"]["loop"]["currentReplan"] =
        json!({"scope":"new"});
    assert!(public_context_continuity(&before, &changed).is_none());
    changed = after.clone();
    changed["sections"]["assignment"]["assignment"]["context"]["loop"]["currentMutation"]
        ["unknownRequired"] = json!("new");
    assert!(public_context_continuity(&before, &changed).is_none());
    changed = after.clone();
    changed["sections"]["evidence"]["items"][0]["artifact"]["content"] = json!("new evidence");
    assert!(public_context_continuity(&before, &changed).is_none());
    after["recipientContext"]["profile"]["content"] = json!("changed instructions");
    assert!(public_context_continuity(&before, &after).is_none());
    after["recipient"] = json!({"assignment":"another actor"});
    assert!(public_context_continuity(&before, &after).is_none());
}

#[test]
fn actor_context_preserves_instruction_content_and_refuses_unknown_transport() {
    let profile = "Exact actor instruction.\n";
    let sha = crate::evidence::hash::bytes(profile.as_bytes());
    let mut detail = json!({"kind":"work_detail","complete":true,"work":"w","binding":"b",
        "recipient":{"agent":"worker","role":"worker","nativeName":"worker"},
        "recipientContext":{"available":true,"complete":true,"agent":"worker","role":"worker",
            "nativeName":"worker","displayName":"worker","authority":"none","rules":[],
            "profile":{"path":"agent.md","sha256":sha,"bytes":profile.len(),
                "complete":true,"content":profile}},
        "sections":{"assignment":{"assignment":{"goal":"goal","purpose":null,
                "declaredBoundary":null,"context":{"goal":"goal","evidence":[],
                    "expansions":[],"loop":{"enabled":true,"state":"ready","budget":3,
                        "spent":0,"currentReplan":null,"currentSensorRequest":null,"evidence":[]},
                    "recovery":{"stale":[],"conflicts":[]}}},
            "residual":{"snapshot":{"inputsSha256":"input"}},"checkPreparation":{"state":"runnable"},
            "memoryQuery":null,"outcomes":null},
            "evidence":{"items":[],"complete":true},"tasks":{"tasks":[]}}});
    let body = public_actor_context(&detail, false).unwrap();
    assert_eq!(body["requiredContentComplete"], true);
    assert_eq!(body["instructions"]["profile"]["content"], profile);
    assert!(body["instructions"]["profile"].get("path").is_none());
    let reused = public_actor_context(&detail, true).unwrap();
    assert_eq!(reused["requiredContentComplete"], false);
    assert_eq!(reused["instructions"]["reuseIfPreviouslyRead"], true);
    assert_eq!(
        reused["instructions"]["recipientSha256"],
        body["contextIdentity"]["recipientSha256"]
    );
    detail["recipientContext"]["guidance"] = json!({"items":[],"overflow":[{"id":"more"}],
        "unavailable":[],"selectedCount":0,"overflowCount":1,"unavailableCount":0});
    assert!(public_actor_context(&detail, false).is_none());
    detail["recipientContext"]["guidance"] = json!({"items":[{"content":"keep this",
        "unknown":{"path":"semantic-not-a-reference"}}],"overflow":[],"unavailable":[],
        "selectedCount":1,"overflowCount":0,"unavailableCount":0});
    assert!(public_actor_context(&detail, false).is_none());
    detail["recipientContext"]
        .as_object_mut()
        .unwrap()
        .remove("guidance");
    detail["sections"]["assignment"]["assignment"]["context"]["unknown"] =
        json!({"path":"private"});
    assert!(public_actor_context(&detail, false).is_none());
    detail["sections"]["assignment"]["assignment"]["context"]
        .as_object_mut()
        .unwrap()
        .remove("unknown");
    detail["sections"]["assignment"]["assignment"]["context"]["evidence"] = json!([{
        "kind":"submission","rawEventRef":{"kind":"ledger_event","selector":"missing",
            "work":"w","exact":true,"id":"ref:missing"}}]);
    assert!(public_actor_context(&detail, false).is_none());
}

#[test]
fn actor_evidence_fold_keeps_bodies_and_refuses_unknown_or_conflicting_facts() {
    let context = json!({"evidence":[
        {"kind":"submission","evidenceId":"submission",
            "artifact":{"bytes":4,"exact":true,"kind":"evidence","sha256":"digest"}},
        {"kind":"check","checkEvidenceId":"check","result":{"kind":"exit","code":0},
            "status":"passed","freshness":"current","checker":"sh check.sh",
            "targetEvidenceId":"submission"}
    ],"other":"keep"});
    let evidence = json!({"complete":true,"items":[
        {"kind":"submission","evidenceId":"submission","artifact":{
            "bytes":4,"exact":true,"kind":"evidence","sha256":"digest",
            "complete":true,"content":"body"}},
        {"kind":"check","evidenceId":"check","result":{"kind":"exit","code":0},
            "stdout":{"content":"log","complete":true}}
    ]});
    let (folded_context, folded_evidence) =
        fold_actor_context_evidence(&context, &evidence).unwrap();
    assert!(folded_context.get("evidence").is_none());
    assert_eq!(folded_context["other"], "keep");
    assert_eq!(folded_evidence["items"][0]["artifact"]["content"], "body");
    assert_eq!(folded_evidence["items"][1]["stdout"]["content"], "log");
    assert_eq!(folded_evidence["items"][1]["status"], "passed");
    assert_eq!(
        folded_evidence["items"][1]["targetEvidenceId"],
        "submission"
    );

    let mut unknown = context.clone();
    unknown["evidence"][0]["futureFact"] = json!("keep");
    assert!(fold_actor_context_evidence(&unknown, &evidence).is_none());
    let mut different = context.clone();
    different["evidence"][0]["artifact"]["sha256"] = json!("other");
    assert!(fold_actor_context_evidence(&different, &evidence).is_none());
    let mut conflicting = evidence.clone();
    conflicting["items"][1]["status"] = json!("failed");
    assert!(fold_actor_context_evidence(&context, &conflicting).is_none());
    let mut duplicate = evidence.clone();
    duplicate["items"]
        .as_array_mut()
        .unwrap()
        .push(evidence["items"][0].clone());
    assert!(fold_actor_context_evidence(&context, &duplicate).is_none());
}

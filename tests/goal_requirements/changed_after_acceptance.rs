//! R55 F2 (FR54-13 gap): an accepted journey member whose frozen checker
//! changed after acceptance reads accepted-at-binding with the change as a
//! field; read-only views exit zero, a closed Goal's head reads closed, and
//! a running Work's check still refuses the changed checker.
use super::*;

#[test]
fn an_accepted_member_with_a_changed_checker_reads_accepted_at_binding_and_the_closed_goal_reads_closed(
) {
    let p = Project::new("changed-after-acceptance")
        .with_native_session("changed-after-acceptance-session");
    let config_path = p.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] =
        json!({"version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"});
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    p.file("checker.sh", "true\n");
    incorporate_native_generic_goal(&p, "drifted", "Accept one checked member and close");
    let member = p.begin_ordinary("Check with the bound checker", "sh checker.sh", &[]);
    p.drive(&member, true);
    let open = p.status();
    assert_eq!(
        open["goalProgress"]["journeyCompletion"]["state"], "current",
        "{open}"
    );
    let close = open["goalProgress"]["nextAction"]["command"]
        .as_array()
        .expect("close route")
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let closed = p.call_exact_argv(&close);
    assert!(closed.status.success(), "{closed:?}");

    // The member's outside checker changes after acceptance (R54: ci-local.sh).
    p.file("checker.sh", "true # changed after acceptance\n");

    let detail = p.call(&["work", "detail", &member, "--json"], None);
    assert!(detail.status.success(), "{detail:?}");
    let detail: Value = serde_json::from_slice(&detail.stdout).unwrap();
    assert_eq!(
        detail["completion"]["workAcceptance"]["state"], "accepted",
        "{detail}"
    );
    assert_eq!(
        detail["acceptedAtBinding"]["state"], "accepted_at_binding",
        "{detail}"
    );
    let drift = &detail["acceptedAtBinding"]["drift"];
    assert_eq!(
        drift["classification"], "verification_checker_changed",
        "{detail}"
    );
    assert_eq!(drift["boundSha256"].as_str().unwrap().len(), 64);
    assert_eq!(drift["currentSha256"].as_str().unwrap().len(), 64);
    assert!(drift["nextAction"]["command"].is_array(), "{detail}");
    let text = p.call(&["work", "detail", &member], None);
    assert!(text.status.success(), "{text:?}");
    let text = String::from_utf8_lossy(&text.stdout).into_owned();
    assert!(
        text.contains("accepted_at_binding") && text.contains("verification_checker_changed"),
        "{text}"
    );
    assert!(!text.contains("EXITBIND_CHANGED:"), "{text}");
    let action = p.call(&["work", "detail", &member, "--view", "action"], None);
    assert!(action.status.success(), "{action:?}");

    let status = p.status();
    assert_eq!(status["closure"]["closed"], true, "{status}");
    let system = status["goalProgress"]["systemText"].as_str().unwrap();
    assert!(system.starts_with("Goal: CLOSED (revision "), "{system}");
    let unresolved = &status["goalProgress"]["journeyCompletion"]["unresolved"][0];
    assert_eq!(unresolved["state"], "accepted_at_binding", "{status}");
    assert_eq!(
        unresolved["drift"]["classification"], "verification_checker_changed",
        "{status}"
    );
    let plain = p.call(&["goal", "status"], None);
    assert!(plain.status.success(), "{plain:?}");

    // A running Work's check still refuses a changed frozen checker, typed.
    let running = p.begin_ordinary("Check again with the bound checker", "sh checker.sh", &[]);
    p.file("checker.sh", "true # changed while running\n");
    let refused = p.call(&["work", "detail", &running, "--json"], None);
    assert!(!refused.status.success(), "{refused:?}");
    let refused: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(
        refused["classification"], "verification_checker_changed",
        "{refused}"
    );
    assert!(refused["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "supersede"));
}

#[test]
fn named_requirement_projection_obeys_resume_input_budget() {
    let p = Project::new("named-requirement-resume-budget");
    p.setup();
    let accepted = p.begin("R1", "sh check.sh R1");
    p.drive(&accepted, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--result-ref",
            &accepted,
        ],
        None,
    );

    // A later blocked named Work selects a terminal snapshot whose packet
    // itself does not fingerprint; named evidence still checks accepted R1.
    let blocked = p.begin("R2", "sh check.sh R2");
    let next = p.value(&["work", "next", &blocked, "--json", "--full"], None);
    p.return_stage_with_config(
        &blocked,
        &next["next"],
        "blocked",
        &p.root.join("exitbind.json"),
    );
    let current = p.value(&["work", "resume", "--json"], None);
    assert_eq!(current["status"], "none", "{current}");
    assert!(current["presentation"]["terminal"].is_null());

    p.file("data.txt", "changed\n");
    let changed = p.value(&["work", "resume", "--json"], None);
    assert_eq!(changed["status"], "none", "{changed}");
    assert!(changed["presentation"]["terminal"].is_null());
    fs::File::create(p.root.join("large"))
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let limited = p.value(&["work", "resume", "--json"], None);
    assert_eq!(limited["status"], "unresolved", "{limited}");
    assert_eq!(limited["reason"]["code"], "discovery_limited", "{limited}");
    assert_eq!(
        limited["discovery"]["limitedReason"], "fingerprint_byte_limit_exceeded",
        "{limited}"
    );
    assert!(limited["presentation"]["terminal"].is_null());
}

#[cfg(target_os = "linux")]
#[test]
fn resume_interrupts_delayed_git_listing_for_named_checker_coverage() {
    use std::os::unix::fs::PermissionsExt;

    let p = Project::new("named-resume-slow-checker-listing");
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("git"))
        .find(|path| path.is_file())
        .expect("Git is required for this fixture")
        .canonicalize()
        .unwrap();
    let init = support::git_topology::command(&git)
        .current_dir(&p.root)
        .args(["init", "-q"])
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    p.setup();
    let accepted = p.begin("R1", "sh check.sh R1");
    p.drive(&accepted, true);
    p.value(
        &[
            "goal",
            "assign",
            "--goal-id",
            "import",
            "--requirement",
            "R1",
            "--result-ref",
            &accepted,
        ],
        None,
    );
    let blocked = p.begin("R2", "sh check.sh R2");
    let next = p.value(&["work", "next", &blocked, "--json", "--full"], None);
    p.return_stage_with_config(
        &blocked,
        &next["next"],
        "blocked",
        &p.root.join("exitbind.json"),
    );

    let wrapper = p._base.join("git-wrapper");
    fs::create_dir(&wrapper).unwrap();
    let script = wrapper.join("git");
    fs::write(
        &script,
        "#!/bin/sh\ncase \" $* \" in\n *\" ls-files \"*\" --others \"*)\n  if [ ! -e \"$R65_FIRST\" ]; then : > \"$R65_FIRST\"; elif [ ! -e \"$R65_SECOND\" ]; then : > \"$R65_SECOND\"; sleep 4; : > \"$R65_FINISHED\"; fi;;\nesac\nexec \"$R65_REAL_GIT\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let first = p._base.join("first");
    let second = p._base.join("second");
    let finished = p._base.join("finished");
    let path = std::env::join_paths(
        std::iter::once(wrapper).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = support::git_topology::command(env!("CARGO_BIN_EXE_exitbind"));
    command
        .current_dir(&p.root)
        .args(["work", "resume", "--json", "--config"])
        .arg(p.root.join("exitbind.json"))
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("EXITBIND_NATIVE_SESSION_ID")
        .env("EXITBIND_SESSION_GOALS_DIR", &p.fixture_store)
        .env("EXITBIND_NO_UPDATE_CHECK", "1")
        .env("PATH", path)
        .env("R65_REAL_GIT", git)
        .env("R65_FIRST", &first)
        .env("R65_SECOND", &second)
        .env("R65_FINISHED", &finished);
    let output = support::run(&mut command);
    assert!(output.status.success(), "{output:?}");
    let resumed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        first.exists() && second.exists(),
        "the delayed checker listing must run"
    );
    assert!(
        !finished.exists(),
        "the delayed owned Git child must be interrupted"
    );
    assert_eq!(resumed["status"], "unresolved", "{resumed}");
    assert_eq!(resumed["reason"]["code"], "discovery_limited", "{resumed}");
    assert!(
        matches!(
            resumed["discovery"]["limitedReason"].as_str(),
            Some("git_listing_deadline_exceeded" | "resume_deadline_exceeded")
        ),
        "{resumed}"
    );
    assert!(resumed["presentation"]["terminal"].is_null());
}

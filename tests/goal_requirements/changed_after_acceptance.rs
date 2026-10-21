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

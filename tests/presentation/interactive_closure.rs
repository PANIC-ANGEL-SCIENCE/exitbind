use super::*;

#[test]
fn governed_begin_refuses_missing_exitbind_namespace_before_ledger_write() {
    let project = Project::new("activation-namespace");
    let namespace = project.root.join(".exitbind");
    let saved = project.root.join("saved-exitbind");
    fs::rename(&namespace, saved).unwrap();
    let refused = project.call(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "material",
            "--check-command",
            "true",
        ],
        None,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("governed activation is unavailable"));
    assert!(!namespace.join("runs").exists());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn interactive_session_closure_card_is_final_once_only_and_machine_silent() {
    let project = Project::new("session-card-pty");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    project.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "accepted",
        ],
        Some(b"accept"),
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --themed --config {} --session-closed",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    // The display-only flag cannot manufacture whole-session closure, even
    // when the run itself is READY.
    let before_goal = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let before_goal_text = String::from_utf8_lossy(&before_goal.stdout).replace('\r', "");
    let card = "+----------------------------+\n| This room remains nothing. |\n+----------------------------+";
    assert_eq!(
        before_goal_text.matches(card).count(),
        0,
        "{before_goal_text}"
    );

    // The card is eligible only after the Lead explicitly incorporates and
    // closes the canonical session goal.
    project.value(
        &[
            "goal",
            "incorporate",
            "--goal-id",
            "session-a",
            "--goal",
            "make it hold",
            "--none-applicable",
            "obligations,findings,blockers,decisions,externalActions",
        ],
        None,
    );
    project.value(
        &[
            "goal",
            "close",
            "--goal-id",
            "session-a",
            "--result-ref",
            work.as_str(),
        ],
        None,
    );
    let plain_command = command.replace(" --themed", "");
    let plain = support::pty(&plain_command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&plain.stdout).contains(card));
    let first = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let first_text = String::from_utf8_lossy(&first.stdout).replace('\r', "");
    assert_eq!(first_text.matches(card).count(), 1, "{first_text}");
    assert_eq!(
        first_text.lines().last(),
        Some("+----------------------------+")
    );
    let second = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    let second_text = String::from_utf8_lossy(&second.stdout).replace('\r', "");
    assert_eq!(second_text.matches(card).count(), 0, "{second_text}");
    fs::remove_file(
        project
            .root
            .join(".exitbind/presentation/session-goal.json"),
    )
    .unwrap();
    let after_loss = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&after_loss.stdout)
            .replace('\r', "")
            .matches(card)
            .count(),
        1
    );
    let machine = project.value(
        &["run", "status", &ledger, "--json", "--session-closed"],
        None,
    );
    assert!(machine["sessionGoal"].is_null());
    assert_eq!(machine["terminal"], "EXIT READY");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn interactive_failure_joke_is_transition_scoped_and_machine_silent() {
    let project = Project::new("failure-joke-pty");
    let work = begin(&project, false);
    let next = project.value(&["work", "next", &work], None)["next"].clone();
    project.value(
        &[
            "work",
            "return",
            &work,
            next["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"blocked"),
    );
    let ledger = format!(".exitbind/runs/work-{}.jsonl", &work["smw_".len()..]);
    let command = format!(
        "{} run status {} --config {}",
        env!("CARGO_BIN_EXE_exitbind"),
        ledger,
        project.root.join("exitbind.json").display()
    );
    let first = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(first.status.success(), "{first:?}");
    let first_text = String::from_utf8_lossy(&first.stdout).replace('\r', "");
    let joke = "Developer special: the bug has requested a second opinion.";
    assert_eq!(first_text.matches(joke).count(), 1, "{first_text}");
    assert!(first_text.find("Lead decision: blocked").unwrap() < first_text.find(joke).unwrap());
    let second = support::pty(&command)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&second.stdout)
            .replace('\r', "")
            .matches(joke)
            .count(),
        0
    );
    let machine = project.value(&["run", "status", &ledger, "--json"], None);
    assert!(machine["joke"].is_null());
    let headless = project.call(&["run", "status", &ledger], None);
    assert!(!String::from_utf8_lossy(&headless.stdout).contains(joke));
    fs::remove_dir_all(project.root).unwrap();
}

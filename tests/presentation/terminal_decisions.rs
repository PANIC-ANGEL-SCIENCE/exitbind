use super::*;

/// `EXIT READY` is the canonical acceptance decision speaking, not a formatting
/// choice. Nothing short of that decision may reach the terminal block, and
/// suppressing the decoration changes no machine value.
#[test]
fn only_the_accepted_decision_reaches_the_terminal_block() {
    // Checks pass, review approves, acceptance is still pending.
    let pending = Project::new("terminal-pending");
    let work = begin(&pending, false);
    let decision = pending.drive_until(&work, "lead_decision");
    let waiting = pending.presentation(&work);
    assert!(waiting["terminal"].is_null(), "{waiting}");
    assert_eq!(waiting["state"]["check"], "current");
    assert_eq!(waiting["state"]["review"], "approved");
    assert_ne!(waiting["progress"], 100);

    // The lead blocks instead. The run is terminal, and still not ready.
    let refused_decision = pending.value(
        &[
            "work",
            "return",
            &work,
            decision["assignment"].as_str().unwrap(),
            "--outcome",
            "blocked",
        ],
        Some(b"blocked"),
    );
    assert!(refused_decision["presentation"]["terminal"].is_null());
    let blocked = pending.presentation(&work);
    assert!(blocked["terminal"].is_null(), "{blocked}");
    assert_ne!(blocked["exitState"], "READY");
    fs::remove_dir_all(pending.root).unwrap();

    // A failing check is evidence against readiness, never a route to it.
    let failing = Project::new("terminal-failing");
    let broken = begin(&failing, false);
    failing.drive_until(&broken, "check");
    fs::write(failing.root.join("source.txt"), b"nothing it looks for\n").unwrap();
    failing.recorded_check_failure(&broken);
    let refused = failing.presentation(&broken);
    assert!(refused["terminal"].is_null(), "{refused}");
    assert_eq!(refused["state"]["check"], "failed");
    fs::remove_dir_all(failing.root).unwrap();

    // Work that was never governed has no terminal block to copy.
    let idle = Project::new("terminal-idle");
    let none = idle.value(&["work", "resume"], None);
    assert_eq!(none["status"], "none");
    assert!(none["presentation"].is_null());
    fs::remove_dir_all(idle.root).unwrap();
}

/// The terminal block is display only: the machine record beneath it keeps its
/// decision, evidence, and reuse values exactly as the run recorded them.
#[test]
fn the_terminal_block_changes_no_machine_value() {
    let project = Project::new("terminal-machine");
    let work = begin(&project, false);
    let decision = project.drive_until(&work, "lead_decision");
    let accepted = project.value(
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
    let seen = project.value(&["work", "next", &work], None);

    // Valid JSON with the recorded decision intact beside the display value.
    assert_eq!(seen["presentation"]["terminal"], "EXIT READY");
    assert_eq!(seen["next"]["progress"]["state"], "READY");
    assert_eq!(seen["next"]["progress"]["percent"], 100);
    assert_eq!(
        seen["next"]["progress"]["state"],
        accepted["next"]["progress"]["state"]
    );
    assert_eq!(seen["residual"]["historical"], true);
    // Acceptance is history: it authorizes no further work.
    let help = seen["residual"]["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap();
    assert!(help.contains("history"), "{help}");
    fs::remove_dir_all(project.root).unwrap();
}

/// A finished run is where "what is the state here?" actually lands, and an
/// acceptance speaks for the present only while the tree it was bound to is
/// still the one on disk.
#[test]
fn a_finished_run_is_reported_and_stops_speaking_when_the_tree_moves() {
    let project = Project::new("terminal-resume");
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

    // Nothing is active, and the product still supplies the block to print.
    let settled = project.value(&["work", "resume"], None);
    assert_eq!(settled["status"], "none");
    assert_eq!(settled["recent"]["work"], work.as_str());
    assert_eq!(settled["recent"]["exitState"], "READY");
    assert_eq!(settled["presentation"]["terminal"], "EXIT READY");

    // One covered file changes: the acceptance is history again, so the
    // terminal block goes quiet while the recorded decision stays readable.
    fs::write(project.root.join("source.txt"), b"env-first\nlater edit\n").unwrap();
    let moved = project.value(&["work", "resume"], None);
    assert_eq!(moved["recent"]["work"], work.as_str());
    assert_eq!(moved["recent"]["exitState"], "READY");
    assert!(
        moved["presentation"]["terminal"].is_null(),
        "history must not speak for a tree that changed under it: {moved}"
    );
    let direct = project.value(&["work", "next", &work], None);
    assert!(direct["presentation"]["terminal"].is_null());
    let help = direct["residual"]["humanHelp"]["whatHappened"]
        .as_str()
        .unwrap();
    assert!(help.contains("history"), "{help}");

    // Restoring the exact tested inputs restores the decision's reach.
    fs::write(project.root.join("source.txt"), b"env-first\n").unwrap();
    let restored = project.value(&["work", "resume"], None);
    assert_eq!(restored["presentation"]["terminal"], "EXIT READY");
    fs::remove_dir_all(project.root).unwrap();
}

/// The display memo is not the run record. It says so in its own bytes,
/// because a live host was observed reading it off disk and reporting it as
/// the status instead of asking the CLI.
#[test]
fn the_display_memo_names_itself_and_claims_no_authority() {
    let project = Project::new("memo-self-describing");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);
    project.presentation(&work);

    let memo: Value = serde_json::from_str(
        &fs::read_to_string(
            project
                .root
                .join(".exitbind/presentation")
                .join(format!("{work}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(memo["kind"], "derived_display_memo");
    assert_eq!(memo["authority"], "none");
    // It carries no decision of its own: no terminal block, no evidence.
    assert!(memo["terminal"].is_null());
    assert!(memo["classification"]["exitState"].is_string());

    // A memo that does not say what it is reads as nothing, and is left alone.
    let path = project
        .root
        .join(".exitbind/presentation")
        .join(format!("{work}.json"));
    let foreign = "{\"classification\":{\"exitState\":\"READY\"},\"signature\":\"x\"}\n";
    fs::write(&path, foreign).unwrap();
    let seen = project.presentation(&work);
    assert_ne!(seen["exitState"], "READY");
    assert_eq!(fs::read_to_string(&path).unwrap(), foreign);
    fs::remove_dir_all(project.root).unwrap();
}

/// A reader who drills down to the low-level run surface must not lose the
/// product's wording: a lead was observed doing exactly that and composing its
/// own closing sentence because this surface offered none.
#[test]
fn the_run_surface_offers_the_same_terminal_block() {
    let project = Project::new("terminal-run-surface");
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

    let printed = project.call(&["run", "status", &ledger], None);
    assert!(printed.status.success(), "{printed:?}");
    let text = String::from_utf8_lossy(&printed.stdout);
    assert_eq!(
        text.lines().last(),
        Some("EXIT READY"),
        "the run surface does not end with the terminal block: {text}"
    );
    assert_eq!(text.matches("EXIT READY").count(), 1);

    let machine = project.value(&["run", "status", &ledger, "--json"], None);
    assert_eq!(machine["terminal"], "EXIT READY");

    // The same guard applies here: a changed tree ends the block.
    fs::write(project.root.join("source.txt"), b"env-first\nlater\n").unwrap();
    let after = String::from_utf8_lossy(&project.call(&["run", "status", &ledger], None).stdout)
        .into_owned();
    assert!(!after.contains("EXIT READY"), "{after}");
    let machine = project.value(&["run", "status", &ledger, "--json"], None);
    assert!(machine["terminal"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

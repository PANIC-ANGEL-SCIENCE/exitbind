use super::*;

#[test]
fn an_unchanged_state_is_read_without_repeating_the_line() {
    let project = Project::new("presentation-unchanged");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);

    let first = project.presentation(&work);
    assert_eq!(first["transition"], "evidence_became_current");
    assert_eq!(first["phrase"], "It fits now.");
    assert!(first["terminal"].is_null());
    for _ in 0..3 {
        let again = project.presentation(&work);
        assert!(again["transition"].is_null(), "{again}");
        assert!(again["phrase"].is_null());
        // The compact status surface stays available.
        assert!(again["neuro"]
            .as_str()
            .unwrap()
            .starts_with("[Neuro] Exitbind progress: "));
    }
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_check_that_goes_stale_speaks_once_and_recovers_once() {
    let project = Project::new("presentation-stale-check");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);
    project.presentation(&work);

    // A covered input changes: the passing check no longer belongs here.
    fs::write(project.root.join("source.txt"), b"env-first\nmore\n").unwrap();
    let stale = project.presentation(&work);
    assert_eq!(stale["transition"], "check_became_stale");
    assert_eq!(stale["phrase"], "That key fit the earlier result.");
    assert_eq!(stale["state"]["check"], "stale");
    assert!(project.presentation(&work)["transition"].is_null());

    project.value(&["work", "check", &work], None);
    let current = project.presentation(&work);
    assert_eq!(current["transition"], "evidence_became_current");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn a_review_that_goes_stale_speaks_once() {
    let project = Project::new("presentation-stale-review");
    let work = begin(&project, false);
    project.drive_until(&work, "lead_decision");
    project.presentation(&work);

    // A covered input changes after the review: the checks must run again, and
    // the approval that belongs to the earlier files cannot carry the result.
    fs::write(project.root.join("source.txt"), b"env-first\nchanged\n").unwrap();
    project.presentation(&work);
    project.value(&["work", "check", &work], None);
    let stale = project.presentation(&work);
    assert_eq!(stale["state"]["review"], "stale");
    assert_eq!(stale["transition"], "review_became_stale");
    assert_eq!(stale["phrase"], "The review is one step behind.");
    assert!(project.presentation(&work)["transition"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn preservation_state_keeps_retired_line_null_and_stays_quiet() {
    let project = Project::new("presentation-preservation");
    let work = begin(&project, true);
    project.drive_until(&work, "check");

    let active = project.presentation(&work);
    assert_eq!(active["state"]["preservation"], "active");
    assert!(active["terminal"].is_null());
    assert!(active["holytail"].is_null());

    let again = project.presentation(&work);
    assert!(again["transition"].is_null(), "{again}");
    assert!(again["holytail"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn terminal_readiness_says_exit_ready_and_nothing_else() {
    let project = Project::new("presentation-ready");
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
    assert_eq!(accepted["next"]["progress"]["state"], "READY");
    let final_state = project.value(&["work", "next", &work], None);
    assert_eq!(final_state["next"]["progress"]["percent"], 100);
    // The decision itself hands back the block to print, so nothing has to be
    // assembled from status and progress.
    assert_eq!(accepted["presentation"]["terminal"], "EXIT READY");
    assert!(accepted["presentation"]["phrase"].is_null());

    let ready = project.presentation(&work);
    assert_eq!(ready["exitState"], "READY");
    assert_eq!(ready["terminal"], "EXIT READY");
    assert!(
        ready["transition"].is_null(),
        "READY takes no flavour line: {ready}"
    );
    assert!(ready["phrase"].is_null());
    assert_eq!(ready["neuro"], "[Neuro] Exitbind progress: 100%.");
    let reread = project.presentation(&work);
    assert_eq!(reread["terminal"], "EXIT READY");
    assert!(reread["transition"].is_null());
    assert!(reread["phrase"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn resuming_with_valid_evidence_can_say_so_once() {
    let project = Project::new("presentation-resume");
    let work = begin(&project, false);
    project.drive_until(&work, "check");
    project.value(&["work", "check", &work], None);
    // A fresh consumer has no memory of what was presented before.
    fs::remove_dir_all(project.root.join(".exitbind/presentation")).unwrap();

    let resumed = project.value(&["work", "resume"], None);
    assert_eq!(resumed["status"], "resumed");
    let presentation = &resumed["presentation"];
    assert_eq!(presentation["transition"], "resumed_with_valid_evidence");
    assert_eq!(presentation["phrase"], "That door is already open.");
    assert!(project.presentation(&work)["transition"].is_null());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn without_governed_work_there_is_no_progress_or_preservation_line() {
    let project = Project::new("presentation-none");
    let idle = project.value(&["work", "resume"], None);
    assert_eq!(idle["status"], "none");
    assert!(idle["presentation"].is_null(), "{idle}");
    assert!(
        !String::from_utf8_lossy(&project.call(&["work", "resume"], None).stdout)
            .contains("[Neuro]")
    );
    fs::remove_dir_all(project.root).unwrap();
}

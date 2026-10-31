use super::scoped_help;

#[test]
fn scoped_work_help_keeps_the_subcommand_syntax() {
    let help = scoped_help("work", &["next".to_owned()]).unwrap();
    assert!(help.contains("Usage: "));
    assert!(help.contains("work next WORK"));
    let begin = scoped_help("work", &["begin".to_owned()]).unwrap();
    assert!(begin.contains("conversation Goal"));
    assert!(begin.contains("--project-goal"));
    assert!(begin.contains("--standalone"));
    assert!(begin.contains("--journey-integration"));
    let recovery = scoped_help("work", &["return".to_owned()]).unwrap();
    assert!(recovery.contains("--result-ref HELD_REFERENCE"));
}

#[test]
fn goal_help_explains_explicit_legacy_project_selection() {
    let help = scoped_help("goal", &["incorporate".to_owned()]).unwrap();
    assert!(help.contains("--project"));
    assert!(help.contains("conversation Goal"));
}

#[test]
fn scoped_read_help_exposes_redundant_json_routes() {
    let continuation = scoped_help("work", &["continuation".to_owned()]).unwrap();
    assert!(continuation.contains("--json"));
    let validate = scoped_help("work", &["validate".to_owned()]).unwrap();
    assert!(validate.contains("--json"));
    let expand = scoped_help("work", &["expand".to_owned()]).unwrap();
    assert!(expand.contains("--json"));
    let supersede = scoped_help("run", &["supersede".to_owned()]).unwrap();
    assert!(supersede.contains("--owner-recovery STATE_ARTIFACT_PATH"));
}

#[test]
fn init_and_update_help_are_scoped_and_non_mutating() {
    let init = scoped_help("init", &[]).unwrap();
    assert!(init.contains("Usage: "));
    assert!(init.contains("--mode local|portable|auto"));
    assert!(init.contains("review placement and boundaries"));

    let update = scoped_help("update", &[]).unwrap();
    assert!(update.contains("Usage: "));
    assert!(update.contains("checks for a stable release"));
    assert!(update.contains("may replace the current executable"));
}

#[test]
fn unknown_scoped_help_falls_back_to_global_help() {
    assert!(scoped_help("work", &["unknown".to_owned()]).is_none());
}

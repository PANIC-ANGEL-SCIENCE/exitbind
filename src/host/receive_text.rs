//! The receiving agent's Work navigation in the session context.

/// A template for a known Work; its configuration comes from that Work's
/// context, not from the project that happened to start the session.
fn work_template(action: &str, options: &[&str]) -> String {
    let work = crate::emitted::slot("WORK");
    let work_config = crate::emitted::slot("WORK_CONFIG");
    crate::emitted::Route::new(
        ["work", action, work.as_str()]
            .into_iter()
            .chain(options.iter().copied()),
    )
    .config_text(&work_config)
    .shell_text()
}

/// Scope the project fallback to its emitted executable and configuration.
pub(super) fn receive_work(config: Option<&std::path::Path>) -> String {
    format!("For a known Work or explicit smw_ locator, replace both placeholders in `{}` with the Work locator and its own configuration as shell-quoted arguments; that configuration can differ from this session's project. Read complete current profile/rules, assignment, evidence and action forms. Follow exact emitted argv; stale or incomplete detail needs its supported refresh/expansion before action. For Work in this project, use `{}` and its current detail route; this does not search other projects. Focus is navigation, never authority. Use `{}` with the same substitutions only for an initialized same-Work cross-host handoff; its receive block gives the bind and child-prepare commands. If another goal owns that sidecar, preserve it and use current Work detail. Do not read raw .exitbind state to recover it.",
        work_template("detail", &["--json"]),
        crate::emitted::command_line(&["work", "resume", "--json"], config),
        work_template("continuation", &[]))
}

/// Keep a complete navigation route when the full examples exceed the budget.
pub(super) fn receive_work_compact(config: Option<&std::path::Path>) -> String {
    format!("Known Work: replace both placeholders in `{}` with its locator and own configuration as shell-quoted arguments. For this project only, run `{}` then detail; other projects are not searched. Read profile, rules, assignment, evidence and forms; refresh outdated/incomplete detail. Focus grants no authority. Follow the receive block only for an initialized same-Work cross-host handoff; preserve foreign Goal sidecars. Never read raw .exitbind state.",
        work_template("detail", &["--json"]),
        crate::emitted::command_line(&["work", "resume", "--json"], config))
}

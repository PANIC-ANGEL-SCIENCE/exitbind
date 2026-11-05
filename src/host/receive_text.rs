//! The receiving agent's Work navigation in the session context.

/// Scope the project fallback to its emitted executable and configuration.
pub(super) fn receive_work(config: Option<&std::path::Path>) -> String {
    format!("For a known Work or explicit smw_ locator, read work detail --json using that Work's own configuration, which can differ from this session's project, for complete current profile/rules, assignment, evidence and action forms. Follow exact emitted argv; stale or incomplete detail needs its supported refresh/expansion before action. For Work in this project, use `{}` and its current detail route; this does not search other projects. Focus is navigation, never authority. Use work continuation with that Work's own configuration only for an initialized same-Work cross-host handoff; its receive block gives the bind and child-prepare commands. If another goal owns that sidecar, preserve it and use current Work detail. Do not read raw .exitbind state to recover it.",
        crate::emitted::command_line(&["work", "resume", "--json"], config))
}

/// Keep a complete navigation route when the full examples exceed the budget.
pub(super) fn receive_work_compact(config: Option<&std::path::Path>) -> String {
    format!("Known Work: read work detail --json with that Work's own configuration. For this project only, run `{}` then detail; other projects are not searched. Read profile, rules, assignment, evidence and forms; refresh outdated/incomplete detail. Focus grants no authority. Use continuation with the Work's configuration only for an initialized same-Work cross-host handoff; preserve foreign Goal sidecars. Never read raw .exitbind state.",
        crate::emitted::command_line(&["work", "resume", "--json"], config))
}

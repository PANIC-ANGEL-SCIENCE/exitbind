//! The receiving agent's Work read routes in the session context, each
//! printed through the emitted-route builder.

/// How a receiving agent reads its Work, with every route printed through
/// the emitted-route builder (this executable, `--config` when known).
pub(super) fn receive_work(config: Option<&std::path::Path>) -> String {
    format!("For a known Work or explicit smw_ locator, run `{}` directly for complete current profile/rules, assignment, evidence and action forms. Follow exact emitted argv; stale or incomplete detail needs its supported refresh/expansion before action. Unknown Work uses `{}` and its current detail route; focus is navigation, never authority. Use `{}` only for an initialized same-Work cross-host handoff; its `receive` block gives the bind and child-prepare commands. If another goal owns that sidecar, preserve it and use current Work detail. Do not read raw .exitbind state to recover it.",
        crate::emitted::command_line(&["work", "detail", &crate::emitted::slot("WORK"), "--json"], config),
        crate::emitted::command_line(&["work", "resume", "--json"], config),
        crate::emitted::command_line(&["work", "continuation", &crate::emitted::slot("WORK")], config))
}

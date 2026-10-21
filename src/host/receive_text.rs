//! The receiving agent's Work read routes in the session context, each
//! printed through the emitted-route builder.

/// How a receiving agent reads its Work, with every route printed through
/// the emitted-route builder (this executable, `--config` when known).
pub(super) fn receive_work(config: Option<&std::path::Path>) -> String {
    receive_text(|words| crate::emitted::command_line(words, config))
}

/// The bytes the printed executable and configuration add to the receiving
/// text beyond a bare product name. The session budget was sized for bare
/// names; these bytes ride on top of it, inside the host's hard cap.
pub(super) fn route_bytes(receive: &str) -> usize {
    receive
        .len()
        .saturating_sub(receive_text(|words| format!("exitbind {}", words.join(" "))).len())
}

fn receive_text(route: impl Fn(&[&str]) -> String) -> String {
    format!("For a known Work or explicit smw_ locator, run `{}` directly for complete current profile/rules, assignment, evidence and action forms. Follow exact emitted argv; stale or incomplete detail needs its supported refresh/expansion before action. Unknown Work uses `{}` and its current detail route; focus is navigation, never authority. Use `{}` only for an initialized same-Work cross-host handoff; its `receive` block gives the bind and child-prepare commands. If another goal owns that sidecar, preserve it and use current Work detail. Do not read raw .exitbind state to recover it.",
        route(&["work", "detail", "WORK", "--json"]),
        route(&["work", "resume", "--json"]),
        route(&["work", "continuation", "WORK"]))
}

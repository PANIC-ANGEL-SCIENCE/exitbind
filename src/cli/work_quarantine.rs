//! Explicit Lead admission of a single separately reviewed verification.

use super::*;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let followup = a.positional.first().map(String::as_str) == Some("quarantine-followup");
    let command = if followup {
        "work quarantine-followup"
    } else {
        "work quarantine"
    };
    args::assert_options(
        command,
        a,
        &[
            "config",
            "json",
            "apply",
            "current-binding",
            "feature-config",
            "feature-work",
            "owner-config",
            "owner-config-sha256",
            "artifact-root",
            "artifact",
            "reason",
            "request-id",
        ],
    )?;
    args::assert_positionals(command, a, 2)?;
    let response = crate::work::quarantine::start(
        l,
        positional(a, 1, "work quarantine requires WORK")?,
        crate::work::quarantine::Options {
            followup,
            owner_config: a.options.get("owner-config").map(String::as_str),
            owner_config_sha256: a.options.get("owner-config-sha256").map(String::as_str),
            feature_config: option(
                a,
                "feature-config",
                "work quarantine requires --feature-config PATH",
            )?,
            feature_work: option(
                a,
                "feature-work",
                "work quarantine requires --feature-work WORK",
            )?,
            artifact_root: a.options.get("artifact-root").map(String::as_str),
            artifact: option(a, "artifact", "work quarantine requires --artifact PATH")?,
            reason: option(a, "reason", "work quarantine requires --reason TEXT")?,
            request_id: option(a, "request-id", "work quarantine requires --request-id ID")?,
            current_binding: Some(option(
                a,
                "current-binding",
                "work quarantine requires --current-binding BINDING",
            )?),
            apply: a.flags.contains_key("apply"),
        },
    )?;
    print_json(&crate::work::effective_action::attach(&response))
}

use super::*;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "work replace-check",
        a,
        &[
            "config",
            "json",
            "apply",
            "current-binding",
            "check-command",
            "pre-return-command",
            "pre-return-timeout-ms",
            "pre-return-capture-bytes",
            "reason",
            "classification",
        ],
    )?;
    args::assert_positionals("work replace-check", a, 2)?;
    let work = positional(a, 1, "work replace-check requires WORK")?;
    let response = crate::work::check_replacement::replace(
        l,
        work,
        crate::work::check_replacement::Options {
            check_command: a.options.get("check-command").map(String::as_str),
            pre_return_command: a.options.get("pre-return-command").map(String::as_str),
            pre_return_timeout_ms: a.options.get("pre-return-timeout-ms").map(String::as_str),
            pre_return_capture_bytes: a
                .options
                .get("pre-return-capture-bytes")
                .map(String::as_str),
            reason: option(a, "reason", "work replace-check requires --reason TEXT")?,
            classification: option(
                a,
                "classification",
                "work replace-check requires --classification defective-contract",
            )?,
            current_binding: Some(option(
                a,
                "current-binding",
                "work replace-check requires --current-binding BINDING",
            )?),
            apply: a.flags.contains_key("apply"),
        },
    )?;
    print_json(&crate::work::effective_action::attach(&response))
}

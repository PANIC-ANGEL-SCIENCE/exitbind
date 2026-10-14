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
            "reason",
            "classification",
        ],
    )?;
    args::assert_positionals("work replace-check", a, 3)?;
    let work = positional(a, 2, "work replace-check requires WORK")?;
    let response = crate::work::check_replacement::replace(
        l,
        work,
        crate::work::check_replacement::Options {
            check_command: option(
                a,
                "check-command",
                "work replace-check requires --check-command COMMAND",
            )?,
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

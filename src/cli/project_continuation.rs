//! Wiring for the explicit project-continuation protocol.
use super::{args, print_json, Arguments};
use crate::{config::Loaded, project::continuation};

pub(super) fn command(loaded: &Loaded, arguments: &Arguments) -> Result<(), String> {
    let action = arguments.positional.get(1).map(String::as_str).ok_or(
        "project advance requires form, propose, select, amend, admit, status, recover or stop",
    )?;
    let named = |index: usize, label: &str| {
        arguments
            .positional
            .get(index)
            .map(String::as_str)
            .ok_or_else(|| format!("project advance {action} requires {label}"))
    };
    let required = |key: &str| {
        arguments
            .options
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("project advance {action} requires --{key}"))
    };
    let expected = arguments.options.get("current-binding").map(String::as_str);
    let apply = arguments.flags.contains_key("apply");
    let value = match action {
        "form" => {
            args::assert_options("project advance form", arguments, &["config", "json"])?;
            args::assert_positionals("project advance form", arguments, 2)?;
            continuation::form()
        }
        "propose" => {
            args::assert_options("project advance propose", arguments, &["config", "json"])?;
            args::assert_positionals("project advance propose", arguments, 3)?;
            continuation::propose(loaded, named(2, "SPECIFICATION")?)?
        }
        "select" => {
            args::assert_options(
                "project advance select",
                arguments,
                &["config", "json", "reason", "current-binding", "apply"],
            )?;
            args::assert_positionals("project advance select", arguments, 3)?;
            continuation::select(
                loaded,
                named(2, "SPECIFICATION")?,
                required("reason")?,
                expected,
                apply,
            )?
        }
        "amend" => {
            args::assert_options(
                "project advance amend",
                arguments,
                &[
                    "config",
                    "json",
                    "reason",
                    "request-id",
                    "current-binding",
                    "apply",
                ],
            )?;
            args::assert_positionals("project advance amend", arguments, 3)?;
            continuation::amend(
                loaded,
                named(2, "WORK")?,
                required("reason")?,
                required("request-id")?,
                expected,
                apply,
            )?
        }
        "admit" => {
            args::assert_options(
                "project advance admit",
                arguments,
                &["config", "json", "proposal", "current-binding", "apply"],
            )?;
            args::assert_positionals("project advance admit", arguments, 3)?;
            continuation::admit(
                loaded,
                named(2, "SPECIFICATION")?,
                required("proposal")?,
                expected,
                apply,
            )?
        }
        "recover" => {
            args::assert_options(
                "project advance recover",
                arguments,
                &["config", "json", "task", "current-binding", "apply"],
            )?;
            args::assert_positionals("project advance recover", arguments, 3)?;
            continuation::recover(
                loaded,
                named(2, "MANDATE_ID")?,
                required("task")?,
                expected,
                apply,
            )?
        }
        "status" => {
            args::assert_options("project advance status", arguments, &["config", "json"])?;
            args::assert_positionals("project advance status", arguments, 3)?;
            continuation::status(loaded, named(2, "MANDATE_ID")?)?
        }
        "stop" => {
            args::assert_options(
                "project advance stop",
                arguments,
                &["config", "json", "reason"],
            )?;
            args::assert_positionals("project advance stop", arguments, 3)?;
            continuation::stop(loaded, named(2, "MANDATE_ID")?, required("reason")?)?
        }
        _ => return Err(
            "project advance requires form, propose, select, amend, admit, status, recover or stop"
                .into(),
        ),
    };
    print_json(&value)
}

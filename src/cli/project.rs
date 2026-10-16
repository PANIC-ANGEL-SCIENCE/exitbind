//! Read-only project context and exact current memory detail.

use super::{args, print_json, Arguments};
use crate::{config::Loaded, project::context};

pub(super) fn command(loaded: &Loaded, arguments: &Arguments) -> Result<(), String> {
    let action = arguments.positional.first().map(String::as_str);
    if action == Some("advance") {
        return super::project_continuation::command(loaded, arguments);
    }
    if action == Some("route") {
        args::assert_options(
            "project route",
            arguments,
            &["config", "json", "task", "purpose", "role", "host", "agent"],
        )?;
        args::assert_positionals("project route", arguments, 1)?;
        let required = |name: &str| {
            arguments
                .options
                .get(name)
                .map(String::as_str)
                .ok_or_else(|| format!("project route requires --{name}"))
        };
        return print_json(&crate::project::specialists::route(
            loaded,
            required("task")?,
            required("purpose")?,
            required("role")?,
            required("host")?,
            arguments.options.get("agent").map(String::as_str),
        )?);
    }
    if action == Some("capabilities") {
        if arguments.positional.get(1).map(String::as_str) == Some("select") {
            args::assert_options(
                "project capabilities select",
                arguments,
                &[
                    "config",
                    "json",
                    "decision",
                    "reason",
                    "current-binding",
                    "apply",
                ],
            )?;
            args::assert_positionals("project capabilities select", arguments, 3)?;
            return print_json(&crate::project::capabilities::select(
                loaded,
                &arguments.positional[2],
                arguments
                    .options
                    .get("decision")
                    .map(String::as_str)
                    .unwrap_or(""),
                arguments
                    .options
                    .get("reason")
                    .map(String::as_str)
                    .unwrap_or(""),
                arguments.options.get("current-binding").map(String::as_str),
                arguments.flags.contains_key("apply"),
            )?);
        }
        args::assert_options(
            "project capabilities",
            arguments,
            &["config", "json", "task", "host"],
        )?;
        args::assert_positionals("project capabilities", arguments, 1)?;
        return print_json(&crate::project::capabilities::inspect(
            loaded,
            arguments
                .options
                .get("task")
                .map(String::as_str)
                .unwrap_or(""),
            arguments
                .options
                .get("host")
                .map(String::as_str)
                .unwrap_or("codex"),
        )?);
    }
    if action == Some("architecture") {
        if arguments.positional.get(1).map(String::as_str) == Some("select") {
            args::assert_options(
                "project architecture select",
                arguments,
                &[
                    "config",
                    "json",
                    "decision",
                    "reason",
                    "current-binding",
                    "apply",
                ],
            )?;
            args::assert_positionals("project architecture select", arguments, 3)?;
            let value = crate::project::architecture::select(
                loaded,
                &arguments.positional[2],
                arguments
                    .options
                    .get("decision")
                    .map(String::as_str)
                    .unwrap_or(""),
                arguments
                    .options
                    .get("reason")
                    .map(String::as_str)
                    .unwrap_or(""),
                arguments.options.get("current-binding").map(String::as_str),
                arguments.flags.contains_key("apply"),
            )?;
            return print_json(&value);
        }
        args::assert_options("project architecture", arguments, &["config", "json"])?;
        let checking = match arguments.positional.get(1).map(String::as_str) {
            None => {
                args::assert_positionals("project architecture", arguments, 1)?;
                false
            }
            Some("check") => {
                args::assert_positionals("project architecture check", arguments, 2)?;
                true
            }
            _ => return Err("project architecture accepts check or select SOURCE".into()),
        };
        let value = if checking {
            crate::project::architecture::check(loaded)?
        } else {
            crate::project::architecture::inspect(loaded)?
        };
        print_json(&value)?;
        return if value["outcome"] == "failed" {
            Err("architecture contract check failed".into())
        } else {
            Ok(())
        };
    }
    if action == Some("agents") {
        args::assert_options("project agents", arguments, &["config", "json", "apply"])?;
        args::assert_positionals("project agents", arguments, 1)?;
        let value = if arguments.flags.contains_key("apply") {
            crate::project::native_profiles::apply(loaded)?
        } else {
            crate::project::native_profiles::status(loaded)?
        };
        return print_json(&value);
    }
    args::assert_options("project context", arguments, &["config", "json", "task"])?;
    if action != Some("context") {
        return Err("project requires context, route, agents, capabilities, architecture or advance; use project advance form --json for bounded continuation".into());
    }
    match arguments.positional.get(1).map(String::as_str) {
        None => {
            args::assert_positionals("project context", arguments, 1)?;
            let value = context::snapshot_for_task(
                loaded,
                arguments.options.get("task").map(String::as_str),
            );
            if arguments.flags.contains_key("json") {
                print_json(&value)
            } else {
                println!("Project context ({})", value["project"]["identity"]["kind"]);
                println!("Focus: {}", value["focus"]["state"]);
                println!("Lead: {}", crate::project::agent_display::lead(loaded));
                println!("Rules: {}", value["rules"].as_array().map_or(0, Vec::len));
                println!("Memory: {}", value["memory"]["state"]);
                println!("Details: exitbind project context --json --config CONFIG");
                Ok(())
            }
        }
        Some("memory") => {
            args::assert_positionals("project context memory", arguments, 3)?;
            let id = arguments
                .positional
                .get(2)
                .ok_or("project context memory requires ITEM_ID")?;
            let value = context::memory_content(
                loaded,
                id,
                arguments.options.get("task").map(String::as_str),
            )?;
            if arguments.flags.contains_key("json") {
                print_json(&value)
            } else {
                print!("{}", value["content"].as_str().unwrap_or_default());
                Ok(())
            }
        }
        _ => Err("project context accepts only memory ITEM_ID".into()),
    }
}

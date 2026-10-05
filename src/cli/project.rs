//! Read-only project context and exact current memory detail.

use super::{args, print_json, Arguments};
use crate::{config::Loaded, project::context};

pub(super) fn command(loaded: &Loaded, arguments: &Arguments) -> Result<(), String> {
    let action = arguments.positional.first().map(String::as_str);
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
    args::assert_options("project context", arguments, &["config", "json"])?;
    if action != Some("context") {
        return Err("project requires context, agents or architecture".into());
    }
    match arguments.positional.get(1).map(String::as_str) {
        None => {
            args::assert_positionals("project context", arguments, 1)?;
            let value = context::snapshot(loaded);
            if arguments.flags.contains_key("json") {
                print_json(&value)
            } else {
                println!("Project context ({})", value["project"]["identity"]["kind"]);
                println!("Focus: {}", value["focus"]["state"]);
                println!("Lead: {}", value["lead"]["agentId"]);
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
            let value = context::memory_content(loaded, id)?;
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

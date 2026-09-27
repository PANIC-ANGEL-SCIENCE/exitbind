//! Read-only project context and exact current memory detail.

use super::{args, print_json, Arguments};
use crate::{config::Loaded, project::context};

pub(super) fn command(loaded: &Loaded, arguments: &Arguments) -> Result<(), String> {
    let action = arguments.positional.first().map(String::as_str);
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
        return Err("project requires context or agents".into());
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

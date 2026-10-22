//! Project context, current memory and bounded continuation commands.

#[path = "project_continuation.rs"]
mod continuation;

use super::{args, print_json, Arguments};
use crate::{config::Loaded, project::context};

/// Every `project` action; `graph` is routed before configuration loads.
pub(crate) const PROJECT_ACTIONS: &[&str] = &[
    "context",
    "route",
    "agents",
    "capabilities",
    "architecture",
    "advance",
    "graph",
];

pub(super) fn command(loaded: &Loaded, arguments: &Arguments) -> Result<(), String> {
    let action = arguments.positional.first().map(String::as_str);
    if !action.is_some_and(|action| action != "graph" && PROJECT_ACTIONS.contains(&action)) {
        let form = crate::emitted::command_line(
            &["project", "advance", "form", "--json"],
            Some(&loaded.path),
        );
        return Err(format!(
            "project requires {}; use {form} for bounded continuation",
            PROJECT_ACTIONS[..PROJECT_ACTIONS.len() - 1].join(", ")
        ));
    }
    if action == Some("advance") {
        return continuation::command(loaded, arguments);
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
    args::assert_options(
        "project context",
        arguments,
        &["config", "json", "task", "agent", "host", "expected-sha256"],
    )?;
    match arguments.positional.get(1).map(String::as_str) {
        None => {
            args::assert_positionals("project context", arguments, 1)?;
            if ["agent", "host", "expected-sha256"]
                .iter()
                .any(|name| arguments.options.contains_key(*name))
            {
                let route = crate::emitted::command_line(
                    &[
                        "project",
                        "context",
                        "memory",
                        &crate::emitted::slot("ITEM_ID"),
                    ],
                    Some(&loaded.path),
                );
                return Err(format!("recipient options require {route}"));
            }
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
                let details = crate::emitted::command_line(
                    &["project", "context", "--json"],
                    Some(&loaded.path),
                );
                println!("Details: {details}");
                Ok(())
            }
        }
        Some("memory") => {
            args::assert_positionals("project context memory", arguments, 3)?;
            let id = arguments
                .positional
                .get(2)
                .ok_or("project context memory requires ITEM_ID")?;
            let has_agent = arguments.options.contains_key("agent");
            let has_source_binding = arguments.options.contains_key("expected-sha256");
            let has_host = arguments.options.contains_key("host");
            let value = match (has_agent, has_source_binding, has_host) {
                (false, false, false) => context::memory_content(
                    loaded,
                    id,
                    arguments.options.get("task").map(String::as_str),
                )?,
                (true, true, _) => context::memory_content_for_recipient(
                    loaded,
                    id,
                    arguments.options.get("agent").map(String::as_str).unwrap_or(""),
                    arguments
                        .options
                        .get("task")
                        .map(String::as_str)
                        .ok_or("recipient memory requires --task TASK")?,
                    arguments.options.get("host").map(String::as_str),
                    arguments
                        .options
                        .get("expected-sha256")
                        .map(String::as_str)
                        .unwrap_or(""),
                )?,
                _ => return Err("recipient memory requires --agent, --task and --expected-sha256; contextual items also require --host".into()),
            };
            if arguments.flags.contains_key("json") {
                print_json(&value)
            } else {
                print!("{}", value["content"].as_str().unwrap_or_default());
                Ok(())
            }
        }
        Some("guidance") => {
            args::assert_positionals("project context guidance", arguments, 3)?;
            let id = arguments
                .positional
                .get(2)
                .ok_or("project context guidance requires ITEM_ID")?;
            let option = |name: &str| arguments.options.get(name).map(String::as_str);
            let (Some(agent), Some(expected)) = (option("agent"), option("expected-sha256")) else {
                return Err(
                    "project context guidance requires --agent AGENT and --expected-sha256 SHA"
                        .into(),
                );
            };
            let value =
                crate::project::agent_context::phase_guidance_content(loaded, id, agent, expected)?;
            if arguments.flags.contains_key("json") {
                print_json(&value)
            } else {
                print!("{}", value["content"].as_str().unwrap_or_default());
                Ok(())
            }
        }
        _ => Err("project context accepts only memory ITEM_ID or guidance ITEM_ID".into()),
    }
}

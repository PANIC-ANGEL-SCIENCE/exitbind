use super::{args, option, positional};
use crate::retrospective;
use std::path::Path;

pub(super) fn command(a: &args::Arguments) -> Result<(), String> {
    let action = positional(a, 0, "retrospective requires inspect or expand")?;
    match action {
        "inspect" => {
            args::assert_options(
                "retrospective inspect",
                a,
                &["repo", "source", "since", "until", "json"],
            )?;
            args::assert_positionals("retrospective inspect", a, 1)?;
            let value = retrospective::inspect(
                Path::new(option(
                    a,
                    "repo",
                    "retrospective inspect requires --repo PATH",
                )?),
                Path::new(option(
                    a,
                    "source",
                    "retrospective inspect requires --source PATH",
                )?),
                option(a, "since", "retrospective inspect requires --since RFC3339")?,
                option(a, "until", "retrospective inspect requires --until RFC3339")?,
            )?;
            if a.flags.contains_key("json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
                );
            } else {
                print!("{}", crate::retrospective::output::human(&value));
            }
            Ok(())
        }
        "expand" => {
            args::assert_options(
                "retrospective expand",
                a,
                &["repo", "source", "since", "until", "json"],
            )?;
            args::assert_positionals("retrospective expand", a, 2)?;
            let value = retrospective::expand(
                Path::new(option(
                    a,
                    "repo",
                    "retrospective expand requires --repo PATH",
                )?),
                Path::new(option(
                    a,
                    "source",
                    "retrospective expand requires --source PATH",
                )?),
                positional(a, 1, "retrospective expand requires REF")?,
                option(a, "since", "retrospective expand requires --since RFC3339")?,
                option(a, "until", "retrospective expand requires --until RFC3339")?,
            )?;
            if a.flags.contains_key("json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
                );
            } else {
                println!("reference: {}", value["reference"].as_str().unwrap_or(""));
                println!(
                    "timestamp: {}",
                    value["timestamp"].as_str().unwrap_or("unknown")
                );
                println!("role: {}", value["role"].as_str().unwrap_or("unknown"));
                println!("{}", value["content"].as_str().unwrap_or(""));
            }
            Ok(())
        }
        _ => Err("retrospective requires inspect or expand".into()),
    }
}

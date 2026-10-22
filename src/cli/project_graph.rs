use super::{args, print_json, Arguments};
use serde_json::Value;

pub(super) fn command(arguments: &Arguments) -> Result<(), String> {
    args::assert_options("project graph", arguments, &["config", "query", "json"])?;
    args::assert_positionals("project graph", arguments, 2)?;
    let action = arguments.positional[1].as_str();
    let value = crate::host::codegraph::command(
        action,
        arguments.options.get("config").map(String::as_str),
        arguments.options.get("query").map(String::as_str),
    )?;
    if arguments.flags.contains_key("json") {
        return print_json(&value);
    }
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    if value.get("enabled").and_then(Value::as_bool) == Some(true) {
        println!("CodeGraph is enabled and prepared for the validated project.");
    } else {
        match status {
            "usable" => {
                let context = &value["context"];
                let nodes = context["nodes"].as_array().map(Vec::len).unwrap_or(0);
                let edges = context["edges"].as_array().map(Vec::len).unwrap_or(0);
                let blocks = context["codeBlocks"].as_array().map(Vec::len).unwrap_or(0);
                println!(
                    "CodeGraph: {} ({nodes} nodes, {edges} edges, {blocks} code blocks).",
                    value["summary"]
                        .as_str()
                        .unwrap_or("query result available")
                );
                println!("Evidence is retained with source, provider, and index provenance.");
            }
            "empty" => println!("CodeGraph returned a genuine empty result for this query."),
            "disabled" => {
                let enable = crate::emitted::command_line(
                    &["project", "graph", "enable"],
                    arguments.options.get("config").map(std::path::Path::new),
                );
                println!("CodeGraph is disabled for this validated project. Enable it with `{enable}` when indexing is authorized.");
            }
            _ => {
                let reason = value["reason"]
                    .as_str()
                    .unwrap_or("target or provider state is unresolved");
                println!("CodeGraph unavailable: {reason}. Continue ordinary source investigation in the validated project.");
            }
        }
    }
    Ok(())
}

//! Existing layout migration CLI, extracted to keep the router below its budget.
use super::{args, positional, print_json, Arguments};
use crate::config;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options("migrate", a, &["config", "apply"])?;
    args::assert_positionals("migrate", a, 1)?;
    let apply = a.flags.contains_key("apply");
    let value = match positional(a, 0, "migrate requires layout or paths")? {
        "layout" => crate::project::layout::run(l, apply),
        "paths" => crate::project::layout::prepare_paths(l, apply),
        _ => return Err("migrate requires layout or paths".into()),
    }?;
    print_json(&value)
}

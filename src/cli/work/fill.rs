//! `work fill`: run one current printed form with its placeholders filled.
use super::*;
use std::collections::BTreeMap;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let mut allowed = vec!["config", "current-binding", "choice", "artifact-file"];
    let options = fill_options();
    allowed.extend(options.iter().map(|option| &option[2..]));
    args::assert_options("work fill", a, &allowed)?;
    args::assert_positionals("work fill", a, 2)?;
    let work = positional(a, 1, "work fill requires WORK")?;
    let binding = option(
        a,
        "current-binding",
        "work fill requires --current-binding BINDING from the printed fill route",
    )?;
    let choice = option(a, "choice", "work fill requires --choice LABEL")?;
    let values: BTreeMap<String, String> = options
        .iter()
        .filter_map(|option| {
            a.options
                .get(&option[2..])
                .map(|value| ((*option).to_owned(), value.clone()))
        })
        .collect();
    let artifact = a.options.get("artifact-file");
    let bound = crate::work::action_forms::fill::bound(
        l,
        work,
        binding,
        choice,
        &values,
        artifact.is_some(),
    )?;
    let stdin = match artifact {
        Some(path) if bound.reads_artifact => std::process::Stdio::from(
            std::fs::File::open(path)
                .map_err(|error| format!("--artifact-file {path} cannot be read: {error}"))?,
        ),
        _ => std::process::Stdio::null(),
    };
    let status = std::process::Command::new(&bound.argv[0])
        .args(&bound.argv[1..])
        .stdin(stdin)
        .status()
        .map_err(|error| format!("the filled route could not start: {error}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Every option a registered placeholder is filled by.
fn fill_options() -> Vec<&'static str> {
    let mut options: Vec<&'static str> = crate::emitted::PLACEHOLDERS
        .iter()
        .flat_map(|placeholder| placeholder.fills.iter())
        .filter_map(|fill| match fill {
            crate::emitted::Fill::Flag(option) => Some(*option),
            _ => None,
        })
        .collect();
    options.sort_unstable();
    options.dedup();
    options
}

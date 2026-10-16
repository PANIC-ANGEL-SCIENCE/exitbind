use super::*;
use std::io::Read;

pub(super) fn command(loaded: &config::Loaded, args_value: &Arguments) -> Result<(), String> {
    match args_value.positional.get(1).map(String::as_str) {
        Some("validate") => {
            args::assert_options("work challenge validate", args_value, &["config", "json"])?;
            args::assert_positionals("work challenge validate", args_value, 3)?;
            let work = positional(args_value, 2, "work challenge validate requires WORK")?;
            let mut bytes = Vec::new();
            std::io::stdin()
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("earlyEvidence input could not be read: {error}"))?;
            super::print_work_json(&crate::work::early_evidence::validate_current(
                loaded, work, &bytes,
            )?)
        }
        Some("prepare") => prepare(loaded, args_value, 2, 3),
        _ => prepare(loaded, args_value, 1, 2),
    }
}

fn prepare(
    loaded: &config::Loaded,
    args_value: &Arguments,
    work_index: usize,
    positional_count: usize,
) -> Result<(), String> {
    args::assert_options(
        "work challenge prepare",
        args_value,
        &[
            "config",
            "question",
            "probe-command",
            "probe-source",
            "json",
        ],
    )?;
    args::assert_positionals("work challenge prepare", args_value, positional_count)?;
    let packet = crate::work::early_evidence::packet(
        loaded,
        positional(
            args_value,
            work_index,
            "work challenge prepare requires WORK",
        )?,
        option(
            args_value,
            "question",
            "work challenge prepare requires --question QUESTION",
        )?,
    )?;
    super::print_work_json(&crate::work::early_evidence::with_probe(
        loaded,
        packet,
        args_value.options.get("probe-command").map(String::as_str),
        args_value.options.get("probe-source").map(String::as_str),
    )?)
}

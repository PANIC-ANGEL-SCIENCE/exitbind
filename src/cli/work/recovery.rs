//! Recovery command parsing for existing checks and interrupted pre-return.
use super::*;

pub(super) fn command(l: &config::Loaded, a: &Arguments, action: &str) -> Result<(), String> {
    let (usage, missing) = match action {
        "recover-check" => ("work recover-check", "work recover-check requires WORK"),
        "recover-pre-return" => (
            "work recover-pre-return",
            "work recover-pre-return requires WORK",
        ),
        _ => return Err("unknown work recovery command".into()),
    };
    args::assert_options(usage, a, &["config", "json", "apply", "current-binding"])?;
    args::assert_positionals(usage, a, 2)?;
    let work = positional(a, 1, missing)?;
    let binding = a.options.get("current-binding").map(String::as_str);
    let apply = a.flags.contains_key("apply");
    let value = match action {
        "recover-check" => crate::work::check_recovery::recover(l, work, binding, apply)?,
        "recover-pre-return" => crate::work::pre_return_recovery::recover(l, work, binding, apply)?,
        _ => unreachable!(),
    };
    print_work_json(&value)
}

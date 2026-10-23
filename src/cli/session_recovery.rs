//! `session recovery show|record`: the native session's recovery read model
//! and its record, including the Lead's pause form (`--step`, `--decision`).

use super::*;

/// Every `session` action this dispatcher accepts (`recovery show|record`).
pub(crate) const SESSION_ACTIONS: &[&str] = &["recovery"];

pub(super) fn command(a: &Arguments) -> Result<(), String> {
    args::assert_options(
        "session recovery",
        a,
        &["config", "json", "step", "decision"],
    )?;
    if !a
        .positional
        .first()
        .is_some_and(|action| SESSION_ACTIONS.contains(&action.as_str()))
    {
        return Err("session requires recovery show or recovery record".into());
    }
    if a.positional.len() == 2 && a.positional[0] == "recovery" && a.positional[1] == "show" {
        let value = crate::host::session_recovery::show()?;
        if a.flags.contains_key("json") {
            return print_json(&crate::producer::observed(value));
        }
        println!(
            "Native Goal: {} revision {}",
            value["goal"]["goalId"], value["goal"]["revision"]
        );
        println!(
            "Current Work selection: {}",
            value["workSelection"]["state"]
        );
        println!(
            "Recovery hint SHA-256: {}",
            value["hintSha256"].as_str().unwrap_or("absent")
        );
        let saved = &value["activeSavedStep"];
        if let Some(step) = saved["step"].as_str() {
            println!(
                "Saved step (Goal revision {}): {step}",
                saved["recordedAtGoalRevision"]
            );
        }
        for decision in saved["leadDecisions"].as_array().into_iter().flatten() {
            println!(
                "{}",
                crate::host::session_recovery::decision_line(decision.as_str().unwrap_or(""))
            );
        }
        for line in value["pauseRecord"]["text"]
            .as_array()
            .into_iter()
            .flatten()
        {
            println!("{}", line.as_str().unwrap_or(""));
        }
        println!("Authority: none; inspect current Goal and Work detail before acting.");
        Ok(())
    } else if a.positional.len() == 2
        && a.positional[0] == "recovery"
        && a.positional[1] == "record"
    {
        let config_path = option(
            a,
            "config",
            "session recovery record requires --config OWNER_CONFIG",
        )?;
        let loaded = config::load(Some(config_path))?;
        let (step, decision) = (a.options.get("step"), a.options.get("decision"));
        if step.is_some() || decision.is_some() {
            let result = crate::host::session_recovery::record_pause(
                &loaded,
                step.map(String::as_str),
                decision.map(String::as_str),
            )?;
            return print_json(&result);
        }
        let mut input = Vec::new();
        std::io::stdin()
            .take(8 * 1024 + 1)
            .read_to_end(&mut input)
            .map_err(|error| format!("session recovery record input: {error}"))?;
        let result = crate::host::session_recovery::record(&loaded, &input)?;
        print_json(&result)
    } else {
        Err("session requires recovery show or recovery record".into())
    }
}

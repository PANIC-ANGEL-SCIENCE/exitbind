//! Read-only goal and run status surfaces, including optional themed display.

use super::*;

pub(super) fn goal_status(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options("goal status", a, &["config", "json", "themed"])?;
    args::assert_positionals("goal status", a, 1)?;
    let record = crate::session_goal::read(&l.state_root)?;
    let presentation = crate::session_goal::presentation_for_loaded(l, record.as_ref())?;
    let mut value = record.clone().unwrap_or_else(|| json!({"closed": false}));
    value["currentReadiness"] = presentation["currentReadiness"].clone();
    value["goalProgress"] = presentation["goalProgress"].clone();
    if record
        .as_ref()
        .is_some_and(crate::session_goal::requirements::named)
    {
        value["presentation"] = json!({"terminal": presentation["terminal"]});
        value["requirements"] = crate::session_goal::requirements::projection(
            l,
            record.as_ref().expect("named record"),
        )?;
        if !a.flags.contains_key("json") {
            if let Some(terminal) = presentation["terminal"].as_str() {
                println!("{terminal}");
                return Ok(());
            }
        }
    }
    print_json(&value)?;
    if !a.flags.contains_key("json") {
        if let Some(card) = crate::presentation_events::session_goal_direct_card_for_human(
            &l.state_root,
            &presentation,
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            !std::io::stdin().is_terminal(),
            a.flags.contains_key("themed"),
        ) {
            println!("{card}");
        }
    }
    Ok(())
}

pub(super) fn run_status(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_positionals("run status", a, 2)?;
    let ledger = positional(a, 1, "run status requires LEDGER")?;
    let json_output = a.flags.contains_key("json");
    if json_output {
        let mut value = run::status(l, ledger).map_err(|error| map_run_error(error, true))?;
        value["goalProgress"] = crate::session_goal::progress_for_loaded(l, &value["progress"])?;
        if let Some(terminal) = run::terminal_display(l, ledger) {
            value["terminal"] = json!(terminal);
        }
        print_json(&value)?;
        return Ok(());
    }
    let config = a.options.get("config").map(String::as_str).map_or_else(
        || {
            l.path
                .to_str()
                .ok_or("configuration path is not valid UTF-8")
        },
        Ok,
    )?;
    let value = run::human_status(l, ledger).map_err(|error| {
        if run::inspect(l, ledger).is_ok() {
            eprintln!(
                "Inspect: {}",
                crate::presentation::read_command("inspect", config, ledger)
            );
        }
        map_run_error(error, false)
    })?;
    let current_goal = crate::session_goal::read(&l.state_root)?;
    let session_goal = crate::session_goal::presentation_for_loaded(l, current_goal.as_ref())?;
    let card_emitted = crate::run_presentation::print_status(
        &value,
        &session_goal,
        &l.state_root,
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        !std::io::stdin().is_terminal(),
        a.flags.contains_key("themed"),
    );
    if !card_emitted {
        if value.status != "running" || value.artifact_status != "current" {
            println!(
                "Inspect: {}",
                crate::presentation::read_command("inspect", config, ledger)
            );
        } else {
            match run::next(l, ledger) {
                Ok(next) if next["status"] == "running" => println!(
                    "Next: {}",
                    crate::presentation::read_command("next", config, ledger)
                ),
                _ => {
                    println!("Guidance: no validated pending progression is available; inspect the run before recovery.");
                    println!(
                        "Inspect: {}",
                        crate::presentation::read_command("inspect", config, ledger)
                    );
                }
            }
        }
    }
    // The same block the work facade offers, last, so a reader who
    // drilled down to the run surface still has the product's own
    // wording instead of composing a sentence from the report above.
    if !card_emitted {
        if let Some(terminal) = run::terminal_display(l, ledger) {
            println!("{terminal}");
        }
    }
    Ok(())
}

pub(super) fn run_explain(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_positionals("run explain", a, 2)?;
    if a.flags.contains_key("json") {
        let machine = run::explain(
            l,
            positional(a, 1, "run explain requires LEDGER")?,
            a.options.get("event").map(String::as_str),
        )
        .map_err(|error| map_run_error(error, true))?;
        print_json(&machine)?;
    } else {
        let value = run::human_explain(
            l,
            positional(a, 1, "run explain requires LEDGER")?,
            a.options.get("event").map(String::as_str),
        )
        .map_err(|error| map_run_error(error, false))?;
        let current_goal = crate::session_goal::read(&l.state_root)?;
        let session_goal = crate::session_goal::presentation_for_loaded(l, current_goal.as_ref())?;
        crate::run_presentation::print_explain(
            &value,
            &session_goal,
            &l.state_root,
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            !std::io::stdin().is_terminal(),
            a.flags.contains_key("themed"),
        );
    }
    Ok(())
}

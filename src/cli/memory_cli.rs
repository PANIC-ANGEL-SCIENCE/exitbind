//! Existing memory CLI and task-selective inspection.
use super::*;
use crate::memory::{self, forgetting};

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "memory requires an action")?;
    match action {
        "correct-policy" => {
            args::assert_options(
                "memory correct-policy",
                a,
                &[
                    "config",
                    "from-config",
                    "reason",
                    "owner-decision",
                    "apply",
                    "json",
                ],
            )?;
            args::assert_positionals("memory correct-policy", a, 2)?;
            print_json(&memory::policy_correction::run(
                l,
                positional(a, 1, "memory correct-policy requires LEDGER")?,
                option(
                    a,
                    "from-config",
                    "memory correct-policy requires --from-config ORIGINAL_CONFIG",
                )?,
                option(
                    a,
                    "reason",
                    "memory correct-policy requires --reason OWNER_REASON",
                )?,
                a.options.get("owner-decision").map(String::as_str),
                a.flags.contains_key("apply"),
            )?)
        }
        "revalidate" => {
            args::assert_options(
                "memory revalidate",
                a,
                &["config", "from-config", "reason", "apply", "json"],
            )?;
            args::assert_positionals("memory revalidate", a, 3)?;
            print_json(&memory::revalidation::run(
                l,
                positional(a, 1, "memory revalidate requires AGENT LEDGER")?,
                positional(a, 2, "memory revalidate requires AGENT LEDGER")?,
                option(
                    a,
                    "from-config",
                    "memory revalidate requires --from-config ORIGINAL_CONFIG",
                )?,
                option(
                    a,
                    "reason",
                    "memory revalidate requires --reason REVIEWED_REASON",
                )?,
                a.flags.contains_key("apply"),
            )?)
        }
        "resolve" => {
            args::assert_options("memory resolve", a, &["config", "json", "task"])?;
            args::assert_positionals("memory resolve", a, 2)?;
            let agent = positional(a, 1, "memory resolve requires AGENT")?;
            let value = if let Some(task) = a.options.get("task") {
                json!({"valid":true,"agent":agent,"references": memory::selection::resolve_for_task(l, agent, Some(task))?,"selection":memory::selection::diagnostics(l, agent)?})
            } else {
                memory::resolve(l, agent)?
            };
            if a.flags.contains_key("json") {
                print_json(&value)?;
            } else if let Some(references) = value["references"].as_array() {
                for reference in references {
                    println!(
                        "{} {} {} {}",
                        reference["itemId"].as_str().unwrap_or(""),
                        reference["scope"].as_str().unwrap_or(""),
                        reference["sourcePath"].as_str().unwrap_or(""),
                        reference["sourceSha256"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(())
        }
        "attest-forgotten" => {
            args::assert_options("memory attest-forgotten", a, &["config", "receipt"])?;
            args::assert_positionals("memory attest-forgotten", a, 3)?;
            let actor = positional(a, 1, "memory attest-forgotten requires AGENT LEDGER")?;
            let ledger = positional(a, 2, "memory attest-forgotten requires AGENT LEDGER")?;
            let receipt = option(a, "receipt", "memory attest-forgotten requires --receipt")?;
            let value = forgetting::attest(l, actor, ledger, receipt)?;
            print_json(&value)?;
            Ok(())
        }
        "inspect" => {
            args::assert_options("memory inspect", a, &["config", "json"])?;
            args::assert_positionals("memory inspect", a, 2)?;
            let value = memory::inspect(l, positional(a, 1, "memory inspect accepts LEDGER")?)?;
            if a.flags.contains_key("json") {
                print_json(&value)?;
            } else if let Some(items) = value["items"].as_array() {
                for item in items {
                    println!("{} {} {}", item["itemId"], item["state"], item["scope"]);
                }
            }
            Ok(())
        }
        "propose" => memory_transition(l, a, true),
        _ => memory_transition(l, a, false),
    }
}

fn memory_transition(l: &config::Loaded, a: &Arguments, propose: bool) -> Result<(), String> {
    let allowed = if propose {
        &["config", "ledger", "scope", "expires-at"][..]
    } else {
        &["config"][..]
    };
    args::assert_options("memory", a, allowed)?;
    args::assert_positionals("memory", a, 3)?;
    let actor = positional(a, 1, "memory action requires AGENT LEDGER")?;
    let ledger = if propose {
        option(a, "ledger", "memory propose requires --ledger")?
    } else {
        positional(a, 2, "memory transition requires LEDGER")?
    };
    let value = memory::action(
        l,
        actor,
        positional(a, 0, "memory requires an action")?,
        if propose {
            a.positional.get(2).map(String::as_str)
        } else {
            None
        },
        a.options.get("scope").map(String::as_str),
        ledger,
        a.options.get("expires-at").map(String::as_str),
    )?;
    let mut response = value["event"].clone();
    if let Some(next) = value.get("nextAction") {
        response["nextAction"] = next.clone();
    }
    print_json(&response)?;
    Ok(())
}

//! Existing memory CLI and task-selective inspection.
use super::*;
use crate::memory::{self, forgetting};
use serde_json::Value;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(a, 0, "memory requires an action")?;
    match action {
        "form" => {
            args::assert_options("memory form", a, &["config", "scope", "json"])?;
            if !(1..=2).contains(&a.positional.len()) {
                return Err("memory form accepts one optional SOURCE positional".into());
            }
            let scope = option(a, "scope", "memory form requires --scope")?;
            if crate::project::specialists::is_scope(scope) {
                return print_json(&crate::project::specialists::input_form(
                    l,
                    scope,
                    a.positional.get(1).map(String::as_str),
                )?);
            }
            if scope == "contextual.v1" {
                args::assert_positionals("memory form", a, 1)?;
                return print_json(&memory::contextual::input_form(l)?);
            }
            if scope != memory::lessons::SCOPE {
                return Err(
                    "memory form requires --scope project-lessons.v1, specialist.AGENT.v1 or contextual.v1".into(),
                );
            }
            let mut form = memory::lessons::input_form(l)?;
            let config = l.path.to_str().ok_or("configuration path is not UTF-8")?;
            let route = crate::emitted::Route::new([
                "memory",
                "form",
                crate::emitted::slot("SOURCE").as_str(),
                "--scope",
                scope,
                "--json",
            ])
            .config_text(config);
            form["validationRoute"] = json!({"command":route.full_argv()?,
                "readOnly":true,"placeholders":["SOURCE"],"meaning":"exact typed source and current owner/project/config/profile binding only; no lifecycle or qualification decision"});
            form["validation"] = a
                .positional
                .get(1)
                .map(|source| memory::lessons::validate_source(l, source))
                .transpose()?
                .unwrap_or(Value::Null);
            print_json(&form)
        }
        "choice" => {
            args::assert_options(
                "memory choice",
                a,
                &["config", "task", "request-id", "json"],
            )?;
            args::assert_positionals("memory choice", a, 2)?;
            print_json(&memory::choices::read(
                l,
                &a.positional[1],
                option(a, "task", "memory choice requires --task")?,
                a.options.get("request-id").map(String::as_str),
            )?)
        }
        "choose" => {
            args::assert_options(
                "memory choose",
                a,
                &[
                    "config",
                    "task",
                    "host",
                    "reason",
                    "previous",
                    "apply",
                    "expected-sha256",
                    "json",
                ],
            )?;
            let agent = positional(
                a,
                1,
                "memory choose requires AGENT and optional eligible ITEM_IDs",
            )?;
            print_json(&memory::choices::choose(
                l,
                memory::choices::Request {
                    agent,
                    task: option(a, "task", "memory choose requires --task")?,
                    host: option(a, "host", "memory choose requires --host")?,
                    reason: option(a, "reason", "memory choose requires --reason")?,
                    items: &a.positional[2..],
                    previous: a.options.get("previous").map_or("absent", String::as_str),
                    apply: a.flags.contains_key("apply"),
                    expected: a.options.get("expected-sha256").map(String::as_str),
                },
            )?)
        }
        "candidates" => {
            args::assert_options("memory candidates", a, &["config", "task", "host", "json"])?;
            if !(2..=3).contains(&a.positional.len()) {
                return Err("memory candidates requires AGENT and optional ITEM_ID".into());
            }
            print_json(&memory::contextual::candidates(
                l,
                positional(a, 1, "memory candidates requires AGENT")?,
                option(a, "task", "memory candidates requires --task TASK")?,
                option(a, "host", "memory candidates requires --host codex|claude")?,
                a.positional.get(2).map(String::as_str),
            )?)
        }
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
            positional(a, 2, "memory revalidate requires AGENT LEDGER...")?;
            let ledgers = a.positional[2..]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            print_json(&memory::revalidation::run(
                l,
                positional(a, 1, "memory revalidate requires AGENT LEDGER...")?,
                &ledgers,
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

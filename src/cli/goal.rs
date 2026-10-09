//! Canonical goal commands and bounded observational routes.

use super::{args, config, option, positional, print_json, status, Arguments};
use serde_json::{json, Value};
use std::io::{IsTerminal, Read};

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(
        a,
        0,
        "goal requires incorporate, require, cover, assign, close, status, or usage",
    )?;
    match action {
        "require" => {
            args::assert_options(
                "goal require",
                a,
                &[
                    "config",
                    "goal-id",
                    "requirement",
                    "obligation",
                    "artifact",
                    "json",
                ],
            )?;
            args::assert_positionals("goal require", a, 1)?;
            print_json(&crate::session_goal::requirements::require(
                l,
                option(a, "goal-id", "goal require requires --goal-id ID")?,
                option(a, "requirement", "goal require requires --requirement ID")?,
                option(
                    a,
                    "obligation",
                    "goal require requires --obligation EXACT_TEXT",
                )?,
                option(
                    a,
                    "artifact",
                    "goal require requires --artifact APPROVED_SOURCE_FILE",
                )?,
            )?)
        }
        "cover" => {
            args::assert_options("goal cover", a, &["config", "goal-id", "json"])?;
            args::assert_positionals("goal cover", a, 1)?;
            print_json(&crate::session_goal::requirements::cover(
                l,
                option(a, "goal-id", "goal cover requires --goal-id ID")?,
            )?)
        }
        "assign" => {
            args::assert_options(
                "goal assign",
                a,
                &[
                    "config",
                    "goal-id",
                    "requirement",
                    "result-ref",
                    "disposition",
                    "json",
                ],
            )?;
            args::assert_positionals("goal assign", a, 1)?;
            let disposition = a
                .options
                .get("disposition")
                .map(String::as_str)
                .unwrap_or("add");
            if !matches!(disposition, "add" | "replace") {
                return Err("goal assign disposition must be add or replace".into());
            }
            print_json(&crate::session_goal::requirements::assign(
                l,
                option(a, "goal-id", "goal assign requires --goal-id ID")?,
                option(a, "requirement", "goal assign requires --requirement IDs")?,
                option(a, "result-ref", "goal assign requires --result-ref WORK")?,
                disposition == "replace",
            )?)
        }
        "incorporate" => {
            args::assert_options(
                "goal incorporate",
                a,
                &[
                    "config",
                    "goal-id",
                    "goal",
                    "obligation",
                    "finding",
                    "blocker",
                    "decision",
                    "external-action",
                    "scope",
                    "disposition",
                    "result-ref",
                    "consider",
                    "none-applicable",
                    "direct",
                    "external-scope",
                    "json",
                ],
            )?;
            args::assert_positionals("goal incorporate", a, 1)?;
            let goal_id = option(a, "goal-id", "goal incorporate requires --goal-id")?;
            let goal = option(a, "goal", "goal incorporate requires --goal")?;
            let direct = a.flags.contains_key("direct");
            let direct_items = [
                ("obligations", a.options.get("obligation")),
                ("findings", a.options.get("finding")),
                ("blockers", a.options.get("blocker")),
                ("decisions", a.options.get("decision")),
                ("externalActions", a.options.get("external-action")),
            ];
            let selected = direct_items
                .iter()
                .filter_map(|(category, value)| value.as_deref().map(|item| (*category, item)))
                .collect::<Vec<_>>();
            let value = if direct && selected.len() == 1 {
                let (category, item) = selected[0];
                crate::session_goal::direct_complete(
                    l,
                    goal_id,
                    goal,
                    category,
                    item,
                    a.options.get("external-scope").map(String::as_str),
                )?
            } else {
                crate::session_goal::incorporate(
                    l,
                    goal_id,
                    goal,
                    a.options.get("obligation").map(String::as_str),
                    a.options.get("finding").map(String::as_str),
                    a.options.get("blocker").map(String::as_str),
                    a.options.get("decision").map(String::as_str),
                    a.options.get("external-action").map(String::as_str),
                    a.options.get("scope").map(String::as_str),
                    a.options.get("disposition").map(String::as_str),
                    a.options.get("result-ref").map(String::as_str),
                    a.options.get("consider").map(String::as_str),
                    a.options.get("none-applicable").map(String::as_str),
                )?
            };
            print_json(&value)
        }
        "close" => {
            args::assert_options(
                "goal close",
                a,
                &["config", "goal-id", "result-ref", "direct", "json"],
            )?;
            args::assert_positionals("goal close", a, 1)?;
            let goal_id = option(a, "goal-id", "goal close requires --goal-id")?;
            let mut value = if a.flags.contains_key("direct") {
                crate::session_goal::close_direct(
                    l,
                    goal_id,
                    a.options.get("result-ref").map(String::as_str),
                )?
            } else {
                crate::session_goal::close(
                    l,
                    goal_id,
                    option(a, "result-ref", "goal close requires --result-ref")?,
                )?
            };
            if crate::session_goal::requirements::named(&value) {
                let rendered = crate::session_goal::presentation_for_loaded(l, Some(&value))?;
                value["presentation"] = json!({"terminal": rendered["terminal"]});
                if !a.flags.contains_key("json") {
                    if let Some(terminal) = rendered["terminal"].as_str() {
                        println!("{terminal}");
                        return Ok(());
                    }
                }
            }
            print_json(&value)
        }
        "usage" => {
            args::assert_options("goal usage", a, &["config", "goal-id", "apply", "json"])?;
            args::assert_positionals("goal usage", a, 1)?;
            let goal_id = option(a, "goal-id", "goal usage requires --goal-id ID")?;
            if goal_id.trim().is_empty() || goal_id.len() > 128 || goal_id.contains('\0') {
                return Err("goal usage requires a non-empty goal identity of at most 128 bytes without NUL".into());
            }
            let value = if a.flags.contains_key("apply") {
                let mut event = numeric_input()?;
                if event.get("goalId").is_some_and(|value| value != goal_id) {
                    return Err("usage observation goal identity does not match --goal-id".into());
                }
                event["goalId"] = json!(goal_id);
                crate::session_goal::usage::record_goal_numeric_event(l, &event)?
            } else {
                crate::session_goal::usage::details_for_goal(l, goal_id)
            };
            print_json(&value)
        }
        "status" => status::goal_status(l, a),
        _ => {
            Err("goal requires incorporate, require, cover, assign, close, status, or usage".into())
        }
    }
}

/// Numeric observations admit bounded allowlisted metadata in the persistence
/// owner. No prompt, output, transcript or provider call belongs to this route.
pub(super) fn numeric_input() -> Result<Value, String> {
    const MAX_BYTES: u64 = 8192;
    if std::io::stdin().is_terminal() {
        return Err("usage observation requires bounded numeric JSON on stdin".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "usage observation input could not be read")?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
        return Err("usage observation input must contain 1–8192 bytes".into());
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "usage observation input must be valid JSON")?;
    if !value.is_object() {
        return Err("usage observation input must be an object".into());
    }
    Ok(value)
}

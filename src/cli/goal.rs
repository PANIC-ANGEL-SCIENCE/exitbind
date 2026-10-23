//! Canonical goal commands and bounded observational routes.

use super::{args, config, option, positional, print_json, status, Arguments};
use serde_json::{json, Value};
use std::io::{IsTerminal, Read};

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    let action = positional(
        a,
        0,
        "goal requires prepare, incorporate, require, cover, assign, close, recover, adopt, status, or usage",
    )?;
    match action {
        "quarantine" => {
            args::assert_options(
                "goal quarantine", a,
                &["config", "json", "source-config", "verification-work", "request-id", "reason", "apply", "current-binding"],
            )?;
            args::assert_positionals("goal quarantine", a, 2)?;
            let response = crate::session_goal::quarantine::record(l,
                crate::session_goal::quarantine::Request {
                    source_config: option(a, "source-config", "goal quarantine requires --source-config PATH")?,
                    old_work: positional(a, 1, "goal quarantine requires OLD_WORK")?,
                    verification_work: option(a, "verification-work", "goal quarantine requires --verification-work WORK")?,
                    request_id: option(a, "request-id", "goal quarantine requires --request-id ID")?,
                    reason: option(a, "reason", "goal quarantine requires --reason TEXT")?,
                    apply: a.flags.contains_key("apply"),
                    current_binding: a.options.get("current-binding").map(String::as_str),
                })?;
            print_json(&response)
        }
        "recover" => {
            args::assert_options(
                "goal recover",
                a,
                &[
                    "config",
                    "project",
                    "source-config",
                    "historical-fixtures",
                    "endpoint-configs",
                    "request-id",
                    "reason",
                    "current-binding",
                    "apply",
                    "json",
                ],
            )?;
            args::assert_positionals("goal recover", a, 1)?;
            if a.flags.contains_key("project") {
                return Err("goal recover is available only in the original native Goal owner context".into());
            }
            let scope = journal_scope(l, a, false)?;
            scope.require_owner()?;
            crate::session_goal::recovery::recover(
                &scope.root,
                l,
                crate::session_goal::recovery::Request {
                    source_config: option(a, "source-config", "goal recover requires --source-config PATH")?,
                    historical_fixtures: a.options.get("historical-fixtures").map(String::as_str).unwrap_or(""),
                    endpoint_configs: a.options.get("endpoint-configs").map(String::as_str),
                    request_id: option(a, "request-id", "goal recover requires --request-id ID")?,
                    reason: option(a, "reason", "goal recover requires --reason TEXT")?,
                    apply: a.flags.contains_key("apply"),
                    current_binding: a.options.get("current-binding").map(String::as_str),
                },
            )
            .and_then(|value| print_json(&value))
        }
        "prepare" => {
            args::assert_options(
                "goal prepare",
                a,
                &[
                    "config",
                    "project",
                    "goal-id",
                    "requirement",
                    "artifact",
                    "check-command",
                    "scope",
                    "basis",
                    "json",
                ],
            )?;
            args::assert_positionals("goal prepare", a, 1)?;
            let scope = journal_scope(l, a, false)?;
            scope.require_owner_or_project()?;
            let goal_id = option(a, "goal-id", "goal prepare requires --goal-id ID")?;
            let ids = option(a, "requirement", "goal prepare requires --requirement IDs")?;
            let command = option(
                a,
                "check-command",
                "goal prepare requires --check-command COMMAND",
            )?;
            let derived = crate::session_goal::requirements::resolve_checker(
                l,
                a.options.get("artifact").map(String::as_str),
                command,
            )?;
            let checker = derived.path.as_str();
            let integration = match a.options.get("scope").map(String::as_str) {
                None => false,
                Some("integration") => true,
                Some(_) => return Err("named Work scope must be integration when supplied".into()),
            };
            let prepared = crate::session_goal::requirements::prepare(
                crate::session_goal::requirements::PrepareRequest {
                    root: &scope.root,
                    loaded: l,
                    goal_id,
                    ids,
                    checker,
                    command,
                    basis: a.options.get("basis").map(String::as_str),
                    integration,
                    scope: if scope.is_session() {
                        crate::session_goal::requirements::ContractScope::Session
                    } else {
                        crate::session_goal::requirements::ContractScope::Project
                    },
                },
            )?;
            let record = crate::session_goal::read(&scope.root)?
                .ok_or("goal prepare requires the current canonical Goal")?;
            // An integration over the conversation journey freezes its
            // accepted members when that freeze would succeed now; otherwise
            // the route omits the flag and the response says why.
            let journey_omitted = if !integration {
                None
            } else if a.flags.contains_key("project") {
                Some("the project Goal is selected; journey integration freezes the conversation Goal's members".to_owned())
            } else {
                crate::session_goal::journey::prepare(l, Some(&prepared.basis), checker, command)
                    .err()
                    .map(|error| crate::drift::plain(&error))
            };
            let mut begin = vec![
                "work".into(),
                "begin".into(),
                "change".into(),
                "--goal".into(),
                record["goal"].as_str().unwrap_or_default().into(),
                "--goal-id".into(),
                goal_id.into(),
                "--requirement".into(),
                ids.into(),
                "--artifact".into(),
                checker.into(),
                "--check-command".into(),
                command.into(),
                "--review-policy".into(),
                crate::emitted::slot("REVIEW_DECISION"),
                "--detail".into(),
            ];
            if integration {
                begin.extend(["--scope".into(), "integration".into()]);
                if journey_omitted.is_none() {
                    begin.push("--journey-integration".into());
                }
            }
            if let Some(basis) = a.options.get("basis") {
                begin.extend(["--basis".into(), basis.clone()]);
            }
            if a.flags.contains_key("project") {
                begin.push("--project-goal".into());
            }
            let route = crate::work::compact::continuation_route(&l.path, begin);
            let scope_label = match prepared.scope {
                crate::session_goal::requirements::ContractScope::Project => "project",
                crate::session_goal::requirements::ContractScope::Session => "session",
            };
            print_json(&json!({"kind":"named_coverage_preparation","readOnly":true,
                "goalId":prepared.goal_id,"selectedRequirementIds":prepared.requirements.split(',').collect::<Vec<_>>(),
                "contractScope":scope_label,"projectIdentitySha256":prepared.project_identity,
                "checker":{"path":checker,"sha256":crate::session_goal::requirements::checker_input(l, checker, command)?.sha256,
                    "derived":!a.options.contains_key("artifact"),"via":derived.via},
                "integration":integration,"beginRoute":route,"journeyIntegrationOmitted":journey_omitted,
                "placeholders":{"REVIEW_DECISION":"the owner's required or omitted review choice"},
                "effect":"no Work is started or frozen until beginRoute is executed"}))
        }
        "require" => {
            args::assert_options(
                "goal require",
                a,
                &[
                    "config",
                    "project",
                    "goal-id",
                    "requirement",
                    "obligation",
                    "artifact",
                    "json",
                ],
            )?;
            args::assert_positionals("goal require", a, 1)?;
            let scope = journal_scope(l, a, false)?;
            scope.require_owner_or_project()?;
            print_json(&crate::session_goal::requirements::require(
                &scope.root,
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
            args::assert_options("goal cover", a, &["config", "project", "goal-id", "json"])?;
            args::assert_positionals("goal cover", a, 1)?;
            let scope = journal_scope(l, a, false)?;
            scope.require_owner_or_project()?;
            print_json(&crate::session_goal::requirements::cover(
                &scope.root,
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
                    "project",
                    "goal-id",
                    "requirement",
                    "result-ref",
                    "disposition",
                    "json",
                ],
            )?;
            args::assert_positionals("goal assign", a, 1)?;
            let scope = journal_scope(l, a, false)?;
            scope.require_owner_or_project()?;
            let disposition = a
                .options
                .get("disposition")
                .map(String::as_str)
                .unwrap_or("add");
            if !matches!(disposition, "add" | "replace") {
                return Err("goal assign disposition must be add or replace".into());
            }
            print_json(&crate::session_goal::requirements::assign(
                &scope.root,
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
                    "project",
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
            let scope = journal_scope(l, a, true)?;
            scope.require_owner_or_project()?;
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
                    &scope.root,
                    l,
                    crate::session_goal::DirectCompletionRequest {
                        goal_id,
                        goal,
                        category,
                        item,
                        external_scope: a.options.get("external-scope").map(String::as_str),
                        native_session: scope.is_session(),
                    },
                )?
            } else {
                crate::session_goal::incorporate(
                    &scope.root,
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
                    scope.is_session(),
                )?
            };
            print_json(&value)
        }
        "close" => {
            args::assert_options(
                "goal close",
                a,
                &[
                    "config",
                    "project",
                    "goal-id",
                    "result-ref",
                    "direct",
                    "json",
                    "view",
                ],
            )?;
            args::assert_positionals("goal close", a, 1)?;
            if a.options.get("view").is_some_and(|view| view != "event") {
                return Err("goal close --view accepts only event".into());
            }
            let scope = journal_scope(l, a, false)?;
            scope.require_owner_or_project()?;
            let goal_id = option(a, "goal-id", "goal close requires --goal-id")?;
            let previous = crate::session_goal::read(&scope.root)?;
            let mut value = if a.flags.contains_key("direct") {
                crate::session_goal::close_direct(
                    &scope.root,
                    l,
                    goal_id,
                    a.options.get("result-ref").map(String::as_str),
                )?
            } else {
                crate::session_goal::close(
                    &scope.root,
                    l,
                    goal_id,
                    option(a, "result-ref", "goal close requires --result-ref")?,
                )?
            };
            if a.options.get("view").is_some() {
                let newly_closed = previous.as_ref().map(|record| &record["eventSha256"])
                    != Some(&value["eventSha256"]);
                return print_json(&super::goal_completion::response(
                    l,
                    Some(&value),
                    newly_closed,
                    a.flags.contains_key("project"),
                )?);
            }
            let named = crate::session_goal::requirements::named(&value);
            let native_governed = scope.is_session() && value["closure"]["kind"] != "direct";
            if named || native_governed {
                let rendered = crate::session_goal::presentation_for_selected(
                    l,
                    Some(&value),
                    &scope.root,
                    a.flags.contains_key("project"),
                )?;
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
            args::assert_options(
                "goal usage",
                a,
                &["config", "project", "goal-id", "apply", "json"],
            )?;
            args::assert_positionals("goal usage", a, 1)?;
            let scope = journal_scope(l, a, false)?;
            scope.require_owner_or_project()?;
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
                crate::session_goal::usage::record_goal_numeric_event(&scope.root, l, &event)?
            } else {
                crate::session_goal::usage::details_for_goal(&scope.root, l, goal_id)
            };
            print_json(&value)
        }
        "adopt" => {
            args::assert_options("goal adopt", a, &["config", "session-key", "json"])?;
            args::assert_positionals("goal adopt", a, 1)?;
            print_json(&crate::session_goal::adoption::adopt(
                l,
                option(a, "session-key", "goal adopt requires --session-key KEY")?,
            )?)
        }
        "status" => status::goal_status(l, a),
        _ => Err(
            "goal requires prepare, incorporate, require, cover, assign, close, recover, adopt, status, or usage"
                .into(),
        ),
    }
}

pub(super) fn journal_scope(
    loaded: &config::Loaded,
    args: &Arguments,
    establish_owner: bool,
) -> Result<crate::session_goal::native::Scope, String> {
    if args.flags.contains_key("project") {
        Ok(crate::session_goal::native::project_scope(loaded))
    } else {
        crate::session_goal::native::scope(loaded, establish_owner)
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

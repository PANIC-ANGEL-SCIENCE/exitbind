//! Build the exact read-only checks and Work activation actions emitted by setup.

use super::SetupFacts;
use crate::emitted::Route;
use serde_json::{json, Value};
use std::{fs, path::Path};

pub(super) fn next_actions(
    root: &str,
    config: &Path,
    hosts: &[String],
    facts: &SetupFacts,
) -> Result<Value, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let cwd = fs::canonicalize(cwd).map_err(|error| error.to_string())?;
    let cwd = cwd.to_str().ok_or("working directory is not valid UTF-8")?;
    let config = config
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let mut commands = Vec::new();
    let mut task_card = None;
    let card = super::setup_card::plan(facts)?;
    if !facts.apply {
        commands.push(printed(Route::new(["check"]), config)?);
    }
    if let (Some(check), Some(review_policy), Some(goal)) =
        (&facts.check_command, &facts.review_policy, &facts.goal)
    {
        let mut work = Route::new(["work", "begin", "change", "--goal", goal.as_str()]);
        let mut project = false;
        if let Some(card) = &card {
            let scope = super::setup_card::card_scope(facts, Path::new(config))?;
            let (named, card_view) = card_start(card, check, config, &scope)?;
            work = work.words(named.begin);
            project = named.project;
            commands.extend(named.goal_commands);
            task_card = Some(card_view);
        }
        work = work.words(["--check-command", check.as_str()]);
        if let Some(command) = &facts.pre_return_command {
            work = work.words(["--pre-return-command", command.as_str()]);
            if let Some(timeout_ms) = &facts.pre_return_timeout_ms {
                work = work.words(["--pre-return-timeout-ms", timeout_ms.as_str()]);
            }
            if let Some(capture_bytes) = &facts.pre_return_capture_bytes {
                work = work.words(["--pre-return-capture-bytes", capture_bytes.as_str()]);
            }
        }
        work = work.words(["--review-policy", review_policy.as_str(), "--detail"]);
        if project {
            work = work.words(["--project-goal"]);
        }
        commands.push(printed(work, config)?);
    }
    let (actions, argv): (Vec<String>, Vec<Vec<String>>) = commands.into_iter().unzip();
    let mut next = json!({
        "root": root,
        "workingDirectory": cwd,
        "argv": argv,
        "commands": actions,
        "hosts": hosts,
        "sessionReload": "not performed",
    });
    if let Some(card) = task_card {
        next["taskCard"] = card;
    }
    Ok(next)
}

/// One next action as the shell line and the argv, with this configuration
/// and the canonical executable.
fn printed(route: Route, config: &str) -> Result<(String, Vec<String>), String> {
    let route = route.config_text(config).canonical()?;
    Ok((route.quoted_line()?, route.full_argv()?))
}

struct NamedStart {
    /// `goal cover` and `goal prepare`, printed.
    goal_commands: Vec<(String, Vec<String>)>,
    /// Named options inserted into the printed `work begin`.
    begin: Vec<String>,
    project: bool,
}

/// The card's start path: cover the registered rows, prepare the first Work's
/// rows with the checker the check command names, then begin that Work.
fn card_start(
    card: &super::setup_card::Card,
    check: &str,
    config: &str,
    scope: &super::setup_card::CardScope,
) -> Result<(NamedStart, Value), String> {
    let project = scope.project;
    let checker = if Path::new(config).is_file() {
        crate::config::load(Some(config)).and_then(|loaded| {
            crate::session_goal::requirements::resolve_checker(&loaded, None, check)
        })
    } else {
        Err("the configuration does not exist yet; goal prepare derives the checker".into())
    };
    let ids = card.ids();
    let mut named = vec![
        "--goal-id".to_owned(),
        card.goal_id.clone(),
        "--requirement".into(),
        ids,
    ];
    if let Ok(derived) = &checker {
        named.extend(["--artifact".into(), derived.path.clone()]);
    }
    let mut cover = Route::new(["goal", "cover", "--goal-id", card.goal_id.as_str()]);
    let mut prepare = Route::new(["goal", "prepare"])
        .words(named.iter().cloned())
        .words(["--check-command", check]);
    if project {
        cover = cover.words(["--project"]);
        prepare = prepare.words(["--project"]);
    }
    let mut view = card.view();
    view["scope"] = json!(if project { "project" } else { "session" });
    if let Some(reason) = scope.reason {
        view["scopeReason"] = json!(reason);
    }
    view["checker"] = match checker {
        Ok(derived) => json!({"path": derived.path, "via": derived.via}),
        Err(error) => json!({"path": null, "reason": error}),
    };
    Ok((
        NamedStart {
            goal_commands: vec![printed(cover, config)?, printed(prepare, config)?],
            begin: named,
            project,
        },
        view,
    ))
}

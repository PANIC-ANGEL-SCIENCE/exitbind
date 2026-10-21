//! Build the exact read-only checks and Work activation actions emitted by setup.

use super::SetupFacts;
use serde_json::{json, Value};
use std::{fs, path::Path};

pub(super) fn next_actions(
    root: &str,
    config: &Path,
    hosts: &[String],
    facts: &SetupFacts,
) -> Result<Value, String> {
    let executable = crate::emitted::canonical_executable()?;
    let executable = executable.as_str();
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let cwd = fs::canonicalize(cwd).map_err(|error| error.to_string())?;
    let cwd = cwd.to_str().ok_or("working directory is not valid UTF-8")?;
    let config = config
        .to_str()
        .ok_or("configuration path is not valid UTF-8")?;
    let check_argv = vec![
        executable.to_owned(),
        "check".into(),
        "--config".into(),
        config.into(),
    ];
    let mut actions = Vec::new();
    let mut argv = Vec::new();
    let mut task_card = None;
    let card = super::setup_card::plan(facts)?;
    if !facts.apply {
        actions.push(shell_command(&check_argv));
        argv.push(check_argv);
    }
    if let (Some(check), Some(review_policy), Some(goal)) =
        (&facts.check_command, &facts.review_policy, &facts.goal)
    {
        let mut work_argv = vec![
            executable.to_owned(),
            "work".into(),
            "begin".into(),
            "change".into(),
            "--goal".into(),
            goal.clone(),
            "--check-command".into(),
            check.clone(),
        ];
        if let Some(command) = &facts.pre_return_command {
            work_argv.extend(["--pre-return-command".into(), command.clone()]);
            if let Some(timeout_ms) = &facts.pre_return_timeout_ms {
                work_argv.extend(["--pre-return-timeout-ms".into(), timeout_ms.clone()]);
            }
            if let Some(capture_bytes) = &facts.pre_return_capture_bytes {
                work_argv.extend(["--pre-return-capture-bytes".into(), capture_bytes.clone()]);
            }
        }
        work_argv.extend([
            "--review-policy".into(),
            review_policy.clone(),
            "--detail".into(),
        ]);
        let mut start = Vec::new();
        if let Some(card) = &card {
            let scope = super::setup_card::card_scope(facts, Path::new(config))?;
            let (named, card_view) = card_start(card, check, config, &scope)?;
            work_argv.splice(6..6, named.begin);
            if named.project {
                work_argv.push("--project-goal".into());
            }
            start = named.goal_commands;
            task_card = Some(card_view);
        }
        work_argv.extend(["--config".into(), config.into()]);
        for mut command in start {
            command.insert(0, executable.to_owned());
            command.extend(["--config".into(), config.into()]);
            actions.push(shell_command(&command));
            argv.push(command);
        }
        actions.push(shell_command(&work_argv));
        argv.push(work_argv);
    }
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

struct NamedStart {
    /// `goal cover` and `goal prepare`, without executable and config.
    goal_commands: Vec<Vec<String>>,
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
    let mut cover = vec![
        "goal".to_owned(),
        "cover".into(),
        "--goal-id".into(),
        card.goal_id.clone(),
    ];
    let mut prepare = vec!["goal".to_owned(), "prepare".into()];
    prepare.extend(named.iter().cloned());
    prepare.extend(["--check-command".into(), check.into()]);
    if project {
        cover.push("--project".into());
        prepare.push("--project".into());
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
            goal_commands: vec![cover, prepare],
            begin: named,
            project,
        },
        view,
    ))
}

fn shell_command(argv: &[String]) -> String {
    argv.iter()
        .map(|item| crate::presentation::shell_quote(item))
        .collect::<Vec<_>>()
        .join(" ")
}

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
    let executable = std::env::current_exe()
        .map_err(|error| format!("invoking executable cannot be resolved: {error}"))?;
    let executable = fs::canonicalize(executable)
        .map_err(|error| format!("invoking executable cannot be canonicalized: {error}"))?;
    let executable = executable
        .to_str()
        .ok_or("invoking executable path is not valid UTF-8")?;
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
            "--config".into(),
            config.into(),
        ]);
        actions.push(shell_command(&work_argv));
        argv.push(work_argv);
    }
    Ok(json!({
        "root": root,
        "workingDirectory": cwd,
        "argv": argv,
        "commands": actions,
        "hosts": hosts,
        "sessionReload": "not performed",
    }))
}

fn shell_command(argv: &[String]) -> String {
    argv.iter()
        .map(|item| crate::presentation::shell_quote(item))
        .collect::<Vec<_>>()
        .join(" ")
}

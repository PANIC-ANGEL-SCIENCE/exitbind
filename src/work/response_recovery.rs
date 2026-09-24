//! Exact executable/config recovery argv for bounded work responses.

use serde_json::{json, Value};
use std::path::Path;

pub(super) struct RecoveryCommand {
    pub(super) argv: Value,
    pub(super) same_config: bool,
    pub(super) same_executable: bool,
}

pub(super) fn recovery_command(
    response: &Value,
    config_path: &Path,
    invoked: &str,
    argv_budget: usize,
) -> RecoveryCommand {
    let mut suffix = vec!["work".to_owned(), invoked.to_owned()];
    if invoked == "next" {
        let Some(work) = response.get("work").and_then(Value::as_str) else {
            return unavailable();
        };
        if work.len() > 128 {
            return unavailable();
        }
        suffix.push(work.to_owned());
    }
    suffix.extend(["--full".to_owned(), "--config".to_owned()]);
    bounded_argv(suffix, config_path.to_str(), argv_budget)
}

pub(super) fn bounded_argv(
    suffix: Vec<String>,
    config: Option<&str>,
    argv_budget: usize,
) -> RecoveryCommand {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned));
    let fits = |argv: &[String]| {
        serde_json::to_vec(argv)
            .expect("recovery argv is serializable")
            .len()
            <= argv_budget
    };
    if let Some(executable) = executable.as_ref() {
        let mut argv = vec![executable.clone()];
        argv.extend(suffix.iter().cloned());
        if let Some(config) = config {
            let mut complete = argv.clone();
            complete.push(config.to_owned());
            if fits(&complete) {
                return RecoveryCommand {
                    argv: json!(complete),
                    same_config: false,
                    same_executable: false,
                };
            }
        }
        if fits(&argv) {
            return RecoveryCommand {
                argv: json!(argv),
                same_config: true,
                same_executable: false,
            };
        }
    }
    if let Some(config) = config {
        let mut with_config = suffix.clone();
        with_config.push(config.to_owned());
        if fits(&with_config) {
            return RecoveryCommand {
                argv: json!(with_config),
                same_config: false,
                same_executable: true,
            };
        }
    }
    RecoveryCommand {
        argv: json!(suffix),
        same_config: true,
        same_executable: true,
    }
}

fn unavailable() -> RecoveryCommand {
    RecoveryCommand {
        argv: Value::Null,
        same_config: true,
        same_executable: true,
    }
}

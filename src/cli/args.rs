use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct Arguments {
    pub positional: Vec<String>,
    pub options: BTreeMap<String, String>,
    pub flags: BTreeMap<String, bool>,
}

const VALUE_OPTIONS: &[&str] = &[
    "artifact",
    "artifact-root",
    "basis",
    "assessment",
    "boundary",
    "check-command",
    "config",
    "confidence",
    "consider",
    "control-root",
    "duration-ms",
    "events",
    "event",
    "external-scope",
    "expires-at",
    "exit-code",
    "forbid-term",
    "goal",
    "harness-receipt",
    "harness-manifest",
    "hosts",
    "hypothesis",
    "identity-source",
    "input-digest",
    "evidence-request",
    "scope-decision",
    "blocker",
    "ledger",
    "mode",
    "material-consequence",
    "name",
    "none-applicable",
    "outcome",
    "operation",
    "output",
    "packet",
    "preservation-check-command",
    "preservation-proof-origin",
    "proposal",
    "preserve-requirement",
    "proof-origin",
    "promotion-required",
    "previous",
    "purpose",
    "project-id",
    "request-id",
    "reason",
    "receipt",
    "requirement",
    "root",
    "scope",
    "goal-id",
    "obligation",
    "finding",
    "decision",
    "external-action",
    "result-ref",
    "review-policy",
    "disposition",
    "sandbox-mode",
    "state-root",
    "state",
    "task",
    "target",
    "timeout-ms",
    "workflow",
];
const BOOLEAN_OPTIONS: &[&str] = &[
    "all",
    "apply",
    "json",
    "event-id",
    "text",
    "help",
    "version",
    "with-coffee",
    "skip-skills",
    "direct",
    "refresh-skills",
    "require-harness-receipt",
    "session-closed",
];

pub fn parse(values: &[String]) -> Result<Arguments, String> {
    let mut arguments = Arguments::default();
    let mut index = 0;
    let mut end_of_options = false;

    while index < values.len() {
        let value = &values[index];
        if end_of_options || !value.starts_with("--") {
            arguments.positional.push(value.clone());
            index += 1;
            continue;
        }
        if value == "--" {
            end_of_options = true;
            index += 1;
            continue;
        }

        let raw = &value[2..];
        let (key, inline) = raw
            .split_once('=')
            .map_or((raw, None), |(key, value)| (key, Some(value)));
        if key.is_empty() {
            return Err("option name cannot be empty".into());
        }
        if BOOLEAN_OPTIONS.contains(&key) {
            if inline.is_some() {
                return Err(format!("--{key} does not accept a value"));
            }
            arguments.flags.insert(key.to_owned(), true);
            index += 1;
            continue;
        }
        if !VALUE_OPTIONS.contains(&key) {
            return Err(format!("unknown option '--{key}'"));
        }

        let option_value = match inline {
            Some(value) => value.to_owned(),
            None => {
                let Some(next) = values.get(index + 1) else {
                    return Err(format!("--{key} requires a value"));
                };
                if next.starts_with("--") {
                    return Err(format!("--{key} requires a value"));
                }
                index += 1;
                next.clone()
            }
        };
        arguments.options.insert(key.to_owned(), option_value);
        index += 1;
    }
    Ok(arguments)
}

pub fn assert_options(
    command: &str,
    arguments: &Arguments,
    allowed: &[&str],
) -> Result<(), String> {
    for key in arguments.options.keys().chain(arguments.flags.keys()) {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("option '--{key}' is not supported by '{command}'"));
        }
    }
    Ok(())
}

pub fn assert_positionals(
    command: &str,
    arguments: &Arguments,
    expected: usize,
) -> Result<(), String> {
    if arguments.positional.len() > expected {
        let suffix = if expected == 1 { "" } else { "s" };
        return Err(format!(
            "{command} accepts {expected} positional argument{suffix}"
        ));
    }
    Ok(())
}

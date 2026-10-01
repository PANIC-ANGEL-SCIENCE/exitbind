//! Bounded projection of a native JSONL turn.

use super::*;

pub(super) fn parse_stream(
    bytes: &[u8],
    want_final: bool,
    process: ProcessOutcome,
    stderr: &[u8],
) -> Result<Observation, RunError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RunError::MalformedStream { line: 1 })?;
    let mut thread_id = None;
    let mut turn = None;
    let mut command_outcomes = Vec::new();
    let mut started_commands = 0usize;
    let mut unobserved_item_count = 0u64;
    let mut usage = None;
    let mut final_text = None;
    let mut diagnostic_codes = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(line)
            .map_err(|_| RunError::MalformedStream { line: line_number })?;
        let object = event
            .as_object()
            .ok_or(RunError::MalformedStream { line: line_number })?;
        match object.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                let id = object
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .ok_or(RunError::MalformedStream { line: line_number })?;
                validate_id(Some(id), "thread ID")
                    .map_err(|_| RunError::MalformedStream { line: line_number })?;
                if thread_id.as_deref().is_some_and(|current| current != id) {
                    return Err(RunError::MalformedStream { line: line_number });
                }
                thread_id = Some(id.to_owned());
            }
            Some("item.completed") => {
                let item = object
                    .get("item")
                    .and_then(Value::as_object)
                    .ok_or(RunError::MalformedStream { line: line_number })?;
                match item.get("type").and_then(Value::as_str) {
                    Some("command_execution") => {
                        let status = item
                            .get("status")
                            .and_then(Value::as_str)
                            .filter(|status| {
                                !status.is_empty()
                                    && status.len() <= MAX_STATUS_BYTES
                                    && !status.chars().any(char::is_control)
                            })
                            .ok_or(RunError::MalformedStream { line: line_number })?;
                        let exit_code = match item.get("exit_code") {
                            None | Some(Value::Null) => None,
                            Some(value) => value
                                .as_i64()
                                .and_then(|code| i32::try_from(code).ok())
                                .ok_or(RunError::MalformedStream { line: line_number })
                                .map(Some)?,
                        };
                        let output_bytes = item
                            .get("aggregated_output")
                            .or_else(|| item.get("output"))
                            .and_then(Value::as_str)
                            .map(str::len)
                            .unwrap_or(0);
                        let host_item_id = match item.get("id") {
                            None | Some(Value::Null) => None,
                            Some(value) => {
                                let id = value
                                    .as_str()
                                    .ok_or(RunError::MalformedStream { line: line_number })?;
                                validate_id(Some(id), "command item ID")
                                    .map_err(|_| RunError::MalformedStream { line: line_number })?;
                                Some(id.to_owned())
                            }
                        };
                        let invocation_sha256 = host_item_id.as_deref().map(|host_item_id| {
                            crate::evidence::hash::value(&json!({
                                "threadId": thread_id.as_deref(),
                                "hostItemId": host_item_id,
                            }))
                        });
                        command_outcomes.push(CommandOutcome {
                            status: status.to_owned(),
                            exit_code,
                            output_bytes,
                            invocation_sha256,
                            host_item_id,
                        });
                    }
                    Some("agent_message") if want_final => {
                        let text = item
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or(RunError::MalformedStream { line: line_number })?;
                        if text.len() > MAX_FINAL_BYTES {
                            return Err(RunError::FinalResultTooLarge);
                        }
                        final_text = Some(text.to_owned());
                    }
                    Some("agent_message") => {}
                    _ => unobserved_item_count = unobserved_item_count.saturating_add(1),
                }
            }
            Some("item.started") => {
                let item = object
                    .get("item")
                    .and_then(Value::as_object)
                    .ok_or(RunError::MalformedStream { line: line_number })?;
                match item.get("type").and_then(Value::as_str) {
                    Some("command_execution") => {
                        started_commands = started_commands.saturating_add(1);
                    }
                    Some("agent_message") => {}
                    _ => unobserved_item_count = unobserved_item_count.saturating_add(1),
                }
            }
            Some(kind) if kind.starts_with("item.") => {
                unobserved_item_count = unobserved_item_count.saturating_add(1);
            }
            Some("turn.completed") => {
                let status = turn_status(object.get("status"), line_number)?;
                if status == TurnStatus::Failed {
                    push_diagnostic(&mut diagnostic_codes, DiagnosticCode::TurnFailed);
                }
                turn = Some(status);
                if let Some(raw_usage) = object.get("usage") {
                    usage = Some(parse_usage(raw_usage, line_number)?);
                }
            }
            Some("turn.failed") => {
                push_diagnostic(&mut diagnostic_codes, DiagnosticCode::TurnFailed);
                turn = Some(TurnStatus::Failed);
            }
            Some("error") => {
                push_diagnostic(&mut diagnostic_codes, DiagnosticCode::ProviderError);
                turn = Some(TurnStatus::Failed);
            }
            _ => {}
        }
    }

    let final_result = match final_text {
        Some(text) => {
            Some(serde_json::from_str(&text).map_err(|_| RunError::MalformedFinalResult)?)
        }
        None => None,
    };
    let mut coverage_gap = Vec::new();
    if thread_id.is_none() {
        coverage_gap.push(CoverageGap::MissingThreadId);
    }
    if turn.is_none() {
        coverage_gap.push(CoverageGap::MissingTurnCompletion);
    }
    if usage.as_ref().map_or(true, |value| {
        value.input_tokens.is_none()
            || value.cached_input_tokens.is_none()
            || value.output_tokens.is_none()
    }) {
        coverage_gap.push(CoverageGap::MissingUsage);
    }
    if started_commands > command_outcomes.len() {
        coverage_gap.push(CoverageGap::MissingCommandOutcome);
    }
    if !command_outcomes.is_empty() {
        if command_outcomes
            .iter()
            .any(|outcome| outcome.exit_code.is_none())
        {
            coverage_gap.push(CoverageGap::MissingCommandExitCode);
        }
        if command_outcomes
            .iter()
            .any(|outcome| outcome.invocation_sha256.is_none())
        {
            coverage_gap.push(CoverageGap::MissingCommandIdentity);
        }
    }
    if want_final && final_result.is_none() {
        coverage_gap.push(CoverageGap::MissingFinalResult);
    }
    if bytes.is_empty() {
        push_diagnostic(&mut diagnostic_codes, DiagnosticCode::NoStdout);
    }
    if !stderr.is_empty() {
        push_diagnostic(&mut diagnostic_codes, DiagnosticCode::StderrNonempty);
        if stderr
            .windows(GIT_PRECONDITION.len())
            .any(|window| window == GIT_PRECONDITION)
        {
            push_diagnostic(&mut diagnostic_codes, DiagnosticCode::GitPrecondition);
        }
    }
    Ok(Observation {
        ephemeral: false,
        process: process.clone(),
        turn: turn.unwrap_or_else(|| {
            if process.timed_out {
                TurnStatus::Interrupted
            } else {
                TurnStatus::Unknown("missing".into())
            }
        }),
        command_outcomes,
        unobserved_item_count,
        usage,
        thread_id,
        final_result,
        diagnostic: DiagnosticEnvelope {
            status: DiagnosticStatus::Observed,
            codes: diagnostic_codes,
            stdout_bytes: bytes.len(),
            stderr_bytes: stderr.len(),
        },
        coverage_gap,
        interrupted: process.timed_out || process.signal.is_some(),
    })
}

const GIT_PRECONDITION: &[u8] =
    b"Not inside a trusted directory and --skip-git-repo-check was not specified.";

fn push_diagnostic(codes: &mut Vec<DiagnosticCode>, code: DiagnosticCode) {
    if !codes.contains(&code) {
        codes.push(code);
    }
}

fn turn_status(value: Option<&Value>, line: usize) -> Result<TurnStatus, RunError> {
    let status = value.and_then(Value::as_str).unwrap_or("completed");
    if status.len() > MAX_STATUS_BYTES || status.chars().any(char::is_control) {
        return Err(RunError::MalformedStream { line });
    }
    Ok(match status {
        "completed" => TurnStatus::Completed,
        "failed" | "error" => TurnStatus::Failed,
        "interrupted" | "cancelled" => TurnStatus::Interrupted,
        other => TurnStatus::Unknown(other.to_owned()),
    })
}

fn parse_usage(value: &Value, line: usize) -> Result<Usage, RunError> {
    let object = value
        .as_object()
        .ok_or(RunError::MalformedStream { line })?;
    let number = |name: &str| match object.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or(RunError::MalformedStream { line }),
    };
    Ok(Usage {
        source: "turn.completed",
        input_tokens: number("input_tokens")?,
        cached_input_tokens: number("cached_input_tokens")?,
        output_tokens: number("output_tokens")?,
    })
}

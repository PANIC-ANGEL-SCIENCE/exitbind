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
        if contains(stderr, GIT_PRECONDITION) {
            push_diagnostic(&mut diagnostic_codes, DiagnosticCode::GitPrecondition);
        }
        // The host reports a sandbox file denial only as errno text. This is
        // the one place that text is read; a line that names a provider
        // config, auth or session read is a startup failure that a wider
        // sandbox cannot help, so it is never a sandbox denial.
        if host_write_denied(stderr) {
            push_diagnostic(&mut diagnostic_codes, DiagnosticCode::HostPermissionDenied);
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

/// The host's sandbox file-write denial, with the libc errno that identifies
/// it. Only these exact forms are classified; a command's own permission
/// error is otherwise indistinguishable and stays unclassified.
const HOST_PERMISSION_DENIALS: &[&[u8]] = &[
    b"Operation not permitted (os error 1)",
    b"Permission denied (os error 13)",
];

/// The provider's own state files. A read of one of these fails with the
/// same errno text but cannot be repaired by a wider sandbox, so its line is
/// not a sandbox write denial. Matching is confined to these file paths: a
/// bare word such as `config`, `auth`, `session` or `token` also sits inside
/// ordinary project paths and prose, and a real sandbox denial there must
/// stay classified.
const PROVIDER_READ_PATHS: &[&[u8]] = &[
    b".codex/config.toml",
    b".codex/auth.json",
    b".codex/sessions/",
    b"codex/sessions/",
    b".codex/log/",
    b"codex/log/",
];

/// Whether a line's denial names one of the provider's own state files.
fn provider_read_failure(line: &[u8]) -> bool {
    let lower = line.to_ascii_lowercase();
    PROVIDER_READ_PATHS
        .iter()
        .any(|path| contains(&lower, path))
}

/// Whether any stderr line carries an errno denial that is not a read of the
/// provider's own state files. A bare errno line (the sandbox's own report)
/// is accepted, and so is a denial whose path merely contains a word such as
/// `config` or `auth`.
fn host_write_denied(stderr: &[u8]) -> bool {
    stderr.split(|byte| *byte == b'\n').any(|line| {
        HOST_PERMISSION_DENIALS
            .iter()
            .any(|marker| contains(line, marker))
            && !provider_read_failure(line)
    })
}

fn contains(source: &[u8], marker: &[u8]) -> bool {
    source.windows(marker.len()).any(|window| window == marker)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_with(stderr: &[u8]) -> Observation {
        parse_stream(
            b"",
            false,
            ProcessOutcome {
                code: Some(1),
                signal: None,
                success: false,
                timed_out: false,
            },
            stderr,
        )
        .unwrap()
    }

    #[test]
    fn host_sandbox_write_denial_becomes_a_safe_category() {
        for stderr in [
            &b"write failed: Operation not permitted (os error 1)\nprivate path"[..],
            &b"cannot create file: Permission denied (os error 13)\nprivate path"[..],
        ] {
            assert!(
                failed_with(stderr)
                    .diagnostic
                    .codes
                    .contains(&DiagnosticCode::HostPermissionDenied),
                "{}",
                String::from_utf8_lossy(stderr)
            );
        }
        // Ordinary prose without the errno suffix is never a host denial.
        assert!(!failed_with(b"Permission denied by a test fixture\n")
            .diagnostic
            .codes
            .contains(&DiagnosticCode::HostPermissionDenied));
    }

    #[test]
    fn provider_read_failure_with_the_same_errno_is_not_a_sandbox_denial() {
        for stderr in [
            &b"failed to read config file .codex/config.toml: Permission denied (os error 13)\n"[..],
            &b"failed to read auth file .codex/auth.json: Operation not permitted (os error 1)\n"[..],
            &b"cannot open session /var/lib/codex/sessions/1.jsonl: Permission denied (os error 13)\n"[..],
        ] {
            let observation = failed_with(stderr);
            assert!(
                !observation
                    .diagnostic
                    .codes
                    .contains(&DiagnosticCode::HostPermissionDenied),
                "{}",
                String::from_utf8_lossy(stderr)
            );
            assert!(observation
                .diagnostic
                .codes
                .contains(&DiagnosticCode::StderrNonempty));
        }
        // A provider-read line does not hide a separate sandbox denial line.
        let mixed =
            b"failed to read config file .codex/config.toml: Permission denied (os error 13)\n\
                      sandbox write: Operation not permitted (os error 1)\n";
        assert!(failed_with(mixed)
            .diagnostic
            .codes
            .contains(&DiagnosticCode::HostPermissionDenied));
    }

    #[test]
    fn a_sandbox_denial_under_a_project_path_word_is_classified() {
        // Words that used to exclude a line no longer do when they sit in an
        // ordinary project path or prose; only the provider's own state files
        // are excluded.
        for stderr in [
            &b"sandbox write denied for /repo/config/app.yaml: Operation not permitted (os error 1)\n"[..],
            &b"cannot write /repo/tokenizer/output.bin: Permission denied (os error 13)\n"[..],
            &b"author file write failed: Permission denied (os error 13)\n"[..],
            &b"cannot write the session store at /repo/session-store/1: Operation not permitted (os error 1)\n"[..],
        ] {
            assert!(
                failed_with(stderr)
                    .diagnostic
                    .codes
                    .contains(&DiagnosticCode::HostPermissionDenied),
                "{}",
                String::from_utf8_lossy(stderr)
            );
        }
    }
}

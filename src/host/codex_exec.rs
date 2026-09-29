//! Bounded native Codex CLI execution and structured-event observation.
//!
//! This adapter owns process invocation and a small projection of Codex's
//! `exec --json` stream. It deliberately does not retain prompts, transcripts,
//! command text, or stderr. Assignment and ledger mutation remain the caller's
//! responsibility.

mod process;

use serde_json::{json, Value};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MAX_PROMPT_BYTES: usize = 256 * 1024;
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_FINAL_BYTES: usize = 256 * 1024;
const MAX_ID_BYTES: usize = 256;
const MAX_STATUS_BYTES: usize = 128;

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) executable: PathBuf,
    pub(crate) cwd: PathBuf,
    pub(crate) prompt: String,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) sandbox: Option<String>,
    pub(crate) output_schema: Option<PathBuf>,
    pub(crate) resume_thread_id: Option<String>,
    /// Persist the native Codex session so a later request can resume it.
    /// Exitbind never writes session files itself.
    pub(crate) persist_session: bool,
    pub(crate) timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessOutcome {
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) success: bool,
    pub(crate) timed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandOutcome {
    pub(crate) status: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) output_bytes: usize,
    pub(crate) invocation_sha256: Option<String>,
    pub(crate) host_item_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Usage {
    /// This is the last provider-reported `turn.completed` record. It is not
    /// synthesized or summed with earlier records, whose counters may be
    /// cumulative.
    pub(crate) source: &'static str,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) cached_input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TurnStatus {
    Completed,
    Failed,
    Interrupted,
    Unknown(String),
}

impl TurnStatus {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoverageGap {
    MissingThreadId,
    MissingTurnCompletion,
    MissingUsage,
    MissingCommandOutcome,
    MissingFinalResult,
    PromptDeliveryFailed,
    MissingCommandExitCode,
    MissingCommandIdentity,
}

impl CoverageGap {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::MissingThreadId => "missing_thread_id",
            Self::MissingTurnCompletion => "missing_turn_completion",
            Self::MissingUsage => "missing_usage",
            Self::MissingCommandOutcome => "missing_command_outcome",
            Self::MissingFinalResult => "missing_final_result",
            Self::PromptDeliveryFailed => "prompt_delivery_failed",
            Self::MissingCommandExitCode => "missing_command_exit_code",
            Self::MissingCommandIdentity => "missing_command_identity",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    /// Mirrors the invocation's session mode. This is explicit so callers do
    /// not mistake a returned thread ID for a session Exitbind persisted.
    pub(crate) ephemeral: bool,
    pub(crate) process: ProcessOutcome,
    pub(crate) turn: TurnStatus,
    pub(crate) command_outcomes: Vec<CommandOutcome>,
    pub(crate) usage: Option<Usage>,
    pub(crate) thread_id: Option<String>,
    pub(crate) final_result: Option<Value>,
    pub(crate) coverage_gap: Vec<CoverageGap>,
    pub(crate) interrupted: bool,
}

#[derive(Debug)]
pub(crate) enum RunError {
    InvalidRequest(&'static str),
    InvalidRequestDetail(String),
    Launch(io::Error),
    StreamTooLarge(&'static str),
    MalformedStream { line: usize },
    MalformedFinalResult,
    FinalResultTooLarge,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(reason) => write!(formatter, "invalid Codex request: {reason}"),
            Self::InvalidRequestDetail(reason) => {
                write!(formatter, "invalid Codex request: {reason}")
            }
            Self::Launch(error) => write!(formatter, "Codex launch failed: {error}"),
            Self::StreamTooLarge(stream) => {
                write!(formatter, "Codex {stream} exceeded the output limit")
            }
            Self::MalformedStream { line } => {
                write!(formatter, "Codex JSONL stream is malformed at line {line}")
            }
            Self::MalformedFinalResult => write!(formatter, "Codex final result is not valid JSON"),
            Self::FinalResultTooLarge => {
                write!(formatter, "Codex final result exceeded the output limit")
            }
        }
    }
}

impl std::error::Error for RunError {}

/// Resolve and pin a Codex executable before a native run.
///
/// An explicit path is accepted as-is only after canonicalization and regular
/// executable checks. A bare name is searched through the current PATH and the
/// selected candidate is canonicalized before it is returned.
pub(crate) fn resolve_codex(explicit: Option<&Path>) -> Result<PathBuf, RunError> {
    let requested = explicit.unwrap_or_else(|| Path::new("codex"));
    if explicit.is_some() || requested.components().count() > 1 {
        return checked_executable(requested);
    }
    let path = std::env::var_os("PATH").ok_or(RunError::InvalidRequest("PATH is unavailable"))?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(requested);
        if candidate.is_file() {
            if let Ok(found) = checked_executable(&candidate) {
                return Ok(found);
            }
        }
    }
    Err(RunError::InvalidRequest(
        "executable 'codex' was not found in PATH",
    ))
}

fn checked_executable(path: &Path) -> Result<PathBuf, RunError> {
    let canonical = fs::canonicalize(path).map_err(RunError::Launch)?;
    let metadata = fs::metadata(&canonical).map_err(RunError::Launch)?;
    if !metadata.is_file() {
        return Err(RunError::InvalidRequest("executable is not a regular file"));
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 == 0 {
        return Err(RunError::InvalidRequest("executable is not executable"));
    }
    Ok(canonical)
}

pub(crate) fn run(request: &Request) -> Result<Observation, RunError> {
    validate_request(request)?;
    let executable = checked_executable(&request.executable)?;
    let cwd = checked_directory(&request.cwd)?;
    let mut command = command_line(request, &executable, &cwd)?;
    let mut child = command.spawn().map_err(RunError::Launch)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RunError::InvalidRequest("Codex stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(RunError::InvalidRequest("Codex stderr is unavailable"))?;
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_tx.send(capture(stdout, MAX_STREAM_BYTES));
    });
    thread::spawn(move || {
        let _ = stderr_tx.send(capture(stderr, MAX_STREAM_BYTES));
    });

    // Deliver the bounded prompt independently so a child that never reads
    // stdin cannot bypass the request timeout by blocking the parent here.
    let prompt = request.prompt.clone();
    let stdin = child.stdin.take();
    let (prompt_tx, prompt_rx) = mpsc::channel();
    thread::spawn(move || {
        let Some(mut stdin) = stdin else {
            let _ = prompt_tx.send(true);
            return;
        };
        let _ = prompt_tx.send(stdin.write_all(prompt.as_bytes()).is_err());
    });

    let (status, timed_out) = process::wait_with_timeout(&mut child, request.timeout)?;
    let reader_grace = Duration::from_secs(2);
    let prompt_delivery_failed =
        bounded_receive(&prompt_rx, reader_grace, &mut child, "prompt writer")?;
    let stdout = bounded_receive(&stdout_rx, reader_grace, &mut child, "stdout reader")?;
    let _stderr = bounded_receive(&stderr_rx, reader_grace, &mut child, "stderr reader")?;
    if stdout.oversized {
        return Err(RunError::StreamTooLarge("stdout"));
    }
    if _stderr.oversized {
        return Err(RunError::StreamTooLarge("stderr"));
    }

    let process = process_outcome(status, timed_out);
    let mut observation = parse_stream(&stdout.bytes, request.output_schema.is_some(), process)?;
    observation.ephemeral = !request.persist_session;
    observation.interrupted = timed_out || observation.process.signal.is_some();
    if prompt_delivery_failed {
        observation
            .coverage_gap
            .push(CoverageGap::PromptDeliveryFailed);
    }
    Ok(observation)
}

pub(crate) fn validate_request(request: &Request) -> Result<(), RunError> {
    if !request.executable.is_absolute() {
        return Err(RunError::InvalidRequest("executable path must be absolute"));
    }
    if !request.cwd.is_absolute() {
        return Err(RunError::InvalidRequest("cwd must be absolute"));
    }
    if request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(RunError::InvalidRequest("prompt is too large"));
    }
    if request.timeout.is_zero() {
        return Err(RunError::InvalidRequest("timeout must be positive"));
    }
    validate_optional_text(request.model.as_deref(), "model")?;
    validate_optional_text(request.effort.as_deref(), "reasoning effort")?;
    if let Some(effort) = request.effort.as_deref() {
        if !matches!(effort, "none" | "low" | "medium" | "high" | "xhigh" | "max") {
            return Err(RunError::InvalidRequest("unsupported reasoning effort"));
        }
    }
    if let Some(sandbox) = request.sandbox.as_deref() {
        if !matches!(
            sandbox,
            "read-only" | "workspace-write" | "danger-full-access"
        ) {
            return Err(RunError::InvalidRequest("unsupported sandbox mode"));
        }
    }
    validate_id(request.resume_thread_id.as_deref(), "resume thread ID")?;
    if request.resume_thread_id.is_some() && !request.persist_session {
        return Err(RunError::InvalidRequest(
            "resume requires persist_session=true",
        ));
    }
    if request.resume_thread_id.is_some() && request.sandbox.is_some() {
        return Err(RunError::InvalidRequest(
            "Codex resume does not support --sandbox; omit sandbox or use a fresh request",
        ));
    }
    if let Some(schema) = request.output_schema.as_deref() {
        if !schema.is_absolute() {
            return Err(RunError::InvalidRequest(
                "output schema path must be absolute",
            ));
        }
        let metadata = fs::symlink_metadata(schema).map_err(RunError::Launch)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(RunError::InvalidRequest(
                "output schema is not a regular file",
            ));
        }
    }
    Ok(())
}

fn validate_optional_text(value: Option<&str>, label: &'static str) -> Result<(), RunError> {
    if let Some(value) = value {
        if value.is_empty() || value.len() > MAX_STATUS_BYTES || value.chars().any(char::is_control)
        {
            return Err(RunError::InvalidRequestDetail(format!(
                "{label} is invalid"
            )));
        }
    }
    Ok(())
}

fn validate_id(value: Option<&str>, label: &'static str) -> Result<(), RunError> {
    if let Some(value) = value {
        if value.is_empty()
            || value.len() > MAX_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err(RunError::InvalidRequestDetail(format!(
                "{label} is invalid"
            )));
        }
    }
    Ok(())
}

fn checked_directory(path: &Path) -> Result<PathBuf, RunError> {
    let canonical = fs::canonicalize(path).map_err(RunError::Launch)?;
    if !canonical.is_dir() {
        return Err(RunError::InvalidRequest("cwd is not a directory"));
    }
    Ok(canonical)
}

fn command_line(request: &Request, executable: &Path, cwd: &Path) -> Result<Command, RunError> {
    let mut command = Command::new(executable);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.current_dir(cwd).arg("exec");
    if let Some(thread_id) = request.resume_thread_id.as_deref() {
        command.args(["resume", thread_id]);
    }
    command.arg("--json");
    if !request.persist_session {
        command.arg("--ephemeral");
    }
    if let Some(model) = request.model.as_deref() {
        command.args(["--model", model]);
    }
    if let Some(effort) = request.effort.as_deref() {
        command.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
    }
    if let Some(sandbox) = request.sandbox.as_deref() {
        command.args(["--sandbox", sandbox]);
    }
    if let Some(schema) = request.output_schema.as_deref() {
        command.args([
            "--output-schema",
            schema.to_str().ok_or(RunError::InvalidRequest(
                "output schema path is not valid UTF-8",
            ))?,
        ]);
    }
    command
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

struct Captured {
    bytes: Vec<u8>,
    oversized: bool,
}

fn capture<R: Read>(mut reader: R, limit: usize) -> Captured {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut oversized = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if bytes.len() < limit {
                    let keep = read.min(limit - bytes.len());
                    bytes.extend_from_slice(&buffer[..keep]);
                    if keep < read {
                        oversized = true;
                    }
                } else {
                    oversized = true;
                }
            }
            Err(_) => {
                oversized = true;
                break;
            }
        }
    }
    Captured { bytes, oversized }
}

fn bounded_receive<T>(
    receiver: &mpsc::Receiver<T>,
    grace: Duration,
    child: &mut std::process::Child,
    label: &'static str,
) -> Result<T, RunError> {
    receiver.recv_timeout(grace).map_err(|_| {
        process::terminate_group(child);
        RunError::InvalidRequestDetail(format!("Codex {label} did not finish after process exit"))
    })
}

fn process_outcome(status: ExitStatus, timed_out: bool) -> ProcessOutcome {
    ProcessOutcome {
        code: status.code(),
        signal: termination_signal(&status),
        success: status.success(),
        timed_out,
    }
}

#[cfg(unix)]
fn termination_signal(status: &ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(status)
}

#[cfg(not(unix))]
fn termination_signal(_: &ExitStatus) -> Option<i32> {
    None
}

fn parse_stream(
    bytes: &[u8],
    want_final: bool,
    process: ProcessOutcome,
) -> Result<Observation, RunError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RunError::MalformedStream { line: 1 })?;
    let mut thread_id = None;
    let mut turn = None;
    let mut command_outcomes = Vec::new();
    let mut started_commands = 0usize;
    let mut usage = None;
    let mut final_text = None;
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
                    _ => {}
                }
            }
            Some("item.started") => {
                let item = object
                    .get("item")
                    .and_then(Value::as_object)
                    .ok_or(RunError::MalformedStream { line: line_number })?;
                if item.get("type").and_then(Value::as_str) == Some("command_execution") {
                    started_commands = started_commands.saturating_add(1);
                }
            }
            Some("turn.completed") => {
                turn = Some(turn_status(object.get("status"), line_number)?);
                if let Some(raw_usage) = object.get("usage") {
                    usage = Some(parse_usage(raw_usage, line_number)?);
                }
            }
            Some("turn.failed") => turn = Some(TurnStatus::Failed),
            Some("error") => turn = Some(TurnStatus::Failed),
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
        usage,
        thread_id,
        final_result,
        coverage_gap,
        interrupted: process.timed_out || process.signal.is_some(),
    })
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
    use serde_json::json;

    #[test]
    fn parses_declared_events_without_retaining_command_text_or_output() {
        let source = concat!(
            r#"{"type":"thread.started","thread_id":"thread-1"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-1","status":"completed","exit_code":0,"command":"secret","aggregated_output":"private output"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"{\"ok\":true}"}}"#,
            "\n",
            r#"{"type":"turn.completed","status":"completed","usage":{"input_tokens":12,"cached_input_tokens":3,"output_tokens":5}}"#,
        );
        let observation = parse_stream(
            source.as_bytes(),
            true,
            ProcessOutcome {
                code: Some(0),
                signal: None,
                success: true,
                timed_out: false,
            },
        )
        .unwrap();
        assert_eq!(observation.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(observation.command_outcomes[0].exit_code, Some(0));
        assert_eq!(observation.command_outcomes[0].output_bytes, 14);
        assert_eq!(
            observation.command_outcomes[0].host_item_id.as_deref(),
            Some("cmd-1")
        );
        assert!(observation.command_outcomes[0].invocation_sha256.is_some());
        assert_eq!(observation.usage.as_ref().unwrap().input_tokens, Some(12));
        assert_eq!(observation.final_result, Some(json!({"ok": true})));
        assert!(observation.coverage_gap.is_empty());
    }

    #[test]
    fn malformed_jsonl_is_rejected_without_exposing_the_line() {
        let error = parse_stream(
            b"not-json\n",
            false,
            ProcessOutcome {
                code: Some(1),
                signal: None,
                success: false,
                timed_out: false,
            },
        )
        .unwrap_err();
        assert!(matches!(error, RunError::MalformedStream { line: 1 }));
        assert!(!error.to_string().contains("not-json"));
    }

    #[test]
    fn missing_native_completion_is_a_gap_with_process_status_preserved() {
        let observation = parse_stream(
            b"",
            false,
            ProcessOutcome {
                code: Some(0),
                signal: None,
                success: true,
                timed_out: false,
            },
        )
        .unwrap();
        assert!(observation.process.success);
        assert!(observation
            .coverage_gap
            .contains(&CoverageGap::MissingTurnCompletion));
        assert!(observation
            .coverage_gap
            .contains(&CoverageGap::MissingThreadId));
    }

    #[test]
    fn incomplete_command_observation_keeps_identity_gap_explicit() {
        let source = br#"{"type":"item.completed","item":{"type":"command_execution","status":"completed"}}"#;
        let observation = parse_stream(
            source,
            false,
            ProcessOutcome {
                code: Some(0),
                signal: None,
                success: true,
                timed_out: false,
            },
        )
        .unwrap();
        assert!(observation
            .coverage_gap
            .contains(&CoverageGap::MissingCommandExitCode));
        assert!(observation
            .coverage_gap
            .contains(&CoverageGap::MissingCommandIdentity));
        assert_eq!(observation.command_outcomes[0].invocation_sha256, None);
    }
}

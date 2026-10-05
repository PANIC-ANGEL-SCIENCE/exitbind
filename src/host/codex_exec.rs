//! Bounded native Codex CLI execution and structured-event observation.
//!
//! This adapter owns process invocation and a small projection of Codex's
//! `exec --json` stream. It deliberately does not retain prompts, transcripts,
//! command text, or stderr. Assignment and ledger mutation remain the caller's
//! responsibility.

mod controlled;
pub(crate) use controlled::Tool as ControlledTool;
mod process;
mod stream;

use stream::parse_stream;

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
    pub(crate) controlled_tool: Option<ControlledTool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessOutcome {
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) success: bool,
    pub(crate) timed_out: bool,
}

/// The provider process identity captured immediately after spawn. The
/// optional start time prevents a reused PID from being mistaken for the
/// original execution on Linux.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) process_group: Option<i32>,
    pub(crate) start_time_ticks: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcessLiveness {
    Alive,
    Ended,
    Uncertain,
}

/// The bounded provider fields that were successfully parsed from the native
/// JSONL stream before the execution result is returned to its caller.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StreamObservation {
    pub(crate) thread_id: Option<String>,
    pub(crate) final_result: Option<Value>,
}

/// Privacy-safe, allowlisted execution facts. Provider text is deliberately
/// not represented here; callers can persist this envelope without retaining
/// stderr, event messages, or request material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticEnvelope {
    pub(crate) status: DiagnosticStatus,
    pub(crate) codes: Vec<DiagnosticCode>,
    pub(crate) stdout_bytes: usize,
    pub(crate) stderr_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticStatus {
    Observed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticCode {
    NoStdout,
    StderrNonempty,
    GitPrecondition,
    TurnFailed,
    ProviderError,
}

impl DiagnosticCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoStdout => "no_stdout",
            Self::StderrNonempty => "stderr_nonempty",
            Self::GitPrecondition => "git_precondition",
            Self::TurnFailed => "turn_failed",
            Self::ProviderError => "provider_error",
        }
    }
}

type ProcessObserver<'a> = dyn FnMut(&ProcessIdentity) -> Result<(), String> + 'a;
type StreamObserver<'a> = dyn FnMut(&StreamObservation) -> Result<(), String> + 'a;

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
            // Provider status text is not an Exitbind contract and may carry
            // arbitrary message material. Keep the distinction without
            // exposing the provider value in journals or inspection output.
            Self::Unknown(_) => "unknown",
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

/// Project the bounded numeric facts available for one native turn.
///
/// Provider usage is a completion snapshot. It may be cumulative on the
/// provider side, so this projection never sums it with a retry, parent,
/// child, or replay. Missing fields stay unknown and an observed zero stays
/// an observed zero.
pub(crate) fn usage_account(observation: &Observation) -> Value {
    let usage = observation.usage.as_ref();
    let complete = usage.is_some_and(|value| {
        value.input_tokens.is_some()
            && value.cached_input_tokens.is_some()
            && value.output_tokens.is_some()
    });
    json!({
        "status": if complete { "observed" } else { "missing" },
        "scope": "native_turn",
        "source": usage.map_or("unavailable", |value| value.source),
        "counterSemantics": "provider_turn_snapshot",
        "additive": false,
        "usage": usage.map(|value| json!({
            "inputTokens": value.input_tokens,
            "cachedInputTokens": value.cached_input_tokens,
            "outputTokens": value.output_tokens,
        })),
        "coverage": {
            "turn": observation.turn.as_str(),
            "commands": observation.command_outcomes.len(),
            "unobservedItems": observation.unobserved_item_count,
            "rootSetup": "unobserved",
            "parentChild": "not_counted",
            "retry": "unknown",
            "replay": "not_counted",
            "gaps": observation.coverage_gap.iter().map(CoverageGap::as_str).collect::<Vec<_>>(),
        },
    })
}

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    /// Mirrors the invocation's session mode. This is explicit so callers do
    /// not mistake a returned thread ID for a session Exitbind persisted.
    pub(crate) ephemeral: bool,
    pub(crate) process: ProcessOutcome,
    pub(crate) turn: TurnStatus,
    pub(crate) command_outcomes: Vec<CommandOutcome>,
    /// Item kinds outside the command/message projection may have effects.
    pub(crate) unobserved_item_count: u64,
    pub(crate) usage: Option<Usage>,
    pub(crate) thread_id: Option<String>,
    pub(crate) final_result: Option<Value>,
    pub(crate) diagnostic: DiagnosticEnvelope,
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
    Observer(String),
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
            Self::Observer(reason) => {
                write!(formatter, "Codex process observation failed: {reason}")
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
    run_inner(request, None, None)
}

/// Observe process identity and bounded stream fields as they arrive. Stream
/// fields are provisional until the complete stream passes `parse_stream`.
pub(crate) fn run_with_observers<P, S>(
    request: &Request,
    mut process_observer: P,
    mut stream_observer: S,
) -> Result<Observation, RunError>
where
    P: FnMut(&ProcessIdentity) -> Result<(), String>,
    S: FnMut(&StreamObservation) -> Result<(), String>,
{
    run_inner(
        request,
        Some(&mut process_observer),
        Some(&mut stream_observer),
    )
}

fn run_inner(
    request: &Request,
    mut observer: Option<&mut ProcessObserver<'_>>,
    mut stream_observer: Option<&mut StreamObserver<'_>>,
) -> Result<Observation, RunError> {
    validate_request(request)?;
    let executable = checked_executable(&request.executable)?;
    let cwd = checked_directory(&request.cwd)?;
    let mut command = command_line(request, &executable, &cwd)?;
    let mut child = command.spawn().map_err(RunError::Launch)?;
    let identity = process::identity(child.id());
    if let Some(observer) = observer.as_mut() {
        if let Err(error) = observer(&identity) {
            process::terminate_group(&mut child);
            let _ = child.wait();
            return Err(RunError::Observer(error));
        }
    }
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
    let stream_enabled = stream_observer.is_some();
    let (stream_tx, stream_rx) = mpsc::channel();
    thread::spawn(move || {
        let captured = if stream_enabled {
            process::capture_with_lines(stdout, MAX_STREAM_BYTES, stream_tx)
        } else {
            capture(stdout, MAX_STREAM_BYTES)
        };
        let _ = stdout_tx.send(captured);
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

    let want_final = request.output_schema.is_some();
    let (status, timed_out) = process::wait_with_timeout_poll(&mut child, request.timeout, || {
        if stream_enabled {
            process::drain_stream_lines(&stream_rx, want_final, &mut stream_observer)
        } else {
            Ok(())
        }
    })?;
    let reader_grace = Duration::from_secs(2);
    let prompt_delivery_failed =
        bounded_receive(&prompt_rx, reader_grace, &mut child, "prompt writer")?;
    let stdout = bounded_receive(&stdout_rx, reader_grace, &mut child, "stdout reader")?;
    let stderr = bounded_receive(&stderr_rx, reader_grace, &mut child, "stderr reader")?;
    if stream_enabled {
        if let Err(error) = process::drain_stream_lines_until_closed(
            &stream_rx,
            want_final,
            &mut stream_observer,
            reader_grace,
        ) {
            // wait_with_timeout has already observed process exit. Reap again
            // defensively so a callback failure cannot leave owned state live.
            let _ = child.wait();
            return Err(error);
        }
    }
    if stdout.oversized {
        return Err(RunError::StreamTooLarge("stdout"));
    }
    if stderr.oversized {
        return Err(RunError::StreamTooLarge("stderr"));
    }

    let process = process_outcome(status, timed_out);
    let mut observation = parse_stream(
        &stdout.bytes,
        request.output_schema.is_some(),
        process,
        &stderr.bytes,
    )?;
    observation.ephemeral = !request.persist_session;
    observation.interrupted = timed_out || observation.process.signal.is_some();
    if prompt_delivery_failed {
        observation
            .coverage_gap
            .push(CoverageGap::PromptDeliveryFailed);
    }
    Ok(observation)
}

/// Classify a previously observed provider process. An inaccessible or
/// unverifiable process is never treated as ended, so callers cannot
/// accidentally launch a duplicate operation.
pub(crate) fn process_liveness(identity: &ProcessIdentity) -> ProcessLiveness {
    process::liveness(identity)
}

pub(crate) fn validate_request(request: &Request) -> Result<(), RunError> {
    controlled::validate(request)?;
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
    if let Some(tool) = &request.controlled_tool {
        controlled::configure(&mut command, tool);
    }
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
            b"",
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
        assert_eq!(observation.unobserved_item_count, 0);
        assert_eq!(observation.diagnostic.status, DiagnosticStatus::Observed);
        assert!(observation.diagnostic.codes.is_empty());
        assert_eq!(observation.diagnostic.stdout_bytes, source.len());
        assert_eq!(observation.diagnostic.stderr_bytes, 0);
        let account = usage_account(&observation);
        assert_eq!(account["status"], "observed");
        assert_eq!(account["counterSemantics"], "provider_turn_snapshot");
        assert_eq!(account["additive"], false);
        assert_eq!(account["usage"]["inputTokens"], 12);
        assert_eq!(account["coverage"]["retry"], "unknown");
        assert_eq!(account["coverage"]["parentChild"], "not_counted");
    }

    #[test]
    fn usage_account_keeps_partial_usage_missing_and_does_not_infer_zero() {
        let source = r#"{"type":"turn.completed","status":"completed","usage":{"input_tokens":0}}"#;
        let observation = parse_stream(
            source.as_bytes(),
            false,
            ProcessOutcome {
                code: Some(0),
                signal: None,
                success: true,
                timed_out: false,
            },
            b"",
        )
        .unwrap();
        let account = usage_account(&observation);
        assert_eq!(account["status"], "missing");
        assert_eq!(account["usage"]["inputTokens"], 0);
        assert!(account["usage"]["cachedInputTokens"].is_null());
        assert_eq!(account["source"], "turn.completed");
        assert!(account["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap == "missing_usage"));
    }

    #[test]
    fn unknown_tool_items_are_counted_without_retaining_content() {
        let source = concat!(
            r#"{"type":"item.started","item":{"type":"mcp_tool_call","secret":"private"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"mcp_tool_call","secret":"private"}}"#,
        );
        let observation = parse_stream(
            source.as_bytes(),
            false,
            ProcessOutcome {
                code: Some(1),
                signal: None,
                success: false,
                timed_out: false,
            },
            b"",
        )
        .unwrap();
        assert_eq!(observation.unobserved_item_count, 2);
        assert!(observation.command_outcomes.is_empty());
    }

    #[test]
    fn failure_diagnostics_keep_only_allowlisted_codes_and_sizes() {
        let source = concat!(
            r#"{"type":"turn.failed","message":"private message"}"#,
            "\n",
            r#"{"type":"error","body":"private body","path":"/private/path","token":"secret"}"#,
        );
        let observation = parse_stream(
            source.as_bytes(),
            false,
            ProcessOutcome {
                code: Some(1),
                signal: None,
                success: false,
                timed_out: false,
            },
            b"private stderr text",
        )
        .unwrap();
        assert_eq!(observation.turn, TurnStatus::Failed);
        assert_eq!(observation.diagnostic.stdout_bytes, source.len());
        assert_eq!(
            observation.diagnostic.stderr_bytes,
            b"private stderr text".len()
        );
        assert_eq!(
            observation.diagnostic.codes,
            vec![
                DiagnosticCode::TurnFailed,
                DiagnosticCode::ProviderError,
                DiagnosticCode::StderrNonempty,
            ]
        );
        let rendered = serde_json::to_string(&json!({
            "codes": observation.diagnostic.codes.iter().map(|code| code.as_str()).collect::<Vec<_>>(),
            "stdoutBytes": observation.diagnostic.stdout_bytes,
            "stderrBytes": observation.diagnostic.stderr_bytes,
        }))
        .unwrap();
        assert!(!rendered.contains("private"));
        assert!(!rendered.contains("/private/path"));
        assert!(!rendered.contains("secret"));
    }

    #[test]
    fn unknown_provider_status_is_redacted_to_unknown() {
        let observation = parse_stream(
            br#"{"type":"turn.completed","status":"private-message"}"#,
            false,
            ProcessOutcome {
                code: Some(0),
                signal: None,
                success: true,
                timed_out: false,
            },
            b"",
        )
        .unwrap();
        assert_eq!(observation.turn.as_str(), "unknown");
    }

    #[test]
    fn trusted_directory_stderr_becomes_a_safe_category() {
        let observation = parse_stream(
            b"",
            false,
            ProcessOutcome {
                code: Some(1),
                signal: None,
                success: false,
                timed_out: false,
            },
            b"Not inside a trusted directory and --skip-git-repo-check was not specified.\nprivate path",
        )
        .unwrap();
        assert!(observation
            .diagnostic
            .codes
            .contains(&DiagnosticCode::GitPrecondition));
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
            b"",
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
            b"",
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
            b"",
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

    #[test]
    fn oversized_capture_is_truncated_and_marked_for_fail_closed_handling() {
        let source = vec![b'x'; MAX_STREAM_BYTES + 1];
        let captured = capture(std::io::Cursor::new(source), MAX_STREAM_BYTES);
        assert!(captured.oversized);
        assert_eq!(captured.bytes.len(), MAX_STREAM_BYTES);
    }
}

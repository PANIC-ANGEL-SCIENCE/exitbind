//! Same-Work recovery for a native operation whose reply was lost.
//!
//! Recovery is admission only. It reads the durable native journal and lets the
//! normal result boundary decide whether the recorded assignment is still
//! admissible. It never starts a provider process.

use super::{
    acquire_lock, journal_paths, journal_result, read_journal, timestamp, JournalLock, Paths,
};
use crate::config::Loaded;
use crate::host::codex_exec::{self, ProcessIdentity, ProcessLiveness, StreamObservation};
use crate::project::layout_types::state_namespace;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

pub(super) fn recover(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    operation: Option<&str>,
    overrides: bool,
) -> Result<Option<Value>, String> {
    // With a pending native assignment, bare --resume continues that current
    // operation. A historical replay needs its explicit operation selector.
    if current["action"] == "spawn" && operation.is_none() {
        return Ok(None);
    }
    let Some(candidate) = find_candidate(loaded, work, current, operation)? else {
        return Ok(None);
    };

    if overrides {
        return Err("native replay refuses changed execution parameters; retry the recorded operation without model, binary, sandbox or timeout overrides".into());
    }

    match candidate.status.as_str() {
        "completed" => {
            super::validate_completed_observation(&candidate.journal)?;
            let bytes = journal_result(&candidate.journal)?;
            super::super::super::return_result_impl::replay_recorded_native(
                loaded, work, &candidate.assignment, &candidate.journal, &bytes,
            ).map(Some)
        }
        "running" => Err(format!(
            "native recovery refused: assignment {} has {} provider execution; no old assignment can be re-admitted after Work advances",
            candidate.assignment,
            match liveness(&candidate.journal) {
                ProcessLiveness::Alive => "a live",
                ProcessLiveness::Ended => "an ended but unretained",
                ProcessLiveness::Uncertain => "an uncertain",
            }
        )),
        "started" => Err(format!(
            "native recovery refused: assignment {} ended without a committed native result; no provider retry is admitted after Work advanced",
            candidate.assignment
        )),
        _ => Err("native recovery journal has an invalid status".into()),
    }
}

pub(super) fn validate_overrides(
    journal: &Value,
    options: &super::Options<'_>,
) -> Result<(), String> {
    let request = &journal["request"];
    let conflict = options.model.is_some_and(|value| request["model"] != value)
        || options
            .reasoning_effort
            .is_some_and(|value| request["reasoningEffort"] != value)
        || options
            .sandbox_mode
            .is_some_and(|value| request["sandbox"] != value)
        || options.timeout_ms.is_some_and(|value| {
            value
                .parse::<u64>()
                .ok()
                .map_or(true, |parsed| request["timeoutMs"] != parsed)
        });
    let executable_conflict = if let Some(value) = options.codex_bin {
        let selected =
            codex_exec::resolve_codex(Some(Path::new(value))).map_err(|error| error.to_string())?;
        request["executable"].as_str() != selected.to_str()
    } else {
        false
    };
    if conflict || executable_conflict {
        return Err(
            "native recovery refuses changed execution parameters for the recorded operation"
                .into(),
        );
    }
    Ok(())
}

pub(super) struct RetryContext {
    pub(super) thread_id: Option<String>,
    pub(super) prior_failure: Value,
}

/// Preserve a provider failure before admitting a new current worker turn.
///
/// The old journal is copied to a private non-journal suffix and the caller
/// atomically replaces the live journal with a new operation record. This
/// keeps the provisional result inspectable while giving the retry a distinct
/// operation identity. A reviewer never uses this route.
pub(super) fn prepare_provisional_retry(
    paths: &Paths,
    journal: &Value,
    role: &str,
) -> Result<RetryContext, String> {
    if role != "worker" {
        return Err(
            "native provisional result can be retried only for the current worker assignment"
                .into(),
        );
    }
    match liveness(journal) {
        ProcessLiveness::Alive => {
            return Err(
                "native assignment provider is alive; keep the existing execution handle".into(),
            )
        }
        ProcessLiveness::Uncertain => {
            return Err(
                "native assignment execution is uncertain; no duplicate provider run is admitted"
                    .into(),
            )
        }
        ProcessLiveness::Ended => {}
    }

    validate_provisional_observation(journal)?;

    let name = paths
        .journal
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("native assignment journal has an invalid name")?;
    let archive = paths.journal.with_file_name(format!(
        ".{name}.provisional.{}.{}",
        std::process::id(),
        timestamp()
    ));
    archive_journal(&paths.journal, &archive)?;
    let prior_failure = json!({
        "operation": journal["operationId"].as_str().unwrap_or(name),
        "status": journal["status"],
        "observation": journal["observation"],
        "provisionalFinalResult": journal["provisionalFinalResult"],
        "provisionalFinalResultBytes": journal["provisionalFinalResultBytes"],
        "error": journal["error"],
        "archive": archive.file_name().and_then(|value| value.to_str()),
    });
    Ok(RetryContext {
        thread_id: journal["threadId"].as_str().map(str::to_owned),
        prior_failure,
    })
}

pub(super) fn validate_provisional_observation(journal: &Value) -> Result<(), String> {
    if journal
        .get("priorFailure")
        .is_some_and(|value| !value.is_null())
    {
        return Err("native provisional result has already used its bounded retry".into());
    }
    let result = journal
        .get("provisionalFinalResult")
        .filter(|value| !value.is_null())
        .ok_or("native provisional result is unavailable for reconciliation")?;
    super::result::validate_final_result("worker", result)?;
    let bytes = serde_json::to_vec(result).map_err(|error| error.to_string())?;
    if bytes.len() > super::MAX_RESULT_BYTES {
        return Err("native provisional result exceeds the journal bound".into());
    }
    let encoded = super::hex_encode(&bytes);
    if journal["provisionalFinalResultBytes"].as_str() != Some(encoded.as_str()) {
        return Err("native provisional result bytes do not match the retained result".into());
    }

    let observation = journal
        .get("observation")
        .and_then(Value::as_object)
        .ok_or("native provisional result has no bounded prior observation")?;
    let process = observation
        .get("process")
        .and_then(Value::as_object)
        .ok_or("native provisional result has no process observation")?;
    let exit_code = process.get("code").and_then(Value::as_i64);
    if process.get("success") != Some(&Value::Bool(false))
        || !exit_code.is_some_and(|code| code != 0)
        || process.get("signal").is_some_and(|value| !value.is_null())
        || process.get("timedOut") != Some(&Value::Bool(false))
        || observation.get("interrupted") != Some(&Value::Bool(false))
        || !matches!(
            observation.get("turn").and_then(Value::as_str),
            Some("completed" | "failed")
        )
        || observation
            .get("threadId")
            .and_then(Value::as_str)
            .is_none()
        || !observation
            .get("coverageGaps")
            .and_then(Value::as_array)
            .is_some_and(|gaps| gaps.is_empty())
    {
        return Err(
            "native provisional result has no complete safe process observation; retry is refused"
                .into(),
        );
    }
    let commands = observation
        .get("commands")
        .and_then(Value::as_array)
        .ok_or("native provisional result has no command observation")?;
    if !commands.is_empty() || observation.get("unobservedItemCount") != Some(&json!(0)) {
        return Err(
            "native provisional result may have external effects; automatic retry is refused"
                .into(),
        );
    }
    for command in commands {
        if command.get("status").and_then(Value::as_str).is_none()
            || command.get("exitCode").and_then(Value::as_i64).is_none()
            || command
                .get("invocationSha256")
                .and_then(Value::as_str)
                .is_none()
        {
            return Err(
                "native provisional result has incomplete command outcomes; retry is refused"
                    .into(),
            );
        }
    }
    let usage = observation
        .get("usage")
        .and_then(Value::as_object)
        .ok_or("native provisional result has no usage observation")?;
    if !usage.get("inputTokens").and_then(Value::as_u64).is_some()
        || !usage
            .get("cachedInputTokens")
            .and_then(Value::as_u64)
            .is_some()
        || !usage.get("outputTokens").and_then(Value::as_u64).is_some()
    {
        return Err(
            "native provisional result has incomplete usage observation; retry is refused".into(),
        );
    }
    Ok(())
}

pub(super) fn validate_rebuilt_request(
    journal: &Value,
    request: &codex_exec::Request,
) -> Result<(), String> {
    let old = &journal["request"];
    let new = super::result::request_projection(request);
    if old["executable"] != new["executable"]
        || old["model"] != new["model"]
        || old["reasoningEffort"] != new["reasoningEffort"]
        || old["timeoutMs"] != new["timeoutMs"]
        || old["persistSession"] != new["persistSession"]
    {
        return Err("native recovery refuses a changed executable or execution parameters".into());
    }
    Ok(())
}

fn archive_journal(source: &Path, archive: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("native assignment journal is not a regular file".into());
    }
    let bytes = fs::read(source).map_err(|error| error.to_string())?;
    if bytes.len() > super::MAX_JOURNAL_BYTES as usize {
        return Err("native assignment journal exceeds its bound".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(archive).map_err(|error| error.to_string())?;
    let result = (|| {
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        if let Some(parent) = archive.parent() {
            #[cfg(unix)]
            File::open(parent)
                .map_err(|error| error.to_string())?
                .sync_all()
                .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(archive);
    }
    result
}

struct Candidate {
    assignment: String,
    status: String,
    journal: Value,
    _lock: JournalLock,
}

fn find_candidate(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    operation: Option<&str>,
) -> Result<Option<Candidate>, String> {
    if !super::super::valid_work_handle(work) {
        return Err("native recovery work handle is invalid".into());
    }
    if operation.is_some_and(|value| !valid_assignment(value)) {
        return Err("native recovery operation is invalid".into());
    }
    let directory = loaded
        .state_root
        .join(state_namespace())
        .join("native-actions")
        .join(work);
    let current_path = current["assignment"].as_str().and_then(|assignment| {
        journal_paths(
            loaded,
            work,
            assignment,
            current["packet"]["substitution"].is_object(),
        )
        .ok()
        .map(|paths| paths.journal)
    });

    // A current native assignment owns its own journal and the ordinary
    // execute path retains its exact resume semantics. Only search history
    // when the current assignment has no journal to claim.
    if let Some(path) = current_path.as_ref() {
        if read_journal(path)?.is_some()
            && (operation.is_none() || path.file_stem().and_then(|stem| stem.to_str()) == operation)
        {
            return Ok(None);
        }
    }

    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !is_journal(&path)
            || current_path
                .as_ref()
                .is_some_and(|current| current == &path)
        {
            continue;
        }
        let lock = acquire_lock(&path.with_extension("lock"))?;
        let Some(journal) = read_journal(&path)? else {
            continue;
        };
        if journal["work"] != work || journal["version"] != 1 {
            continue;
        }
        if !matches!(
            journal["role"].as_str(),
            Some("worker" | "reviewer" | "adviser")
        ) || !journal["agent"]
            .as_str()
            .is_some_and(|agent| !agent.is_empty() && agent.len() <= 128)
        {
            continue;
        }
        let Some(assignment) = journal["assignment"].as_str() else {
            continue;
        };
        if !valid_assignment(assignment) {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if key != assignment && key != format!("{assignment}-fallback") {
            continue;
        }
        if operation.is_some_and(|selected| selected != key) {
            continue;
        }
        let Some(status) = journal["status"].as_str() else {
            continue;
        };
        if !matches!(status, "completed" | "started" | "running") {
            continue;
        }
        let Some(attempt) = journal["attempt"].as_u64() else {
            continue;
        };
        candidates.push((
            attempt,
            key.to_owned(),
            assignment.to_owned(),
            status.to_owned(),
            journal,
            lock,
        ));
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    if candidates.len() > 1 {
        let commands = candidates
            .iter()
            .map(|candidate| {
                format!(
                    "exitbind work act {work} --resume --operation {}",
                    candidate.1
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "native recovery is ambiguous; choose one recorded operation: {commands}"
        ));
    }
    let Some((_, _, assignment, status, journal, lock)) = candidates.pop() else {
        return if operation.is_some() {
            Err("native recovery operation has no matching same-Work journal".into())
        } else {
            Ok(None)
        };
    };
    Ok(Some(Candidate {
        assignment,
        status,
        journal,
        _lock: lock,
    }))
}

fn valid_assignment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

fn is_journal(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("json")
        && !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.ends_with(".schema.json"))
}

pub(super) fn persist_identity(path: &Path, identity: &ProcessIdentity) -> Result<(), String> {
    let mut journal =
        read_journal(path)?.ok_or("native journal disappeared after provider spawn")?;
    if journal["status"] != "running" {
        return Err("native journal is not claimed for the spawned provider".into());
    }
    journal["processIdentity"] = json!({
        "pid": identity.pid,
        "processGroup": identity.process_group,
        "startTimeTicks": identity.start_time_ticks,
    });
    super::write_journal(path, &journal, false)
}

pub(super) fn persist_stream(path: &Path, stream: &StreamObservation) -> Result<(), String> {
    let mut journal =
        read_journal(path)?.ok_or("native journal disappeared during provider stream")?;
    if journal["status"] != "running" {
        return Err("native journal is not claimed for provider stream".into());
    }
    if let Some(thread_id) = stream.thread_id.as_deref() {
        if journal["threadId"]
            .as_str()
            .is_some_and(|old| old != thread_id)
        {
            return Err("native provider thread ID changed during one operation".into());
        }
        journal["threadId"] = json!(thread_id);
    }
    if let Some(result) = stream.final_result.as_ref() {
        let bytes = serde_json::to_vec(result).map_err(|error| error.to_string())?;
        if bytes.len() > super::MAX_RESULT_BYTES {
            return Err("native provisional result exceeds the journal bound".into());
        }
        journal["provisionalFinalResult"] = result.clone();
        journal["provisionalFinalResultBytes"] = json!(super::hex_encode(&bytes));
    }
    super::write_journal(path, &journal, false)
}

pub(super) fn liveness(journal: &Value) -> ProcessLiveness {
    let Some(pid) = journal["processIdentity"]["pid"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
    else {
        return ProcessLiveness::Uncertain;
    };
    let process_group = journal["processIdentity"]["processGroup"]
        .as_i64()
        .and_then(|value| i32::try_from(value).ok());
    let start_time_ticks = journal["processIdentity"]["startTimeTicks"].as_u64();
    codex_exec::process_liveness(&ProcessIdentity {
        pid,
        process_group,
        start_time_ticks,
    })
}

pub(super) fn mark_same_work_return(mut response: Value, work: &str, assignment: &str) -> Value {
    response["recoveryEvent"] = json!({
        "kind": "native_saved_return",
        "cue": "same_door",
        "work": work,
        "assignment": assignment,
        "eventSha256": response["eventSha256"],
        "providerExecuted": false,
        "nextObligation": response["next"]["action"],
    });
    response
}

//! Same-Work recovery for a native operation whose reply was lost.
//!
//! Recovery is admission only. It reads the durable native journal and lets the
//! normal result boundary decide whether the recorded assignment is still
//! admissible. It never starts a provider process.

use super::{acquire_lock, journal_paths, journal_result, read_journal, JournalLock};
use crate::config::Loaded;
use crate::host::codex_exec::{self, ProcessIdentity, ProcessLiveness, StreamObservation};
use crate::project::layout_types::state_namespace;
use serde_json::{json, Value};
use std::fs;
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
        if serde_json::to_vec(result)
            .map_err(|error| error.to_string())?
            .len()
            > super::MAX_RESULT_BYTES
        {
            return Err("native provisional result exceeds the journal bound".into());
        }
        journal["provisionalFinalResult"] = result.clone();
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

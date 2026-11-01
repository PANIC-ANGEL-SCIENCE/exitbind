//! Native-owned observations and mandate control at actual provider admission.
//! Account authorization remains with the continuation runtime; journal keys
//! and provider observations never establish that authorization themselves.
use super::*;
use crate::project::continuation::{control::Kind, runtime};
use crate::project::path::SecureBytesResult;
use std::time::{Duration, Instant};

#[path = "continuation/failure.rs"]
mod failure;
pub(super) use failure::reconcile_failed;

#[derive(Clone, Debug)]
pub(crate) struct ActiveNative {
    pub key: String,
    pub request_sha256: String,
    pub role: String,
    pub agent: String,
    pub assignment: String,
    pub work: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SavedNative {
    pub key: String,
    pub request_sha256: String,
    pub role: String,
    pub agent: String,
    pub assignment: String,
    pub result_sha256: String,
    pub outcome: String,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeControlFacts {
    pub all_ended: bool,
    pub retirement_blocked: bool,
    pub active: Vec<ActiveNative>,
    pub saved: Vec<SavedNative>,
}

fn bounded_id(value: &Value, label: &str) -> Result<String, String> {
    value
        .as_str()
        .filter(|text| {
            !text.trim().is_empty() && text.len() <= 256 && !text.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| format!("native continuation {label} is invalid"))
}

fn collect(
    journal: &Value,
    stem: &str,
    work: &str,
    pending: &[Value],
    liveness: ProcessLiveness,
    facts: &mut NativeControlFacts,
) -> Result<(), String> {
    if journal["version"] != 1
        || journal["work"] != work
        || !matches!(
            journal["status"].as_str(),
            Some("started" | "running" | "completed" | "not_started")
        )
    {
        return Err("native continuation journal identity or status is invalid".into());
    }
    let assignment = bounded_id(&journal["assignment"], "assignment")?;
    if !assignment
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        || !(stem == assignment || stem == format!("{assignment}-fallback"))
    {
        return Err("native continuation journal filename does not match its assignment".into());
    }
    let role = bounded_id(&journal["role"], "role")?;
    let agent = bounded_id(&journal["agent"], "agent")?;
    if !matches!(role.as_str(), "worker" | "reviewer" | "adviser")
        || journal["stage"]
            .as_u64()
            .filter(|value| *value > 0)
            .is_none()
        || journal["attempt"]
            .as_u64()
            .filter(|value| *value > 0)
            .is_none()
        || !journal["request"].is_object()
    {
        return Err("native continuation journal recipient or request is invalid".into());
    }
    let current = pending.iter().any(|packet| {
        crate::run::assignment::handle(work, packet).ok().as_deref() == Some(assignment.as_str())
            && packet["role"] == role
            && packet["agent"] == agent
            && packet["stage"] == journal["stage"]
            && packet["attempt"] == journal["attempt"]
            && packet["substitution"].is_object() == stem.ends_with("-fallback")
    });
    facts.retirement_blocked |= blocks_retirement(journal, current);
    let continuation_key = journal
        .get("continuationOperation")
        .map(|key| bounded_id(key, "operation"))
        .transpose()?;
    if continuation_key.is_some() {
        bounded_id(&journal["operationId"], "operation identity")?;
    }
    if journal["status"] == "completed" {
        validate_completed_observation(journal)?;
        let bytes = journal_result(journal)?;
        let (outcome, _) = outcome_from_bytes(&bytes)?;
        result::allowed_outcome(&role, &outcome)?;
        if current {
            if let Some(key) = continuation_key.as_ref() {
                facts.saved.push(SavedNative {
                    key: key.clone(),
                    request_sha256: hash::value(&journal["request"]),
                    role: role.clone(),
                    agent: agent.clone(),
                    assignment: assignment.clone(),
                    result_sha256: hash::bytes(&bytes),
                    outcome,
                });
            }
        }
    }
    // A not_started journal holds no provider thread and no effect; the
    // observer recorded the ended process (or that no child was claimed), so
    // it never keeps an unrelated launch waiting.
    if journal["status"] != "not_started" && liveness != ProcessLiveness::Ended {
        facts.all_ended = false;
    }
    if current && liveness == ProcessLiveness::Alive {
        if let Some(key) = continuation_key {
            facts.active.push(ActiveNative {
                key,
                request_sha256: hash::value(&journal["request"]),
                role,
                agent,
                assignment,
                work: work.into(),
            });
        }
    }
    Ok(())
}

/// PID absence is not proof that an owned provider ended. Only a recorded
/// terminal observation or an explicit no-spawn observation establishes it.
fn blocks_retirement(journal: &Value, current: bool) -> bool {
    let process = &journal["observation"]["process"];
    let observed_end = (process["code"].is_i64() || process["signal"].is_i64())
        && process["success"].is_boolean()
        && process["timedOut"].is_boolean()
        && journal["observation"]["interrupted"].is_boolean();
    let observed_no_spawn = journal["controlledInterruption"]["spawned"] == false
        && !journal["processIdentity"].is_object();
    match journal["status"].as_str() {
        Some("running") => true,
        Some("started") => {
            !observed_end || (current && !journal["provisionalFinalResult"].is_null())
        }
        Some("completed") => current,
        Some("not_started") => !observed_end && !observed_no_spawn,
        _ => true,
    }
}

/// No directory, lock, schema or journal is created by this observation.
pub(crate) fn facts(
    loaded: &Loaded,
    work: &str,
    state: &Value,
) -> Result<NativeControlFacts, String> {
    let mut facts = NativeControlFacts {
        all_ended: true,
        retirement_blocked: false,
        active: Vec::new(),
        saved: Vec::new(),
    };
    let Some(paths) = journal_paths_readonly(loaded, work, "observation", false)? else {
        return Ok(facts);
    };
    let directory = paths
        .journal
        .parent()
        .ok_or("native journal directory is missing")?;
    let pending = crate::run::assignment::pending(state);
    let mut entries = 0usize;
    let mut journals = 0usize;
    for entry in fs::read_dir(directory).map_err(|_| "native journal directory is unreadable")? {
        entries += 1;
        if entries > 512 {
            return Err("native journal directory exceeds its bounded entry count".into());
        }
        let entry = entry.map_err(|_| "native journal directory entry is unreadable")?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("native journal name is not UTF-8")?;
        if !name.ends_with(".json") || name.ends_with(".schema.json") {
            continue;
        }
        journals += 1;
        if journals > 128 {
            return Err("native journal count exceeds the 128-operation bound".into());
        }
        let relative = path
            .strip_prefix(&loaded.state_root)
            .map_err(|_| "native journal escapes state root")?
            .to_str()
            .ok_or("native journal path is not UTF-8")?;
        let bytes = match crate::project::path::secure_bytes_observation_single_link_bounded(
            &loaded.state_root,
            relative,
            "native continuation journal",
            MAX_JOURNAL_BYTES,
        ) {
            SecureBytesResult::Bytes(bytes) => bytes,
            _ => return Err("native continuation journal is unavailable or unsafe".into()),
        };
        let journal: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "native continuation journal is malformed")?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("native journal stem is invalid")?;
        collect(
            &journal,
            stem,
            work,
            &pending,
            recovery::liveness(&journal),
            &mut facts,
        )?;
    }
    Ok(facts)
}

pub(super) fn state(loaded: &Loaded, work: &str) -> Result<Value, String> {
    let relative = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &relative)?;
    crate::run::state::reduce(&events)
}

fn saved_recovery_proof(
    journal: &Value,
    work: &str,
    liveness: ProcessLiveness,
) -> Result<Option<(String, Value)>, String> {
    let Some(key) = journal.get("continuationOperation") else {
        return Ok(None);
    };
    let key = bounded_id(key, "operation")?;
    if journal["version"] != 1 || journal["work"] != work || journal["status"] != "completed" {
        return Err("saved native recovery journal does not match its completed Work".into());
    }
    bounded_id(&journal["operationId"], "operation identity")?;
    let role = bounded_id(&journal["role"], "role")?;
    if !journal["request"].is_object() {
        return Err("saved native recovery request is invalid".into());
    }
    validate_completed_observation(journal)?;
    let bytes = journal_result(journal)?;
    let (outcome, _) = outcome_from_bytes(&bytes)?;
    result::allowed_outcome(&role, &outcome)?;
    if liveness != ProcessLiveness::Ended {
        // Exact saved bytes may still be read through the ordinary result
        // owner, but cannot prove that an uncertain reservation has ended.
        return Ok(None);
    }
    Ok(Some((key, journal["request"].clone())))
}

/// Recover only an existing native debit from its exact durable owner result
/// and original process-group end. This neither launches nor reserves work.
pub(super) fn reconcile_saved(loaded: &Loaded, work: &str, journal: &Value) -> Result<(), String> {
    if journal.get("continuationOperation").is_none() {
        return Ok(());
    }
    let state = state(loaded, work)?;
    if state.get("projectContinuation").is_none() {
        return Err("saved native continuation result has no Work association".into());
    }
    if let Some((key, input)) = saved_recovery_proof(journal, work, recovery::liveness(journal))? {
        runtime::recover_observed(loaded, &state, &key, Kind::Native, &input, true)?;
    }
    Ok(())
}

pub(super) fn operation_id(
    state: &Value,
    work: &str,
    assignment: &str,
    request: &codex_exec::Request,
    prior: Option<&Value>,
    relaunch: Option<&str>,
) -> Option<String> {
    state.get("projectContinuation")?;
    Some(format!(
        "native-{}",
        hash::value(&json!({"work":work,"assignment":assignment,
        "request":request_projection(request),"retryOf":prior.map(|value|&value["operation"]),
        "relaunchOf":relaunch}))
    ))
}

pub(super) struct Launch<'a> {
    pub state: &'a Value,
    pub current: &'a Value,
    pub paths: &'a Paths,
    pub request: &'a codex_exec::Request,
    pub prepared_journal: Option<(Value, bool)>,
    pub delivery_proof: &'a Value,
}

/// Persist the native operation under the same Run lock used by retirement.
/// The lock is released before the provider process is launched.
pub(super) fn admit(
    loaded: &Loaded,
    work: &str,
    launch: Launch<'_>,
) -> Result<Option<runtime::Ticket>, String> {
    crate::work::launch_fence::with_live_work(loaded, work, || {
        super::delivery::validate_before_launch(loaded, work, launch.delivery_proof)?;
        admit_locked(loaded, work, launch)
    })
}

fn admit_locked(
    loaded: &Loaded,
    work: &str,
    launch: Launch<'_>,
) -> Result<Option<runtime::Ticket>, String> {
    let Launch {
        state,
        current,
        paths,
        request,
        prepared_journal,
        ..
    } = launch;
    if state.get("projectContinuation").is_none() {
        if let Some((journal, exclusive)) = prepared_journal {
            write_journal(&paths.journal, &journal, exclusive)?;
        }
        claim_started(&paths.journal)?;
        return Ok(None);
    }
    let identity = assignment_identity(current)?;
    let assignment = identity.assignment.as_str();
    let role = current["role"]
        .as_str()
        .ok_or("current native role is missing")?;
    let agent = current["agent"]
        .as_str()
        .ok_or("current native agent is missing")?;
    let fresh = prepared_journal.is_some();
    let (mut journal, exclusive) = match prepared_journal {
        Some((journal, exclusive)) => (journal, exclusive),
        None => (
            read_journal(&paths.journal)?.ok_or("native journal disappeared before admission")?,
            false,
        ),
    };
    if !fresh && recovery::liveness(&journal) != ProcessLiveness::Ended {
        return Err("associated native operation is unresolved; inspect its original identity without relaunch".into());
    }
    let operation = bounded_id(&journal["operationId"], "operation identity")?;
    let input = request_projection(request);
    let key = hash::value(
        &json!({"work":work,"assignment":assignment,"operation":operation,"request":input}),
    );
    let ticket = runtime::begin(
        loaded,
        state,
        runtime::Operation {
            kind: Kind::Native,
            key: &key,
            role,
            agent: Some(agent),
            paths: &[],
            commands: &[],
            input: &input,
            predecessor: None,
        },
    )?;
    if ticket.is_none() {
        return Err("associated native admission returned no verified runtime ticket".into());
    }
    if let Some(bound) = ticket.as_ref() {
        journal["continuationOperation"] = json!(bound.key());
        journal["request"] = input;
        if let Err(error) = write_journal(&paths.journal, &journal, exclusive) {
            runtime::complete(loaded, &ticket, false, true)?;
            return Err(error);
        }
    }
    if let Err(error) = claim_started(&paths.journal) {
        runtime::complete(loaded, &ticket, false, true)?;
        return Err(error);
    }
    Ok(ticket)
}

pub(super) fn execute(
    loaded: &Loaded,
    work: &str,
    paths: &Paths,
    request: &codex_exec::Request,
    ticket: Option<runtime::Ticket>,
) -> Result<(codex_exec::Observation, Option<runtime::Ticket>), String> {
    let result = if ticket.is_some() {
        let mut last_guard = None;
        codex_exec::run_with_control(
            request,
            |identity| recovery::persist_identity(&paths.journal, identity),
            |stream| recovery::persist_stream(&paths.journal, stream),
            || {
                if last_guard.map_or(true, |last: Instant| {
                    last.elapsed() >= Duration::from_millis(250)
                }) {
                    runtime::guard_work(loaded, work)?;
                    last_guard = Some(Instant::now());
                }
                Ok(())
            },
        )
    } else {
        codex_exec::run_with_observers(
            request,
            |identity| recovery::persist_identity(&paths.journal, identity),
            |stream| recovery::persist_stream(&paths.journal, stream),
        )
    };
    match result {
        Err(error) => {
            if let Some(facts) = error.interruption_facts() {
                let mut journal = read_journal(&paths.journal)?
                    .ok_or("native journal disappeared during interruption")?;
                journal["controlledInterruption"] = facts;
                write_journal(&paths.journal, &journal, false)?;
            }
            record_error(&paths.journal, &error.to_string())?;
            let journal =
                read_journal(&paths.journal)?.ok_or("native journal disappeared after failure")?;
            let ended = recovery::liveness(&journal) == ProcessLiveness::Ended
                || (journal["controlledInterruption"]["spawned"] == false
                    && !journal["processIdentity"].is_object());
            if let Err(accounting) = complete_owned(loaded, paths, &ticket, false, ended) {
                return Err(format!(
                    "{error}; native accounting recovery pending: {accounting}"
                ));
            }
            Err(error.to_string())
        }
        // The caller must retain the full observation and exact accepted
        // result in the native owner's journal before aggregate observation.
        Ok(observation) => Ok((observation, ticket)),
    }
}

fn observe_account<F>(
    path: &Path,
    key: &str,
    success: bool,
    known_ended: bool,
    observe: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    let mut journal = read_journal(path)?.ok_or("native journal disappeared before accounting")?;
    journal["accountingRecovery"] = json!({"version":1,"operation":key,"status":"pending",
        "success":success,"knownEnded":known_ended});
    write_journal(path, &journal, false)?;
    observe()?;
    journal["accountingRecovery"]["status"] = json!("observed");
    write_journal(path, &journal, false)
}

pub(super) fn complete_owned(
    loaded: &Loaded,
    paths: &Paths,
    ticket: &Option<runtime::Ticket>,
    success: bool,
    known_ended: bool,
) -> Result<(), String> {
    let Some(bound) = ticket.as_ref() else {
        return Ok(());
    };
    observe_account(&paths.journal, bound.key(), success, known_ended, || {
        runtime::complete(loaded, ticket, success, known_ended)
    })
}

#[cfg(test)]
#[path = "continuation/tests.rs"]
mod tests;

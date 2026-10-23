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
            Some("started" | "running" | "completed")
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
    if liveness != ProcessLiveness::Ended {
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

/// No directory, lock, schema or journal is created by this observation.
pub(crate) fn facts(
    loaded: &Loaded,
    work: &str,
    state: &Value,
) -> Result<NativeControlFacts, String> {
    let mut facts = NativeControlFacts {
        all_ended: true,
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
) -> Option<String> {
    state.get("projectContinuation")?;
    Some(format!(
        "native-{}",
        hash::value(&json!({"work":work,"assignment":assignment,
        "request":request_projection(request),"retryOf":prior.map(|value|&value["operation"])}))
    ))
}

pub(super) fn execute(
    loaded: &Loaded,
    work: &str,
    state: &Value,
    current: &Value,
    paths: &Paths,
    request: &codex_exec::Request,
    prepared_journal: Option<(Value, bool)>,
) -> Result<(codex_exec::Observation, Option<runtime::Ticket>), String> {
    if state.get("projectContinuation").is_none() {
        if let Some((journal, exclusive)) = prepared_journal {
            write_journal(&paths.journal, &journal, exclusive)?;
        }
        claim_started(&paths.journal)?;
        return match codex_exec::run_with_observers(
            request,
            |identity| recovery::persist_identity(&paths.journal, identity),
            |stream| recovery::persist_stream(&paths.journal, stream),
        ) {
            Ok(observation) => Ok((observation, None)),
            Err(error) => {
                record_error(&paths.journal, &error.to_string())?;
                Err(error.to_string())
            }
        };
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
mod tests {
    use super::*;

    fn pending() -> Value {
        json!({"stage":2,"attempt":1,"agent":"worker","role":"worker"})
    }

    fn journal(packet: &Value) -> Value {
        json!({"version":1,"work":"fixture-work","status":"running",
            "assignment":crate::run::assignment::handle("fixture-work",packet).unwrap(),
            "role":"worker","agent":"worker","stage":2,"attempt":1,
            "operationId":"native-fixture-operation",
            "request":{"executable":"/fixture/native","persistSession":true},
            "continuationOperation":"untrusted-until-account-verified"})
    }

    fn empty() -> NativeControlFacts {
        NativeControlFacts {
            all_ended: true,
            active: Vec::new(),
            saved: Vec::new(),
        }
    }

    fn observe(
        journal: &Value,
        pending: &[Value],
        liveness: ProcessLiveness,
    ) -> Result<NativeControlFacts, String> {
        let mut facts = empty();
        collect(
            journal,
            journal["assignment"].as_str().unwrap(),
            "fixture-work",
            pending,
            liveness,
            &mut facts,
        )?;
        Ok(facts)
    }

    #[test]
    fn active_facts_require_current_pending_owner_and_observed_alive_identity() {
        let packet = pending();
        let journal = journal(&packet);
        let active = observe(&journal, &[packet.clone()], ProcessLiveness::Alive).unwrap();
        assert!(!active.all_ended);
        assert_eq!(active.active.len(), 1);
        assert_eq!(active.active[0].key, "untrusted-until-account-verified");
        assert_eq!(
            active.active[0].request_sha256,
            hash::value(&journal["request"])
        );
        assert_eq!(active.active[0].work, "fixture-work");
        for field in ["stage", "attempt", "agent", "role"] {
            let mut other = packet.clone();
            other[field] = match field {
                "stage" | "attempt" => json!(3),
                _ => json!("another"),
            };
            let stale = observe(&journal, &[other], ProcessLiveness::Alive).unwrap();
            assert!(!stale.all_ended);
            assert!(stale.active.is_empty(), "{field}");
        }
        let uncertain = observe(&journal, &[packet.clone()], ProcessLiveness::Uncertain).unwrap();
        assert!(!uncertain.all_ended);
        assert!(uncertain.active.is_empty());
        let ended = observe(&journal, &[packet], ProcessLiveness::Ended).unwrap();
        assert!(ended.all_ended);
        assert!(ended.active.is_empty());
    }

    fn completed(packet: &Value) -> (Value, Vec<u8>) {
        let mut journal = journal(packet);
        let bytes = serde_json::to_vec(
            &json!({"outcome":"completed","summary":"bounded fixture","reason":""}),
        )
        .unwrap();
        journal["status"] = json!("completed");
        journal["result"] = json!(hex_encode(&bytes));
        journal["observation"] = json!({"process":{"success":true,"code":0,"signal":null,"timedOut":false},
            "turn":"completed","interrupted":false,"coverageGaps":[]});
        (journal, bytes)
    }

    #[test]
    fn saved_facts_bind_exact_bytes_outcome_and_current_assignment_without_reexecution() {
        let packet = pending();
        let (journal, bytes) = completed(&packet);
        let facts = observe(&journal, &[packet.clone()], ProcessLiveness::Ended).unwrap();
        assert!(facts.all_ended);
        assert!(facts.active.is_empty());
        assert_eq!(facts.saved.len(), 1);
        let saved = &facts.saved[0];
        assert_eq!(saved.result_sha256, hash::bytes(&bytes));
        assert_eq!(saved.request_sha256, hash::value(&journal["request"]));
        assert_eq!(saved.outcome, "completed");
        assert_eq!(saved.agent, "worker");
        assert_eq!(saved.assignment, journal["assignment"].as_str().unwrap());
        let mut next = packet;
        next["attempt"] = json!(2);
        assert!(observe(&journal, &[next], ProcessLiveness::Ended)
            .unwrap()
            .saved
            .is_empty());
    }

    #[test]
    fn saved_debit_recovery_requires_exact_completed_result_and_known_group_end() {
        let (journal, _) = completed(&pending());
        let proof = saved_recovery_proof(&journal, "fixture-work", ProcessLiveness::Ended)
            .unwrap()
            .unwrap();
        assert_eq!(proof.0, "untrusted-until-account-verified");
        assert_eq!(proof.1, journal["request"]);
        for liveness in [ProcessLiveness::Alive, ProcessLiveness::Uncertain] {
            assert!(saved_recovery_proof(&journal, "fixture-work", liveness)
                .unwrap()
                .is_none());
        }
        for (field, value) in [
            ("version", json!(2)),
            ("work", json!("other-work")),
            ("status", json!("running")),
            ("operationId", Value::Null),
            ("role", json!("lead")),
            ("request", Value::Null),
            ("result", json!("not-hex")),
            ("observation", Value::Null),
        ] {
            let mut invalid = journal.clone();
            invalid[field] = value;
            assert!(
                saved_recovery_proof(&invalid, "fixture-work", ProcessLiveness::Ended).is_err(),
                "{field}"
            );
        }
        let mut ordinary = journal;
        ordinary
            .as_object_mut()
            .unwrap()
            .remove("continuationOperation");
        assert!(
            saved_recovery_proof(&ordinary, "fixture-work", ProcessLiveness::Uncertain)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn live_agent_a_does_not_create_native_facts_for_pending_agent_b_of_same_role() {
        let packet_a = pending();
        let mut packet_b = packet_a.clone();
        packet_b["agent"] = json!("worker-b");
        let journal_a = journal(&packet_a);
        let facts = observe(
            &journal_a,
            &[packet_a.clone(), packet_b.clone()],
            ProcessLiveness::Alive,
        )
        .unwrap();
        assert_eq!(facts.active.len(), 1);
        assert_eq!(facts.active[0].agent, "worker");
        assert_eq!(facts.active[0].role, "worker");
        assert_eq!(
            facts.active[0].assignment,
            crate::run::assignment::handle("fixture-work", &packet_a).unwrap()
        );
        assert_ne!(
            facts.active[0].assignment,
            crate::run::assignment::handle("fixture-work", &packet_b).unwrap()
        );
        let only_b = observe(&journal_a, &[packet_b], ProcessLiveness::Alive).unwrap();
        assert!(only_b.active.is_empty());
        assert!(!only_b.all_ended);
    }

    #[test]
    fn aggregate_failure_preserves_exact_owner_result_and_pending_recovery() {
        let directory = crate::test_root::temp("native-account-fault");
        let path = directory.join("native.json");
        let (journal, bytes) = completed(&pending());
        write_journal(&path, &journal, true).unwrap();
        let error = observe_account(&path, "actual-reservation", true, true, || {
            let retained = read_journal(&path).unwrap().unwrap();
            assert_eq!(retained["status"], "completed");
            assert_eq!(journal_result(&retained).unwrap(), bytes);
            assert_eq!(retained["observation"], journal["observation"]);
            assert_eq!(retained["accountingRecovery"]["status"], "pending");
            Err("injected aggregate append failure".into())
        })
        .unwrap_err();
        assert_eq!(error, "injected aggregate append failure");
        let retained = read_journal(&path).unwrap().unwrap();
        assert_eq!(journal_result(&retained).unwrap(), bytes);
        assert_eq!(retained["observation"], journal["observation"]);
        assert_eq!(retained["accountingRecovery"]["status"], "pending");
        assert_eq!(retained["accountingRecovery"]["knownEnded"], true);

        observe_account(&path, "actual-reservation", true, true, || Ok(())).unwrap();
        assert_eq!(
            read_journal(&path).unwrap().unwrap()["accountingRecovery"]["status"],
            "observed"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn aggregate_failure_keeps_owned_interruption_and_unknown_end() {
        let directory = crate::test_root::temp("native-interrupt-fault");
        let path = directory.join("native.json");
        let mut journal = journal(&pending());
        journal["error"] = json!("controlled eligibility hold");
        journal["controlledInterruption"] = json!({"version":1,"rootReaped":true,
            "wholeGroupEnded":null,"ownedRemainingLiveness":"uncertain"});
        write_journal(&path, &journal, true).unwrap();
        assert!(
            observe_account(&path, "actual-reservation", false, false, || {
                Err("injected aggregate append failure".into())
            })
            .is_err()
        );
        let retained = read_journal(&path).unwrap().unwrap();
        assert_eq!(retained["error"], journal["error"]);
        assert_eq!(
            retained["controlledInterruption"],
            journal["controlledInterruption"]
        );
        assert_eq!(retained["accountingRecovery"]["status"], "pending");
        assert_eq!(retained["accountingRecovery"]["knownEnded"], false);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_or_unverified_completed_journals_are_holds() {
        let packet = pending();
        let (journal, _) = completed(&packet);
        for (field, value) in [
            ("version", json!(2)),
            ("work", json!("other-work")),
            ("status", json!("assumed_done")),
            ("request", Value::Null),
            ("result", json!("not-hex")),
        ] {
            let mut invalid = journal.clone();
            invalid[field] = value;
            assert!(
                observe(&invalid, &[packet.clone()], ProcessLiveness::Ended).is_err(),
                "{field}"
            );
        }
        let mut failed = journal;
        failed["observation"]["process"]["success"] = json!(false);
        assert!(observe(&failed, &[packet], ProcessLiveness::Ended).is_err());
    }
}

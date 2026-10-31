//! Bind new pre-return admissions to their owning run before execution.

use super::*;

pub(crate) const FIELD: &str = "preReturnAdmissionProtocol";
pub(crate) const ACTION: &str = "pre_return_admitted";
const PROTOCOL: u64 = 1;
const ASSIGNMENT_FIELDS: &[&str] = &["runId", "ledger", "stage", "attempt", "agent", "role"];

fn sha(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn exact(value: &Value, fields: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field))
    })
}

fn assignment(binding: &Value) -> Value {
    ASSIGNMENT_FIELDS
        .iter()
        .filter_map(|field| {
            binding
                .get(*field)
                .map(|value| ((*field).to_owned(), value.clone()))
        })
        .collect::<serde_json::Map<String, Value>>()
        .into()
}

pub(crate) fn add_start_marker(start: &mut Value) {
    start[FIELD] = json!(PROTOCOL);
}

pub(crate) fn validate_start(start: &Value, line: usize) -> Result<(), String> {
    if let Some(marker) = start.get(FIELD) {
        if start["version"] != 8 || marker != PROTOCOL || start.get("preReturnPolicy").is_none() {
            return Err(format!(
                "invalid run ledger line {line}: pre-return admission protocol requires a supported checked start"
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate(event: &Value) -> Result<(), String> {
    let admission = &event["admission"];
    let identity = &admission["assignment"];
    if event["version"] != 8
        || event["action"] != ACTION
        || !exact(
            admission,
            &[
                "attemptId",
                "recordSha256",
                "assignment",
                "ownRepairEventSha256",
            ],
        )
        || !sha(&admission["attemptId"])
        || !sha(&admission["recordSha256"])
        || !exact(identity, ASSIGNMENT_FIELDS)
        || !sha(&identity["runId"])
        || identity["runId"] != event["runId"]
        || ["ledger", "agent"].iter().any(|field| {
            !identity[*field]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        })
        || ["stage", "attempt"]
            .iter()
            .any(|field| identity[*field].as_u64().is_none())
        || identity["role"] != "worker"
        || (!admission["ownRepairEventSha256"].is_null()
            && (!sha(&admission["ownRepairEventSha256"])
                || admission["ownRepairEventSha256"] != event["previousEventSha256"]))
    {
        return Err("invalid pre-return admission ownership".into());
    }
    Ok(())
}

pub(crate) fn validate_location(
    event: &Value,
    path: &run_ledger::LedgerPath,
) -> Result<(), String> {
    let ledger = path
        .path
        .strip_prefix(&path.root)
        .map_err(|_| "ledger is outside the canonical state root")?
        .to_str()
        .ok_or("ledger path is not valid UTF-8")?
        .replace('\\', "/");
    if event["admission"]["assignment"]["ledger"] != ledger {
        return Err("pre-return admission names a different ledger".into());
    }
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    validate(event)?;
    let admission = &event["admission"];
    let identity = &admission["assignment"];
    let pending = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| {
            ["stage", "attempt", "agent", "role"]
                .iter()
                .all(|field| item[*field] == identity[*field])
        });
    let occurrences = state["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|candidate| {
            candidate["action"] == ACTION
                && candidate["admission"]["attemptId"] == admission["attemptId"]
        })
        .count();
    let linked_repair = if admission["ownRepairEventSha256"].is_null() {
        true
    } else {
        let handle_sha = pending.as_ref().and_then(|assignment| {
            let work = crate::work::locator_for_ledger(identity["ledger"].as_str()?)?;
            let handle = crate::run::assignment::handle(&work, assignment).ok()?;
            Some(crate::evidence::hash::text(&handle))
        });
        state["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .position(|item| item["eventSha256"] == event["eventSha256"])
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| events.get(index))
                .is_some_and(|repair| {
                    repair["eventSha256"] == admission["ownRepairEventSha256"]
                        && repair["action"] == "govern"
                        && repair["operation"] == "authorized_repair_recovery_v1"
                        && handle_sha
                            .as_deref()
                            .is_some_and(|sha| repair["assignmentSha256"].as_str() == Some(sha))
                        && pending.as_ref().is_some_and(|assignment| {
                            repair["governorEvent"]["assignmentPacketSha256"]
                                == crate::evidence::hash::value(assignment)
                        })
                        && ["runId", "stage", "attempt", "agent", "role"]
                            .iter()
                            .all(|field| repair[*field] == identity[*field])
                })
        })
    };
    if state[FIELD] != PROTOCOL
        || state["status"] != "running"
        || state.get("preReturnPolicy").is_none()
        || identity["runId"] != state["runId"]
        || pending.is_none()
        || !linked_repair
        || occurrences != 1
    {
        return Err("pre-return admission is not bound to the current assignment".into());
    }
    Ok(())
}

/// The attempt file and the ledger event are durable before the caller may
/// launch the checker. The ledger is already locked by the caller.
pub(super) fn admit(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    state: &Value,
    attempt: &Value,
    own_repair: Option<&str>,
) -> Result<Option<String>, String> {
    if state.get(FIELD).is_none() {
        return Ok(None);
    }
    let (_, events, source) = load_at(loaded, path)?;
    let last = events.last().ok_or("run ledger has no head event")?;
    if state["events"].as_array().and_then(|events| events.last()) != Some(last) {
        return Err("run ledger changed before pre-return admission".into());
    }
    let event = run_state::make_event(json!({"version":8,"kind":"run",
        "producer":crate::producer::evidence_for_version(8),"action":ACTION,
        "runId":state["runId"],
        "admission":{"attemptId":attempt["attemptId"],"recordSha256":attempt["recordSha256"],
            "ownRepairEventSha256":own_repair,
            "assignment":assignment(&attempt["binding"])},
        "previousEventSha256":last["eventSha256"],"timestamp":nondecreasing(&last["timestamp"])?}));
    validate_location(&event, path)?;
    let mut projected = events;
    projected.push(event.clone());
    run_state::reduce(&projected)?;
    append(path, &event, false, &source)?;
    Ok(event["eventSha256"].as_str().map(str::to_owned))
}

pub(super) fn events<'a>(state: &'a Value, identity: Option<&Value>) -> Vec<&'a Value> {
    state["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| {
            event["action"] == ACTION
                && identity.map_or(true, |binding| {
                    event["admission"]["assignment"] == assignment(binding)
                })
        })
        .collect()
}

pub(super) fn read_owned(loaded: &Loaded, event: &Value) -> Result<Value, String> {
    let admission = &event["admission"];
    let id = admission["attemptId"]
        .as_str()
        .ok_or("missing pre-return admission identity")?;
    let record = read_record(&record_path(loaded, id, "attempt")?)?;
    if record["attemptId"] != id
        || record["recordSha256"] != admission["recordSha256"]
        || assignment(&record["binding"]) != admission["assignment"]
    {
        return Err("pre-return admission differs from its owning ledger".into());
    }
    Ok(record)
}

/// Return the exact pre-admission ledger prefix for an already saved result.
/// A different result, owner, unsettled diagnostic or intervening event cannot
/// be used to reconstruct a native assignment packet.
pub(crate) fn saved_result_prefix(
    loaded: &Loaded,
    work: &str,
    assignment_handle: &str,
    result_bytes: &[u8],
) -> Result<Option<(Vec<Value>, String)>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (path, events, source) = load(loaded, &ledger)?;
    let state = reduce_live(loaded, &events)?;
    if state.get(FIELD).is_none() {
        return Ok(None);
    }
    let Some(event) = events.last().filter(|event| event["action"] == ACTION) else {
        return Ok(None);
    };
    let Some(current) = crate::run::assignment::pending(&state)
        .into_iter()
        .find(|item| {
            crate::run::assignment::handle(work, item).ok().as_deref() == Some(assignment_handle)
        })
    else {
        return Ok(None);
    };
    let identity = &event["admission"]["assignment"];
    if identity["runId"] != state["runId"]
        || identity["ledger"] != ledger
        || ["stage", "attempt", "agent", "role"]
            .iter()
            .any(|field| identity[*field] != current[*field])
    {
        return Ok(None);
    }
    let id = event["admission"]["attemptId"]
        .as_str()
        .ok_or("missing pre-return admission identity")?;
    let attempt = super::predecessor::read_attempt(loaded, &state, id)?;
    super::assert_current_configuration(loaded, &state)?;
    crate::run::artifact::assert_current(loaded, &state)?;
    let policy = crate::run_value::pre_return_policy_from_value(&state["preReturnPolicy"], 1)?;
    let assignment_sha = crate::evidence::hash::text(assignment_handle);
    let current_binding = super::current_binding(
        loaded,
        &path,
        &events,
        &state,
        &current,
        Some(&assignment_sha),
        &json!({"sha256":crate::evidence::hash::bytes(result_bytes)}),
        &policy,
    )?;
    if attempt["binding"] != current_binding || attempt["policy"] != policy.value() {
        return Ok(None);
    }
    let result = read_record(&record_path(loaded, id, "result")?)?;
    if !super::predecessor::validate_saved(loaded, &attempt, &result)? {
        return Err("saved pre-return result still has an uncertain outcome".into());
    }
    let event_sha = event["eventSha256"]
        .as_str()
        .ok_or("missing admission event identity")?;
    let repair = event["admission"]["ownRepairEventSha256"].as_str();
    let prefix = super::super::repair_recovery::own_event_prefix(&events, repair, Some(event_sha))
        .ok_or("pre-return admission suffix is not current")?;
    let source = source
        .split_inclusive('\n')
        .take(prefix)
        .collect::<String>();
    Ok(Some((events[..prefix].to_vec(), source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_repair_requires_the_exact_adjacent_assignment() {
        let run_id = "1".repeat(64);
        let work = format!("smw_{run_id}");
        let ledger = format!(
            "{}/runs/work-{run_id}.jsonl",
            crate::project::layout_types::state_namespace()
        );
        let mut state = json!({"runId":run_id,"status":"running","currentStage":1,"attempt":1,
            "plan":{"maxParallel":1,"stages":[{"stage":1,"agents":[{"name":"worker","role":"worker"}]}]},
            "submissions":[],"preReturnPolicy":{},"preReturnAdmissionProtocol":1});
        let current = crate::run::assignment::pending(&state).remove(0);
        let handle = crate::run::assignment::handle(&work, &current).unwrap();
        let repair = json!({"eventSha256":"2".repeat(64),"action":"govern",
            "operation":"authorized_repair_recovery_v1","runId":run_id,
            "stage":1,"attempt":1,"agent":"worker","role":"worker",
            "assignmentSha256":crate::evidence::hash::text(&handle),
            "governorEvent":{"assignmentPacketSha256":crate::evidence::hash::value(&current)}});
        let event = json!({"version":8,"action":ACTION,"runId":run_id,
            "eventSha256":"3".repeat(64),"previousEventSha256":repair["eventSha256"],
            "admission":{"attemptId":"4".repeat(64),"recordSha256":"5".repeat(64),
                "ownRepairEventSha256":repair["eventSha256"],
                "assignment":{"runId":run_id,"ledger":ledger,"stage":1,"attempt":1,"agent":"worker","role":"worker"}}});
        state["events"] = json!([repair, event]);
        let candidate = state["events"][1].clone();
        assert!(apply(&mut state, &candidate).is_ok());

        for field in ["operation", "assignmentSha256", "packet", "intervening"] {
            let mut invalid = state.clone();
            match field {
                "operation" => invalid["events"][0]["operation"] = json!("foreign"),
                "assignmentSha256" => {
                    invalid["events"][0]["assignmentSha256"] = json!("6".repeat(64))
                }
                "packet" => {
                    invalid["events"][0]["governorEvent"]["assignmentPacketSha256"] =
                        json!("7".repeat(64))
                }
                "intervening" => invalid["events"]
                    .as_array_mut()
                    .unwrap()
                    .insert(1, json!({"eventSha256":"8".repeat(64)})),
                _ => unreachable!(),
            }
            let last = invalid["events"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .clone();
            assert!(apply(&mut invalid, &last).is_err(), "{field}");
        }
    }
}

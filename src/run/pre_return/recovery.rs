//! Ledger-bound reconciliation of an interrupted pre-return admission.
//!
//! This records a reported Lead decision, not a checker result. The retained
//! predecessor stays immutable and only one subsequent generation is allowed.
use super::*;
pub(crate) mod evidence;

pub(crate) const ACTION: &str = "pre_return_reconciled";

pub(crate) fn observer_evidence(loaded: &Loaded, attempt: &Value) -> Result<Value, String> {
    let id = attempt["attemptId"]
        .as_str()
        .ok_or("missing pre-return attempt identity")?;
    let facts = crate::run::capture_supervisor::durable_evidence(
        &super::journal::observer_path(loaded, id)?,
        id,
    )?;
    Ok(
        json!({"type":"owned_observer_v1","attemptId":id,"facts":facts,
        "provenance":"product_owned_observer"}),
    )
}

fn sha(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

fn nonblank(value: &Value, maximum: usize) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.len() <= maximum && !s.contains('\0'))
}

fn exact(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

/// The uniquely configured coordinator frozen in the run's plan. This names
/// the reported decision actor; it is not OS authentication of the caller.
pub(crate) fn configured_lead(state: &Value) -> Result<Value, String> {
    let owners = state["plan"]["stages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|stage| stage["agents"].as_array().into_iter().flatten())
        .filter(|agent| agent["role"] == "lead")
        .filter_map(|agent| Some((agent["name"].as_str()?, agent["nativeTaskName"].as_str()?)))
        .collect::<std::collections::BTreeSet<_>>();
    if owners.len() != 1 {
        return Err("pre-return recovery needs one configured Lead".into());
    }
    let &(agent, native) = owners.iter().next().ok_or("configured Lead is missing")?;
    if agent.trim().is_empty() || native.trim().is_empty() {
        return Err("configured Lead identity is incomplete".into());
    }
    Ok(json!({"agent":agent,"nativeTaskName":native,"authority":"caller_reported"}))
}

/// Stable components must match even though a repaired producer and the new
/// exclusive scratch generation necessarily differ.
const SUBJECT_FIELDS: &[&str] = &[
    "work",
    "ledger",
    "selector",
    "runId",
    "stage",
    "attempt",
    "agent",
    "role",
    "assignmentSha256",
    "resultSha256",
    "inputsSha256",
    "subjectSha256",
    "configSha256",
    "policySha256",
    "grantSetSha256",
];

fn same_subject(old: &Value, current: &Value) -> bool {
    SUBJECT_FIELDS
        .iter()
        .all(|field| old.get(*field).is_some() && old[*field] == current[*field])
}

fn subject(binding: &Value) -> Value {
    SUBJECT_FIELDS
        .iter()
        .filter_map(|field| {
            binding
                .get(*field)
                .map(|value| ((*field).to_owned(), value.clone()))
        })
        .collect::<serde_json::Map<String, Value>>()
        .into()
}

pub(crate) fn predecessor_refusal(
    loaded: &Loaded,
    work: &str,
    bound: &Value,
    current: &Value,
) -> String {
    crate::drift::refusal(
        crate::drift::Class::PreReturnRecoveryPredecessor,
        &crate::evidence::hash::value(bound),
        Some(&crate::evidence::hash::value(current)),
        None,
        crate::drift::Route::WorkDetail {
            work,
            config: &loaded.path,
        },
    )
}

pub(crate) fn for_assignment<'a>(
    state: &'a Value,
    assignment: &Value,
) -> Result<Option<&'a Value>, String> {
    let identity = crate::evidence::hash::value(assignment);
    let matches = state["preReturnRecoveries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["recovery"]["snapshot"]["assignmentSha256"] == identity)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [event] => Ok(Some(*event)),
        _ => Err("multiple pre-return recoveries name the same assignment".into()),
    }
}

pub(super) fn permits_predecessor(
    loaded: &Loaded,
    state: &Value,
    binding: &Value,
    attempt: &Value,
) -> Result<bool, String> {
    let Some(event) = state["preReturnRecoveries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|event| event["eventSha256"] == binding["preReturnReconciliationSha256"])
    else {
        return Ok(false);
    };
    let snapshot = &event["recovery"]["snapshot"];
    if snapshot["predecessor"] != *attempt
        || !same_subject(&attempt["binding"], binding)
        || snapshot["newProducer"]["executableSha256"] != binding["checkerExecutableSha256"]
        || snapshot["newProducer"]["producer"] != binding["producer"]
    {
        let work = binding["work"].as_str().ok_or("missing Work identity")?;
        let bound = json!({"predecessor":snapshot["predecessor"],
            "subject":subject(&snapshot["predecessor"]["binding"]),
            "producer":snapshot["newProducer"]});
        let current = json!({"predecessor":attempt,"subject":subject(binding),
            "producer":{"executableSha256":binding["checkerExecutableSha256"],
                "producer":binding["producer"]}});
        return Err(predecessor_refusal(loaded, work, &bound, &current));
    }
    Ok(true)
}

pub(super) fn assert_predecessor_unchanged(
    loaded: &Loaded,
    state: &Value,
    binding: &Value,
) -> Result<(), String> {
    if binding.get("preReturnReconciliationSha256").is_none() {
        return Ok(());
    }
    let event = state["preReturnRecoveries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|event| event["eventSha256"] == binding["preReturnReconciliationSha256"])
        .ok_or("pre-return reconciliation is unavailable")?;
    let predecessor = &event["recovery"]["snapshot"]["predecessor"];
    let work = binding["work"].as_str().ok_or("missing Work identity")?;
    evidence::verify_saved(loaded, work, &event["recovery"]["snapshot"]["evidence"])?;
    let id = predecessor["attemptId"]
        .as_str()
        .ok_or("missing predecessor identity")?;
    let current = read_record(&record_path(loaded, id, "attempt")?)?;
    if current != *predecessor {
        return Err(predecessor_refusal(loaded, work, predecessor, &current));
    }
    if !permits_predecessor(loaded, state, binding, predecessor)? {
        return Err("pre-return reconciliation is unavailable".into());
    }
    match fs::symlink_metadata(record_path(loaded, id, "result")?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(
            "pre-return predecessor acquired a result after reconciliation; no retry is admitted"
                .into(),
        ),
        Err(error) => Err(format!(
            "pre-return predecessor result is unavailable: {error}"
        )),
    }
}

pub(crate) fn pending(
    loaded: &Loaded,
    ledger: &str,
    state: &Value,
    assignment: &Value,
) -> Result<Vec<Value>, String> {
    if state.get("preReturnPolicy").is_none() || assignment["role"] != "worker" {
        return Ok(Vec::new());
    }
    super::predecessor::unresolved(
        loaded,
        &json!({"runId":state["runId"],"ledger":ledger,
        "stage":assignment["stage"],"attempt":assignment["attempt"],
        "agent":assignment["agent"],"role":assignment["role"]}),
        state,
    )
}

/// Reconstruct the current immutable binding rather than accepting caller
/// versions of assignment, inputs, policy, grant set, or retained result.
pub(crate) fn candidate(
    loaded: &Loaded,
    path: &run_ledger::LedgerPath,
    events: &[Value],
    state: &Value,
    assignment: &Value,
    assignment_handle: &str,
    result_sha256: &str,
) -> Result<(Value, Value), String> {
    if state.get("projectContinuation").is_some() {
        return Err("pre-return recovery is unavailable for continuation-managed Work".into());
    }
    if state["verificationPreparationProtocol"] != 1 {
        return Err("pre-return recovery requires previously selected owned verification preparation; no fresh scratch can be promised without changing the frozen environment".into());
    }
    if for_assignment(state, assignment)?.is_some() {
        return Err("this assignment already used its one pre-return reconciliation".into());
    }
    super::assert_current_configuration(loaded, state)?;
    crate::run::artifact::assert_current(loaded, state)?;
    let policy = crate::run_value::pre_return_policy_from_value(&state["preReturnPolicy"], 1)?;
    let binding = super::current_binding(
        loaded,
        path,
        events,
        state,
        assignment,
        Some(&crate::evidence::hash::text(assignment_handle)),
        &json!({"sha256":result_sha256}),
        &policy,
    )?;
    let candidates = super::predecessor::unresolved(loaded, &binding, state)?;
    let [attempt] = candidates.as_slice() else {
        return Err(
            "pre-return recovery requires one unique unfinished admission for this assignment"
                .into(),
        );
    };
    let id = attempt["attemptId"]
        .as_str()
        .ok_or("missing predecessor identity")?;
    match fs::symlink_metadata(record_path(loaded, id, "result")?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err("this route only reconciles an admission with no result".into()),
        Err(error) => return Err(error.to_string()),
    }
    if !same_subject(&attempt["binding"], &binding) || attempt["policy"] != policy.value() {
        let work = binding["work"].as_str().ok_or("missing Work identity")?;
        let bound = json!({"subject":subject(&attempt["binding"]),"policy":attempt["policy"]});
        let current = json!({"subject":subject(&binding),"policy":policy.value()});
        return Err(predecessor_refusal(loaded, work, &bound, &current));
    }
    Ok((binding, attempt.clone()))
}

pub(crate) fn request_binding(snapshot: &Value) -> String {
    crate::evidence::hash::value(&json!({"domain":"pre_return_recovery_v1","snapshot":snapshot}))
}

pub(crate) fn event(
    state: &Value,
    last: &Value,
    snapshot: Value,
    decision: Value,
) -> Result<Value, String> {
    let binding = request_binding(&snapshot);
    Ok(run_state::make_event(json!({"version":8,"kind":"run",
        "producer":crate::producer::evidence_for_version(8),"action":ACTION,"runId":state["runId"],
        "recovery":{"version":1,"snapshot":snapshot,"decision":decision,"currentBinding":binding},
        "previousEventSha256":last["eventSha256"],"timestamp":nondecreasing(&last["timestamp"])?})))
}

pub(crate) fn validate(event: &Value) -> Result<(), String> {
    let recovery = &event["recovery"];
    let snapshot = &recovery["snapshot"];
    let decision = &recovery["decision"];
    let predecessor = &snapshot["predecessor"];
    let mut unsigned = predecessor.clone();
    if let Some(object) = unsigned.as_object_mut() {
        object.remove("recordSha256");
    }
    let work_suffix = snapshot["work"]
        .as_str()
        .and_then(|work| work.strip_prefix("smw_"))
        .unwrap_or("");
    let expected_held = format!(
        "hld_{}_{}_{}",
        work_suffix,
        crate::evidence::hash::text(snapshot["assignment"].as_str().unwrap_or("")),
        snapshot["held"]["sha256"].as_str().unwrap_or("")
    );
    if event["version"] != 8
        || event["action"] != ACTION
        || !exact(
            recovery,
            &["version", "snapshot", "decision", "currentBinding"],
        )
        || recovery["version"] != 1
        || !exact(
            snapshot,
            &[
                "work",
                "assignment",
                "assignmentSha256",
                "predecessor",
                "held",
                "ledgerSourceSha256",
                "governorSha256",
                "lead",
                "newProducer",
                "evidence",
                "nativeExit",
                "capture",
                "checkCredit",
            ],
        )
        || !exact(
            decision,
            &[
                "version",
                "work",
                "agent",
                "authority",
                "approved",
                "reason",
                "executionDomain",
                "effects",
                "effectsReason",
                "currentBinding",
            ],
        )
        || !exact(
            predecessor,
            &[
                "version",
                "kind",
                "attemptId",
                "binding",
                "policy",
                "state",
                "createdAt",
                "recordSha256",
            ],
        )
        || predecessor["version"] != 1
        || predecessor["kind"] != "pre_return_attempt"
        || predecessor["state"] != "admitted"
        || predecessor["recordSha256"] != crate::evidence::hash::value(&unsigned)
        || predecessor["attemptId"]
            != crate::evidence::hash::value(&json!({
            "binding":predecessor["binding"],"limits":predecessor["policy"]}))
        || !crate::work::valid_work_handle(snapshot["work"].as_str().unwrap_or(""))
        || !nonblank(&snapshot["assignment"], 128)
        || !sha(&snapshot["assignmentSha256"])
        || !sha(&snapshot["ledgerSourceSha256"])
        || !sha(&snapshot["governorSha256"])
        || !sha(&snapshot["held"]["sha256"])
        || snapshot["held"]["reference"] != expected_held
        || snapshot["held"]["bytes"].as_u64().is_none()
        || !sha(&snapshot["predecessor"]["recordSha256"])
        || !sha(&snapshot["predecessor"]["attemptId"])
        || !sha(&snapshot["newProducer"]["executableSha256"])
        || !crate::producer::valid(&snapshot["newProducer"]["producer"])
        || event["producer"] != snapshot["newProducer"]["producer"]
        || snapshot["evidence"]["type"] != "owned_observer_v1"
        || snapshot["evidence"]["attemptId"] != snapshot["predecessor"]["attemptId"]
        || !sha(&snapshot["evidence"]["facts"]["proofSha256"])
        || snapshot["evidence"]["facts"]["ownedChildrenDrained"] != true
        || snapshot["evidence"]["facts"]["nativeExit"] != "unknown"
        || snapshot["evidence"]["facts"]["checkCredit"] != false
        || snapshot["nativeExit"] != Value::Null
        || snapshot["capture"] != "incomplete"
        || snapshot["checkCredit"] != false
        || recovery["currentBinding"] != request_binding(snapshot)
        || decision["version"] != 1
        || decision["work"] != snapshot["work"]
        || decision["currentBinding"] != recovery["currentBinding"]
        || decision["approved"] != true
        || decision["agent"] != snapshot["lead"]["agent"]
        || decision["authority"] != "caller_reported"
        || decision["executionDomain"] != "ended"
        || decision["effects"] != "reconciled"
        || !nonblank(&decision["reason"], 1024)
        || !nonblank(&decision["effectsReason"], 2048)
    {
        return Err("invalid reported pre-return reconciliation or evidence binding".into());
    }
    Ok(())
}

pub(crate) fn validate_prefix(
    event: &Value,
    prefix: &str,
    path: &run_ledger::LedgerPath,
) -> Result<(), String> {
    let snapshot = &event["recovery"]["snapshot"];
    if snapshot["ledgerSourceSha256"] != crate::evidence::hash::text(&format!("{prefix}\n"))
        || snapshot["work"] != canonical_work_for_ledger(path)?
    {
        return Err(
            "pre-return reconciliation does not bind the exact Work and ledger prefix".into(),
        );
    }
    Ok(())
}

pub(crate) fn apply(state: &mut Value, event: &Value) -> Result<(), String> {
    validate(event)?;
    let snapshot = &event["recovery"]["snapshot"];
    let old = &snapshot["predecessor"]["binding"];
    let assignment = crate::run::assignment::pending(state)
        .into_iter()
        .find(|item| item["role"] == "worker" && item["agent"] == old["agent"])
        .ok_or("pre-return reconciliation requires the current worker assignment")?;
    let work = snapshot["work"].as_str().ok_or("missing Work")?;
    if state["status"] != "running"
        || state.get("projectContinuation").is_some()
        || state["verificationPreparationProtocol"] != 1
        || snapshot["lead"] != configured_lead(state)?
        || snapshot["assignment"] != crate::run::assignment::handle(work, &assignment)?
        || snapshot["assignmentSha256"] != crate::evidence::hash::value(&assignment)
        || snapshot["assignmentSha256"] != old["assignmentSha256"]
        || old["runId"] != state["runId"]
        || old["work"] != work
        || old["stage"] != assignment["stage"]
        || old["attempt"] != assignment["attempt"]
        || old["role"] != "worker"
        || old["resultSha256"] != snapshot["held"]["sha256"]
        || !sha(&old["verificationOperationId"])
        || !sha(&old["verificationPreparationSha256"])
        || old["configSha256"] != state["configSha256"]
        || old["subjectSha256"] != state["subject"]["sha256"]
        || snapshot["predecessor"]["policy"] != state["preReturnPolicy"]
        || old["policySha256"] != crate::evidence::hash::value(&state["preReturnPolicy"])
        || snapshot["governorSha256"] != crate::evidence::hash::value(&state["governor"])
        || (state["governor"]["enabled"] == true && state["governor"]["state"] != "ready")
        || for_assignment(state, &assignment)?.is_some()
    {
        return Err(
            "pre-return reconciliation is not current for the frozen worker and governor".into(),
        );
    }
    if !state["preReturnRecoveries"].is_array() {
        state["preReturnRecoveries"] = json!([]);
    }
    state["preReturnRecoveries"]
        .as_array_mut()
        .unwrap()
        .push(event.clone());
    Ok(())
}

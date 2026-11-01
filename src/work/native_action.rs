#[path = "native_action/continuation.rs"]
pub(super) mod continuation;
#[path = "native_action/delivery.rs"]
mod delivery;
#[path = "native_action/evidence.rs"]
mod evidence;
#[cfg(test)]
use evidence::current_worker_events;
use evidence::{verified_worker_results, ReviewEvidence};
#[path = "native_action/host_return.rs"]
mod host_return;
#[path = "native_action/journal.rs"]
mod journal;
#[path = "native_action/prompt.rs"]
mod prompt;
#[path = "native_action/reconcile.rs"]
mod reconcile;
#[path = "native_action/recovery.rs"]
mod recovery;
#[path = "native_action/request.rs"]
pub(super) mod request;
#[path = "native_action/result.rs"]
mod result;
#[path = "native_action/retained.rs"]
mod retained;
#[path = "native_action/start_failure.rs"]
mod start_failure;
#[path = "native_action/storage.rs"]
mod storage;
#[cfg(test)]
use storage::journal_key;
use storage::{
    acquire_lock, find_resume_source, journal_paths, journal_paths_readonly, JournalLock,
};

pub(crate) use host_return::{host_result_contract, return_host_result, validate_host_result};
use journal::{
    claim_started, journal_result, read_journal, record_error, update_started,
    validate_completed_observation, verify_journal, write_completed, write_journal,
};
use result::{
    bounded_json, projected_result, projection, request_projection, role_schema,
    validate_final_result,
};
pub(crate) use retained::{expand as expand_retained_result, is_reference as is_retained_result};

use crate::config::Loaded;
use crate::evidence::hash;
use crate::host::codex_exec::{self, ProcessLiveness, TurnStatus};
use crate::project::managed_files;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn delivery_packet(assignment: &Value) -> Value {
    prompt::delivery_packet(assignment)
}

const MAX_RESULT_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_BYTES: u64 = 256 * 1024;
const MAX_PACKET_BYTES: usize = 64 * 1024;

pub(crate) struct Options<'a> {
    pub(crate) codex_bin: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) sandbox_mode: Option<&'a str>,
    pub(crate) timeout_ms: Option<&'a str>,
    pub(crate) resume: bool,
    pub(crate) controlled_effects: bool,
}

pub(crate) fn recover(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    operation: Option<&str>,
    overrides: bool,
) -> Result<Option<Value>, String> {
    recovery::recover(loaded, work, current, operation, overrides)
}

pub(crate) fn inspect(loaded: &Loaded, work: &str, current: &Value) -> Result<Value, String> {
    reconcile::inspect(loaded, work, current)
}

pub(crate) fn execute(
    loaded: &Loaded,
    work: &str,
    current: &Value,
    options: Options<'_>,
) -> Result<Value, String> {
    let identity = assignment_identity(current)?;
    let role = current["role"]
        .as_str()
        .ok_or("current native assignment has no role")?;
    let agent = current["agent"]
        .as_str()
        .ok_or("current native assignment has no agent")?;
    let paths = journal_paths(
        loaded,
        work,
        &identity.assignment,
        current["packet"]["substitution"].is_object(),
    )?;
    let _lock = acquire_lock(&paths.lock)?;
    let mut existing = read_journal(&paths.journal)?;
    let recorded_request = existing.as_ref().map(|journal| journal["request"].clone());
    let inherited = if existing.is_none() && options.resume {
        find_resume_source(&paths, role, agent, current["packet"]["attempt"].as_u64())?
    } else {
        None
    };
    let mut retry_thread_id = None;
    let mut prior_failure = None;
    let mut relaunch_of: Option<String> = None;
    let mut replay_binding = None;

    if let Some(journal) = existing.clone() {
        if let Err(error) = verify_journal(&journal, work, &identity, role, agent) {
            if !options.resume || journal["status"] != "completed" {
                return Err(error);
            }
            let previous = match journal::pre_return_replay_identity(
                loaded,
                work,
                &journal,
                &identity.assignment,
            )? {
                Some(previous) => Some(previous),
                None => {
                    journal::permit_replay_identity(loaded, work, &journal, &identity.assignment)?
                }
            }
            .ok_or(error)?;
            verify_journal(&journal, work, &previous, role, agent)?;
            replay_binding = Some(
                current["current"]["binding"]
                    .as_str()
                    .ok_or("current native replay binding is unavailable")?,
            );
        }
        // A not_started journal never reserved a provider effect to reconcile,
        // and its only route is the relaunch below.
        if options.resume && journal["status"] != "not_started" {
            continuation::reconcile_failed(loaded, work, &journal)?;
        }
        // A pre-thread, no-effect start recorded no provider execution, so
        // only its sandbox may change; every other override stays refused.
        if journal["status"] == "not_started" {
            recovery::validate_relaunch_overrides(&journal, &options)?;
        } else {
            recovery::validate_overrides(&journal, &options)?;
        }
        match journal["status"].as_str() {
            Some("completed") => {
                validate_completed_observation(&journal)?;
                let bytes = journal_result(&journal)?;
                continuation::reconcile_saved(loaded, work, &journal)?;
                let response = delivery::submit_saved_checked(
                    loaded,
                    work,
                    &identity.assignment,
                    bytes,
                    replay_binding,
                    &journal,
                )?;
                return Ok(recovery::mark_same_work_return(
                    response,
                    work,
                    &identity.assignment,
                ));
            }
            Some("started" | "running")
                if journal
                    .get("priorFailure")
                    .is_some_and(|value| !value.is_null()) =>
            {
                return Err("native assignment already used its bounded provider retry; further execution is refused".into());
            }
            Some("started" | "running") if !options.resume => {
                let route = crate::emitted::command_line(
                    &["work", "act", work, "--resume"],
                    Some(&loaded.path),
                );
                return Err(format!(
                    "native assignment is already started; use {route} after inspecting its private journal"
                ));
            }
            Some("started") if role == "reviewer" => {
                return Err(
                    "reviewer resume is refused; a reviewer must use a fresh native session".into(),
                );
            }
            Some("started") if journal.get("provisionalFinalResult").is_some() => {
                if !options.resume {
                    return Err("native assignment retains a provisional result; use --resume to reconcile the ended worker operation".into());
                }
                let retry = recovery::prepare_provisional_retry(&paths, &journal, role)?;
                retry_thread_id = retry.thread_id;
                prior_failure = Some(retry.prior_failure);
                existing = None;
            }
            Some("not_started") if options.resume => {
                let route = start_failure::route(work, &journal, &loaded.path).shell_text();
                let repeat = if start_failure::prior_relaunch(&paths.journal) {
                    start_failure::REPEAT_NOTE
                } else {
                    ""
                };
                return Err(format!(
                    "native assignment did not start; it has no provider thread to resume. Launch the same assignment with {route}{repeat}"
                ));
            }
            Some("not_started") => {
                // Pre-thread, no-effect failure: the same assignment relaunches.
                relaunch_of = Some(recovery::prepare_start_relaunch(&paths, &journal)?);
                existing = None;
            }
            Some("started") => {}
            Some("running") => {
                match recovery::liveness(&journal) {
                    ProcessLiveness::Alive => {
                        return Err("native assignment provider is alive; keep the existing execution handle".into());
                    }
                    ProcessLiveness::Uncertain => {
                        return Err("native assignment execution is uncertain; no duplicate provider run is admitted".into());
                    }
                    ProcessLiveness::Ended if role == "reviewer" => {
                        return Err("reviewer resume is refused; a reviewer needs a fresh independent session".into());
                    }
                    ProcessLiveness::Ended if journal.get("provisionalFinalResult").is_some() => {
                        let retry = recovery::prepare_provisional_retry(&paths, &journal, role)?;
                        retry_thread_id = retry.thread_id;
                        prior_failure = Some(retry.prior_failure);
                        existing = None;
                    }
                    ProcessLiveness::Ended => {}
                }
            }
            Some(_) | None => return Err("native assignment journal has an invalid status".into()),
        }
    } else if options.resume && inherited.is_none() {
        return Err("--resume requires a started native assignment journal".into());
    }

    let selected = crate::work::details::managed_required_assignment(loaded, work, current)?;
    let recipient_sha256 = crate::work::details::managed_recipient_fingerprint(loaded, current)?;
    let required_delivery_proof = delivery::proof(loaded, work, current, &selected)?;
    let form = &selected["sections"]["currentAction"]["beforeEditing"];
    let permit_delivery_proof =
        crate::work::action_forms::permit_request_id(form).map(|request_id| {
            json!({"binding":selected["binding"], "formSha256":hash::value(form),
            "requestId":request_id, "requiredFactsSha256":hash::value(&selected["sections"]),
            "recipientViewSha256":selected["recipientSha256"]})
        });
    let assignment = &selected["canonicalAssignment"];
    let _canonical_packet = bounded_json(assignment, MAX_PACKET_BYTES, "assignment packet")?;
    let packet = bounded_json(
        &selected["sections"],
        crate::work::readable::MAX_GROUPED_BYTES,
        "native readable delivery",
    )?;
    let profile = request::profile_bytes(loaded, agent, assignment)?;
    let review_evidence = verified_worker_results(loaded, work, role, assignment)?;
    if let Some(evidence) = &review_evidence {
        for item in evidence {
            if !selected["sections"]["evidence"]["items"]
                .as_array()
                .is_some_and(|items| {
                    items.iter().any(|entry| {
                        entry["artifact"]["sha256"] == item.sha256
                            && entry["artifact"]["content"] == item.content
                    })
                })
            {
                let route = crate::emitted::refresh(work, &loaded.path);
                return Err(format!(
                    "required reviewer result is missing from readable delivery; {route}"
                ));
            }
        }
    }
    let mut schema: Value =
        serde_json::from_str(role_schema(role)?).map_err(|_| "native role schema is invalid")?;
    if let Some(evidence) = review_evidence.as_ref() {
        schema["properties"]["evidenceReferences"]["items"]["enum"] = json!(evidence
            .iter()
            .map(|item| &item.reference)
            .collect::<Vec<_>>());
    }
    let schema_path = paths.schema.clone();
    ensure_schema(&schema_path, &schema.to_string())?;

    let thread_id = existing
        .as_ref()
        .and_then(|journal| journal["threadId"].as_str())
        .or(retry_thread_id.as_deref())
        .or_else(|| inherited.as_ref().map(|source| source.thread_id.as_str()));
    if options.resume && thread_id.is_none() {
        return Err(
            "native assignment journal has no provider thread ID; safe resume is unavailable"
                .into(),
        );
    }
    let resuming = options.resume;
    let request = request::build(
        loaded,
        options,
        role,
        work,
        current,
        assignment,
        packet,
        profile,
        review_evidence.as_ref(),
        &schema_path,
        thread_id,
    )?;
    let current_recipient = crate::work::details::managed_recipient_fingerprint(loaded, current)?;
    if current_recipient != recipient_sha256 {
        return Err(crate::work::details::recipient_drift(
            loaded,
            work,
            &recipient_sha256,
            &current_recipient,
        ));
    }
    delivery::validate_before_launch(loaded, work, &required_delivery_proof)?;
    if resuming {
        if let Some(recorded) = recorded_request.as_ref() {
            recovery::validate_rebuilt_request(&json!({"request": recorded}), &request)?;
        }
    }
    codex_exec::validate_request(&request).map_err(|error| error.to_string())?;
    let state = continuation::state(loaded, work)?;
    let prepared_journal = if existing.is_none() {
        let operation_id = continuation::operation_id(
            &state,
            work,
            &identity.assignment,
            &request,
            prior_failure.as_ref(),
            relaunch_of.as_deref(),
        )
        .unwrap_or_else(|| {
            prior_failure
                .as_ref()
                .and_then(|failure| failure["operation"].as_str())
                .map(|operation| format!("native-retry-{operation}-{}", timestamp()))
                .or_else(|| {
                    relaunch_of
                        .as_deref()
                        .map(|operation| format!("native-relaunch-{operation}-{}", timestamp()))
                })
                .unwrap_or_else(|| format!("native-{}-{}", std::process::id(), timestamp()))
        });
        let resumed_from_operation = prior_failure
            .as_ref()
            .map(|failure| failure["operation"].clone())
            .unwrap_or(Value::Null);
        let exclusive = prior_failure.is_none();
        Some((
            json!({
                "version": 1,
                "status": "started",
                "work": work,
                "assignment": identity.assignment,
                "assignmentSha256": identity.packet_sha256,
                "recipientProofVersion": 1,
                "recipientSha256": recipient_sha256,
                "readSubjectSha256": current["packet"]["context"]["subject"]["sha256"],
                "deliveryProofVersion": 1,
                "requiredDeliveryProof": required_delivery_proof,
                "permitDeliveryProof": permit_delivery_proof,
                "role": role,
                "agent": agent,
                "stage": current["packet"]["stage"],
                "attempt": current["packet"]["attempt"],
                "operationId": operation_id,
                "resumedFromOperation": resumed_from_operation,
                "threadId": thread_id.map_or(Value::Null, |value| json!(value)),
                "resumedFromAssignment": inherited
                    .as_ref()
                    .map_or(Value::Null, |source| json!(source.assignment)),
                "priorFailure": prior_failure.unwrap_or(Value::Null),
                "startedAt": timestamp(),
                "request": request_projection(&request),
            }),
            exclusive,
        ))
    } else {
        None
    };

    let ticket = continuation::admit(
        loaded,
        work,
        continuation::Launch {
            state: &state,
            current,
            paths: &paths,
            request: &request,
            prepared_journal,
            delivery_proof: &required_delivery_proof,
        },
    )?;
    let (observation, ticket) = match continuation::execute(loaded, work, &paths, &request, ticket)
    {
        Ok(observation) => observation,
        Err(error) => {
            let journal = start_failure::settle(&paths.journal)?;
            return Err(start_failure::message(
                loaded,
                work,
                &error,
                journal.as_ref(),
                start_failure::prior_relaunch(&paths.journal),
            ));
        }
    };
    let mut observation_value = projection(&observation);
    if let Some(evidence) = review_evidence.as_ref() {
        observation_value["verifiedEvidence"] = json!(evidence
            .iter()
            .map(|item| json!({
                "reference": item.reference,
                "sha256": item.sha256,
                "bytes": item.bytes,
            }))
            .collect::<Vec<_>>());
    }
    update_started(
        &paths.journal,
        &observation_value,
        observation.final_result.as_ref(),
    )?;
    let retained = (|| {
        let final_result = observation
            .final_result
            .as_ref()
            .ok_or_else(|| format!("native assignment has no typed {role} result"))?;
        let outcome = validate_final_result(role, final_result)?;
        if let Some(evidence) = review_evidence.as_ref() {
            let expected = evidence
                .iter()
                .map(|item| json!(item.reference))
                .collect::<Vec<_>>();
            if final_result["evidenceReferences"] != json!(expected) {
                return Err(
                    "native reviewer did not cite every verified worker result reference".into(),
                );
            }
        }
        if !observation.process.success
            || observation.interrupted
            || !matches!(observation.turn, TurnStatus::Completed)
            || !observation.coverage_gap.is_empty()
        {
            return Err(format!(
            "native assignment did not produce a complete accepted observation (outcome {outcome})"
        ));
        }
        let result = projected_result(role, final_result, &observation_value)?;
        let bytes = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_RESULT_BYTES {
            return Err("native result exceeds the bounded result journal".into());
        }
        write_completed(&paths.journal, &observation_value, &bytes)?;
        Ok::<_, String>(bytes)
    })();
    let accounted = if ticket.is_some() {
        if let Err(error) = retained.as_ref() {
            record_error(&paths.journal, error)?;
        }
        let journal = read_journal(&paths.journal)?
            .ok_or("native journal disappeared after retaining its observation")?;
        continuation::complete_owned(
            loaded,
            &paths,
            &ticket,
            retained.is_ok(),
            recovery::liveness(&journal) == ProcessLiveness::Ended,
        )
    } else {
        Ok(())
    };
    // Classify only a retained failure: the journal now holds the complete
    // observation and process identity, and a pre-thread, no-effect failure
    // becomes the reconciliation state that may launch the same assignment.
    let classified = if retained.is_err() {
        start_failure::settle(&paths.journal)?
    } else {
        None
    };
    let bytes = match (retained, accounted) {
        (Ok(bytes), Ok(())) => bytes,
        (Err(error), Ok(())) => {
            return Err(start_failure::message(
                loaded,
                work,
                &error,
                classified.as_ref(),
                start_failure::prior_relaunch(&paths.journal),
            ))
        }
        (Ok(_), Err(error)) => return Err(format!("native accounting recovery pending: {error}")),
        (Err(error), Err(accounting)) => {
            return Err(start_failure::message(
                loaded,
                work,
                &format!("{error}; native accounting recovery pending: {accounting}"),
                classified.as_ref(),
                start_failure::prior_relaunch(&paths.journal),
            ));
        }
    };
    let journal = read_journal(&paths.journal)?.ok_or("native result journal disappeared")?;
    delivery::submit_saved_checked(loaded, work, &identity.assignment, bytes, None, &journal)
}

struct Paths {
    journal: PathBuf,
    schema: PathBuf,
    lock: PathBuf,
}

struct Identity {
    assignment: String,
    packet_sha256: String,
}

fn assignment_identity(current: &Value) -> Result<Identity, String> {
    let assignment = current["assignment"]
        .as_str()
        .ok_or("current native assignment has no assignment identity")?;
    if assignment.len() > 128
        || !assignment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err("current native assignment identity is invalid".into());
    }
    Ok(Identity {
        assignment: assignment.to_owned(),
        packet_sha256: hash::value(&current["packet"]),
    })
}

fn ensure_schema(path: &Path, expected: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("native output schema is not a regular file".into());
            }
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            if bytes != expected.as_bytes() {
                return Err("native output schema does not match the expected role schema".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            managed_files::write_state_exclusive(path, expected.as_bytes())
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn submit_saved(
    loaded: &Loaded,
    work: &str,
    assignment: &str,
    bytes: Vec<u8>,
    current_binding: Option<&str>,
) -> Result<Value, String> {
    let (outcome, reason) = outcome_from_bytes(&bytes)?;
    super::super::return_result_impl::return_result_with(
        loaded,
        work,
        assignment,
        &outcome,
        reason.as_deref(),
        None,
        None,
        Some(bytes),
        current_binding,
    )
}

fn outcome_from_bytes(bytes: &[u8]) -> Result<(String, Option<String>), String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "native result is not JSON")?;
    let outcome = value["outcome"]
        .as_str()
        .filter(|outcome| !outcome.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "native result has no outcome".to_owned())?;
    let reason = value
        .get("reason")
        .map(|reason| {
            reason
                .as_str()
                .filter(|reason| reason.len() <= 8192 && !reason.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or("native result reason is invalid")
        })
        .transpose()?;
    Ok((outcome, reason))
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("completed native journal result is not hex".into());
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text =
                std::str::from_utf8(pair).map_err(|_| "invalid journal result".to_owned())?;
            u8::from_str_radix(text, 16).map_err(|_| "invalid journal result".to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_outcomes_are_strictly_bounded() {
        assert!(result::allowed_outcome("worker", "completed").is_ok());
        assert!(result::allowed_outcome("reviewer", "completed").is_err());
        assert!(result::allowed_outcome("adviser", "approved").is_err());
    }

    #[test]
    fn native_role_schemas_require_every_declared_property() {
        for role in ["worker", "reviewer", "adviser"] {
            let schema: Value = serde_json::from_str(role_schema(role).unwrap()).unwrap();
            let properties = schema["properties"].as_object().unwrap();
            let required = schema["required"].as_array().unwrap();
            assert_eq!(required.len(), properties.len(), "{role}");
            for property in properties.keys() {
                assert!(
                    required.iter().any(|name| name == property),
                    "{role}: {property}"
                );
            }
        }
    }

    #[test]
    fn journal_result_round_trip_is_bounded_and_content_preserving() {
        let value = b"{\"outcome\":\"blocked\"}";
        let encoded = hex_encode(value);
        assert_eq!(hex_decode(&encoded).unwrap(), value);
        assert!(hex_decode("xyz").is_err());
    }

    #[test]
    fn current_reviewer_requires_all_completed_workers_in_its_attempt() {
        let assignment = json!({"attempt": 2, "stage": 3});
        let events = vec![
            json!({"action":"submit","role":"worker","outcome":"completed","attempt":1,"stage":2,"agent":"old"}),
            json!({"action":"submit","role":"worker","outcome":"completed","attempt":2,"stage":2,"agent":"first"}),
            json!({"action":"submit","role":"worker","outcome":"completed","attempt":2,"stage":2,"agent":"second"}),
            json!({"action":"submit","role":"reviewer","outcome":"approved","attempt":2,"stage":3,"agent":"reviewer"}),
        ];
        let selected = current_worker_events(&events, &assignment);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0]["agent"], "first");
        assert_eq!(selected[1]["agent"], "second");
    }

    #[test]
    fn fallback_journal_is_distinct_from_primary_assignment() {
        assert_eq!(journal_key("sma_123", false), "sma_123");
        assert_eq!(journal_key("sma_123", true), "sma_123-fallback");
    }

    #[test]
    fn unavailable_reason_must_be_an_allowed_operational_code() {
        let valid = json!({"outcome":"unavailable","summary":"provider unavailable","reason":"rate_limit","evidenceReferences":["ref:abc123"]});
        assert!(validate_final_result("reviewer", &valid).is_ok());
        let invalid = json!({"outcome":"unavailable","summary":"provider unavailable","reason":"some prose","evidenceReferences":["ref:abc123"]});
        assert!(validate_final_result("reviewer", &invalid).is_err());
        let missing = json!({"outcome":"unavailable","summary":"provider unavailable","evidenceReferences":["ref:abc123"]});
        assert!(validate_final_result("reviewer", &missing).is_err());
    }
}

//! Bounded, context-only Work membership on the existing canonical Goal.

use serde_json::{json, Value};
use std::{io::Write, path::Path};

pub(crate) const PROTOCOL: u64 = 1;
const MAX_ASSOCIATIONS: usize = 128;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    goal_id: String,
    revision: u64,
    event_sha256: String,
    session_key: String,
    host: &'static str,
    owner_project_id: Option<String>,
    owner_identity_sha256: String,
}

#[derive(Clone, Debug)]
pub(crate) enum WorkContext {
    Local,
    Owner(Value),
    Session(Value),
    Unavailable,
    Invalid,
}

const COMPANION_MAX_BYTES: u64 = 4096;
const COMPANION_KIND: &str = "native_session_goal_context";
const VERIFIED_CONTEXT_FIELD: &str = "__verifiedContextOnlyWork";

/// Shared immutable sidecar writer for native context and named-Goal scope.
/// Common Work/start binding, size, privacy and digest rules live here.
pub(crate) fn write_private_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    suffix: &str,
    label: &str,
    kind: &str,
    protocol: u64,
    start_event_sha256: &str,
    fields: Value,
) -> Result<(), String> {
    if !valid_work(work) || !valid_sha(start_event_sha256) {
        return Err("Work companion identity is malformed".into());
    }
    let mut value = json!({
        "kind": kind,
        "protocol": protocol,
        "work": work,
        "startEventSha256": start_event_sha256,
    });
    let fields = fields
        .as_object()
        .ok_or("Work companion payload is malformed")?;
    for (key, field) in fields {
        if ["kind", "protocol", "work", "startEventSha256", "sha256"].contains(&key.as_str()) {
            return Err("Work companion payload uses a reserved field".into());
        }
        value
            .as_object_mut()
            .expect("new companion object")
            .insert(key.clone(), field.clone());
    }
    value["sha256"] = json!(crate::evidence::hash::value(&value));
    let bytes = value.to_string();
    if bytes.len() > COMPANION_MAX_BYTES as usize {
        return Err("Work companion exceeds its bounded size".into());
    }
    let relative = companion_path_with_suffix(work, suffix)?;
    let mut file = crate::project::path::secure_create_new(&loaded.state_root, &relative, label)?;
    file.write_all(bytes.as_bytes())
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

/// Shared bounded single-link reader and exact Work/start/digest validation.
/// A missing companion stays distinguishable from an invalid one.
pub(crate) fn read_private_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    suffix: &str,
    label: &str,
    kind: &str,
    protocol: u64,
    expected_fields: &[&str],
) -> Result<Option<Value>, String> {
    use crate::project::path::SecureBytesResult;
    let relative = companion_path_with_suffix(work, suffix)?;
    let bytes = match crate::project::path::secure_bytes_observation_single_link_bounded(
        &loaded.state_root,
        &relative,
        label,
        COMPANION_MAX_BYTES,
    ) {
        SecureBytesResult::Bytes(bytes) => bytes,
        SecureBytesResult::Absent(_) => return Ok(None),
        SecureBytesResult::Unsafe(error) | SecureBytesResult::Unreadable(error) => {
            return Err(error)
        }
        #[cfg(not(unix))]
        SecureBytesResult::Unsupported(error) => return Err(error),
    };
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| format!("{label} is malformed"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} is malformed"))?;
    if object.len() != expected_fields.len() + 5
        || value["kind"].as_str() != Some(kind)
        || value["protocol"].as_u64() != Some(protocol)
        || value["work"].as_str() != Some(work)
        || !value["startEventSha256"].as_str().is_some_and(valid_sha)
        || expected_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(format!("{label} is invalid or unsupported"));
    }
    let digest = value["sha256"]
        .as_str()
        .filter(|digest| valid_sha(digest))
        .ok_or_else(|| format!("{label} digest is invalid"))?;
    let mut unsigned = value.clone();
    unsigned
        .as_object_mut()
        .expect("validated object")
        .remove("sha256");
    if crate::evidence::hash::value(&unsigned) != digest {
        return Err(format!("{label} digest mismatch"));
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work start event is missing")?;
    if start["action"] != "start" || start["eventSha256"] != value["startEventSha256"] {
        return Err(format!("{label} does not match the exact Work start"));
    }
    Ok(Some(value))
}

fn has_extension(record: &Value) -> bool {
    record.get("workAssociationProtocol").is_some() || record.get("workAssociations").is_some()
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

fn valid_work(value: &str) -> bool {
    value.strip_prefix("smw_").is_some_and(valid_sha)
}

fn entries(record: &Value) -> Result<Vec<Value>, String> {
    if !has_extension(record) {
        return Ok(Vec::new());
    }
    if record["workAssociationProtocol"].as_u64() != Some(PROTOCOL) {
        return Err("unsupported context-only Work association protocol".into());
    }
    let values = record["workAssociations"]
        .as_array()
        .ok_or("context-only Work associations are malformed")?;
    if values.is_empty() || values.len() > MAX_ASSOCIATIONS {
        return Err("context-only Work association bound is invalid".into());
    }
    let mut works = std::collections::BTreeSet::new();
    for value in values {
        let object = value
            .as_object()
            .ok_or("context-only Work association entry is malformed")?;
        if object.len() != 3 || object.get("kind").and_then(Value::as_str) != Some("context_only") {
            return Err("context-only Work association entry is malformed".into());
        }
        let work = object
            .get("work")
            .and_then(Value::as_str)
            .filter(|value| valid_work(value))
            .ok_or("context-only Work association has an invalid Work identity")?;
        let start = object
            .get("startEventSha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha(value))
            .ok_or("context-only Work association has an invalid start identity")?;
        if !works.insert(work.to_owned()) || start.trim().is_empty() {
            return Err("context-only Work association identity is duplicated".into());
        }
    }
    Ok(values.clone())
}

pub(super) fn validate_record(record: &Value, line: usize) -> Result<(), String> {
    entries(record)
        .map(|_| ())
        .map_err(|error| format!("invalid canonical session goal line {line}: {error}"))
}

/// Retain the extension only while ordinary updates continue the same Goal ID.
pub(super) fn preserve_same_goal(previous: &Value, next: &mut Value) {
    if previous["goalId"] != next["goalId"] || !has_extension(previous) || has_extension(next) {
        return;
    }
    next["workAssociationProtocol"] = previous["workAssociationProtocol"].clone();
    next["workAssociations"] = previous["workAssociations"].clone();
}

/// Read-only selection before a Work start. Legacy single-Work continuation
/// records keep their established exclusive binding behavior.
pub(crate) fn prepare(scope: &super::native::Scope) -> Result<Option<Candidate>, String> {
    let (Some(session), Some(owner)) = (scope.session.as_ref(), scope.owner.as_ref()) else {
        return Ok(None);
    };
    let Some(record) = super::read(&scope.root)? else {
        return Ok(None);
    };
    match record["closure"]["closed"].as_bool() {
        Some(false | true) => {}
        None => return Err("canonical Goal closure state is invalid; no Work was started".into()),
    }
    if record["continuation"]["work"].is_string() {
        return Ok(None);
    }
    let work_associations = entries(&record)?;
    if work_associations.len() >= MAX_ASSOCIATIONS {
        return Err("context-only Work association bound reached; no Work was started".into());
    }
    super::journey::ensure_admission_capacity(&record)?;
    super::journey::ensure_member_capacity(&record)?;
    let goal_id = record["goalId"]
        .as_str()
        .ok_or("canonical Goal identity is missing")?;
    if goal_id.trim().is_empty() || goal_id.len() > 128 || goal_id.contains('\0') {
        return Err("native session Goal identity exceeds the context association bound; no Work was started".into());
    }
    Ok(Some(Candidate {
        goal_id: goal_id.to_owned(),
        revision: record["revision"]
            .as_u64()
            .ok_or("canonical Goal revision is missing")?,
        event_sha256: record["eventSha256"]
            .as_str()
            .ok_or("canonical Goal event identity is missing")?
            .to_owned(),
        session_key: session.key.clone(),
        host: session.host,
        owner_project_id: owner.project_id.clone(),
        owner_identity_sha256: owner.identity_sha256.clone(),
    }))
}

/// Atomically reopen the current session Goal and record the exact start
/// intention before the immutable Work start is created.
pub(crate) fn admit(
    root: &Path,
    loaded: &crate::config::Loaded,
    candidate: &Candidate,
    work: &str,
    start_intent_sha256: &str,
    work_goal: &str,
) -> Result<Value, String> {
    if !valid_work(work) || !valid_sha(start_intent_sha256) {
        return Err("Work admission identity is malformed".into());
    }
    let session = super::native::Session {
        host: candidate.host,
        key: candidate.session_key.clone(),
        root: root.to_path_buf(),
    };
    let owner = super::native::Owner {
        project_id: candidate.owner_project_id.clone(),
        identity_sha256: candidate.owner_identity_sha256.clone(),
    };
    let admission = super::journey::new_admission(
        loaded,
        work,
        start_intent_sha256,
        work_goal,
        &candidate.goal_id,
        &candidate.event_sha256,
        &session,
        &owner,
    )?;
    super::mutate(root, |previous| {
        let previous = previous.ok_or("canonical Goal disappeared before Work admission")?;
        if previous["goalId"].as_str() != Some(candidate.goal_id.as_str())
            || previous["revision"].as_u64() != Some(candidate.revision)
            || previous["eventSha256"].as_str() != Some(candidate.event_sha256.as_str())
            || previous["continuation"]["work"].is_string()
        {
            return Err("canonical Goal changed before Work admission; no Work was started".into());
        }
        entries(previous)?;
        super::journey::ensure_admission_capacity(previous)?;
        let revision = previous["revision"]
            .as_u64()
            .ok_or("canonical Goal revision is missing")?
            .checked_add(1)
            .ok_or("canonical Goal revision exhausted")?;
        let mut next = previous.clone();
        next.as_object_mut()
            .ok_or("canonical Goal record is malformed")?
            .remove("eventSha256");
        next["revision"] = json!(revision);
        next["predecessor"] = json!({
            "goalId": candidate.goal_id,
            "revision": candidate.revision,
        });
        next["successorOf"] = Value::Null;
        next["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
        super::journey::append_admission(&mut next, admission.clone())?;
        Ok(super::sealed(next, Some(previous)))
    })
}

fn companion_fields(
    host: &str,
    session_key: &str,
    parent_goal_id: &str,
    parent_event_sha256: &str,
    owner_project_id: &Option<String>,
    owner_identity_sha256: &str,
) -> Value {
    json!({
        "host": host,
        "sessionKey": session_key,
        "parentGoalId": parent_goal_id,
        "parentObservedGoalEventSha256": parent_event_sha256,
        "ownerProjectId": owner_project_id,
        "ownerIdentitySha256": owner_identity_sha256,
    })
}

fn verify_or_write_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    start_event_sha256: &str,
    fields: Value,
) -> Result<(), String> {
    if !valid_work(work) || !valid_sha(start_event_sha256) {
        return Err("Work start identity is malformed".into());
    }
    const FIELDS: &[&str] = &[
        "host",
        "sessionKey",
        "parentGoalId",
        "parentObservedGoalEventSha256",
        "ownerProjectId",
        "ownerIdentitySha256",
    ];
    if let Some(existing) = read_private_companion(
        loaded,
        work,
        ".session-goal.json",
        "Work Goal context companion",
        COMPANION_KIND,
        PROTOCOL,
        FIELDS,
    )? {
        if existing["startEventSha256"] == start_event_sha256
            && FIELDS
                .iter()
                .all(|field| existing[*field] == fields[*field])
        {
            return Ok(());
        }
        return Err(
            "existing Work Goal context companion does not match the recorded admission".into(),
        );
    }
    match write_private_companion(
        loaded,
        work,
        ".session-goal.json",
        "Work Goal context companion",
        COMPANION_KIND,
        PROTOCOL,
        start_event_sha256,
        fields.clone(),
    ) {
        Ok(()) => Ok(()),
        Err(write_error) => {
            // A concurrent exact recovery may have won secure_create_new.
            // Readback is accepted only for identical immutable bytes.
            match read_private_companion(
                loaded,
                work,
                ".session-goal.json",
                "Work Goal context companion",
                COMPANION_KIND,
                PROTOCOL,
                FIELDS,
            )? {
                Some(existing)
                    if existing["startEventSha256"] == start_event_sha256
                        && FIELDS
                            .iter()
                            .all(|field| existing[*field] == fields[*field]) =>
                {
                    Ok(())
                }
                _ => Err(write_error),
            }
        }
    }
}

/// Resolve only an existing pre-start admission and the actual immutable
/// Work start. The route never starts, checks, accepts, or mutates the Work.
pub(crate) fn link_session(loaded: &crate::config::Loaded, work: &str) -> Result<Value, String> {
    let scope = super::native::scope(loaded, false)?;
    let session = scope
        .session
        .clone()
        .ok_or("work link-session requires the current native host session identity")?;
    let owner = scope
        .owner
        .clone()
        .ok_or("work link-session requires an already incorporated native session Goal")?;
    let record = link_admitted(&scope.root, loaded, &session, &owner, work)?;
    Ok(json!({
        "kind":"native_session_goal_link",
        "state":"linked",
        "work":work,
        "currentGoalId":record["goalId"],
        "currentGoalRevision":record["revision"],
        "workLedger":"unchanged",
        "provider":"none",
        "readOnlyWorkRecovery":true,
    }))
}

fn link_admitted(
    root: &Path,
    loaded: &crate::config::Loaded,
    session: &super::native::Session,
    owner: &super::native::Owner,
    work: &str,
) -> Result<Value, String> {
    if !valid_work(work) {
        return Err("work link-session requires a valid Work handle".into());
    }
    super::mutate(root, |previous| {
        let previous = previous.ok_or("native session Goal journal is unavailable")?;
        let journey_member = super::journey::member_for_work(previous, work)?;
        if let Some(admission) = super::journey::find_admission(previous, work)? {
            validate_admission_for_loaded(loaded, &admission, session, owner)?;
            let (start, start_sha) = verified_start(loaded, work)?;
            if crate::run::start_event_intent_sha256(&start)? != admission.start_intent_sha256 {
                return Err("actual Work start does not match the exact admitted request and basis; pending admission remains unresolved".into());
            }
            if crate::evidence::hash::text(start["goal"].as_str().unwrap_or_default())
                != admission.work_goal_sha256
            {
                return Err("actual Work goal differs from the admitted request; pending admission remains unresolved".into());
            }
            if !super::records(root)?.iter().any(|record| {
                record["goalId"] == admission.parent_goal_id
                    && record["eventSha256"] == admission.parent_observed_goal_event_sha256
            }) {
                return Err(
                    "the exact Goal observation in the pending admission is unavailable".into(),
                );
            }
            verify_or_write_companion(
                loaded,
                work,
                &start_sha,
                companion_fields(
                    &admission.host,
                    &admission.session_key,
                    &admission.parent_goal_id,
                    &admission.parent_observed_goal_event_sha256,
                    &admission.owner_project_id,
                    &admission.owner_identity_sha256,
                ),
            )?;
            let mut next = previous.clone();
            if !super::journey::remove_admission(&mut next, work)? {
                return Err("pending Work admission changed during link recovery".into());
            }
            super::journey::append_work(
                &mut next,
                loaded,
                work,
                &start_sha,
                start["goal"].as_str().unwrap_or_default(),
                &admission.parent_goal_id,
                &admission.parent_observed_goal_event_sha256,
                session,
                owner,
            )?;
            commit_link(previous, next)
        } else if let Some(member) = journey_member {
            validate_member_for_loaded(loaded, &member, session, owner)?;
            let (start, start_sha) = verified_start(loaded, work)?;
            if member["startEventSha256"] != start_sha
                || crate::evidence::hash::text(start["goal"].as_str().unwrap_or_default())
                    != member["workGoalSha256"]
            {
                return Err("recorded journey member does not match the exact Work start".into());
            }
            verify_or_write_companion(
                loaded,
                work,
                &start_sha,
                companion_fields(
                    member["host"].as_str().unwrap_or_default(),
                    member["sessionKey"].as_str().unwrap_or_default(),
                    member["parentGoalId"].as_str().unwrap_or_default(),
                    member["parentObservedGoalEventSha256"]
                        .as_str()
                        .unwrap_or_default(),
                    &member["ownerProjectId"].as_str().map(str::to_owned),
                    member["ownerIdentitySha256"].as_str().unwrap_or_default(),
                ),
            )?;
            let _ = start;
            Ok(previous.clone())
        } else {
            Err("no exact pending or bound native journey admission exists for this Work; no association was inferred".into())
        }
    })
}

fn commit_link(previous: &Value, mut next: Value) -> Result<Value, String> {
    let revision = previous["revision"]
        .as_u64()
        .ok_or("canonical Goal revision is missing")?
        .checked_add(1)
        .ok_or("canonical Goal revision exhausted")?;
    next.as_object_mut()
        .ok_or("canonical Goal record is malformed")?
        .remove("eventSha256");
    next["revision"] = json!(revision);
    next["predecessor"] = json!({
        "goalId": previous["goalId"],
        "revision": previous["revision"],
    });
    next["successorOf"] = Value::Null;
    next["closure"] = json!({"closed":false,"revision":null,"resultRefs":[]});
    Ok(super::sealed(next, Some(previous)))
}

fn verified_start(loaded: &crate::config::Loaded, work: &str) -> Result<(Value, String), String> {
    let ledger = crate::work::resolve(loaded, work).map_err(|error| {
        format!("actual Work start is unavailable; admission remains unresolved: {error}")
    })?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger).map_err(|error| {
        format!(
            "actual Work start is unavailable or invalid; admission remains unresolved: {error}"
        )
    })?;
    let start = events
        .first()
        .ok_or("actual Work start is absent; pending admission remains unresolved")?
        .clone();
    if start["action"] != "start" {
        return Err("actual Work start is invalid; pending admission remains unresolved".into());
    }
    let start_sha = start["eventSha256"]
        .as_str()
        .filter(|sha| valid_sha(sha))
        .ok_or("actual Work start identity is invalid; pending admission remains unresolved")?
        .to_owned();
    Ok((start, start_sha))
}

fn validate_admission_for_loaded(
    loaded: &crate::config::Loaded,
    admission: &super::journey::Admission,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<(), String> {
    validate_scope_values(
        loaded,
        &admission.host,
        &admission.session_key,
        &admission.owner_identity_sha256,
        &admission.owner_project_id,
        &admission.project_identity_sha256,
        &admission.local_identity_sha256,
        &admission.config_path,
        &admission.config_sha256,
        &admission.product_root,
        &admission.state_root,
        session,
        owner,
    )
}

fn validate_member_for_loaded(
    loaded: &crate::config::Loaded,
    member: &Value,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<(), String> {
    super::journey::validate_member_for_session(loaded, member, session, owner)
}

#[allow(clippy::too_many_arguments)]
fn validate_scope_values(
    loaded: &crate::config::Loaded,
    host: &str,
    session_key: &str,
    owner_identity_sha256: &str,
    owner_project_id: &Option<String>,
    project_identity_sha256: &str,
    local_identity_sha256: &str,
    config_path: &str,
    config_sha256: &str,
    product_root: &str,
    state_root: &str,
    session: &super::native::Session,
    owner: &super::native::Owner,
) -> Result<(), String> {
    let canonical = |path: &Path, label: &str| -> Result<String, String> {
        std::fs::canonicalize(path)
            .map_err(|error| format!("{label} unavailable: {error}"))?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{label} is not valid UTF-8"))
    };
    if host != session.host
        || session_key != session.key
        || owner_identity_sha256 != owner.identity_sha256
        || owner_project_id != &owner.project_id
        || project_identity_sha256 != super::requirements::identity(loaded)?
        || local_identity_sha256 != super::native::owner_for(loaded)?.identity_sha256
        || config_path != canonical(&loaded.path, "configured project path")?
        || config_sha256 != crate::evidence::hash::text(&loaded.source)
        || product_root != canonical(&loaded.product_root, "configured product root")?
        || state_root != canonical(&loaded.state_root, "configured state root")?
    {
        return Err(
            "current Work configuration, roots or native session do not match the exact admission"
                .into(),
        );
    }
    Ok(())
}

/// Resolve an optional native-session context without making malformed or
/// foreign context a failure of ordinary Work detail and recovery routes.
pub(crate) fn context_for_work(loaded: &crate::config::Loaded, work: &str) -> WorkContext {
    let value = match read_private_companion(
        loaded,
        work,
        ".session-goal.json",
        "Work Goal context companion",
        COMPANION_KIND,
        PROTOCOL,
        &[
            "host",
            "sessionKey",
            "parentGoalId",
            "parentObservedGoalEventSha256",
            "ownerProjectId",
            "ownerIdentitySha256",
        ],
    ) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return match super::native::current() {
                Ok(None) => WorkContext::Local,
                Ok(Some(_)) => {
                    if legacy_goal_is_bound(loaded, work) {
                        WorkContext::Local
                    } else {
                        WorkContext::Unavailable
                    }
                }
                Err(_) => WorkContext::Invalid,
            };
        }
        Err(_) => return WorkContext::Invalid,
    };
    match resolve_companion(loaded, work, &value) {
        Ok((record, owner_matches)) if owner_matches => WorkContext::Owner(record),
        Ok((record, _)) => WorkContext::Session(record),
        Err(_) => WorkContext::Invalid,
    }
}

fn legacy_goal_is_bound(loaded: &crate::config::Loaded, work: &str) -> bool {
    super::read(&loaded.state_root)
        .ok()
        .flatten()
        .is_some_and(|record| {
            super::progress::record_bound_to_work(&record, work)
                || record["continuation"]["work"].as_str() == Some(work)
        })
}

fn resolve_companion(
    loaded: &crate::config::Loaded,
    work: &str,
    value: &Value,
) -> Result<(Value, bool), String> {
    validate_companion(value)?;
    if value["work"].as_str() != Some(work) {
        return Err("Work Goal context companion names another Work".into());
    }
    let session = super::native::current()?.ok_or("native session identity is unavailable")?;
    if value["host"] != session.host || value["sessionKey"] != session.key {
        return Err("Work Goal context belongs to another native session".into());
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work start event is missing")?;
    if start["action"] != "start" || start["eventSha256"] != value["startEventSha256"] {
        return Err("Work start identity does not match its Goal context".into());
    }
    let owner = session
        .owner()?
        .ok_or("native session Goal owner is unavailable")?;
    if owner.identity_sha256 != value["ownerIdentitySha256"]
        || owner.project_id.as_deref() != value["ownerProjectId"].as_str()
    {
        return Err("Work Goal context owner identity does not match".into());
    }
    let history = super::records(&session.root)?;
    if !history.iter().any(|record| {
        record["goalId"] == value["parentGoalId"]
            && record["eventSha256"] == value["parentObservedGoalEventSha256"]
    }) {
        return Err("parent Goal observation is absent from the session journal".into());
    }
    let mut linked_member = None;
    for record in history.iter().rev() {
        if let Some(member) = super::journey::member_for_work(record, work)? {
            linked_member = Some(member);
            break;
        }
    }
    let member = linked_member.ok_or("parent Goal has no exact Work/start membership")?;
    if member["startEventSha256"] != value["startEventSha256"]
        || member["parentGoalId"] != value["parentGoalId"]
        || member["parentObservedGoalEventSha256"] != value["parentObservedGoalEventSha256"]
        || member["host"] != value["host"]
        || member["sessionKey"] != value["sessionKey"]
        || member["ownerProjectId"] != value["ownerProjectId"]
        || member["ownerIdentitySha256"] != value["ownerIdentitySha256"]
        || crate::evidence::hash::text(start["goal"].as_str().unwrap_or_default())
            != member["workGoalSha256"]
    {
        return Err(
            "Work Goal companion does not match the exact journey member and parent observation"
                .into(),
        );
    }
    super::journey::validate_member_for_session(loaded, &member, &session, &owner)?;
    let parent_observation = history
        .iter()
        .position(|record| {
            record["goalId"] == value["parentGoalId"]
                && record["eventSha256"] == value["parentObservedGoalEventSha256"]
        })
        .ok_or("parent Goal observation is absent from the session journal")?;
    // Same-Goal-ID revisions may add explicit named mappings or current
    // context, and remain part of the Work's parent. Stop at the first
    // different Goal ID so an old Work never follows a successor or a later
    // reuse of the same textual ID.
    let parent = history[parent_observation..]
        .iter()
        .take_while(|record| record["goalId"] == value["parentGoalId"])
        .last()
        .ok_or("parent Goal observation is absent from the session journal")?;
    let mut parent = parent.clone();
    parent[VERIFIED_CONTEXT_FIELD] = json!({
        "kind":"context_only",
        "work":work,
        "startEventSha256":value["startEventSha256"],
    });
    let local = super::native::owner_for(loaded)?;
    let owner_matches =
        owner.identity_sha256 == local.identity_sha256 && owner.project_id == local.project_id;
    Ok((parent, owner_matches))
}

fn companion_path_with_suffix(work: &str, suffix: &str) -> Result<String, String> {
    if !valid_work(work) {
        return Err("Work identity is malformed".into());
    }
    let token = work.strip_prefix("smw_").expect("validated Work prefix");
    if suffix.is_empty() || suffix.contains('/') || suffix.contains('\\') || suffix.contains('\0') {
        return Err("Work companion suffix is invalid".into());
    }
    Ok(format!(
        "{}/runs/work-{token}{suffix}",
        crate::project::layout_types::state_namespace()
    ))
}

fn validate_companion(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("Work Goal context companion is malformed")?;
    if object.len() != 11
        || value["kind"] != COMPANION_KIND
        || value["protocol"] != 1
        || !value["work"].as_str().is_some_and(valid_work)
        || !value["startEventSha256"].as_str().is_some_and(valid_sha)
        || !value["sessionKey"].as_str().is_some_and(valid_sha)
        || !value["parentGoalId"]
            .as_str()
            .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128)
        || !value["parentObservedGoalEventSha256"]
            .as_str()
            .is_some_and(valid_sha)
        || !(value["ownerProjectId"].is_null() || value["ownerProjectId"].is_string())
        || !value["ownerIdentitySha256"].as_str().is_some_and(valid_sha)
        || !value["host"]
            .as_str()
            .is_some_and(|host| matches!(host, "codex" | "claude"))
    {
        return Err("Work Goal context companion is invalid or unsupported".into());
    }
    Ok(())
}

pub(super) fn for_work(record: &Value, work: &str) -> Option<Value> {
    entries(record)
        .ok()?
        .into_iter()
        .find(|entry| entry["work"].as_str() == Some(work))
        .or_else(|| {
            (record[VERIFIED_CONTEXT_FIELD]["work"].as_str() == Some(work))
                .then(|| record[VERIFIED_CONTEXT_FIELD].clone())
        })
}

pub(super) fn public_record(record: &Value) -> Value {
    let mut public = record.clone();
    if let Some(object) = public.as_object_mut() {
        object.remove(VERIFIED_CONTEXT_FIELD);
    }
    public
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use std::fs;

    fn goal(goal_id: &str, revision: u64) -> Value {
        json!({
            "kind": "lead_session_goal",
            "version": 2,
            "goalId": goal_id,
            "revision": revision,
            "predecessor": Value::Null,
            "successorOf": Value::Null,
            "closure": {"closed": false, "revision": Value::Null, "resultRefs": []},
            "continuation": Value::Null,
        })
    }

    fn sealed(mut value: Value, previous: Option<&Value>) -> Value {
        super::super::sealed(value.take(), previous)
    }

    #[test]
    fn legacy_records_remain_readable_and_unknown_protocols_refuse() {
        let legacy = sealed(goal("legacy", 1), None);
        let raw = format!("{}\n", legacy);
        assert!(super::super::records_from_raw(Some(&raw)).is_ok());

        let mut current = goal("current", 1);
        current["workAssociationProtocol"] = json!(super::PROTOCOL);
        current["workAssociations"] = json!([{
            "kind": "context_only",
            "work": format!("smw_{}", "a".repeat(64)),
            "startEventSha256": "b".repeat(64),
        }]);
        let current = sealed(current, None);
        let raw = format!("{}\n", current);
        assert!(super::super::records_from_raw(Some(&raw)).is_ok());

        let mut unknown = current;
        unknown["workAssociationProtocol"] = json!(super::PROTOCOL + 1);
        let unknown = sealed(unknown, None);
        let raw = format!("{}\n", unknown);
        assert!(super::super::records_from_raw(Some(&raw)).is_err());
    }

    #[test]
    fn same_goal_sealing_preserves_associations_but_successors_start_unbound() {
        let mut previous = goal("same", 1);
        previous["workAssociationProtocol"] = json!(super::PROTOCOL);
        previous["workAssociations"] = json!([{
            "kind": "context_only",
            "work": format!("smw_{}", "a".repeat(64)),
            "startEventSha256": "b".repeat(64),
        }]);
        let previous = sealed(previous, None);

        let mut same = goal("same", 2);
        same["predecessor"] = json!({"goalId":"same", "revision":1});
        let same = sealed(same, Some(&previous));
        assert_eq!(same["workAssociationProtocol"], super::PROTOCOL);
        assert_eq!(same["workAssociations"], previous["workAssociations"]);

        let mut successor = goal("next", 2);
        successor["predecessor"] = json!({"goalId":"same", "revision":1});
        successor["successorOf"] = json!({"goalId":"same", "revision":1});
        let successor = sealed(successor, Some(&previous));
        assert!(successor.get("workAssociationProtocol").is_none());
        assert!(successor.get("workAssociations").is_none());
    }

    #[test]
    fn prestart_admission_reopens_closed_goal_blocks_readiness_and_survives_successor() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let project_root = std::env::temp_dir().join(format!(
            "exitbind-session-admission-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&project_root).expect("create private test project root");
        let config_path = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: project_root.to_str().expect("UTF-8 test project path"),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .expect("initialize isolated fixture project");
        let loaded = crate::config::load(Some(
            config_path.to_str().expect("UTF-8 fixture config path"),
        ))
        .expect("load isolated fixture project");
        let session_root = project_root.join("native-session-store");
        crate::project::portability::create_private_directory(&session_root)
            .expect("create isolated private native session root");
        crate::project::portability::create_private_directory(
            &session_root.join(crate::project::layout_types::state_namespace()),
        )
        .expect("create isolated private native session journal directory");
        let goal = super::super::incorporate(
            &session_root,
            &loaded,
            "main",
            "Complete the full conversation journey",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("obligations,findings,blockers,decisions,externalActions"),
            true,
        )
        .expect("incorporate native session Goal");
        let closed = super::super::close_direct(&session_root, &loaded, "main", None)
            .expect("close empty journey before the next side Work");
        assert_eq!(closed["closure"]["closed"], true);
        let owner = super::super::native::owner_for(&loaded).expect("fixture owner identity");
        let work = format!("smw_{}", "a".repeat(64));
        let candidate = super::Candidate {
            goal_id: goal["goalId"].as_str().unwrap_or("main").to_owned(),
            revision: closed["revision"].as_u64().expect("closed Goal revision"),
            event_sha256: closed["eventSha256"]
                .as_str()
                .expect("closed Goal event")
                .to_owned(),
            session_key: "b".repeat(64),
            host: "codex",
            owner_project_id: owner.project_id.clone(),
            owner_identity_sha256: owner.identity_sha256,
        };
        let reopened = super::admit(
            &session_root,
            &loaded,
            &candidate,
            &work,
            &"c".repeat(64),
            "A separate side request",
        )
        .expect("record admission before any Work start");
        assert_eq!(reopened["closure"]["closed"], false);
        assert_eq!(reopened["journeyAdmissions"].as_array().unwrap().len(), 1);
        let history = super::super::records(&session_root).expect("read Goal history");
        assert!(history
            .iter()
            .any(|record| record["closure"]["closed"] == true));
        assert!(history.iter().all(|record| record["goalId"] == "main"));
        let readiness = super::super::journey::readiness_for_state(
            &loaded,
            super::super::journey::state(&reopened).expect("validate pending journey"),
        );
        assert_eq!(readiness.state, "blocked");
        assert_eq!(readiness.pending_admissions, 1);
        assert_eq!(
            readiness.reason.as_deref(),
            Some("native_journey_has_unresolved_work_admissions")
        );

        let successor = super::super::incorporate(
            &session_root,
            &loaded,
            "side-follow-up",
            "Keep all requests in this conversation",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("obligations,findings,blockers,decisions,externalActions"),
            true,
        )
        .expect("incorporate explicit successor Goal");
        assert_eq!(successor["journeyMembershipProtocol"], 2);
        assert_eq!(
            successor["journeyAdmissions"],
            reopened["journeyAdmissions"]
        );
        assert_eq!(successor["journeyAdmissions"][0]["parentGoalId"], "main");
        let _ = fs::remove_dir_all(project_root);
    }
}

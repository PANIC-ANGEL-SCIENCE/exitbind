use super::*;
use serde_json::{json, Value};
use std::path::Path;

/// Read-only selection before a Work start. Legacy single-Work continuation
/// records keep their established exclusive binding behavior.
pub(crate) fn prepare(scope: &super::super::native::Scope) -> Result<Option<Candidate>, String> {
    let (Some(session), Some(owner)) = (scope.session.as_ref(), scope.owner.as_ref()) else {
        return Ok(None);
    };
    let Some(record) = super::super::read(&scope.root)? else {
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
    super::super::journey::ensure_admission_capacity(&record)?;
    super::super::journey::ensure_member_capacity(&record)?;
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
    let session = super::super::native::Session {
        host: candidate.host,
        key: candidate.session_key.clone(),
        root: root.to_path_buf(),
    };
    let owner = super::super::native::Owner {
        project_id: candidate.owner_project_id.clone(),
        identity_sha256: candidate.owner_identity_sha256.clone(),
    };
    let admission = super::super::journey::new_admission(
        loaded,
        super::super::journey::AdmissionRequest {
            work,
            start_intent_sha256,
            work_goal,
            parent_goal_id: &candidate.goal_id,
            parent_event_sha256: &candidate.event_sha256,
            session: &session,
            owner: &owner,
        },
    )?;
    super::super::mutate(root, |previous| {
        let previous = previous.ok_or("canonical Goal disappeared before Work admission")?;
        if previous["goalId"].as_str() != Some(candidate.goal_id.as_str())
            || previous["revision"].as_u64() != Some(candidate.revision)
            || previous["eventSha256"].as_str() != Some(candidate.event_sha256.as_str())
            || previous["continuation"]["work"].is_string()
        {
            return Err("canonical Goal changed before Work admission; no Work was started".into());
        }
        entries(previous)?;
        super::super::journey::ensure_admission_capacity(previous)?;
        super::super::journey::ensure_member_capacity(previous)?;
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
        super::super::journey::append_admission(&mut next, admission.clone())?;
        Ok(super::super::sealed(next, Some(previous)))
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
        PrivateCompanionWrite {
            suffix: ".session-goal.json",
            label: "Work Goal context companion",
            kind: COMPANION_KIND,
            protocol: PROTOCOL,
            start_event_sha256,
            fields: fields.clone(),
        },
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
    let scope = super::super::native::scope(loaded, false)?;
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
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
    work: &str,
) -> Result<Value, String> {
    if !valid_work(work) {
        return Err("work link-session requires a valid Work handle".into());
    }
    super::super::mutate(root, |previous| {
        let previous = previous.ok_or("native session Goal journal is unavailable")?;
        let journey_member = super::super::journey::member_for_work(previous, work)?;
        if let Some(admission) = super::super::journey::find_admission(previous, work)? {
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
            if !super::super::records(root)?.iter().any(|record| {
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
            if !super::super::journey::remove_admission(&mut next, work)? {
                return Err("pending Work admission changed during link recovery".into());
            }
            super::super::journey::append_work(
                &mut next,
                loaded,
                super::super::journey::WorkLink {
                    work,
                    start_event_sha256: &start_sha,
                    work_goal: start["goal"].as_str().unwrap_or_default(),
                    parent_goal_id: &admission.parent_goal_id,
                    parent_event_sha256: &admission.parent_observed_goal_event_sha256,
                    session,
                    owner,
                },
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
    Ok(super::super::sealed(next, Some(previous)))
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
    admission: &super::super::journey::Admission,
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
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
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
) -> Result<(), String> {
    super::super::journey::validate_member_for_session(loaded, member, session, owner)
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
    session: &super::super::native::Session,
    owner: &super::super::native::Owner,
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
        || project_identity_sha256 != super::super::requirements::identity(loaded)?
        || local_identity_sha256 != super::super::native::owner_for(loaded)?.identity_sha256
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

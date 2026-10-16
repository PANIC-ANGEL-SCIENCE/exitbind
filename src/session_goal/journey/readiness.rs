use super::*;

fn member_currentness(
    loaded: &Loaded,
    state: &JourneyState,
    member: &Member,
) -> Result<(&'static str, Option<String>), String> {
    let project = project_loaded(loaded, member)?;
    let session =
        super::super::native::current()?.ok_or("native session identity is unavailable")?;
    let scope = super::super::native::scope(loaded, false)?;
    let owner = scope.owner.ok_or("native session owner is unavailable")?;
    if member.host != session.host
        || member.session_key != session.key
        || member.owner_identity_sha256 != owner.identity_sha256
        || member.owner_project_id != owner.project_id
    {
        return Err("journey member belongs to another native session or owner".into());
    }
    validate_member_loaded(
        &project,
        member,
        &Header {
            protocol: PROTOCOL,
            host: session.host.to_owned(),
            session_key: session.key,
            owner_identity_sha256: owner.identity_sha256,
            owner_project_id: owner.project_id,
            project_identity_sha256: member.project_identity_sha256.clone(),
            local_identity_sha256: member.local_identity_sha256.clone(),
            config_binding_sha256: config_binding(&project)?,
            checker: super::super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        },
    )?;
    let Some(endpoint) = accepted_endpoint(loaded, member)? else {
        return Ok(("unresolved", None));
    };
    let evidence = crate::run::result_ref_evidence(loaded, &endpoint.work)?
        .ok_or("journey member result evidence is unavailable")?;
    if !evidence.accepted || !evidence.artifact_current || evidence.drift.is_some() {
        return Ok(("unresolved", None));
    }
    match evidence.inputs_current {
        Some(true) => Ok(("current", None)),
        Some(false) => {
            let supported = current_integration_for_project(
                loaded,
                state,
                &state.members,
                &member.project_identity_sha256,
            )?;
            if supported.is_some() {
                Ok(("integration_supported", supported))
            } else {
                Ok(("stale", None))
            }
        }
        None => Ok(("unresolved", None)),
    }
}

pub(in crate::session_goal) fn readiness_for_state(
    loaded: &Loaded,
    state: Option<JourneyState>,
) -> Readiness {
    let Some(state) = state else {
        return Readiness {
            protocol: 0,
            state: "unavailable",
            reason: Some("journey_membership_protocol_missing".into()),
            total: 0,
            current: 0,
            integrated: 0,
            pending_admissions: 0,
            unresolved: vec![
                json!({"state":"unavailable","reason":"explicit_native_goal_incorporation_required"}),
            ],
            integration_work: None,
        };
    };
    let mut current = 0;
    let mut integrated = 0;
    let mut integration_work = None;
    let mut unresolved = Vec::new();
    if state.protocol != MEMBERSHIP_PROTOCOL {
        unresolved.push(json!({
            "state":"unavailable",
            "reason":"historical_journey_membership_has_no_admission_coverage",
        }));
    }
    for admission in &state.admissions {
        unresolved.push(json!({
            "work": admission.work,
            "state": "admission_pending_start_or_link",
            "reason": "exact_immutable_work_start_and_context_link_required",
        }));
    }
    for member in &state.members {
        match member_currentness(loaded, &state, member) {
            Ok(("current", _)) => current += 1,
            Ok(("integration_supported", Some(work))) => {
                integrated += 1;
                integration_work.get_or_insert(work);
            }
            Ok((status, _)) => unresolved.push(json!({"work":member.work,"state":status})),
            Err(error) => {
                unresolved.push(json!({"work":member.work,"state":"unavailable","reason":error}))
            }
        }
    }
    let state_name = if unresolved.is_empty() {
        "current"
    } else {
        "blocked"
    };
    Readiness {
        protocol: state.protocol,
        state: state_name,
        reason: (state_name != "current").then(|| {
            if state.protocol != MEMBERSHIP_PROTOCOL {
                "historical_journey_membership_has_no_admission_coverage".into()
            } else if !state.admissions.is_empty() {
                "native_journey_has_unresolved_work_admissions".into()
            } else {
                "journey_members_need_current_acceptance_or_checked_integration".into()
            }
        }),
        total: state.members.len(),
        current,
        integrated,
        pending_admissions: state.admissions.len(),
        unresolved: unresolved
            .into_iter()
            .take(MAX_MEMBERS + MAX_ADMISSIONS + 1)
            .collect(),
        integration_work,
    }
}

fn belongs_to_current_session(
    loaded: &Loaded,
    record: &Value,
) -> Result<Option<(super::super::native::Scope, Value)>, String> {
    let scope = super::super::native::scope(loaded, false)?;
    if !scope.is_session() || !scope.owner_matches {
        return Ok(None);
    }
    let history = super::super::records(&scope.root)?;
    if !history
        .iter()
        .any(|item| item["eventSha256"] == record["eventSha256"])
    {
        return Ok(None);
    }
    let current = history
        .last()
        .cloned()
        .ok_or("native session Goal history is empty")?;
    Ok(Some((scope, current)))
}

/// Applies the native-journey guard only to records in this host-reported
/// session journal. Explicit project Goal behavior remains legacy-compatible.
pub(crate) fn readiness_for_record(
    loaded: &Loaded,
    record: &Value,
) -> Result<Option<Readiness>, String> {
    let Some((_scope, current)) = belongs_to_current_session(loaded, record)? else {
        return Ok(None);
    };
    if current["goalId"] != record["goalId"] {
        return Ok(Some(Readiness {
            protocol: state(&current)?.map_or(0, |state| state.protocol),
            state: "blocked",
            reason: Some("a_newer_session_goal_is_current".into()),
            total: state(&current)?.map_or(0, |state| state.members.len()),
            current: 0,
            integrated: 0,
            pending_admissions: state(&current)?.map_or(0, |state| state.admissions.len()),
            unresolved: vec![
                json!({"state":"unresolved","reason":"newer_session_goal_is_current"}),
            ],
            integration_work: None,
        }));
    }
    Ok(Some(readiness_for_state(loaded, state(&current)?)))
}

pub(crate) fn require_close_ready(
    loaded: &Loaded,
    root: &Path,
    record: &Value,
) -> Result<(), String> {
    if root == loaded.state_root {
        return Ok(());
    }
    let scope = super::super::native::scope(loaded, false)?;
    if !scope.is_session() || !scope.owner_matches || scope.root != root {
        return Ok(());
    }
    let readiness = readiness_for_state(loaded, state(record)?);
    if readiness.ready() {
        Ok(())
    } else {
        Err(format!(
            "native session journey is not ready to close: {}",
            readiness.reason.unwrap_or_else(|| readiness.state.into())
        ))
    }
}

/// Replacement may continue a failed member, but never bypass an unfinished
/// admission or attach that member to a newer conversation commitment.
pub(crate) fn require_resolved_replacement_admissions(record: &Value) -> Result<(), String> {
    let session = super::super::native::current()?
        .ok_or("native session identity is unavailable for check replacement")?;
    let history = super::super::records(&session.root)?;
    if !history
        .iter()
        .any(|item| item["eventSha256"] == record["eventSha256"])
    {
        return Err("check replacement Goal observation is absent from the native session".into());
    }
    let current = history.last().ok_or("native Goal history is unavailable")?;
    if current["goalId"] != record["goalId"] {
        return Err("check replacement belongs to an older native Goal".into());
    }
    let state = state(current)?.ok_or("native Goal admission coverage is unavailable")?;
    if state.protocol != MEMBERSHIP_PROTOCOL || !state.admissions.is_empty() {
        return Err("check replacement is blocked by unresolved native Goal admissions".into());
    }
    Ok(())
}

pub(crate) fn progress_for_record(loaded: Option<&Loaded>, record: &Value) -> Option<Value> {
    let loaded = loaded?;
    readiness_for_record(loaded, record)
        .ok()
        .flatten()
        .map(|readiness| readiness.value())
}

/// An exact journey member may retain its recorded result reference after its
/// tested inputs drift, but only a current accepted integration for the full
/// current local member set can refresh that currentness.
pub(crate) fn result_ref_current_or_supported(
    loaded: &Loaded,
    record: &Value,
    reference: &str,
    inputs_current: Option<bool>,
) -> Result<bool, String> {
    if inputs_current == Some(true) {
        return Ok(true);
    }
    if inputs_current != Some(false) {
        return Ok(false);
    }
    let Some(journey) = state(record)? else {
        return Ok(false);
    };
    let matched = journey
        .members
        .iter()
        .filter(|member| member.work == reference)
        .collect::<Vec<_>>();
    if matched.len() != 1 {
        return Ok(false);
    }
    let member = matched[0];
    project_loaded(loaded, member)?;
    let local_endpoint = accepted_endpoint(loaded, member)?;
    if local_endpoint.is_none() {
        return Ok(false);
    }
    let evidence = crate::run::result_ref_evidence(loaded, reference)?;
    if !evidence
        .as_ref()
        .is_some_and(|e| e.accepted && e.artifact_current && e.drift.is_none())
    {
        return Ok(false);
    }
    Ok(current_integration_for_project(
        loaded,
        &journey,
        &journey.members,
        &member.project_identity_sha256,
    )?
    .is_some())
}

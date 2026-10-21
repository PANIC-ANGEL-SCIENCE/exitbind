use super::*;

fn member_currentness(
    loaded: &Loaded,
    record: Option<&Value>,
    state: &JourneyState,
    member: &Member,
) -> Result<(&'static str, Option<String>), String> {
    let recovery = record
        .map(|record| super::super::recovery::context(loaded, record))
        .transpose()?
        .flatten();
    if recovery
        .as_ref()
        .is_some_and(|context| context.historical.contains(&member.work))
    {
        return Ok(("historical", None));
    }
    // A recovery bridges only the exact Source members it selected. Every
    // other retained member keeps the pre-recovery owner-config behavior.
    let recovered_endpoint = recovery
        .as_ref()
        .and_then(|context| context.endpoints.get(&member.work));
    let selected = match (recovery.as_ref(), recovered_endpoint) {
        (Some(context), Some(_)) => &context.source,
        _ => loaded,
    };
    let endpoint_loaded = recovered_endpoint.unwrap_or(selected);
    let project = project_loaded(selected, member)?;
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
            recovery_sha256: recovery
                .as_ref()
                .map(|value| value.active.decision_sha256.clone()),
            checker: super::super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        },
    )?;
    let Some(endpoint) = accepted_endpoint(endpoint_loaded, member)? else {
        return Ok(("unresolved", None));
    };
    let evidence = crate::run::result_ref_evidence(endpoint_loaded, &endpoint.work)?
        .ok_or("journey member result evidence is unavailable")?;
    if evidence.accepted && evidence.artifact_current {
        if let Some(drift) = &evidence.drift {
            // Accepted at binding: the acceptance stays the recorded fact and
            // what changed since is reported beside it (the second slot carries
            // the drift for the listing).
            return Ok(("accepted_at_binding", Some(drift.to_string())));
        }
    }
    if !evidence.accepted || !evidence.artifact_current {
        return Ok(("unresolved", None));
    }
    match evidence.inputs_current {
        Some(true) => {
            if recovered_endpoint.is_some() {
                let integration = super::super::journey::current_recovery_integration(
                    loaded,
                    record.ok_or("native Goal recovery record is unavailable")?,
                )?;
                let Some((work, _)) = integration else {
                    return Ok(("unresolved", None));
                };
                Ok(("current", Some(work)))
            } else {
                Ok(("current", None))
            }
        }
        Some(false) => {
            let supported = if recovered_endpoint.is_some() {
                super::super::journey::current_recovery_integration(
                    loaded,
                    record.ok_or("native Goal recovery record is unavailable")?,
                )?
                .map(|(work, _)| work)
            } else {
                current_integration_for_project(
                    selected,
                    record.unwrap_or(&Value::Null),
                    state,
                    &state.members,
                    &member.project_identity_sha256,
                )?
            };
            if supported.is_some() {
                Ok(("integration_supported", supported))
            } else {
                Ok(("stale", None))
            }
        }
        None => Ok(("unresolved", None)),
    }
}

#[cfg(test)]
pub(in crate::session_goal) fn readiness_for_state(
    loaded: &Loaded,
    state: Option<JourneyState>,
) -> Readiness {
    readiness_for_state_inner(loaded, None, state)
}

fn readiness_for_state_inner(
    loaded: &Loaded,
    record: Option<&Value>,
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
            historical: 0,
            quarantined_uncredited: 0,
            historical_details: Vec::new(),
            quarantined_details: Vec::new(),
            pending_admissions: 0,
            unresolved: vec![
                json!({"state":"unavailable","reason":"explicit_native_goal_incorporation_required"}),
            ],
            integration_work: None,
        };
    };
    let mut current = 0;
    let mut integrated = 0;
    let mut historical = 0;
    let mut quarantined_uncredited = 0;
    let mut quarantined_details = Vec::new();
    let mut integration_work = None;
    let mut unresolved = Vec::new();
    if state.protocol < MEMBERSHIP_PROTOCOL {
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
        if let Some(record) = record {
            match super::super::quarantine::current_for_member(loaded, record, &member.work) {
                Ok(Some(detail)) => {
                    if detail["state"] == "quarantined_uncredited" {
                        quarantined_uncredited += 1;
                        quarantined_details.push(detail);
                    } else if detail["state"] == "current" {
                        current += 1;
                    } else {
                        unresolved.push(json!({"work":member.work,"state":"unavailable",
                            "reason":"quarantine residual has an unsupported readiness category"}));
                    }
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    unresolved
                        .push(json!({"work":member.work,"state":"unavailable","reason":error}));
                    continue;
                }
            }
        }
        match member_currentness(loaded, record, &state, member) {
            Ok(("current", integration)) => {
                current += 1;
                if let Some(work) = integration {
                    integration_work.get_or_insert(work);
                }
            }
            Ok(("integration_supported", Some(work))) => {
                integrated += 1;
                integration_work.get_or_insert(work);
            }
            Ok(("historical", _)) => historical += 1,
            Ok(("accepted_at_binding", drift)) => unresolved.push(json!({"work":member.work,
                "state":"accepted_at_binding",
                "drift":drift.and_then(|drift| serde_json::from_str::<Value>(&drift).ok())})),
            Ok((status, _)) => unresolved.push(json!({"work":member.work,"state":status})),
            Err(error) => unresolved.push(json!({"work":member.work,"state":"unavailable",
                "reason":crate::drift::plain(&error)})),
        }
    }
    let state_name = if unresolved.is_empty() {
        "current"
    } else {
        "blocked"
    };
    let historical_details = record
        .and_then(|record| {
            super::super::recovery::context(loaded, record)
                .ok()
                .flatten()
        })
        .map_or_else(Vec::new, |context| context.historical_items);
    Readiness {
        protocol: state.protocol,
        state: state_name,
        reason: (state_name != "current").then(|| {
            if state.protocol < MEMBERSHIP_PROTOCOL {
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
        historical,
        quarantined_uncredited,
        historical_details,
        quarantined_details,
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
            historical: 0,
            quarantined_uncredited: 0,
            historical_details: Vec::new(),
            quarantined_details: Vec::new(),
            pending_admissions: state(&current)?.map_or(0, |state| state.admissions.len()),
            unresolved: vec![
                json!({"state":"unresolved","reason":"newer_session_goal_is_current"}),
            ],
            integration_work: None,
        }));
    }
    Ok(Some(readiness_for_state_inner(
        loaded,
        Some(&current),
        state(&current)?,
    )))
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
    let readiness = readiness_for_state_inner(loaded, Some(record), state(record)?);
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
    if state.protocol < MEMBERSHIP_PROTOCOL || !state.admissions.is_empty() {
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
    let recovery = super::super::recovery::context(loaded, record)?;
    let mut recovered_matches = Vec::new();
    if let Some(context) = &recovery {
        for member in &journey.members {
            if let Some(endpoint_loaded) = context.endpoints.get(&member.work) {
                if super::super::journey::accepted_endpoint_for_recovery(endpoint_loaded, member)?
                    .as_deref()
                    == Some(reference)
                {
                    recovered_matches.push(member);
                }
            }
        }
    }
    let recovered_member = if recovered_matches.len() == 1 {
        recovered_matches.first().copied()
    } else {
        None
    };
    let selected = match (recovery.as_ref(), recovered_member) {
        (Some(context), Some(_)) => &context.source,
        _ => loaded,
    };
    let matched = if let Some(member) = recovered_member {
        vec![member]
    } else {
        journey
            .members
            .iter()
            .filter(|member| member.work == reference)
            .collect::<Vec<_>>()
    };
    if matched.len() != 1 {
        return Ok(false);
    }
    let member = matched[0];
    project_loaded(selected, member)?;
    let endpoint_loaded = recovery
        .as_ref()
        .filter(|_| recovered_member.is_some())
        .and_then(|context| context.endpoints.get(&member.work))
        .unwrap_or(selected);
    let local_endpoint = accepted_endpoint(endpoint_loaded, member)?;
    if local_endpoint.is_none() {
        return Ok(false);
    }
    let evidence = crate::run::result_ref_evidence(endpoint_loaded, reference)?;
    if !evidence
        .as_ref()
        .is_some_and(|e| e.accepted && e.artifact_current && e.drift.is_none())
    {
        return Ok(false);
    }
    let has_integration = if recovered_member.is_some() {
        super::super::journey::current_recovery_integration(loaded, record)?.is_some()
    } else {
        current_integration_for_project(
            selected,
            record,
            &journey,
            &journey.members,
            &member.project_identity_sha256,
        )?
        .is_some()
    };
    Ok(has_integration)
}

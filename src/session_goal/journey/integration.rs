use super::*;

mod lineage;
pub(super) use lineage::{accepted_endpoint, descends_from_member};

#[derive(Clone, Debug)]
struct Contract {
    header: Header,
    members: Vec<CoveredMember>,
}

/// A Work is either a context-only journey member or the explicitly selected
/// integration proof for the exact snapshot in its frozen start basis.
pub(crate) fn append_work(
    record: &mut Value,
    loaded: &Loaded,
    request: WorkLink<'_>,
) -> Result<(), String> {
    let WorkLink {
        work,
        start_event_sha256,
        session,
        owner,
        ..
    } = request;
    let Some(mut journey) = state(record)? else {
        return Ok(());
    };
    if journey.members.len() >= MAX_MEMBERS && journey.integrations.len() >= MAX_INTEGRATIONS {
        return Err("native journey Work bound reached; no new membership was recorded".into());
    }
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events.first().ok_or("Work ledger has no start event")?;
    if start["action"] != "start" || start["eventSha256"] != start_event_sha256 {
        return Err("Work start identity changed before native journey linkage".into());
    }
    if let Some(contract) = parse_contract(start)? {
        validate_header_for_loaded(loaded, &contract.header)?;
        if contract.header.host != session.host
            || contract.header.session_key != session.key
            || contract.header.owner_identity_sha256 != owner.identity_sha256
            || contract.header.owner_project_id != owner.project_id
        {
            return Err("journey integration belongs to another native session or owner".into());
        }
        let expected = covered_snapshot(loaded, record, &journey.members, &contract.header)?;
        if expected != contract.members {
            return Err(
                "journey integration snapshot changed before its Work start was linked".into(),
            );
        }
        let member = new_member(loaded, request)?;
        if journey.integrations.len() >= MAX_INTEGRATIONS
            || journey
                .integrations
                .iter()
                .any(|entry| entry.member.work == work)
        {
            return Err("native journey integration bound or identity is invalid".into());
        }
        journey.integrations.push(IntegrationEntry {
            member,
            contract_sha256: contract_sha256(&contract),
        });
    } else {
        let member = new_member(loaded, request)?;
        if journey.members.len() >= MAX_MEMBERS
            || journey.members.iter().any(|item| item.work == work)
            || journey
                .integrations
                .iter()
                .any(|entry| entry.member.work == work)
        {
            return Err("native journey member bound or identity is invalid".into());
        }
        journey.members.push(member);
    }
    write_state(record, journey);
    Ok(())
}

fn validate_header_for_loaded(loaded: &Loaded, header: &Header) -> Result<(), String> {
    let scope = super::super::native::scope(loaded, false)?;
    let session = scope
        .session
        .ok_or("journey integration requires a current native session identity")?;
    let owner = scope
        .owner
        .ok_or("journey integration requires the fixed native session owner")?;
    let local = super::super::native::owner_for(loaded)?;
    if !matches!(header.protocol, PROTOCOL | RECOVERY_INTEGRATION_PROTOCOL)
        || (header.protocol == PROTOCOL && header.recovery_sha256.is_some())
        || (header.protocol == RECOVERY_INTEGRATION_PROTOCOL
            && !header.recovery_sha256.as_deref().is_some_and(valid_sha))
        || !matches!(header.host.as_str(), "codex" | "claude")
        || !valid_sha(&header.session_key)
        || !valid_sha(&header.owner_identity_sha256)
        || !(header.owner_project_id.is_none()
            || header
                .owner_project_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty() && id.len() <= 128))
        || !valid_sha(&header.project_identity_sha256)
        || !valid_sha(&header.local_identity_sha256)
        || !valid_sha(&header.config_binding_sha256)
        || !valid_sha(&header.checker.sha256)
        || header.checker.path.trim().is_empty()
        || header.checker.path.len() > 256
        || header.checker.path.contains('\0')
        || header.host != session.host
        || header.session_key != session.key
        || header.owner_identity_sha256 != owner.identity_sha256
        || header.owner_project_id != owner.project_id
        || header.project_identity_sha256 != super::super::requirements::identity(loaded)?
        || header.local_identity_sha256 != local.identity_sha256
        || header.config_binding_sha256 != config_binding(loaded)?
    {
        return Err("journey integration scope or configured checker binding changed".into());
    }
    Ok(())
}

pub(super) fn marker(prefix: &str, value: &impl Serialize) -> Result<String, String> {
    let marker = format!(
        "{prefix}{}",
        serde_json::to_string(value).map_err(|error| error.to_string())?
    );
    if marker.len() > MAX_MARKER_BYTES {
        return Err("journey integration marker exceeds the frozen 1024-byte basis bound".into());
    }
    Ok(marker)
}

fn reject_reserved_constraints(basis: &Value) -> Result<(), String> {
    let constraints = basis["constraints"]
        .as_array()
        .ok_or("basis constraints are malformed")?;
    if constraints.iter().any(|item| {
        item.as_str()
            .is_some_and(|text| text.starts_with(RESERVED_PREFIX))
    }) {
        return Err("journey integration scope must be selected through --journey-integration, not an injected basis marker".into());
    }
    Ok(())
}

/// Refuse caller-controlled journey markers at the shared Work-start boundary.
pub(crate) fn reject_unprepared_markers(basis_text: &str) -> Result<(), String> {
    let basis = crate::kernel::basis::parse_basis_text(basis_text, "basis")?.value();
    reject_reserved_constraints(&basis)
}

fn parse_contract(start: &Value) -> Result<Option<Contract>, String> {
    let Some(basis) = start.get("basis") else {
        return Ok(None);
    };
    let constraints = basis["constraints"]
        .as_array()
        .ok_or("Work basis constraints are malformed")?;
    let mut header_markers = Vec::new();
    let mut member_markers = Vec::new();
    for value in constraints.iter().filter_map(Value::as_str) {
        if value.starts_with(RESERVED_PREFIX) && value.len() > MAX_MARKER_BYTES {
            return Err("journey integration marker exceeds its bounded size".into());
        }
        if let Some(raw) = value.strip_prefix(HEADER_PREFIX) {
            header_markers.push(raw);
        } else if let Some(raw) = value.strip_prefix(MEMBER_PREFIX) {
            member_markers.push(raw);
        } else if value.starts_with(RESERVED_PREFIX) {
            return Err("unsupported journey integration marker".into());
        }
    }
    if header_markers.is_empty() && member_markers.is_empty() {
        return Ok(None);
    }
    if header_markers.len() != 1 || member_markers.is_empty() || member_markers.len() > MAX_MEMBERS
    {
        return Err(
            "journey integration header/member markers are missing, duplicated or oversized".into(),
        );
    }
    let header: Header = serde_json::from_str(header_markers[0])
        .map_err(|_| "journey integration header is malformed or has unknown fields")?;
    let mut members = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in member_markers {
        let member: CoveredMember = serde_json::from_str(raw)
            .map_err(|_| "journey integration member is malformed or has unknown fields")?;
        if !valid_work(&member.work)
            || !valid_sha(&member.member_sha256)
            || !valid_sha(&member.accepted_endpoint_sha256)
            || !seen.insert(member.work.clone())
        {
            return Err("journey integration member identity is invalid or duplicated".into());
        }
        members.push(member);
    }
    members.sort_by(|a, b| a.work.cmp(&b.work));
    Ok(Some(Contract { header, members }))
}

fn contract_sha256(contract: &Contract) -> String {
    hash::value(&json!({"header":&contract.header,"members":&contract.members}))
}

fn member_contract_marker(contract: &Contract) -> Result<Vec<String>, String> {
    let mut result = vec![marker(HEADER_PREFIX, &contract.header)?];
    result.extend(
        contract
            .members
            .iter()
            .map(|member| marker(MEMBER_PREFIX, member))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(result)
}

fn current_scope(
    loaded: &Loaded,
) -> Result<
    (
        super::super::native::Scope,
        super::super::native::Session,
        super::super::native::Owner,
    ),
    String,
> {
    let scope = super::super::native::scope(loaded, false)?;
    let session = scope
        .session
        .clone()
        .ok_or("journey integration requires a current native session identity")?;
    let owner = scope
        .owner
        .clone()
        .ok_or("journey integration requires an incorporated current session Goal")?;
    Ok((scope, session, owner))
}

/// Freeze the exact accepted local journey set before creating the integration
/// Work's immutable start event.
pub(crate) fn prepare(
    loaded: &Loaded,
    supplied_basis: Option<&str>,
    checker: &str,
    command: &str,
) -> Result<Prepared, String> {
    if let Some(basis) = supplied_basis {
        reject_unprepared_markers(basis)?;
    }
    let (scope, session, owner) = current_scope(loaded)?;
    let record =
        super::super::read(&scope.root)?.ok_or("no native session Goal is incorporated")?;
    if record["closure"]["closed"] != false {
        return Err("native session Goal is closed; start a new side Work to reopen the journey before integration".into());
    }
    let journey = state(&record)?.ok_or("native session Goal has no explicit journey membership protocol; explicitly incorporate it before integration")?;
    if journey.integrations.len() >= MAX_INTEGRATIONS {
        return Err("native journey integration bound reached; no Work was started".into());
    }
    let project_identity_sha256 = super::super::requirements::identity(loaded)?;
    let local_identity_sha256 = super::super::native::owner_for(loaded)?.identity_sha256;
    let config_binding_sha256 = config_binding(loaded)?;
    let checker = super::super::requirements::checker_input(loaded, checker, command)?;
    let recovery = super::super::recovery::context(loaded, &record)?;
    let recovered_context = recovery.as_ref().filter(|context| {
        context.active.source_config.project_identity_sha256 == project_identity_sha256
    });
    let mut covered = covered_snapshot(
        loaded,
        &record,
        &journey.members,
        &Header {
            protocol: if recovered_context.is_some() {
                RECOVERY_INTEGRATION_PROTOCOL
            } else {
                PROTOCOL
            },
            host: session.host.to_owned(),
            session_key: session.key.clone(),
            owner_identity_sha256: owner.identity_sha256.clone(),
            owner_project_id: owner.project_id.clone(),
            project_identity_sha256: project_identity_sha256.clone(),
            local_identity_sha256: local_identity_sha256.clone(),
            config_binding_sha256: config_binding_sha256.clone(),
            recovery_sha256: recovered_context
                .as_ref()
                .map(|value| value.active.decision_sha256.clone()),
            checker: checker.clone(),
        },
    )?;
    covered.sort_by(|a, b| a.work.cmp(&b.work));
    if covered.is_empty() {
        return Err("no native journey members are recorded for this configured project".into());
    }
    let contract = Contract {
        header: Header {
            protocol: if recovered_context.is_some() {
                RECOVERY_INTEGRATION_PROTOCOL
            } else {
                PROTOCOL
            },
            host: session.host.to_owned(),
            session_key: session.key,
            owner_identity_sha256: owner.identity_sha256,
            owner_project_id: owner.project_id,
            project_identity_sha256,
            local_identity_sha256,
            config_binding_sha256,
            recovery_sha256: recovered_context
                .as_ref()
                .map(|value| value.active.decision_sha256.clone()),
            checker,
        },
        members: covered,
    };
    validate_header_for_loaded(loaded, &contract.header)?;
    let mut basis = if let Some(basis) = supplied_basis {
        crate::kernel::basis::parse_basis_text(basis, "basis")?.value()
    } else {
        json!({"version":1,"constraints":["Preserve the selected task scope, existing authority and evidence."],
            "openZones":["Implementation within the approved project boundary."],
            "decisiveCases":["A current observed integration check covers the frozen accepted journey members."]})
    };
    basis
        .as_object_mut()
        .ok_or("basis is malformed")?
        .remove("sha256");
    let constraints = basis["constraints"]
        .as_array_mut()
        .ok_or("basis constraints are malformed")?;
    constraints.extend(
        member_contract_marker(&contract)?
            .into_iter()
            .map(Value::String),
    );
    let basis_text = serde_json::to_string(&basis).map_err(|error| error.to_string())?;
    crate::kernel::basis::parse_basis_text(&basis_text, "basis")?;
    Ok(Prepared { basis: basis_text })
}

pub(super) fn validate_member_loaded(
    loaded: &Loaded,
    member: &Member,
    header: &Header,
) -> Result<(), String> {
    if member.host != header.host
        || member.session_key != header.session_key
        || member.owner_identity_sha256 != header.owner_identity_sha256
        || member.owner_project_id != header.owner_project_id
        || member.project_identity_sha256 != super::super::requirements::identity(loaded)?
        || member.local_identity_sha256 != super::super::native::owner_for(loaded)?.identity_sha256
        || member.config_path != canonical_path(&loaded.path, "configured project path")?
        || member.config_sha256 != hash::text(&loaded.source)
        || member.product_root != canonical_path(&loaded.product_root, "configured product root")?
        || member.state_root != canonical_path(&loaded.state_root, "configured state root")?
    {
        return Err(
            "journey member belongs to another or changed project/session configuration".into(),
        );
    }
    Ok(())
}

fn covered_snapshot(
    loaded: &Loaded,
    record: &Value,
    members: &[Member],
    header: &Header,
) -> Result<Vec<CoveredMember>, String> {
    let mut covered = Vec::new();
    let project_identity = super::super::requirements::identity(loaded)?;
    let recovery = super::super::recovery::context(loaded, record)?;
    let is_recovered_source = recovery.as_ref().is_some_and(|context| {
        context.active.source_config.project_identity_sha256 == project_identity
    });
    let expected_recovery = recovery
        .as_ref()
        .filter(|_| is_recovered_source)
        .map(|value| value.active.decision_sha256.as_str());
    if expected_recovery != header.recovery_sha256.as_deref()
        || (is_recovered_source && header.protocol != RECOVERY_INTEGRATION_PROTOCOL)
        || (!is_recovered_source && header.protocol != PROTOCOL)
    {
        return Err(
            "journey integration recovery context changed; prepare a fresh integration".into(),
        );
    }
    for member in members
        .iter()
        .filter(|member| member.project_identity_sha256 == project_identity)
    {
        let original = project_loaded(loaded, member)?;
        validate_member_loaded(&original, member, header)?;
        let endpoint_loaded = recovery
            .as_ref()
            .and_then(|context| context.endpoints.get(&member.work))
            .unwrap_or(loaded);
        let endpoint = accepted_endpoint(endpoint_loaded, member)?
            .ok_or_else(|| format!("journey member {} has no terminal accepted result; integration cannot create acceptance", member.work))?;
        let evidence = crate::run::result_ref_evidence(endpoint_loaded, &endpoint.work)?
            .ok_or_else(|| format!("journey member {} evidence is unavailable", member.work))?;
        if !evidence.accepted
            || !evidence.artifact_current
            || evidence.drift.is_some()
            || evidence.inputs_current.is_none()
        {
            return Err(format!(
                "journey member {} has unavailable acceptance/configuration evidence",
                member.work
            ));
        }
        covered.push(CoveredMember {
            work: member.work.clone(),
            member_sha256: member_identity(member),
            accepted_endpoint_sha256: endpoint.identity_sha256,
        });
    }
    covered.sort_by(|a, b| a.work.cmp(&b.work));
    Ok(covered)
}

fn contract_for_work(loaded: &Loaded, work: &str) -> Result<Option<(Contract, Value)>, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let start = events
        .first()
        .ok_or("journey integration Work has no start event")?;
    let Some(contract) = parse_contract(start)? else {
        return Ok(None);
    };
    validate_header_for_loaded(loaded, &contract.header)?;
    Ok(Some((contract, start.clone())))
}

pub(super) fn current_integration_for_project(
    loaded: &Loaded,
    record: &Value,
    state: &JourneyState,
    members: &[Member],
    project_identity: &str,
) -> Result<Option<String>, String> {
    let recovery = super::super::recovery::context(loaded, record)?;
    let project_members = members
        .iter()
        .filter(|member| member.project_identity_sha256 == project_identity)
        .collect::<Vec<_>>();
    if project_members.is_empty() {
        return Ok(None);
    }
    for entry in state
        .integrations
        .iter()
        .filter(|entry| entry.member.project_identity_sha256 == project_identity)
    {
        let member = &entry.member;
        let recovered_context = recovery.as_ref().filter(|context| {
            context.active.source_config.project_identity_sha256 == project_identity
        });
        let header_binding = Header {
            protocol: if recovered_context.is_some() {
                RECOVERY_INTEGRATION_PROTOCOL
            } else {
                PROTOCOL
            },
            host: member.host.clone(),
            session_key: member.session_key.clone(),
            owner_identity_sha256: member.owner_identity_sha256.clone(),
            owner_project_id: member.owner_project_id.clone(),
            project_identity_sha256: member.project_identity_sha256.clone(),
            local_identity_sha256: member.local_identity_sha256.clone(),
            config_binding_sha256: config_binding(loaded)?,
            recovery_sha256: recovered_context
                .as_ref()
                .map(|value| value.active.decision_sha256.clone()),
            checker: super::super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        };
        let original = project_loaded(loaded, member)?;
        validate_member_loaded(&original, member, &header_binding)?;
        let endpoint_loaded = recovery
            .as_ref()
            .and_then(|context| context.endpoints.get(&member.work))
            .unwrap_or(loaded);
        let Some(endpoint) = accepted_endpoint(endpoint_loaded, member)? else {
            continue;
        };
        let Some((contract, _start)) = contract_for_work(loaded, &endpoint.work)? else {
            continue;
        };
        if contract_sha256(&contract) != entry.contract_sha256 {
            continue;
        }
        match covered_snapshot(loaded, record, members, &contract.header) {
            Ok(expected) if expected == contract.members => {}
            Ok(_) => continue,
            Err(error)
                if error
                    == "journey integration recovery context changed; prepare a fresh integration" =>
            {
                continue
            }
            Err(error) => return Err(error),
        }
        let evidence = super::super::requirements::current_checker_evidence(
            loaded,
            &endpoint.work,
            &contract.header.checker,
        )?;
        if evidence["state"] == "current" && evidence["accepted"] == true {
            if recovered_context.is_some() && !current_required_review(loaded, &endpoint.work)? {
                continue;
            }
            return Ok(Some(endpoint.work));
        }
    }
    Ok(None)
}

/// Recovery integrations have an explicit frozen independent-review
/// obligation in addition to the ordinary Lead acceptance and check gates.
/// Baseline integrations retain their existing owner-selected review policy.
fn current_required_review(loaded: &Loaded, work: &str) -> Result<bool, String> {
    let ledger = crate::work::resolve(loaded, work)?;
    let (_, events, _) = crate::run::ledger::load(loaded, &ledger)?;
    let state = crate::run::state::reduce(&events)?;
    let exit = crate::run::gate::exit::ExitState::reduce_with_artifact(&state, Some(true))?;
    Ok(exit.status == "accepted"
        && exit.lead_accepted()
        && exit.review_required
        && exit.reviewer_approved())
}

pub(super) fn project_loaded(loaded: &Loaded, member: &Member) -> Result<Loaded, String> {
    let original = crate::config::load(Some(member.config_path.as_str()))?;
    let selected_product = canonical_path(&loaded.product_root, "configured product root")?;
    let selected_state = canonical_path(&loaded.state_root, "configured state root")?;
    let selected_identity = super::super::requirements::identity(loaded)?;
    let selected_local = super::super::native::owner_for(loaded)?.identity_sha256;
    if selected_product != member.product_root
        || selected_state != member.state_root
        || selected_identity != member.project_identity_sha256
        || selected_local != member.local_identity_sha256
        || original.project_id != loaded.project_id
    {
        return Err(
            "selected journey configuration belongs to another project or state owner".into(),
        );
    }
    validate_member_loaded(
        &original,
        member,
        &Header {
            protocol: PROTOCOL,
            host: member.host.clone(),
            session_key: member.session_key.clone(),
            owner_identity_sha256: member.owner_identity_sha256.clone(),
            owner_project_id: member.owner_project_id.clone(),
            project_identity_sha256: member.project_identity_sha256.clone(),
            local_identity_sha256: member.local_identity_sha256.clone(),
            config_binding_sha256: config_binding(&original)?,
            recovery_sha256: None,
            checker: super::super::requirements::CheckerInput {
                path: String::new(),
                sha256: String::new(),
            },
        },
    )?;
    Ok(original)
}
